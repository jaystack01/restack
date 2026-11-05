use crate::db::{
    get_continue_watching_single, get_full_library, init_schema, mark_deleted, mark_restored,
    open_db, reclassify_media,
};
use crate::models::{AppState, ContinueWatchingItem, LibraryResponse, ScanGuard};
use crate::run_blocking;
use crate::utils::trash::{move_to_trash, restore_from_trash};
use std::sync::atomic::AtomicBool;
use tauri::{AppHandle, Manager};

/// Run a Trash operation only while no scan is running, and keep scans out until it ends.
/// A scan that collected a path before it was trashed (or restored) would otherwise upsert
/// it back with `deleted_at = NULL` (or hard-delete it) behind the user's back.
fn exclusive_with_scan<T>(
    scan_running: &AtomicBool,
    action: &str,
    op: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _scan = ScanGuard::try_acquire(scan_running)
        .map_err(|_| format!("Failed to {action}: a library scan is running"))?;
    op()
}

#[tauri::command]
pub async fn get_library_grouped(app: AppHandle) -> Result<LibraryResponse, String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        get_full_library(&conn)
    })
    .await
}

#[tauri::command]
pub async fn get_continue_watching_single_cmd(
    app: AppHandle,
) -> Result<Option<ContinueWatchingItem>, String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        get_continue_watching_single(&conn)
    })
    .await
}

#[tauri::command]
pub async fn delete_episodes_cmd(app: AppHandle, file_paths: Vec<String>) -> Result<(), String> {
    run_blocking(move || {
        exclusive_with_scan(&app.state::<AppState>().scan_running, "delete", || {
            let conn = open_db(&app)?;
            init_schema(&conn)?;
            // Mark each file right after it moves, so a mid-batch failure leaves the DB consistent
            for file_path in &file_paths {
                move_to_trash(file_path)?;
                mark_deleted(&conn, file_path)?;
            }
            Ok(())
        })
    })
    .await
}

#[tauri::command]
pub async fn restore_episodes_cmd(app: AppHandle, file_paths: Vec<String>) -> Result<(), String> {
    run_blocking(move || {
        exclusive_with_scan(&app.state::<AppState>().scan_running, "restore", || {
            let conn = open_db(&app)?;
            init_schema(&conn)?;
            for file_path in &file_paths {
                restore_from_trash(file_path)?;
                mark_restored(&conn, file_path)?;
            }
            Ok(())
        })
    })
    .await
}

#[tauri::command]
pub async fn reclassify_media_cmd(
    app: AppHandle,
    media_ids: Vec<i64>,
    target_type: String,
    target_group_id: Option<i64>,
) -> Result<(), String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        reclassify_media(&conn, &media_ids, &target_type, target_group_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn trash_ops_rejected_while_scan_runs() {
        let flag = AtomicBool::new(false);
        let scan = ScanGuard::try_acquire(&flag).unwrap();
        let mut ran = false;
        let res = exclusive_with_scan(&flag, "delete", || {
            ran = true;
            Ok(())
        });
        assert_eq!(
            res.err().as_deref(),
            Some("Failed to delete: a library scan is running")
        );
        assert!(!ran);
        drop(scan);

        // With no scan, the op runs and keeps scans out until it returns, even on error
        let res = exclusive_with_scan(&flag, "restore", || {
            assert!(ScanGuard::try_acquire(&flag).is_err());
            Err::<(), _>("Failed to restore: boom".to_string())
        });
        assert!(res.is_err());
        assert!(!flag.load(Ordering::SeqCst));
    }
}
