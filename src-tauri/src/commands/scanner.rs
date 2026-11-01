use crate::db::{
    get_full_library, get_media_paths, init_schema, open_db, sync_missing_media, upsert_episode, upsert_movie,
    upsert_other,
};
use crate::models::LibraryResponse;
use crate::utils::parser::{parse_filename, ParsedMedia};
use crate::utils::scanner::{
    collect_video_files, discover_default_scan_roots, ScanWalk, get_duration, is_confirmed_gone, reconcile_unseen,
};

use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tauri::{AppHandle, Emitter};

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
    sync_library(&conn, &walk, |index, file_path, is_new| {
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

        // Only run ffmpeg / duration for new files (expensive)
        if is_new {
            (get_duration(file_path).unwrap_or(0), crate::utils::thumbnail::generate_thumbnail(app, file_path))
        } else {
            (0, None)
        }
    })?;

    println!("Scan completed in {:?}", start.elapsed());

    get_full_library(&conn)
}

/// Upsert every walked file, then remove or hide rows whose files weren't seen.
/// `on_file(index, path, is_new)` returns `(duration, thumbnail)` for the upsert.
fn sync_library(
    conn: &Connection,
    walk: &ScanWalk,
    mut on_file: impl FnMut(usize, &str, bool) -> (i64, Option<String>),
) -> Result<(), String> {
    // Load existing files from DB (path → soft-deleted?)
    let existing_files: HashMap<String, bool> = get_media_paths(conn)?.into_iter().collect();
    let mut seen_files: HashSet<String> = HashSet::new();

    for (index, file_path) in walk.files.iter().enumerate() {
        seen_files.insert(file_path.clone());
        let (duration, thumb) = on_file(index, file_path, !existing_files.contains_key(file_path));

        let Some(stem) = Path::new(file_path).file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        // Upsert — preserves watch progress, only updates metadata
        upsert_parsed(conn, file_path, stem, duration, thumb)?;
    }

    // Reconcile rows whose files weren't seen this scan
    let (removed, missing) = reconcile_unseen(&existing_files, &seen_files, walk, is_confirmed_gone);
    sync_missing_media(conn, &removed, &missing)
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
        ParsedMedia::Movie { title, year } => upsert_movie(conn, file_path, &title, year, duration, thumb),
        ParsedMedia::Other { raw_name } => upsert_other(conn, file_path, &raw_name, duration, thumb),
    }
}

#[tauri::command]
pub fn scan_user_storage(app: AppHandle) -> Result<LibraryResponse, String> {
    let roots = discover_default_scan_roots();
    scan_roots_and_sync(&app, &roots)
}

#[tauri::command]
pub fn scan_and_sync(app: AppHandle, root_path: String) -> Result<LibraryResponse, String> {
    let roots = vec![root_path];
    scan_roots_and_sync(&app, &roots)
}

#[tauri::command]
pub fn get_scan_roots() -> Vec<String> {
    discover_default_scan_roots()
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

    /// Run a scan of `roots` with a fake probe; returns the files that were probed.
    fn scan(conn: &Connection, roots: &[String]) -> Vec<String> {
        let walk = collect_video_files(roots);
        let mut probed = Vec::new();
        sync_library(conn, &walk, |_, path, is_new| {
            if is_new {
                probed.push(path.to_string());
                (1200, Some(format!("{path}.jpg")))
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
            .flat_map(|g| g.seasons.iter().flat_map(|s| s.episodes.iter().map(|e| e.file_path.clone())))
            .chain(lib.movies.iter().map(|m| m.file_path.clone()))
            .chain(lib.others.iter().map(|o| o.file_path.clone()))
            .collect();
        paths.sort();
        paths
    }

    fn known(conn: &Connection) -> Vec<String> {
        let mut paths: Vec<String> = get_media_paths(conn).unwrap().into_iter().map(|(p, _)| p).collect();
        paths.sort();
        paths
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
            .query_row("SELECT duration FROM media WHERE file_path = ?1", [&a], |r| r.get(0))
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
        assert!(scan(&conn, &[tmp.path("media")]).is_empty(), "no re-probe on return");
        assert_eq!(visible(&conn), vec![ep.clone()]);
        let pos: i64 = conn
            .query_row("SELECT last_position FROM media WHERE file_path = ?1", [&ep], |r| r.get(0))
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
}
