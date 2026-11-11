use crate::db::{
    begin_write, get_full_library, get_known_media, init_schema, open_db, sync_missing_media,
    upsert_episode, upsert_movie, upsert_other, KnownMedia,
};
use crate::models::{AppState, LibraryResponse, ScanGuard};
use crate::run_blocking;
use crate::utils::parser::{parse_filename, ParsedMedia};
use crate::utils::scanner::{
    collect_video_files, discover_default_scan_roots, get_duration, is_confirmed_gone,
    reconcile_unseen, ScanWalk,
};

use rusqlite::{Connection, Transaction};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[derive(serde::Serialize, Clone)]
struct ScanStartPayload {
    total_files: usize,
}

#[derive(serde::Serialize, Clone)]
struct ScanProgressPayload {
    current: usize,
    total: usize,
    file_name: String,
}

fn scan_roots_and_sync(app: &AppHandle, roots: &[String]) -> Result<LibraryResponse, String> {
    // One scan at a time: a second caller (e.g. Ctrl+R during the startup scan)
    // gets an error instead of queueing another full walk. Released on drop.
    let state = app.state::<AppState>();
    let _scan = ScanGuard::try_acquire(&state.scan_running)?;

    let start = std::time::Instant::now();

    let conn = open_db(app)?;
    init_schema(&conn)?;

    // ── Pass 1: Collect all video file paths ─────────────────────────────────
    // This lets us know the total count before we start processing,
    // so we can show accurate "x / N" progress on the frontend.
    let walk = collect_video_files(roots);
    let total_files = walk.files.len();

    // Emit scan:start so the frontend knows the total upfront
    let _ = app.emit("scan:start", ScanStartPayload { total_files });

    // ── Pass 2: Process each file and emit per-file progress ─────────────────
    sync_library(&conn, &walk, |index, file_path, need| {
        // Emit progress (1-based so "1 / N" shows on the first file)
        let file_name = Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let _ = app.emit(
            "scan:progress",
            ScanProgressPayload {
                current: index + 1,
                total: total_files,
                file_name,
            },
        );

        // ffprobe / ffmpeg only for what the row lacks (both expensive and time-limited);
        // a failure or timeout just leaves duration 0 / no thumbnail until the next scan.
        let duration = if need.duration {
            get_duration(file_path).unwrap_or_else(|e| {
                eprintln!("No duration for {file_path}: {e}");
                0
            })
        } else {
            need.known_duration
        };
        let thumb = need
            .thumbnail
            .then(|| crate::utils::thumbnail::generate_thumbnail(app, file_path, duration))
            .flatten();
        (duration, thumb)
    })?;

    println!("Scan completed in {:?}", start.elapsed());

    get_full_library(&conn)
}

/// What a scan still has to probe for one file. New files need everything; known files
/// are retried only for what is missing (BUG-19), so a file with a duration and a
/// thumbnail on disk is never probed again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProbeNeed {
    /// Run ffprobe: the file is new, or its row has duration 0.
    duration: bool,
    /// Run ffmpeg: the row has no thumbnail, or its file is gone (e.g. a cleared cache).
    thumbnail: bool,
    /// The row's duration, which picks the thumbnail frame when ffprobe isn't rerun.
    known_duration: i64,
}

impl ProbeNeed {
    const NEW: ProbeNeed = ProbeNeed {
        duration: true,
        thumbnail: true,
        known_duration: 0,
    };

    fn for_known(row: &KnownMedia) -> Self {
        let thumbnail_ok = row
            .thumbnail_path
            .as_deref()
            .is_some_and(|p| Path::new(p).exists());
        ProbeNeed {
            duration: row.duration <= 0,
            thumbnail: !thumbnail_ok,
            known_duration: row.duration.max(0),
        }
    }

    fn any(&self) -> bool {
        self.duration || self.thumbnail
    }
}

