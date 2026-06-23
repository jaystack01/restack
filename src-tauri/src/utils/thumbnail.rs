use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Manager};

pub fn get_thumbnail_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app
        .path()
        .app_data_dir() // using app_data_dir to ensure it sits alongside DB
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let thumb_dir = cache_dir.join("thumbnails");
    fs::create_dir_all(&thumb_dir).map_err(|e| format!("Failed to create thumb dir: {}", e))?;
    Ok(thumb_dir)
}

pub fn generate_thumbnail(app: &AppHandle, video_path: &str) -> Option<String> {
    let thumb_dir = get_thumbnail_dir(app).ok()?;
    
    // Create a stable hash based on file path to avoid duplicates
    let file_hash = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, video_path.as_bytes());
    let filename = format!("{}.jpg", file_hash);
    let output_path = thumb_dir.join(&filename);
    
    // If it already exists, just return it
    if output_path.exists() {
        return Some(output_path.to_string_lossy().to_string());
    }

    let status = Command::new("ffmpeg")
        .args([
            "-ss", "00:00:15",
            "-i", video_path,
            "-frames:v", "1",
            "-q:v", "2",
            "-y",
            output_path.to_str().unwrap()
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if status.success() && output_path.exists() {
        Some(output_path.to_string_lossy().to_string())
    } else {
        None
    }
}
