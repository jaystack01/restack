use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

pub fn is_supported_video(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mkv" | "mp4" | "avi" | "webm"
    )
}

/// How long ffprobe may take on one file before the scan gives up on its duration.
pub const FFPROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// How often `run_with_timeout` checks whether the child has exited.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `cmd` to completion, killing it if it runs longer than `timeout`, so a
/// truncated file or a hung mount can't stall the scan (and hold the scan guard).
/// Stdin is closed and stdout is captured; stderr is left as the caller set it.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Output, String> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run {program}: {e}"))?;

    // Drain stdout on its own thread: a child blocked on a full pipe would never exit.
    let mut stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout.as_mut() {
            let _ = out.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = reader.join().unwrap_or_default();
                return Ok(Output {
                    status,
                    stdout,
                    stderr: Vec::new(),
                });
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                // A process stuck in uninterruptible I/O (dead NFS) may not die right
                // away: reap it in the background instead of blocking the scan on it.
                // The reader thread ends once the pipe closes.
                thread::spawn(move || child.wait());
                return Err(format!(
                    "Failed to run {program}: timed out after {timeout:?}"
                ));
            }
            Err(e) => {
                let _ = child.kill();
                thread::spawn(move || child.wait());
                return Err(format!("Failed to wait for {program}: {e}"));
            }
        }
    }
}

pub fn get_duration(file_path: &str) -> Result<i64, String> {
    let output = run_with_timeout(
        Command::new("ffprobe")
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                file_path,
            ])
            .stderr(Stdio::null()),
        FFPROBE_TIMEOUT,
    )?;

    if !output.status.success() {
        return Err("ffprobe failed".to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse ffprobe output: {}", e))?;

    if let Some(format) = json.get("format") {
        if let Some(duration_str) = format.get("duration") {
            if let Some(dur) = duration_str.as_str() {
                if let Ok(d) = dur.parse::<f64>() {
                    return Ok(d as i64); // duration in seconds
                }
            }
        }
    }

    Err("Duration not found in ffprobe output".to_string())
}

/// Why the walk leaves a directory out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// Trash (`Trash`, `.Trash-<uid>` on non-home drives): never media. Rows under
    /// it stay subject to reconciliation, so files indexed there by older builds
    /// are cleaned up once they leave the trash.
    Trash,
    /// Hidden dirs and build/tool dirs: their contents are simply not scanned.
    /// Rows already indexed under them are left exactly as they are.
    Excluded,
}

/// Whether the walk skips a directory named `name` below a scan root.
pub fn skip_kind(name: &str) -> Option<SkipKind> {
    // `.Trash-<uid>` is the freedesktop trash on non-home drives
    if name == "Trash" || name.starts_with(".Trash-") {
        return Some(SkipKind::Trash);
    }
    // Hidden dirs cover `.cache`, `.git`, `.local/share/Steam`, `.var`, browser
    // profiles... (BUG-20). `node_modules`/`target`/`dist` are matched anywhere (BUG-21).
    if name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist") {
        return Some(SkipKind::Excluded);
    }
    None
}

/// The login name, used for the removable-media roots. `$HOME`'s basename is only
/// a last resort: home dirs don't have to be named after the user.
pub fn current_username(
    user: Option<String>,
    logname: Option<String>,
    home: Option<String>,
) -> Option<String> {
    let valid = |name: &str| !name.is_empty() && !name.contains('/') && name != "." && name != "..";
    user.filter(|u| valid(u))
        .or_else(|| logname.filter(|l| valid(l)))
        .or_else(|| {
            home.as_deref()
                .and_then(|h| Path::new(h).file_name())
                .and_then(|n| n.to_str())
                .filter(|n| valid(n))
                .map(str::to_string)
        })
}

/// Per-user mount dirs for removable drives: `/media/<user>` (Debian/Ubuntu) and
/// `/run/media/<user>` (udisks2 on Arch/Fedora).
pub fn removable_media_dirs(user: &str) -> [PathBuf; 2] {
    [
        Path::new("/media").join(user),
        Path::new("/run/media").join(user),
    ]
}

pub fn discover_default_scan_roots() -> Vec<String> {
    let mut roots = BTreeSet::new();
    let home = env::var("HOME").ok();

    if let Some(home) = &home {
        let home_path = Path::new(home);

        if home_path.exists() && home_path.is_dir() {
            roots.insert(home.clone());
        }

        // for candidate in ["Videos", "Downloads", "Movies"] {
        //     let path = home_path.join(candidate);
        //     if path.exists() && path.is_dir() {
        //         roots.insert(path.to_string_lossy().to_string());
        //     }
        // }
    }

    if let Some(user) = current_username(env::var("USER").ok(), env::var("LOGNAME").ok(), home) {
        for dir in removable_media_dirs(&user) {
            if dir.is_dir() {
                roots.insert(dir.to_string_lossy().to_string());
            }
        }
    }

    for dir in mount_point_dirs(Path::new("/mnt")) {
        roots.insert(dir.to_string_lossy().to_string());
    }

    roots.into_iter().collect()
}