/// Upsert every walked file, then remove or hide rows whose files weren't seen.
/// `on_file(index, path, need)` probes what `need` asks for and returns
/// `(duration, thumbnail)` for the upsert; the upsert never lets a failed probe
/// (0 / `None`) clear values the row already has.
fn sync_library(
    conn: &Connection,
    walk: &ScanWalk,
    mut on_file: impl FnMut(usize, &str, ProbeNeed) -> (i64, Option<String>),
) -> Result<(), String> {
    let known: HashMap<String, KnownMedia> = get_known_media(conn)?.into_iter().collect();
    // path → soft-deleted?, for reconciliation
    let existing_files: HashMap<String, bool> = known
        .iter()
        .map(|(path, row)| (path.clone(), row.deleted))
        .collect();
    let mut seen_files: HashSet<String> = HashSet::new();
    let mut batch = UpsertBatch::new(conn);
    let mut skipped = 0usize;
    let mut consecutive_failures = 0usize;

    for (index, file_path) in walk.files.iter().enumerate() {
        seen_files.insert(file_path.clone());
        let need = known
            .get(file_path)
            .map_or(ProbeNeed::NEW, ProbeNeed::for_known);

        // Probes run ffprobe / ffmpeg (seconds each): never hold the write lock
        // across that, or the mpv tracker's progress writes hit SQLITE_BUSY.
        if need.any() {
            batch.commit()?;
        }
        let (duration, thumb) = on_file(index, file_path, need);

        let Some(stem) = Path::new(file_path).file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        // Upsert — preserves watch progress, only updates metadata. One bad file is
        // logged and skipped; it stays in `seen_files`, so reconciliation leaves its
        // existing row alone instead of deleting or hiding it.
        match batch.run(|tx| upsert_parsed(tx, file_path, stem, duration, thumb))? {
            Ok(()) => consecutive_failures = 0,
            Err(e) => {
                eprintln!("Skipping {file_path}: {e}");
                skipped += 1;
                consecutive_failures += 1;
                // A read-only DB, a schema problem or a lock held past the busy timeout
                // fails every row without rolling the transaction back: that's the
                // whole scan failing, not bad files. Dropping `batch` rolls back.
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    return Err(format!(
                        "Failed to sync library: {consecutive_failures} consecutive files failed, last error: {e}"
                    ));
                }
            }
        }
    }
    batch.commit()?;
    if skipped > 0 {
        eprintln!("Scan skipped {skipped} file(s) that failed to save");
    }

    // Reconcile rows whose files weren't seen this scan (one transaction in db.rs)
    let (removed, missing) =
        reconcile_unseen(&existing_files, &seen_files, walk, is_confirmed_gone);
    sync_missing_media(conn, &removed, &missing)
}

/// Per-file upsert failures in a row (no success in between) after which the scan
/// gives up: past this point the database is failing, not individual files.
const MAX_CONSECUTIVE_FAILURES: usize = 20;

/// Max upserts per scan transaction, and how long one may stay open.
/// Tests use a small batch so they cross several commit boundaries cheaply.
const BATCH_MAX_ROWS: usize = if cfg!(test) { 25 } else { 500 };
const BATCH_MAX_AGE: Duration = Duration::from_secs(1);

/// Groups scan upserts into short write transactions: one fsync per batch
/// instead of per file, while keeping the write lock free for the player.
struct UpsertBatch<'c> {
    conn: &'c Connection,
    tx: Option<Transaction<'c>>,
    opened_at: Instant,
    rows: usize,
}

impl<'c> UpsertBatch<'c> {
    fn new(conn: &'c Connection) -> Self {
        UpsertBatch {
            conn,
            tx: None,
            opened_at: Instant::now(),
            rows: 0,
        }
    }

