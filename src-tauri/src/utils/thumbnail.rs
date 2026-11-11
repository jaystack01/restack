use crate::utils::scanner::run_with_timeout;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// How long ffmpeg may take to grab one frame before the scan skips the thumbnail.
pub const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(60);

/// Preferred thumbnail frame, past most cold opens and logos.
const PREFERRED_SEEK_SECS: f64 = 15.0;
/// Frame used when the duration is unknown (ffprobe failed): early enough to exist in
/// short clips, late enough to skip a black first frame.
const UNKNOWN_DURATION_SEEK_SECS: f64 = 1.0;

/// Where to grab the thumbnail frame: 15 s in, or 10% of the way through a shorter clip
/// (a fixed 15 s seek gives no frame at all for clips shorter than that).
pub fn thumbnail_seek(duration_secs: i64) -> f64 {
    if duration_secs > 0 {
        PREFERRED_SEEK_SECS.min(duration_secs as f64 * 0.1)
    } else {
        UNKNOWN_DURATION_SEEK_SECS
    }
}

pub fn get_thumbnail_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache_dir = app
        .path()
        .app_data_dir() // using app_data_dir to ensure it sits alongside DB
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let thumb_dir = cache_dir.join("thumbnails");
    fs::create_dir_all(&thumb_dir).map_err(|e| format!("Failed to create thumb dir: {}", e))?;
    Ok(thumb_dir)
}

/// `duration_secs` is the probed duration (0 if unknown); it picks the frame.
pub fn generate_thumbnail(app: &AppHandle, video_path: &str, duration_secs: i64) -> Option<String> {
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
    let seek = format!("{:.3}", thumbnail_seek(duration_secs));
    let output = run_with_timeout(
        Command::new("ffmpeg")
            .args([
                "-ss",
                seek.as_str(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_is_15s_or_a_tenth_of_short_clips() {
        assert_eq!(thumbnail_seek(3600), 15.0);
        assert_eq!(thumbnail_seek(150), 15.0);
        assert_eq!(thumbnail_seek(100), 10.0);
        assert_eq!(thumbnail_seek(5), 0.5);
        assert_eq!(thumbnail_seek(1), 0.1);
        // Unknown duration: a small offset, never past the end of a short clip
        assert_eq!(thumbnail_seek(0), 1.0);
        assert_eq!(thumbnail_seek(-1), 1.0);
    }
}