/// Each directory directly in `base` (e.g. `/mnt`), as a scan root. Hidden
/// entries such as `/mnt/.snapshots` are left out: roots are walked in full,
/// since the hidden-dir skip only applies below a root (BUG-45).
fn mount_point_dirs(base: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Result of walking the scan roots.
pub struct ScanWalk {
    pub files: Vec<String>,
    /// Roots that existed and were walked this scan.
    pub walked_roots: Vec<PathBuf>,
    /// Paths that couldn't be read (permissions, I/O errors), so their contents are unknown.
    pub incomplete_dirs: Vec<PathBuf>,
    /// Dirs left out on purpose (`SkipKind::Excluded`). Their contents weren't looked
    /// at, so rows under them are kept as they are, like rows under `incomplete_dirs`.
    pub skipped_dirs: Vec<PathBuf>,
}

pub fn collect_video_files(roots: &[String]) -> ScanWalk {
    let mut walk = ScanWalk {
        files: Vec::new(),
        walked_roots: Vec::new(),
        incomplete_dirs: Vec::new(),
        skipped_dirs: Vec::new(),
    };
    for root in roots {
        let root_path = Path::new(root);
        if !root_path.exists() || !root_path.is_dir() {
            continue;
        }
        walk.walked_roots.push(root_path.to_path_buf());
        let skipped = &mut walk.skipped_dirs;
        let iter = WalkDir::new(root_path).into_iter().filter_entry(|entry| {
            // A root is walked even if its own name matches (e.g. a hidden folder
            // the user chose); only what's below it is skipped.
            if entry.depth() == 0 || !entry.file_type().is_dir() {
                return true;
            }
            let Some(kind) = entry.file_name().to_str().and_then(skip_kind) else {
                return true;
            };
            if kind == SkipKind::Excluded {
                skipped.push(entry.path().to_path_buf());
            }
            false
        });

        for entry in iter {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    walk.incomplete_dirs
                        .push(err.path().unwrap_or(root_path).to_path_buf());
                    continue;
                }
            };
            let path = entry.path();
            if entry.file_type().is_file() && is_supported_video(path) {
                walk.files.push(path.to_string_lossy().to_string());
            }
        }
    }
    walk
}

/// Decide what happens to DB rows whose files weren't seen this scan.
/// Returns `(removed, missing)`: rows to hard-delete and rows to hide.
/// Only rows under a walked root, and outside any unreadable or skipped subtree,
/// are touched.
pub fn reconcile_unseen(
    existing: &HashMap<String, bool>,
    seen: &HashSet<String>,
    walk: &ScanWalk,
    is_gone: impl Fn(&Path) -> bool,
) -> (Vec<String>, Vec<String>) {
    let mut removed: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (old_file, is_deleted) in existing {
        // Trashed rows are kept so Undo can still restore them
        if *is_deleted || seen.contains(old_file) {
            continue;
        }
        let path = Path::new(old_file);
        let in_scope = walk.walked_roots.iter().any(|root| path.starts_with(root))
            && !walk
                .incomplete_dirs
                .iter()
                .chain(&walk.skipped_dirs)
                .any(|dir| path.starts_with(dir));
        if !in_scope {
            continue;
        }
        if is_gone(path) {
            removed.push(old_file.clone());
        } else {
            missing.push(old_file.clone());
        }
    }
    removed.sort();
    missing.sort();
    (removed, missing)
}

