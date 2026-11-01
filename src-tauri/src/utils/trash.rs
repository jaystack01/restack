use std::path::{Path, PathBuf};
use trash::os_limited::{list, restore_all};

pub fn move_to_trash(file_path: &str) -> Result<(), String> {
    trash::delete(file_path).map_err(|e| format!("Failed to trash file {file_path}: {e}"))
}

/// Restore the most recently trashed item that came from `file_path`.
/// Uses the freedesktop Trash API, so it handles `$topdir/.Trash-$UID` on other
/// drives, renamed duplicates (`name.2.mkv`) and `$XDG_DATA_HOME`.
pub fn restore_from_trash(file_path: &str) -> Result<(), String> {
    let target = resolve_original_path(file_path);
    let items = list().map_err(|e| format!("Failed to list Trash: {e}"))?;
    let item = items
        .into_iter()
        .filter(|item| {
            let original = item.original_path();
            original == target || original == Path::new(file_path)
        })
        .max_by_key(|item| item.time_deleted)
        .ok_or_else(|| format!("Failed to find {file_path} in Trash"))?;
    restore_all([item]).map_err(|e| format!("Failed to restore {file_path}: {e}"))
}

// `trash::delete` records the canonical path, so resolve symlinks in the parent dir
fn resolve_original_path(file_path: &str) -> PathBuf {
    let path = Path::new(file_path);
    match (path.parent().and_then(|p| p.canonicalize().ok()), path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}
