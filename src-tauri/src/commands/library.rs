use crate::db::{get_continue_watching_single, get_full_library, init_schema, mark_deleted, mark_restored, open_db, reclassify_media};
use crate::models::{ContinueWatchingItem, LibraryResponse};
use crate::utils::trash::{move_to_trash, restore_from_trash};
use tauri::AppHandle;

#[tauri::command]
pub fn get_library_grouped(app: AppHandle) -> Result<LibraryResponse, String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    get_full_library(&conn)
}

#[tauri::command]
pub fn get_continue_watching_single_cmd(app: AppHandle) -> Result<Option<ContinueWatchingItem>, String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    get_continue_watching_single(&conn)
}

#[tauri::command]
pub fn delete_episodes_cmd(app: AppHandle, file_paths: Vec<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    // Mark each file right after it moves, so a mid-batch failure leaves the DB consistent
    for file_path in &file_paths {
        move_to_trash(file_path)?;
        mark_deleted(&conn, file_path)?;
    }
    Ok(())
}

#[tauri::command]
pub fn restore_episodes_cmd(app: AppHandle, file_paths: Vec<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    for file_path in &file_paths {
        restore_from_trash(file_path)?;
        mark_restored(&conn, file_path)?;
    }
    Ok(())
}

#[tauri::command]
pub fn reclassify_media_cmd(
    app: AppHandle,
    media_ids: Vec<i64>,
    target_type: String,
    target_group_id: Option<i64>,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    reclassify_media(&conn, &media_ids, &target_type, target_group_id)
}
