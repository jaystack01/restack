use crate::utils::scanner::run_with_timeout;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How long ffmpeg may take to grab one frame before the scan skips the thumbnail.
pub const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(60);

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

    // ffmpeg writes to a temp name that is renamed into place only on success, so a
    // killed, crashed or late-writing ffmpeg never leaves a partial JPEG under the
    // final name (which the exists() check above would serve forever). Format, codec
    // and single-image mode are explicit because ffmpeg can't infer them from `.part`.
    let temp_path = thumb_dir.join(format!("{filename}.part"));
    let output = run_with_timeout(
        Command::new("ffmpeg")
            .args([
                "-ss",
                "00:00:15",
                "-i",
                video_path,
                "-frames:v",
                "1",
                "-q:v",
                "2",
                "-f",
                "image2",
                "-c:v",
                "mjpeg",
                "-update",
                "1",
                "-y",
                temp_path.to_str()?,
            ])
            .stderr(Stdio::null()),
        THUMBNAIL_TIMEOUT,
    );

    let finished = match output {
        Ok(out) if out.status.success() && temp_path.exists() => {
            fs::rename(&temp_path, &output_path)
                .map_err(|e| format!("Failed to save thumbnail: {e}"))
        }
        Ok(_) => Err("ffmpeg failed".to_string()),
        Err(e) => Err(e),
    };
    match finished {
        Ok(()) => Some(output_path.to_string_lossy().to_string()),
        Err(e) => {
            eprintln!("No thumbnail for {video_path}: {e}");
            let _ = fs::remove_file(&temp_path);
            None
        }
    }
}
