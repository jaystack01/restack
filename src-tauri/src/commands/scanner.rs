use crate::db::{get_full_library, init_schema, open_db, upsert_episode, upsert_movie, upsert_other};
use crate::models::LibraryResponse;
use crate::utils::parser::{parse_filename, ParsedMedia};
use crate::utils::scanner::{discover_default_scan_roots, get_duration, is_supported_video, should_skip_dir};

use std::collections::HashSet;
use std::path::Path;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

#[derive(serde::Serialize, Clone)]
struct ScanStartPayload {
    total_files: usize,
}

#[derive(serde::Serialize, Clone)]
struct ScanProgressPayload {
    current: usize,
    total: usize,
    file_name: String,
}

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

    // ── Pass 1: Collect all video file paths ─────────────────────────────────
    // This lets us know the total count before we start processing,
    // so we can show accurate "x / N" progress on the frontend.
    let mut all_files: Vec<String> = Vec::new();
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
            if entry.file_type().is_file() && is_supported_video(path) {
                all_files.push(path.to_string_lossy().to_string());
            }
        }
    }

    let total_files = all_files.len();

    // Emit scan:start so the frontend knows the total upfront
    let _ = app.emit("scan:start", ScanStartPayload { total_files });

    // ── Pass 2: Process each file and emit per-file progress ─────────────────
    let mut seen_files: HashSet<String> = HashSet::new();

    for (index, file_path) in all_files.iter().enumerate() {
        seen_files.insert(file_path.clone());

        // Emit progress (1-based so "1 / N" shows on the first file)
        let file_name = Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let _ = app.emit(
            "scan:progress",
            ScanProgressPayload {
                current: index + 1,
                total: total_files,
                file_name,
            },
        );

        let Some(stem) = Path::new(file_path).file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        let is_new = !existing_files.contains(file_path);

        // Only run ffmpeg / duration for new files (expensive)
        let duration = if is_new { get_duration(file_path).unwrap_or(0) } else { 0 };
        let thumb = if is_new { crate::utils::thumbnail::generate_thumbnail(app, file_path) } else { None };

        // Upsert — preserves watch progress, only updates metadata
        match parse_filename(stem) {
            ParsedMedia::Series(parsed) => {
                upsert_episode(&conn, file_path, &parsed, duration, thumb)?;
            }
            ParsedMedia::Movie { title, year } => {
                upsert_movie(&conn, file_path, &title, year, duration, thumb)?;
            }
            ParsedMedia::Other { raw_name } => {
                upsert_other(&conn, file_path, &raw_name, duration, thumb)?;
            }
        }
    }

    // Remove deleted files
    for old_file in &existing_files {
        if !seen_files.contains(old_file) {
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
