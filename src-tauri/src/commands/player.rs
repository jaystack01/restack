use crate::db::{
    get_episode_meta, get_next_episode, get_playback_state, init_schema, open_db, set_watched,
    update_progress,
};
use crate::models::AppState;
use rusqlite::Connection;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// Fraction of the duration after which an episode counts as watched.
const WATCHED_THRESHOLD: f64 = 0.9;

/// Minimum playback-position change (seconds) or wall time between progress writes.
const PERSIST_INTERVAL_SECS: u64 = 5;

#[derive(Clone, serde::Serialize)]
struct ProgressPayload {
    file_path: String,
    last_position: i64,
    watched: bool,
}

#[tauri::command]
pub fn set_watched_cmd(app: AppHandle, media_ids: Vec<i64>, watched: bool) -> Result<(), String> {
    let conn = open_db(&app)?;
    init_schema(&conn)?;
    set_watched(&conn, &media_ids, watched)
}

/// Once watched, stays watched (rewatching doesn't un-mark it).
fn is_watched(previously_watched: bool, pos: f64, duration: f64) -> bool {
    previously_watched || (duration > 0.0 && pos >= duration * WATCHED_THRESHOLD)
}

/// Resume from the saved position, unless it's in the credits zone, where a
/// replay should start from the beginning.
fn resume_position(last_position: i64, duration: i64) -> u64 {
    let finished = duration > 0 && last_position as f64 >= duration as f64 * WATCHED_THRESHOLD;
    if finished { 0 } else { last_position.max(0) as u64 }
}

/// Throttle for progress writes. `last` is the `(position, watched)` last persisted
/// (`None` = nothing yet). Writes when the watched flag flips (so crossing the
/// threshold lands promptly), or when the position moved and either ≥ 5 s of
/// position or ≥ 5 s of wall time passed since the last write.
fn should_persist(last: Option<(i64, bool)>, pos: i64, watched: bool, since_last: Duration) -> bool {
    let Some((last_pos, last_watched)) = last else {
        return true;
    };
    if watched != last_watched {
        return true;
    }
    pos != last_pos
        && ((pos - last_pos).unsigned_abs() >= PERSIST_INTERVAL_SECS
            || since_last >= Duration::from_secs(PERSIST_INTERVAL_SECS))
}

/// Write progress to the DB (if it opened) and tell the frontend.
fn persist_progress(app: &AppHandle, conn: Option<&Connection>, file_path: &str, pos: i64, watched: bool) {
    if let Some(conn) = conn {
        if let Err(e) = update_progress(conn, file_path, pos, watched) {
            println!("{e}");
        }
    }
    let _ = app.emit(
        "video_progress_updated",
        ProgressPayload {
            file_path: file_path.to_string(),
            last_position: pos,
            watched,
        },
    );
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

    let (last_position, duration, previously_watched) =
        get_playback_state(&conn, &file_path)?.unwrap_or((0, 0, false));
    let start = start_seconds.unwrap_or_else(|| resume_position(last_position, duration));

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
        track_progress(app_handle, socket_path, file_clone, previously_watched);
    });

    Ok(())
}

