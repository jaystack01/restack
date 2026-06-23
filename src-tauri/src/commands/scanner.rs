use crate::db::{get_full_library, init_schema, open_db, upsert_episode, upsert_movie, upsert_other};
use crate::models::LibraryResponse;
use crate::utils::parser::{parse_filename, ParsedMedia};
use crate::utils::scanner::{discover_default_scan_roots, get_duration, is_supported_video, should_skip_dir};

use std::collections::HashSet;
use std::path::Path;
use tauri::AppHandle;
use walkdir::WalkDir;

fn scan_roots_and_sync(app: &AppHandle, roots: &[String]) -> Result<LibraryResponse, String> {
    let start = std::time::Instant::now();

    let conn = open_db(app)?;
    init_schema(&conn)?;

    // Load existing files from DB
    let mut existing_files: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn.prepare("SELECT file_path FROM media").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows {
            if let Ok(path) = row {
                existing_files.insert(path);
            }
        }
    }

    // Track what we see during scan
    let mut seen_files: HashSet<String> = HashSet::new();

    for root in roots {
        let root_path = Path::new(root);
        if !root_path.exists() || !root_path.is_dir() {
            continue;
        }

        let iter = WalkDir::new(root_path)
            .into_iter()
            .filter_entry(|entry| !should_skip_dir(entry.path()));

        for entry in iter.filter_map(Result::ok) {
            let path = entry.path();
            if !entry.file_type().is_file() || !is_supported_video(path) {
                continue;
            }

            let file_path = path.to_string_lossy().to_string();
            seen_files.insert(file_path.clone());

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            let is_new = !existing_files.contains(&file_path);

            // Only compute duration + thumbnail for new files (expensive)
            let duration = if is_new { get_duration(&file_path).unwrap_or(0) } else { 0 };
            let thumb = if is_new { crate::utils::thumbnail::generate_thumbnail(app, &file_path) } else { None };

            // Always re-parse and upsert (fixes misclassified entries)
            // The upsert uses ON CONFLICT — preserves watch progress, only updates metadata
            match parse_filename(stem) {
                ParsedMedia::Series(parsed) => {
                    upsert_episode(&conn, &file_path, &parsed, duration, thumb)?;
                }
                ParsedMedia::Movie { title, year } => {
                    upsert_movie(&conn, &file_path, &title, year, duration, thumb)?;
                }
                ParsedMedia::Other { raw_name } => {
                    upsert_other(&conn, &file_path, &raw_name, duration, thumb)?;
                }
            }
        }
    }

    // Remove deleted files
    for old_file in existing_files {
        if !seen_files.contains(&old_file) {
            conn.execute("DELETE FROM media WHERE file_path = ?1", [old_file])
                .map_err(|e| e.to_string())?;
        }
    }

    println!("Scan completed in {:?}", start.elapsed());

    get_full_library(&conn)
}

#[tauri::command]
pub fn scan_user_storage(app: AppHandle) -> Result<LibraryResponse, String> {
    let roots = discover_default_scan_roots();
    scan_roots_and_sync(&app, &roots)
}

#[tauri::command]
pub fn scan_and_sync(app: AppHandle, root_path: String) -> Result<LibraryResponse, String> {
    let roots = vec![root_path];
    scan_roots_and_sync(&app, &roots)
}

#[tauri::command]
pub fn get_scan_roots() -> Vec<String> {
    discover_default_scan_roots()
}
