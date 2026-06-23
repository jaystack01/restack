use crate::db::{delete_episodes, get_continue_watching_single, get_full_library, init_schema, open_db, restore_episodes, reclassify_media};
use crate::models::{ContinueWatchingItem, LibraryResponse};
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
    delete_episodes(&conn, &file_paths)
}

#[tauri::command]
pub fn restore_episodes_cmd(app: AppHandle, file_paths: Vec<String>) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    restore_episodes(&conn, &file_paths)
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
