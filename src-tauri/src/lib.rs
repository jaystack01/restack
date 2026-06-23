pub mod commands;
pub mod db;
pub mod models;
pub mod utils;

use crate::db::{init_schema, migrate_existing_groups, open_db};
use crate::commands::scanner::{scan_user_storage, scan_and_sync, get_scan_roots};
use crate::commands::library::{delete_episodes_cmd, get_continue_watching_single_cmd, get_library_grouped, restore_episodes_cmd, reclassify_media_cmd};
use crate::commands::player::{update_watch_progress, play_episode};
use crate::commands::groups::{
    rename_group_cmd, merge_groups_cmd, move_episodes_cmd,
    get_all_groups_cmd, get_merge_suggestions_cmd,
};
use crate::models::AppState;
use std::sync::Mutex;
use tauri::Manager;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = open_db(app.handle())?;
            init_schema(&conn)?;
            migrate_existing_groups(&conn)?;
            
            app.manage(AppState {
                player_process: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_user_storage,
            scan_and_sync,
            get_scan_roots,
            get_library_grouped,
            get_continue_watching_single_cmd,
            update_watch_progress,
            play_episode,
            rename_group_cmd,
            merge_groups_cmd,
            move_episodes_cmd,
            get_all_groups_cmd,
            get_merge_suggestions_cmd,
            delete_episodes_cmd,
            restore_episodes_cmd,
            reclassify_media_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
