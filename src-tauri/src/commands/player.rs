use crate::db::{
    get_episode_meta, get_last_position, get_next_episode, init_schema, open_db,
    update_progress,
};
use crate::models::{ProgressUpdatePayload, AppState};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    file_path: String,
    last_position: i64,
    watched: bool,
}

#[tauri::command]
pub fn update_watch_progress(
    app: AppHandle,
    payload: ProgressUpdatePayload,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;

    update_progress(
        &conn,
        &payload.file_path,
        payload.last_position.max(0),
        payload.watched,
    )
}

#[tauri::command]
pub fn play_episode(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    start_seconds: Option<u64>,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;

    let db_position = get_last_position(&conn, &file_path)? as u64;
    let start = start_seconds.unwrap_or(db_position);

    // 🧠 unique socket per playback
    let socket_path = format!("/tmp/mpv-{}.sock", uuid::Uuid::new_v4());

    let mut cmd = Command::new("mpv");

    cmd.arg(&file_path)
        .arg(format!("--input-ipc-server={}", socket_path))
        .arg("--keep-open=no")
        .arg("--no-resume-playback") // avoid mpv watch-later vs our DB resume
        .arg("--script-opts=autoload-disabled=yes") // disable community autoload scripts
        .arg("--force-window=no"); // prevents weird lingering window states

    if start > 0 {
        cmd.arg(format!("--start={}", start));
    }

    if let Ok(mut process_lock) = state.player_process.lock() {
        if let Some(mut old_process) = process_lock.take() {
            println!("Killing previous mpv process...");
            let _ = old_process.kill();
            let _ = old_process.wait();
        }
    }

    let child = cmd.spawn()
        .map_err(|e| format!("Failed to start mpv: {}", e))?;

    if let Ok(mut process_lock) = state.player_process.lock() {
        *process_lock = Some(child);
    }
    
    // Notify frontend
    let _ = app.emit("video_playback_started", file_path.clone());

    // 🧠 spawn background tracker
    let app_handle = app.clone();
    let file_clone = file_path.clone();

    thread::spawn(move || {
        track_progress(app_handle, socket_path, file_clone);
    });

    Ok(())
}

fn track_progress(app: AppHandle, socket_path: String, file_path: String) {
    println!("IPC connecting to {}", socket_path);

    // wait for mpv socket
    for _ in 0..15 {
        if std::path::Path::new(&socket_path).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let mut stream = match UnixStream::connect(&socket_path) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to connect IPC: {}", e);
            return;
        }
    };

    println!("IPC connected");

    // 🔹 Instruct mpv to emit events for time-pos and duration changes
    let _ = stream.write_all(br#"{"command":["observe_property", 1, "time-pos"]}"#);
    let _ = stream.write_all(b"\n");
    let _ = stream.write_all(br#"{"command":["observe_property", 2, "duration"]}"#);
    let _ = stream.write_all(b"\n");

    let reader = BufReader::new(stream.try_clone().expect("Failed to clone UnixStream"));
    let mut last_pos: Option<f64> = None;
    let mut last_duration: Option<f64> = None;
    // Emit after IPC loop so the frontend never kills mpv while we still write to the socket (avoids Broken pipe).
    let mut play_next_after_close: Option<String> = None;

    for line in reader.lines() {
        if let Ok(line_str) = line {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line_str) {
                if let Some(event) = json.get("event").and_then(|v| v.as_str()) {
                    if event == "property-change" {
                        if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                            if name == "time-pos" {
                                last_pos = json.get("data").and_then(|v| v.as_f64());
                            } else if name == "duration" {
                                last_duration = json.get("data").and_then(|v| v.as_f64());
                            }
                        }

                        if let (Some(pos), Some(duration)) = (last_pos, last_duration) {
                            let watched = pos >= duration * 0.9;
                            if let Ok(conn) = open_db(&app) {
                                let _ = update_progress(&conn, &file_path, pos as i64, watched);

                                let _ = app.emit(
                                    "video_progress_updated",
                                    ProgressPayload {
                                        file_path: file_path.clone(),
                                        last_position: pos as i64,
                                        watched,
                                    },
                                );
                            }
                        }
                    } else if event == "end-file" {
                        let reason = json.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                        if reason == "eof" {
                            println!("Episode finished → finding next");

                            if let Ok(conn) = open_db(&app) {
                                if let Some((group_id, season, episode)) = get_episode_meta(&conn, &file_path) {
                                    if let Some(next_path) = get_next_episode(&conn, group_id, season, episode) {
                                        println!("Next episode: {}", next_path);
                                        play_next_after_close = Some(next_path);
                                    } else {
                                        println!("End of series");
                                    }
                                }
                            }
                            // keep-open=no exits after EOF; do not send quit here — it races with play_next
                            // (frontend would kill mpv while we still write to IPC → Broken pipe on mpv side).
                            break;
                        } else if reason == "quit" || reason == "error" || reason == "stop" {
                            // User quit manually or we quit via script
                            break;
                        }
                    }
                }
            }
        } else {
            break; // Socket error or closed
        }
    }

    // CLEANUP SOCKET (IMPORTANT)
    let _ = std::fs::remove_file(&socket_path);

    println!("Tracker ended for {}", file_path);
    let _ = app.emit("video_playback_stopped", file_path.clone());

    if let Some(next_path) = play_next_after_close {
        let _ = app.emit("play_next_episode", next_path);
    }
}