    /// Run `f` in the open batch (opening one if needed); commits once the batch is full or old.
    /// `f` runs inside a savepoint: if it fails, only its own writes are undone and the
    /// batch carries on, so the row error comes back as `Ok(Err(..))`. The outer `Err`
    /// means the batch itself broke (can't open/commit, or SQLite rolled back the whole
    /// transaction), which aborts the scan; earlier batches stay committed.
    fn run(
        &mut self,
        f: impl FnOnce(&Connection) -> Result<(), String>,
    ) -> Result<Result<(), String>, String> {
        let mut tx = match self.tx.take() {
            Some(tx) => tx,
            None => {
                self.opened_at = Instant::now();
                self.rows = 0;
                begin_write(self.conn)
                    .map_err(|e| format!("Failed to start scan transaction: {e}"))?
            }
        };

        let sp = tx
            .savepoint()
            .map_err(|e| format!("Failed to start scan savepoint: {e}"))?;
        let row = f(&sp);
        match &row {
            Ok(()) => sp
                .commit()
                .map_err(|e| format!("Failed to release scan savepoint: {e}"))?,
            // Dropping the savepoint rolls back to it, keeping the batch's other rows
            Err(_) => drop(sp),
        }
        // Some errors (disk full, I/O) make SQLite abandon the whole transaction,
        // taking the batch's earlier rows with it: that is not a per-file failure.
        if self.conn.is_autocommit() {
            let cause = row.err().unwrap_or_default();
            return Err(format!(
                "Failed to save scan batch, transaction rolled back: {cause}"
            ));
        }
        if row.is_ok() {
            self.rows += 1;
        }
        self.tx = Some(tx);

        if self.rows >= BATCH_MAX_ROWS || self.opened_at.elapsed() >= BATCH_MAX_AGE {
            self.commit()?;
        }
        Ok(row)
    }

    fn commit(&mut self) -> Result<(), String> {
        match self.tx.take() {
            Some(tx) => tx
                .commit()
                .map_err(|e| format!("Failed to commit scan batch: {e}")),
            None => Ok(()),
        }
    }
}

fn upsert_parsed(
    conn: &Connection,
    file_path: &str,
    stem: &str,
    duration: i64,
    thumb: Option<String>,
) -> Result<(), String> {
    match parse_filename(stem) {
        ParsedMedia::Series(parsed) => upsert_episode(conn, file_path, &parsed, duration, thumb),
        ParsedMedia::Movie { title, year } => {
            upsert_movie(conn, file_path, &title, year, duration, thumb)
        }
        ParsedMedia::Other { raw_name } => {
            upsert_other(conn, file_path, &raw_name, duration, thumb)
        }
    }
}

#[tauri::command]
pub async fn scan_user_storage(app: AppHandle) -> Result<LibraryResponse, String> {
    run_blocking(move || {
        let roots = discover_default_scan_roots();
        scan_roots_and_sync(&app, &roots)
    })
    .await
}

#[tauri::command]
pub async fn scan_and_sync(app: AppHandle, root_path: String) -> Result<LibraryResponse, String> {
    run_blocking(move || {
        let roots = vec![root_path];
        scan_roots_and_sync(&app, &roots)
    })
    .await
}

