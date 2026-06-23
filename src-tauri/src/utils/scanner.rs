use std::collections::BTreeSet;
use std::env;
use std::path::Path;
use std::process::Command;
use std::fs;

pub fn is_supported_video(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(ext.to_ascii_lowercase().as_str(), "mkv" | "mp4" | "avi" | "webm")
}

pub fn get_duration(file_path: &str) -> Result<i64, String> {
    let output = Command::new("ffprobe")
        .args(&[
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            file_path
        ])
        .output()
        .map_err(|e| format!("Failed to run ffprobe: {}", e))?;

    if !output.status.success() {
        return Err("ffprobe failed".to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse ffprobe output: {}", e))?;

    if let Some(format) = json.get("format") {
        if let Some(duration_str) = format.get("duration") {
            if let Some(dur) = duration_str.as_str() {
                if let Ok(d) = dur.parse::<f64>() {
                    return Ok(d as i64); // duration in seconds
                }
            }
        }
    }

    Err("Duration not found in ffprobe output".to_string())
}

pub fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };

    matches!(name, ".cache" | ".git" | "node_modules" | "target" | "dist" | "Trash")
}

pub fn discover_default_scan_roots() -> Vec<String> {
    let mut roots = BTreeSet::new();

    if let Ok(home) = env::var("HOME") {
        let home_path = Path::new(&home);

        if home_path.exists() && home_path.is_dir() {
            roots.insert(home.clone());
        }

        // for candidate in ["Videos", "Downloads", "Movies"] {
        //     let path = home_path.join(candidate);
        //     if path.exists() && path.is_dir() {
        //         roots.insert(path.to_string_lossy().to_string());
        //     }
        // }

        let media_user = Path::new("/media").join(
            home_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        );

        if media_user.exists() && media_user.is_dir() {
            roots.insert(media_user.to_string_lossy().to_string());
        }
    }

    let mnt = Path::new("/mnt");

    if mnt.exists() && mnt.is_dir() {
        if let Ok(entries) = fs::read_dir(mnt) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    roots.insert(path.to_string_lossy().to_string());
                }
            }
        }
    }

    roots.into_iter().collect()
}
