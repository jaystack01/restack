use crate::db::{
    get_all_groups, get_merge_suggestions, init_schema, merge_groups, move_episodes, open_db,
    rename_group,
};
use crate::models::{GroupInfo, MergeSuggestion};
use crate::run_blocking;
use tauri::AppHandle;

#[tauri::command]
pub async fn rename_group_cmd(
    app: AppHandle,
    group_id: i64,
    new_name: String,
) -> Result<(), String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        rename_group(&conn, group_id, &new_name)
    })
    .await
}

#[tauri::command]
pub async fn merge_groups_cmd(
    app: AppHandle,
    source_group_id: i64,
    target_group_id: i64,
) -> Result<(), String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        merge_groups(&conn, source_group_id, target_group_id)
    })
    .await
}

#[tauri::command]
pub async fn move_episodes_cmd(
    app: AppHandle,
    media_ids: Vec<i64>,
    target_group_id: i64,
) -> Result<(), String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        move_episodes(&conn, &media_ids, target_group_id)
    })
    .await
}

#[tauri::command]
pub async fn get_all_groups_cmd(app: AppHandle) -> Result<Vec<GroupInfo>, String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        get_all_groups(&conn)
    })
    .await
}

#[tauri::command]
pub async fn get_merge_suggestions_cmd(app: AppHandle) -> Result<Vec<MergeSuggestion>, String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        get_merge_suggestions(&conn)
    })
    .await
}
