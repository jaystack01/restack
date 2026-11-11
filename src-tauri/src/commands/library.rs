use crate::db::{
    get_continue_watching_single, get_full_library, init_schema, mark_deleted, mark_restored,
    open_db, reclassify_media, resolve_trash_paths, TrashAction,
};
use crate::models::{AppState, ContinueWatchingItem, LibraryResponse, ScanGuard};
use crate::run_blocking;
use crate::utils::trash::{move_to_trash, restore_from_trash};
use rusqlite::Connection;
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

/// Resolve `media_ids` to paths, then run `file_op` (move to or restore from Trash) and
/// update each row right after its file, so a mid-batch failure leaves the DB consistent.
/// The whole request is validated before the first file is touched.
fn run_trash_batch(
    conn: &Connection,
    media_ids: &[i64],
    action: TrashAction,
    mut file_op: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let paths = resolve_trash_paths(conn, media_ids, action)?;
    for path in &paths {
        file_op(path)?;
        match action {
            TrashAction::Delete => mark_deleted(conn, path)?,
            TrashAction::Restore => mark_restored(conn, path)?,
        }
    }
    Ok(())
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
pub async fn delete_episodes_cmd(app: AppHandle, media_ids: Vec<i64>) -> Result<(), String> {
    run_blocking(move || {
        exclusive_with_scan(&app.state::<AppState>().scan_running, "delete", || {
            let conn = open_db(&app)?;
            init_schema(&conn)?;
            run_trash_batch(&conn, &media_ids, TrashAction::Delete, move_to_trash)
        })
    })
    .await
}

#[tauri::command]
pub async fn restore_episodes_cmd(app: AppHandle, media_ids: Vec<i64>) -> Result<(), String> {
    run_blocking(move || {
        exclusive_with_scan(&app.state::<AppState>().scan_running, "restore", || {
            let conn = open_db(&app)?;
            init_schema(&conn)?;
            run_trash_batch(&conn, &media_ids, TrashAction::Restore, restore_from_trash)
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
    use crate::db::{get_media_paths, sync_missing_media, upsert_episode};
    use crate::models::ParsedEpisode;
    use std::sync::atomic::Ordering;

    fn library_with(paths: &[&str]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        for (i, path) in paths.iter().enumerate() {
            let ep = ParsedEpisode {
                show_name: "Show".to_string(),
                season: 1,
                episode: i as u32 + 1,
                title: None,
            };
            upsert_episode(&conn, path, &ep, 100, None).unwrap();
        }
        conn
    }

    fn id_of(conn: &Connection, path: &str) -> i64 {
        conn.query_row("SELECT id FROM media WHERE file_path = ?1", [path], |r| {
            r.get(0)
        })
        .unwrap()
    }

    fn trashed(conn: &Connection) -> Vec<String> {
        get_media_paths(conn)
            .unwrap()
            .into_iter()
            .filter(|(_, deleted)| *deleted)
            .map(|(p, _)| p)
            .collect()
    }

    #[test]
    fn trash_batch_moves_then_marks_resolved_paths() {
        let conn = library_with(&["/v/show.s01e01.mkv", "/v/show.s01e02.mkv"]);
        let a = id_of(&conn, "/v/show.s01e01.mkv");
        let b = id_of(&conn, "/v/show.s01e02.mkv");

        // Request order is kept and duplicates are dropped
        let mut moved = Vec::new();
        run_trash_batch(&conn, &[b, a, b], TrashAction::Delete, |p| {
            moved.push(p.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(moved, vec!["/v/show.s01e02.mkv", "/v/show.s01e01.mkv"]);
        assert_eq!(trashed(&conn).len(), 2);

        let mut restored = Vec::new();
        run_trash_batch(&conn, &[a], TrashAction::Restore, |p| {
            restored.push(p.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(restored, vec!["/v/show.s01e01.mkv"]);
        assert_eq!(trashed(&conn), vec!["/v/show.s01e02.mkv"]);
    }

    #[test]
    fn trash_batch_rejects_ineligible_ids_before_touching_files() {
        let conn = library_with(&["/v/show.s01e01.mkv", "/v/show.s01e02.mkv"]);
        let a = id_of(&conn, "/v/show.s01e01.mkv");
        let b = id_of(&conn, "/v/show.s01e02.mkv");
        let never = |p: &str| -> Result<(), String> { panic!("touched {p}") };

        // An unknown id anywhere in the batch refuses the whole batch
        let err = run_trash_batch(&conn, &[a, 9999], TrashAction::Delete, never).unwrap_err();
        assert_eq!(
            err,
            "Failed to delete: media [9999] not found or not in the library"
        );
        assert!(trashed(&conn).is_empty());

        // Restore only takes trashed rows
        let err = run_trash_batch(&conn, &[a], TrashAction::Restore, never).unwrap_err();
        assert_eq!(
            err,
            format!("Failed to restore: media [{a}] not found or not in the Trash")
        );

        // Delete doesn't take rows already in the Trash or whose storage is missing
        mark_deleted(&conn, "/v/show.s01e01.mkv").unwrap();
        sync_missing_media(&conn, &[], &["/v/show.s01e02.mkv".into()]).unwrap();
        let err = run_trash_batch(&conn, &[a, b], TrashAction::Delete, never).unwrap_err();
        assert_eq!(
            err,
            format!("Failed to delete: media [{a}, {b}] not found or not in the library")
        );
        assert_eq!(trashed(&conn), vec!["/v/show.s01e01.mkv"]);
    }

    #[test]
    fn trash_batch_stops_at_the_first_failed_file() {
        let conn = library_with(&["/v/show.s01e01.mkv", "/v/show.s01e02.mkv"]);
        let a = id_of(&conn, "/v/show.s01e01.mkv");
        let b = id_of(&conn, "/v/show.s01e02.mkv");
        let res = run_trash_batch(&conn, &[a, b], TrashAction::Delete, |p| {
            if p.ends_with("e02.mkv") {
                Err("Failed to move to Trash: boom".to_string())
            } else {
                Ok(())
            }
        });
        assert!(res.is_err());
        // Only the file that really moved is marked
        assert_eq!(trashed(&conn), vec!["/v/show.s01e01.mkv"]);
    }

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
