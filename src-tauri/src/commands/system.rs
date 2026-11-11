use crate::models::DependencyStatus;
use crate::run_blocking;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// True if `name` is an executable regular file in one of the `PATH`
/// directories. Empty entries are skipped rather than read as the current
/// directory, since the app's working directory says nothing about the host.
pub fn find_in_path(name: &str, path: &OsStr) -> bool {
    env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| is_executable(&dir.join(name)))
}

fn is_executable(path: &Path) -> bool {
    // metadata() follows symlinks, so /usr/bin/ffmpeg -> ffmpeg-6 counts
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Which runtime tools are reachable through the given `PATH`. Only reads
/// the filesystem, so tests can pass their own `PATH`.
pub fn dependency_status(path: &OsStr) -> DependencyStatus {
    DependencyStatus {
        mpv: find_in_path("mpv", path),
        ffmpeg: find_in_path("ffmpeg", path),
        ffprobe: find_in_path("ffprobe", path),
    }
}

/// Report whether mpv, ffmpeg and ffprobe are on `PATH`. Looks for the files
/// instead of spawning them, so it is cheap and has no side effects.
#[tauri::command]
pub async fn check_dependencies() -> Result<DependencyStatus, String> {
    run_blocking(|| {
        let path = env::var_os("PATH").unwrap_or_default();
        Ok(dependency_status(&path))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::scanner::tests::TempDir;
    use std::ffi::OsString;
    use std::os::unix::fs::symlink;

    fn set_mode(path: &str, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn path_of(dirs: &[String]) -> OsString {
        env::join_paths(dirs).unwrap()
    }

    #[test]
    fn finds_an_executable_in_any_path_dir() {
        let tmp = TempDir::new("deps-found");
        set_mode(&tmp.file("b/mpv"), 0o755);
        let path = path_of(&[tmp.path("a"), tmp.path("b")]);

        assert!(find_in_path("mpv", &path));
        assert!(!find_in_path("ffmpeg", &path));
    }

    #[test]
    fn ignores_non_executable_files_and_directories() {
        let tmp = TempDir::new("deps-not-exec");
        set_mode(&tmp.file("bin/ffprobe"), 0o644);
        fs::create_dir_all(tmp.path("bin/ffmpeg")).unwrap();
        set_mode(&tmp.path("bin/ffmpeg"), 0o755);
        let path = path_of(&[tmp.path("bin")]);

        assert!(!find_in_path("ffprobe", &path));
        assert!(!find_in_path("ffmpeg", &path));
    }

    #[test]
    fn follows_symlinks_and_rejects_dangling_ones() {
        let tmp = TempDir::new("deps-symlink");
        let real = tmp.file("opt/ffmpeg-6");
        set_mode(&real, 0o755);
        fs::create_dir_all(tmp.path("bin")).unwrap();
        symlink(&real, tmp.path("bin/ffmpeg")).unwrap();
        symlink(tmp.path("opt/gone"), tmp.path("bin/mpv")).unwrap();
        let path = path_of(&[tmp.path("bin")]);

        assert!(find_in_path("ffmpeg", &path));
        assert!(!find_in_path("mpv", &path));
    }

    #[test]
    fn empty_and_missing_path_entries_find_nothing() {
        let tmp = TempDir::new("deps-empty");
        let path = OsString::from(format!(":{}::", tmp.path("nope")));

        assert!(!find_in_path("mpv", &path));
        assert!(!find_in_path("mpv", OsStr::new("")));
    }

    #[test]
    fn dependency_status_reports_each_tool() {
        let tmp = TempDir::new("deps-status");
        set_mode(&tmp.file("bin/mpv"), 0o755);
        set_mode(&tmp.file("bin/ffprobe"), 0o755);
        let path = path_of(&[tmp.path("bin")]);

        assert_eq!(
            dependency_status(&path),
            DependencyStatus {
                mpv: true,
                ffmpeg: false,
                ffprobe: true,
            }
        );
    }
}