#[tauri::command]
pub async fn get_scan_roots() -> Result<Vec<String>, String> {
    run_blocking(|| Ok(discover_default_scan_roots())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{get_media_paths, mark_deleted, update_progress};
    use crate::utils::scanner::tests::TempDir;
    use std::fs;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    /// Run a scan of `roots` with a fake probe that always succeeds; returns the files
    /// that were probed. The fake thumbnail is the video file itself, so it exists on disk
    /// (and later scans don't retry it) without adding files to the scanned folders.
    fn scan(conn: &Connection, roots: &[String]) -> Vec<String> {
        let walk = collect_video_files(roots);
        let mut probed = Vec::new();
        sync_library(conn, &walk, |_, path, need| {
            if need.any() {
                probed.push(path.to_string());
                (1200, Some(path.to_string()))
            } else {
                (0, None)
            }
        })
        .unwrap();
        probed
    }

    fn visible(conn: &Connection) -> Vec<String> {
        let lib = get_full_library(conn).unwrap();
        let mut paths: Vec<String> = lib
            .series
            .iter()
            .flat_map(|g| {
                g.seasons
                    .iter()
                    .flat_map(|s| s.episodes.iter().map(|e| e.file_path.clone()))
            })
            .chain(lib.movies.iter().map(|m| m.file_path.clone()))
            .chain(lib.others.iter().map(|o| o.file_path.clone()))
            .collect();
        paths.sort();
        paths
    }

    fn known(conn: &Connection) -> Vec<String> {
        let mut paths: Vec<String> = get_media_paths(conn)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        paths.sort();
        paths
    }

    fn duration_and_thumb(conn: &Connection, path: &str) -> (i64, Option<String>) {
        conn.query_row(
            "SELECT duration, thumbnail_path FROM media WHERE file_path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// Scan `root` with `probe` standing in for ffprobe + ffmpeg; returns each probed
    /// file with what it needed.
    fn scan_with(
        conn: &Connection,
        root: &str,
        mut probe: impl FnMut(&str, ProbeNeed) -> (i64, Option<String>),
    ) -> Vec<(String, ProbeNeed)> {
        let walk = collect_video_files(&[root.to_string()]);
        let mut probed = Vec::new();
        sync_library(conn, &walk, |_, path, need| {
            if !need.any() {
                return (0, None);
            }
            probed.push((path.to_string(), need));
            probe(path, need)
        })
        .unwrap();
        probed
    }

    #[test]
    fn short_clip_gets_a_thumbnail() {
        let tmp = TempDir::new("sync-short");
        let thumbs = TempDir::new("sync-short-thumbs");
        let clip = tmp.file("Clip.mkv");
        let conn = test_db();

        // Fake ffmpeg: like the real one, it only yields a frame if the seek is inside
        // the clip. The seek comes from the duration probed first.
        let probed = scan_with(&conn, &tmp.path(""), |_, need| {
            assert_eq!(need, ProbeNeed::NEW);
            let duration = 5;
            let seek = crate::utils::thumbnail::thumbnail_seek(duration);
            let thumb = (seek < duration as f64).then(|| thumbs.file("clip.jpg"));
            (duration, thumb)
        });

        assert_eq!(probed.len(), 1);
        assert_eq!(
            duration_and_thumb(&conn, &clip),
            (5, Some(thumbs.path("clip.jpg")))
        );
    }

    #[test]
    fn known_row_without_thumbnail_is_retried() {
        let tmp = TempDir::new("sync-retry-thumb");
        let thumbs = TempDir::new("sync-retry-thumb-thumbs");
        let ep = tmp.file("Show.S01E01.mkv");
        let conn = test_db();

        // First scan: ffprobe works, ffmpeg fails
        scan_with(&conn, &tmp.path(""), |_, _| (600, None));
        assert_eq!(duration_and_thumb(&conn, &ep), (600, None));

        // Next scan retries only the thumbnail, framed by the stored duration
        let thumb = thumbs.file("ep.jpg");
        let probed = scan_with(&conn, &tmp.path(""), |_, need| {
            (need.known_duration, Some(thumb.clone()))
        });
        let expected = ProbeNeed {
            duration: false,
            thumbnail: true,
            known_duration: 600,
        };
        assert_eq!(probed, vec![(ep.clone(), expected)]);
        assert_eq!(duration_and_thumb(&conn, &ep), (600, Some(thumb.clone())));

        // Now complete: not probed again
        assert!(scan_with(&conn, &tmp.path(""), |_, _| unreachable!()).is_empty());

        // The thumbnail cache was cleared: retried, and a failed retry keeps the old values
        fs::remove_file(&thumb).unwrap();
        let probed = scan_with(&conn, &tmp.path(""), |_, need| (need.known_duration, None));
        assert_eq!(probed, vec![(ep.clone(), expected)]);
        assert_eq!(duration_and_thumb(&conn, &ep), (600, Some(thumb)));
    }

    #[test]
    fn known_row_without_duration_is_reprobed() {
        let tmp = TempDir::new("sync-retry-duration");
        let thumbs = TempDir::new("sync-retry-duration-thumbs");
        let ep = tmp.file("Show.S01E01.mkv");
        let thumb = thumbs.file("ep.jpg");
        let conn = test_db();

        // ffprobe failed (or timed out) on the first scan; ffmpeg still made a thumbnail
        scan_with(&conn, &tmp.path(""), |_, _| (0, Some(thumb.clone())));

        // A failed retry changes nothing; a working one stores the duration
        let probed = scan_with(&conn, &tmp.path(""), |_, _| (0, None));
        let expected = ProbeNeed {
            duration: true,
            thumbnail: false,
            known_duration: 0,
        };
        assert_eq!(probed, vec![(ep.clone(), expected)]);
        assert_eq!(duration_and_thumb(&conn, &ep), (0, Some(thumb.clone())));

        scan_with(&conn, &tmp.path(""), |_, _| (1300, None));
        assert_eq!(duration_and_thumb(&conn, &ep), (1300, Some(thumb)));
        assert!(scan_with(&conn, &tmp.path(""), |_, _| unreachable!()).is_empty());
    }

    #[test]
    fn rescan_only_probes_new_files() {
        let tmp = TempDir::new("sync-probe");
        let a = tmp.file("Show.S01E01.mkv");
        let conn = test_db();
        assert_eq!(scan(&conn, &[tmp.path("")]), vec![a.clone()]);

        let b = tmp.file("Show.S01E02.mkv");
        assert_eq!(scan(&conn, &[tmp.path("")]), vec![b]);
        let duration: i64 = conn
            .query_row(
                "SELECT duration FROM media WHERE file_path = ?1",
                [&a],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(duration, 1200);
    }

    #[test]
    fn unmounted_drive_hides_rows_and_remount_restores_progress() {
        let tmp = TempDir::new("sync-unmount");
        let ep = tmp.file("media/disk/Show/Show.S01E01.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("media")]);
        update_progress(&conn, &ep, 600, false).unwrap();

        // Unplug: the mount dir disappears, the parent root still exists
        fs::rename(tmp.0.join("media/disk"), tmp.0.join("offline")).unwrap();
        scan(&conn, &[tmp.path("media")]);
        assert!(visible(&conn).is_empty());
        assert_eq!(known(&conn), vec![ep.clone()]);

        // Replug
        fs::rename(tmp.0.join("offline"), tmp.0.join("media/disk")).unwrap();
        assert!(
            scan(&conn, &[tmp.path("media")]).is_empty(),
            "no re-probe on return"
        );
        assert_eq!(visible(&conn), vec![ep.clone()]);
        let pos: i64 = conn
            .query_row(
                "SELECT last_position FROM media WHERE file_path = ?1",
                [&ep],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pos, 600);
    }

    #[test]
    fn missing_root_keeps_all_its_rows() {
        let tmp = TempDir::new("sync-missing-root");
        let ep = tmp.file("mnt/nas/Show.S01E01.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("mnt/nas")]);

        fs::rename(tmp.0.join("mnt/nas"), tmp.0.join("gone")).unwrap();
        scan(&conn, &[tmp.path("mnt/nas")]);

        assert_eq!(visible(&conn), vec![ep]);
    }

    #[test]
    fn scanning_one_root_leaves_other_roots_alone() {
        let tmp = TempDir::new("sync-one-root");
        let a = tmp.file("videos/A.S01E01.mkv");
        let b = tmp.file("downloads/B.S01E01.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("videos"), tmp.path("downloads")]);

        scan(&conn, &[tmp.path("videos")]);

        assert_eq!(visible(&conn), {
            let mut v = vec![a, b];
            v.sort();
            v
        });
    }

    #[test]
    fn deleted_file_is_removed_when_folder_still_has_content() {
        let tmp = TempDir::new("sync-deleted");
        let a = tmp.file("Show/Show.S01E01.mkv");
        let b = tmp.file("Show/Show.S01E02.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("")]);

        fs::remove_file(&b).unwrap();
        scan(&conn, &[tmp.path("")]);

        assert_eq!(known(&conn), vec![a]);
    }

    #[test]
    fn rescan_keeps_trashed_rows_for_undo() {
        let tmp = TempDir::new("sync-trashed");
        let a = tmp.file("Show/Show.S01E01.mkv");
        let b = tmp.file("Show/Show.S01E02.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("")]);

        // What delete_episodes_cmd does, minus the real Trash
        fs::remove_file(&b).unwrap();
        mark_deleted(&conn, &b).unwrap();
        scan(&conn, &[tmp.path("")]);
        assert_eq!(known(&conn), vec![a.clone(), b.clone()]);
        assert_eq!(visible(&conn), vec![a.clone()]);

        // Undo puts the file back; the next scan shows it again
        fs::write(&b, b"").unwrap();
        crate::db::mark_restored(&conn, &b).unwrap();
        scan(&conn, &[tmp.path("")]);
        assert_eq!(visible(&conn), vec![a, b]);
    }

    #[test]
    fn large_scan_keeps_every_row_across_batches() {
        let tmp = TempDir::new("sync-batches");
        let ep = |n: usize| format!("Show/Show.S{:02}E{:02}.mkv", n / 50 + 1, n % 50 + 1);
        let total = BATCH_MAX_ROWS * 4 + 10;
        let mut paths: Vec<String> = (0..total).map(|n| tmp.file(&ep(n))).collect();
        paths.sort();
        let conn = test_db();

        assert_eq!(scan(&conn, &[tmp.path("")]).len(), total);
        assert_eq!(visible(&conn), paths);

        // Rescan with new files spread through the walk: only those are probed,
        // and the unchanged rows on either side of each commit survive.
        let mut added: Vec<String> = (0..total / 10)
            .map(|k| tmp.file(&format!("Show/Show.S{:02}E01.mkv", 60 + k)))
            .collect();
        added.sort();
        let mut probed = scan(&conn, &[tmp.path("")]);
        probed.sort();
        assert_eq!(probed, added);

        paths.extend(added);
        paths.sort();
        assert_eq!(visible(&conn), paths);
        let unprobed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM media WHERE duration != 1200",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unprobed, 0, "rescan must not clobber probed durations");
    }

    #[test]
    fn write_lock_is_free_while_probing_new_files() {
        let tmp = TempDir::new("sync-lock");
        let db_path = tmp.path("restack.db");
        let conn = Connection::open(&db_path).unwrap();
        init_schema(&conn).unwrap();
        for n in 1..=20 {
            tmp.file(&format!("Show/Show.S01E{n:02}.mkv"));
        }
        scan(&conn, &[tmp.path("Show")]);
        for n in 1..=20 {
            tmp.file(&format!("Show/Show.S02E{n:02}.mkv"));
            tmp.file(&format!("Other/Show.S03E{n:02}.mkv"));
        }

        // Stand-in for the mpv tracker: a second connection that must not get SQLITE_BUSY
        let player = Connection::open(&db_path).unwrap();
        player.busy_timeout(Duration::ZERO).unwrap();
        let walk = collect_video_files(&[tmp.path("Show"), tmp.path("Other")]);
        let mut probes = 0;
        sync_library(&conn, &walk, |_, _, need| {
            if !need.any() {
                return (0, None);
            }
            probes += 1;
            player
                .execute(
                    "INSERT OR REPLACE INTO app_state (key, value_text) VALUES ('probe', ?1)",
                    [probes],
                )
                .expect("player write during probe");
            (1200, None)
        })
        .unwrap();
        assert_eq!(probes, 40);
    }

    #[test]
    fn scan_batch_waits_for_a_concurrent_writer() {
        let tmp = TempDir::new("sync-busy");
        let db_path = tmp.path("restack.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        init_schema(&conn).unwrap();
        for n in 1..=5 {
            tmp.file(&format!("Show/Show.S01E{n:02}.mkv"));
        }
        scan(&conn, &[tmp.path("Show")]);

        // Another connection (the mpv tracker, a UI command) holds the write lock when the
        // rescan opens its batch. Each upsert reads before it writes: in a deferred batch
        // that fails at once with SQLITE_BUSY and the file is skipped; IMMEDIATE waits.
        let (locked_tx, locked) = std::sync::mpsc::channel();
        let other_path = db_path.clone();
        let other = std::thread::spawn(move || {
            let other = Connection::open(other_path).unwrap();
            other
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     INSERT OR REPLACE INTO app_state (key, value_text) VALUES ('other', 'x');",
                )
                .unwrap();
            locked_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            other.execute_batch("COMMIT").unwrap();
        });
        locked.recv().unwrap();
        let walk = collect_video_files(&[tmp.path("Show")]);
        sync_library(&conn, &walk, |_, _, _| (999, None)).unwrap();
        other.join().unwrap();

        let rewritten: i64 = conn
            .query_row("SELECT COUNT(*) FROM media WHERE duration = 999", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rewritten, 5, "no file was skipped as busy");
    }

    /// Make every write to a media row whose file name starts with `Bad.` fail, the way a
    /// corrupt or constraint-violating row would. `action` is a SQLite RAISE action.
    fn fail_writes_to_bad_paths(conn: &Connection, action: &str) {
        for event in ["INSERT", "UPDATE"] {
            conn.execute_batch(&format!(
                "CREATE TRIGGER fail_bad_{event} BEFORE {event} ON media
                 WHEN NEW.file_path GLOB '*/Bad.*'
                 BEGIN SELECT RAISE({action}, 'forced failure'); END;"
            ))
            .unwrap();
        }
    }

    #[test]
    fn one_failing_file_does_not_abort_the_scan() {
        let tmp = TempDir::new("sync-bad-new");
        // Enough files on both sides of the bad one to cross batch boundaries
        let mut good: Vec<String> = (1..=BATCH_MAX_ROWS * 2)
            .map(|n| tmp.file(&format!("Show/Show.S01E{n:02}.mkv")))
            .collect();
        tmp.file("Lonely/Bad.Lonely.S01E01.mkv");
        good.push(tmp.file("Movie (2001).mkv"));
        good.sort();
        let conn = test_db();
        fail_writes_to_bad_paths(&conn, "ABORT");

        scan(&conn, &[tmp.path("")]);

        assert_eq!(visible(&conn), good);
        // The failed row's savepoint also undid the group it created
        let lonely: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM show_groups WHERE normalized_name = 'badlonely'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lonely, 0);
    }

    #[test]
    fn failing_rescan_of_existing_file_keeps_its_row() {
        let tmp = TempDir::new("sync-bad-existing");
        let bad = tmp.file("Show/Bad.Show.S01E01.mkv");
        let ok = tmp.file("Show/Show.S01E02.mkv");
        let gone = tmp.file("Show/Show.S01E03.mkv");
        let conn = test_db();
        scan(&conn, &[tmp.path("")]);
        update_progress(&conn, &bad, 600, false).unwrap();

        // Next scan: the bad row can't be written, and a sibling file was deleted
        fail_writes_to_bad_paths(&conn, "ABORT");
        fs::remove_file(&gone).unwrap();
        scan(&conn, &[tmp.path("")]);

        // Reconciliation still ran, but treated the failed file as seen
        assert_eq!(known(&conn), vec![bad.clone(), ok.clone()]);
        assert_eq!(visible(&conn), vec![bad.clone(), ok]);
        let pos: i64 = conn
            .query_row(
                "SELECT last_position FROM media WHERE file_path = ?1",
                [&bad],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pos, 600);
    }

    /// Walk `root`, then process its files in `order` (walkdir order is unspecified).
    fn ordered_walk(root: &str, order: Vec<String>) -> ScanWalk {
        let mut walk = collect_video_files(&[root.to_string()]);
        let mut found = walk.files.clone();
        let mut wanted = order.clone();
        found.sort();
        wanted.sort();
        assert_eq!(found, wanted, "order must list every walked file once");
        walk.files = order;
        walk
    }

    #[test]
    fn whole_transaction_rollback_still_fails_the_scan() {
        let tmp = TempDir::new("sync-bad-rollback");
        let mut order: Vec<String> = (1..=4)
            .map(|n| tmp.file(&format!("Show/Show.S01E{n:02}.mkv")))
            .collect();
        let bad = tmp.file("Show/Bad.Show.S01E05.mkv");
        order.push(bad.clone());
        let conn = test_db();
        scan(&conn, &[tmp.path("")]);

        // Rescan with every file already known (no probe commits), the bad one
        // last, so its RAISE(ROLLBACK) on UPDATE drops a batch holding the four
        // earlier rows, like a disk-full error would. Carrying on would lose them.
        conn.execute_batch(
            "CREATE TRIGGER fail_bad_update BEFORE UPDATE ON media
             WHEN NEW.file_path GLOB '*/Bad.*'
             BEGIN SELECT RAISE(ROLLBACK, 'forced rollback'); END;",
        )
        .unwrap();
        let walk = ordered_walk(&tmp.path(""), order);
        let result = sync_library(&conn, &walk, |_, _, _| (999, None));

        assert!(result.is_err(), "{result:?}");
        assert!(conn.is_autocommit());
        let rewritten: i64 = conn
            .query_row("SELECT COUNT(*) FROM media WHERE duration = 999", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rewritten, 0, "the batch's earlier rows were rolled back");
    }

    #[test]
    fn scan_fails_when_every_write_fails() {
        let tmp = TempDir::new("sync-all-fail");
        for n in 1..=MAX_CONSECUTIVE_FAILURES + 5 {
            tmp.file(&format!("Show/Show.S01E{n:02}.mkv"));
        }
        let conn = test_db();
        // Stand-in for a read-only DB: every write fails, the transaction survives
        conn.execute_batch(
            "CREATE TRIGGER fail_all BEFORE INSERT ON media
             BEGIN SELECT RAISE(ABORT, 'attempt to write a readonly database'); END;",
        )
        .unwrap();

        let walk = collect_video_files(&[tmp.path("")]);
        let err = sync_library(&conn, &walk, |_, _, _| (0, None)).unwrap_err();

        assert!(err.contains("consecutive files failed"), "{err}");
        assert!(err.contains("readonly"), "{err}");
        assert!(conn.is_autocommit(), "open batch rolled back");
        assert!(known(&conn).is_empty());
    }

    #[test]
    fn scattered_bad_files_do_not_trip_the_failure_limit() {
        let tmp = TempDir::new("sync-scattered");
        let good: Vec<String> = (1..=60)
            .map(|n| tmp.file(&format!("Show/Show.S01E{n:02}.mkv")))
            .collect();
        // One run just under the limit, plus single bad files spread through the walk
        let run = MAX_CONSECUTIVE_FAILURES - 1;
        let bad: Vec<String> = (1..=run + 6)
            .map(|n| tmp.file(&format!("Show/Bad.Show.S02E{n:02}.mkv")))
            .collect();
        let conn = test_db();
        fail_writes_to_bad_paths(&conn, "ABORT");

        // Order: the run of bad files, then good files with a bad one after every 10th
        let mut order: Vec<String> = bad[..run].to_vec();
        let mut scattered = bad[run..].iter();
        for (i, g) in good.iter().enumerate() {
            order.push(g.clone());
            if i % 10 == 9 {
                order.extend(scattered.next().cloned());
            }
        }
        order.extend(scattered.cloned());
        let walk = ordered_walk(&tmp.path(""), order);
        sync_library(&conn, &walk, |_, _, _| (0, None)).unwrap();

        assert_eq!(visible(&conn), good);
    }
}
