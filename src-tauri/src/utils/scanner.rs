use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;
use walkdir::WalkDir;

pub fn is_supported_video(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(ext.to_ascii_lowercase().as_str(), "mkv" | "mp4" | "avi" | "webm")
}

pub fn get_duration(file_path: &str) -> Result<i64, String> {
    let output = Command::new("ffprobe")
        .args(&[
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            file_path
        ])
        .output()
        .map_err(|e| format!("Failed to run ffprobe: {}", e))?;

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

pub fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };

    // `.Trash-<uid>` is the freedesktop trash on non-home drives
    matches!(name, ".cache" | ".git" | "node_modules" | "target" | "dist" | "Trash") || name.starts_with(".Trash-")
}

pub fn discover_default_scan_roots() -> Vec<String> {
    let mut roots = BTreeSet::new();

    if let Ok(home) = env::var("HOME") {
        let home_path = Path::new(&home);

        if home_path.exists() && home_path.is_dir() {
            roots.insert(home.clone());
        }

        // for candidate in ["Videos", "Downloads", "Movies"] {
        //     let path = home_path.join(candidate);
        //     if path.exists() && path.is_dir() {
        //         roots.insert(path.to_string_lossy().to_string());
        //     }
        // }

        let media_user = Path::new("/media").join(
            home_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        );

        if media_user.exists() && media_user.is_dir() {
            roots.insert(media_user.to_string_lossy().to_string());
        }
    }

    let mnt = Path::new("/mnt");

    if mnt.exists() && mnt.is_dir() {
        if let Ok(entries) = fs::read_dir(mnt) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    roots.insert(path.to_string_lossy().to_string());
                }
            }
        }
    }

    roots.into_iter().collect()
}

/// Result of walking the scan roots.
pub struct ScanWalk {
    pub files: Vec<String>,
    /// Roots that existed and were walked this scan.
    pub walked_roots: Vec<PathBuf>,
    /// Paths that couldn't be read (permissions, I/O errors), so their contents are unknown.
    pub incomplete_dirs: Vec<PathBuf>,
}

pub fn collect_video_files(roots: &[String]) -> ScanWalk {
    let mut walk = ScanWalk { files: Vec::new(), walked_roots: Vec::new(), incomplete_dirs: Vec::new() };
    for root in roots {
        let root_path = Path::new(root);
        if !root_path.exists() || !root_path.is_dir() {
            continue;
        }
        walk.walked_roots.push(root_path.to_path_buf());
        let iter = WalkDir::new(root_path)
            .into_iter()
            .filter_entry(|entry| !should_skip_dir(entry.path()));

        for entry in iter {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    walk.incomplete_dirs.push(err.path().unwrap_or(root_path).to_path_buf());
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
/// Only rows under a walked root, and outside any unreadable subtree, are touched.
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
            && !walk.incomplete_dirs.iter().any(|dir| path.starts_with(dir));
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
        }
    }

    fn existing(rows: &[(&str, bool)]) -> HashMap<String, bool> {
        rows.iter().map(|(p, d)| (p.to_string(), *d)).collect()
    }

    #[test]
    fn reconcile_ignores_rows_outside_walked_roots() {
        let rows = existing(&[("/home/u/a.mkv", false), ("/media/u/disk/b.mkv", false), ("/home/u2/c.mkv", false)]);
        let (removed, missing) = reconcile_unseen(&rows, &HashSet::new(), &walk_of(&["/home/u"], &[]), |_| true);
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
        let (removed, missing) = reconcile_unseen(&rows, &HashSet::new(), &walk_of(&["/r"], &["/r/locked"]), |_| false);
        assert!(removed.is_empty());
        assert_eq!(missing, vec!["/r/open/b.mkv"]);
    }

    #[test]
    fn reconcile_splits_removed_and_missing() {
        let rows = existing(&[("/r/gone.mkv", false), ("/r/offline/x.mkv", false)]);
        let (removed, missing) =
            reconcile_unseen(&rows, &HashSet::new(), &walk_of(&["/r"], &[]), |p| p == Path::new("/r/gone.mkv"));
        assert_eq!(removed, vec!["/r/gone.mkv"]);
        assert_eq!(missing, vec!["/r/offline/x.mkv"]);
    }

    #[test]
    fn confirmed_gone_needs_a_non_empty_parent() {
        let tmp = TempDir::new("gone");
        tmp.file("full/other.mkv");
        fs::create_dir_all(tmp.0.join("empty")).unwrap();

        assert!(is_confirmed_gone(Path::new(&tmp.path("full/deleted.mkv"))));
        assert!(!is_confirmed_gone(Path::new(&tmp.path("empty/deleted.mkv"))));
        assert!(!is_confirmed_gone(Path::new(&tmp.path("absent/deleted.mkv"))));
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
}