fn track_progress(app: AppHandle, socket_path: String, file_path: String, previously_watched: bool) {
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

    // 🔹 Pause is a flush point so the saved position is exact when the user stops watching
    let _ = stream.write_all(br#"{"command":["observe_property", 3, "pause"]}"#);
    let _ = stream.write_all(b"\n");

    // One connection for the whole playback instead of one per tick
    let conn = match open_db(&app) {
        Ok(c) => Some(c),
        Err(e) => {
            println!("{e}");
            None
        }
    };

    let reader = BufReader::new(stream.try_clone().expect("Failed to clone UnixStream"));
    let mut last_pos: Option<f64> = None;
    let mut last_duration: Option<f64> = None;
    // Latched for the whole session: seeking back below the threshold keeps it watched
    let mut watched_latched = previously_watched;
    // Latest known progress vs. what was last written (throttled, see should_persist)
    let mut current: Option<(i64, bool)> = None;
    let mut persisted: Option<(i64, bool)> = None;
    let mut persisted_at = Instant::now();
    // Emit after IPC loop so the frontend never kills mpv while we still write to the socket (avoids Broken pipe).
    let mut play_next_after_close: Option<String> = None;

    for line in reader.lines() {
        if let Ok(line_str) = line {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line_str) {
                if let Some(event) = json.get("event").and_then(|v| v.as_str()) {
                    if event == "property-change" {
                        let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if name == "pause" {
                            let paused = json.get("data").and_then(|v| v.as_bool()).unwrap_or(false);
                            if let (true, Some((pos, watched))) = (paused && current != persisted, current) {
                                persist_progress(&app, conn.as_ref(), &file_path, pos, watched);
                                persisted = current;
                                persisted_at = Instant::now();
                            }
                            continue;
                        }
                        if name == "time-pos" {
                            last_pos = json.get("data").and_then(|v| v.as_f64());
                        } else if name == "duration" {
                            last_duration = json.get("data").and_then(|v| v.as_f64());
                        }

                        if let (Some(pos), Some(duration)) = (last_pos, last_duration) {
                            watched_latched = is_watched(watched_latched, pos, duration);
                            let pos = pos as i64;
                            current = Some((pos, watched_latched));
                            if should_persist(persisted, pos, watched_latched, persisted_at.elapsed()) {
                                persist_progress(&app, conn.as_ref(), &file_path, pos, watched_latched);
                                persisted = current;
                                persisted_at = Instant::now();
                            }
                        }
                    } else if event == "end-file" {
                        let reason = json.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                        if reason == "eof" {
                            println!("Episode finished → finding next");

                            // Finished: the next play starts from the beginning
                            persist_progress(&app, conn.as_ref(), &file_path, 0, true);
                            current = Some((0, true));
                            persisted = current;
                            if let Some(conn) = conn.as_ref() {
                                if let Some((group_id, season, episode)) = get_episode_meta(conn, &file_path) {
                                    if let Some(next_path) = get_next_episode(conn, group_id, season, episode) {
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

    // Flush whatever the throttle held back (quit, stop, error, killed mpv)
    if let Some((pos, watched)) = current {
        if current != persisted {
            persist_progress(&app, conn.as_ref(), &file_path, pos, watched);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watched_latches_once_reached() {
        assert!(!is_watched(false, 100.0, 1000.0));
        assert!(is_watched(false, 900.0, 1000.0));
        assert!(is_watched(true, 5.0, 1000.0), "rewatching keeps it watched");
        assert!(!is_watched(false, 0.0, 0.0), "unknown duration never counts as watched");
    }

    #[test]
    fn resume_skips_the_credits_zone() {
        assert_eq!(resume_position(600, 1200), 600);
        assert_eq!(resume_position(1150, 1200), 0);
        assert_eq!(resume_position(0, 1200), 0);
        assert_eq!(resume_position(300, 0), 300, "unknown duration resumes as saved");
        assert_eq!(resume_position(-5, 1200), 0);
    }

    #[test]
    fn persist_throttles_position_ticks() {
        let now = Duration::ZERO;
        assert!(should_persist(None, 0, false, now), "first tick always writes");
        assert!(!should_persist(Some((100, false)), 100, false, Duration::from_secs(60)), "no change, no write");
        assert!(!should_persist(Some((100, false)), 103, false, Duration::from_secs(3)));
        assert!(should_persist(Some((100, false)), 105, false, now), "5 s of position");
        assert!(should_persist(Some((100, false)), 40, false, now), "seeking back counts too");
        assert!(should_persist(Some((100, false)), 101, false, Duration::from_secs(5)), "5 s of wall time");
    }

    #[test]
    fn persist_immediately_when_watched_flips() {
        assert!(should_persist(Some((899, false)), 900, true, Duration::ZERO));
        assert!(!should_persist(Some((900, true)), 901, true, Duration::ZERO));
    }
}