/// A missing file only counts as removed when its folder still exists and has
/// other content. An absent or empty folder usually means an unmounted drive.
pub fn is_confirmed_gone(path: &Path) -> bool {
    path.parent()
        .and_then(|dir| fs::read_dir(dir).ok())
        .is_some_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Fresh directory under the system temp dir, removed on drop.
    pub(crate) struct TempDir(pub PathBuf);

    impl TempDir {
        pub(crate) fn new(name: &str) -> Self {
            let dir = env::temp_dir().join(format!("restack-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        pub(crate) fn file(&self, rel: &str) -> String {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"").unwrap();
            path.to_string_lossy().to_string()
        }

        pub(crate) fn path(&self, rel: &str) -> String {
            self.0.join(rel).to_string_lossy().to_string()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn walk_of(roots: &[&str], incomplete: &[&str]) -> ScanWalk {
        ScanWalk {
            files: Vec::new(),
            walked_roots: roots.iter().map(PathBuf::from).collect(),
            incomplete_dirs: incomplete.iter().map(PathBuf::from).collect(),
            skipped_dirs: Vec::new(),
        }
    }

    fn existing(rows: &[(&str, bool)]) -> HashMap<String, bool> {
        rows.iter().map(|(p, d)| (p.to_string(), *d)).collect()
    }

    #[test]
    fn reconcile_ignores_rows_outside_walked_roots() {
        let rows = existing(&[
            ("/home/u/a.mkv", false),
            ("/media/u/disk/b.mkv", false),
            ("/home/u2/c.mkv", false),
        ]);
        let (removed, missing) =
            reconcile_unseen(&rows, &HashSet::new(), &walk_of(&["/home/u"], &[]), |_| {
                true
            });
        // "/home/u2" shares a string prefix with "/home/u" but isn't under it
        assert_eq!(removed, vec!["/home/u/a.mkv"]);
        assert!(missing.is_empty());
    }

    #[test]
    fn reconcile_keeps_soft_deleted_and_seen_rows() {
        let rows = existing(&[("/r/trashed.mkv", true), ("/r/seen.mkv", false)]);
        let seen: HashSet<String> = ["/r/seen.mkv".to_string()].into();
        let (removed, missing) = reconcile_unseen(&rows, &seen, &walk_of(&["/r"], &[]), |_| true);
        assert!(removed.is_empty() && missing.is_empty());
    }

    #[test]
    fn reconcile_keeps_rows_under_unreadable_dirs() {
        let rows = existing(&[("/r/locked/a.mkv", false), ("/r/open/b.mkv", false)]);
        let (removed, missing) = reconcile_unseen(
            &rows,
            &HashSet::new(),
            &walk_of(&["/r"], &["/r/locked"]),
            |_| false,
        );
        assert!(removed.is_empty());
        assert_eq!(missing, vec!["/r/open/b.mkv"]);
    }

    #[test]
    fn reconcile_splits_removed_and_missing() {
        let rows = existing(&[("/r/gone.mkv", false), ("/r/offline/x.mkv", false)]);
        let (removed, missing) =
            reconcile_unseen(&rows, &HashSet::new(), &walk_of(&["/r"], &[]), |p| {
                p == Path::new("/r/gone.mkv")
            });
        assert_eq!(removed, vec!["/r/gone.mkv"]);
        assert_eq!(missing, vec!["/r/offline/x.mkv"]);
    }

    #[test]
    fn confirmed_gone_needs_a_non_empty_parent() {
        let tmp = TempDir::new("gone");
        tmp.file("full/other.mkv");
        fs::create_dir_all(tmp.0.join("empty")).unwrap();

        assert!(is_confirmed_gone(Path::new(&tmp.path("full/deleted.mkv"))));
        assert!(!is_confirmed_gone(Path::new(
            &tmp.path("empty/deleted.mkv")
        )));
        assert!(!is_confirmed_gone(Path::new(
            &tmp.path("absent/deleted.mkv")
        )));
    }

    #[test]
    fn collect_skips_missing_roots_and_trash_dirs() {
        let tmp = TempDir::new("collect");
        let keep = tmp.file("Videos/a.mkv");
        tmp.file("Videos/notes.txt");
        tmp.file(".local/share/Trash/files/b.mkv");
        tmp.file("disk/.Trash-1000/files/c.mkv");
        tmp.file("project/node_modules/d.mp4");

        let walk = collect_video_files(&[tmp.path(""), tmp.path("not-mounted")]);

        assert_eq!(walk.files, vec![keep]);
        assert_eq!(walk.walked_roots, vec![PathBuf::from(tmp.path(""))]);
        assert!(walk.incomplete_dirs.is_empty());
    }

    #[test]
    fn collect_records_unreadable_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("unreadable");
        tmp.file("locked/a.mkv");
        let locked = tmp.0.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&locked).is_ok() {
            // Running as root: permissions aren't enforced, nothing to test
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let walk = collect_video_files(&[tmp.path("")]);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(walk.files.is_empty());
        assert_eq!(walk.incomplete_dirs, vec![locked]);
    }

    #[test]
    fn collect_skips_hidden_dirs_below_the_root_only() {
        let tmp = TempDir::new("hidden");
        let keep = tmp.file("Videos/a.mkv");
        tmp.file(".local/share/Steam/b.mkv");
        tmp.file("Videos/.thumbs/c.mkv");
        tmp.file(".var/app/d.mp4");
        let hidden_root = tmp.file(".chosen/e.mkv");

        let mut walk = collect_video_files(&[tmp.path("")]);
        assert_eq!(walk.files, vec![keep]);
        walk.skipped_dirs.sort();
        assert_eq!(
            walk.skipped_dirs,
            [".chosen", ".local", ".var", "Videos/.thumbs"]
                .map(|d| tmp.0.join(d))
                .to_vec()
        );

        // A hidden folder passed as a root is still walked
        let walk = collect_video_files(&[tmp.path(".chosen")]);
        assert_eq!(walk.files, vec![hidden_root]);
        assert!(walk.skipped_dirs.is_empty());
    }

    #[test]
    fn collect_does_not_protect_trash_dirs() {
        let tmp = TempDir::new("trash-kind");
        tmp.file("disk/.Trash-1000/files/a.mkv");
        tmp.file("disk/Trash/files/b.mkv");
        let walk = collect_video_files(&[tmp.path("disk")]);
        assert!(walk.files.is_empty());
        assert!(walk.skipped_dirs.is_empty());
    }

    #[test]
    fn reconcile_leaves_rows_under_skipped_hidden_dirs_untouched() {
        let tmp = TempDir::new("hidden-reconcile");
        // Indexed by an older build that walked hidden dirs. The folder still exists
        // and has other content, which would otherwise mean "really removed".
        let hidden = tmp.file(".hidden/x.mkv");
        tmp.file(".hidden/notes.txt");
        let hidden_gone = tmp.path(".hidden/deleted.mkv");
        // Indexed from a drive's trash by an older build: still cleaned up
        let trashed = tmp.path("disk/.Trash-1000/files/old.mkv");
        tmp.file("disk/.Trash-1000/files/other.mkv");
        let removed_file = tmp.path("Videos/removed.mkv");
        let seen_file = tmp.file("Videos/kept.mkv");

        let walk = collect_video_files(&[tmp.path("")]);
        assert_eq!(walk.files, vec![seen_file.clone()]);
        let rows = existing(&[
            (&hidden, false),
            (&hidden_gone, false),
            (&trashed, false),
            (&removed_file, false),
            (&seen_file, false),
        ]);
        let seen: HashSet<String> = walk.files.iter().cloned().collect();
        let (removed, missing) = reconcile_unseen(&rows, &seen, &walk, is_confirmed_gone);

        let mut want = vec![removed_file, trashed];
        want.sort();
        assert_eq!(removed, want);
        assert!(missing.is_empty());
    }

    #[test]
    fn username_prefers_user_then_logname_then_home() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(
            current_username(s("jay"), s("other"), s("/home/data")),
            s("jay")
        );
        assert_eq!(current_username(None, s("jay"), s("/home/data")), s("jay"));
        assert_eq!(current_username(s(""), s(""), s("/home/jay")), s("jay"));
        assert_eq!(current_username(s("a/b"), None, s("/home/jay/")), s("jay"));
        assert_eq!(current_username(None, None, s("/")), None);
        assert_eq!(current_username(None, None, None), None);
    }

    #[test]
    fn mount_point_dirs_skip_hidden_entries_and_files() {
        let base = TempDir::new("mount-points");
        for dir in ["usb", "nas", ".snapshots", ".Trash-1000"] {
            fs::create_dir_all(base.0.join(dir)).unwrap();
        }
        base.file("readme.txt");
        assert_eq!(
            mount_point_dirs(&base.0),
            [base.0.join("nas"), base.0.join("usb")]
        );
    }

    #[test]
    fn mount_point_dirs_of_a_missing_base_is_empty() {
        let base = TempDir::new("mount-points-missing");
        assert!(mount_point_dirs(&base.0.join("absent")).is_empty());
    }

    #[test]
    fn removable_media_dirs_include_udisks2_mounts() {
        assert_eq!(
            removable_media_dirs("jay"),
            [PathBuf::from("/media/jay"), PathBuf::from("/run/media/jay")]
        );
    }

    #[test]
    fn run_with_timeout_returns_stdout_of_a_fast_command() {
        let out = run_with_timeout(
            Command::new("sh").args(["-c", "echo hello; exit 3"]),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stdout, b"hello\n");
        assert_eq!(out.status.code(), Some(3));
    }

    #[test]
    fn run_with_timeout_reads_output_larger_than_the_pipe_buffer() {
        // 1 MiB: far past the 64 KiB pipe buffer, so an unread pipe would deadlock
        let out = run_with_timeout(
            Command::new("head").args(["-c", "1048576", "/dev/zero"]),
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 1 << 20);
    }

    #[test]
    fn run_with_timeout_kills_a_hung_command() {
        let started = Instant::now();
        let err = run_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(200))
            .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn run_with_timeout_reports_a_missing_program() {
        let err = run_with_timeout(
            &mut Command::new("restack-no-such-program"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(
            err.starts_with("Failed to run restack-no-such-program"),
            "{err}"
        );
    }
}
