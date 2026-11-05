use crate::db::{
    get_episode_meta, get_next_episode, get_playback_state, init_schema, open_db, set_watched,
    update_progress,
};
use crate::models::AppState;
use crate::run_blocking;
use rusqlite::Connection;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

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
pub async fn set_watched_cmd(
    app: AppHandle,
    media_ids: Vec<i64>,
    watched: bool,
) -> Result<(), String> {
    run_blocking(move || {
        let conn = open_db(&app)?;
        init_schema(&conn)?;
        set_watched(&conn, &media_ids, watched)
    })
    .await
}

/// Once watched, stays watched (rewatching doesn't un-mark it).
fn is_watched(previously_watched: bool, pos: f64, duration: f64) -> bool {
    previously_watched || (duration > 0.0 && pos >= duration * WATCHED_THRESHOLD)
}

/// Resume from the saved position, unless it's in the credits zone, where a
/// replay should start from the beginning.
fn resume_position(last_position: i64, duration: i64) -> u64 {
    let finished = duration > 0 && last_position as f64 >= duration as f64 * WATCHED_THRESHOLD;
    if finished {
        0
    } else {
        last_position.max(0) as u64
    }
}

/// Throttle for progress writes. `last` is the `(position, watched)` last persisted
/// (`None` = nothing yet). Writes when the watched flag flips (so crossing the
/// threshold lands promptly), or when the position moved and either ≥ 5 s of
/// position or ≥ 5 s of wall time passed since the last write.
fn should_persist(
    last: Option<(i64, bool)>,
    pos: i64,
    watched: bool,
    since_last: Duration,
) -> bool {
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
fn persist_progress(
    app: &AppHandle,
    conn: Option<&Connection>,
    file_path: &str,
    pos: i64,
    watched: bool,
) {
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
pub async fn play_episode(
    app: AppHandle,
    file_path: String,
    start_seconds: Option<u64>,
) -> Result<(), String> {
    run_blocking(move || start_playback(app, file_path, start_seconds)).await
}

/// Kill a replaced mpv and reap it. Called outside the `player_process` lock.
fn stop_player(old_process: Option<Child>) {
    if let Some(mut old_process) = old_process {
        println!("Killing previous mpv process...");
        let _ = old_process.kill();
        let _ = old_process.wait();
    }
}

fn start_playback(
    app: AppHandle,
    file_path: String,
    start_seconds: Option<u64>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
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

    // Only swap the Child under the lock; kill/wait and spawn happen outside it
    let old_process = state
        .player_process
        .lock()
        .ok()
        .and_then(|mut lock| lock.take());
    stop_player(old_process);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start mpv: {}", e))?;

    // A concurrent play_episode may have stored its mpv meanwhile: the newest wins
    let raced = state
        .player_process
        .lock()
        .ok()
        .and_then(|mut lock| lock.replace(child));
    stop_player(raced);

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

/// The mpv JSON-IPC messages the tracker reacts to. Everything else (command
/// replies, other events) is ignored.
#[derive(Debug, Clone, Copy, PartialEq)]
enum IpcEvent {
    /// `time-pos` changed; `None` when mpv reports `null` (nothing loaded).
    TimePos(Option<f64>),
    /// `duration` changed; `None` when mpv reports `null`.
    Duration(Option<f64>),
    /// `pause` changed; a `null` value counts as not paused.
    Pause(bool),
    EndFile(EndReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndReason {
    Eof,
    Quit,
    Error,
    Stop,
    /// Any other reason (e.g. `redirect`): playback goes on, keep tracking.
    Other,
}

/// Parse one line of mpv IPC output. `None` for malformed or irrelevant lines.
fn parse_ipc_line(line: &str) -> Option<IpcEvent> {
    let json = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let data = json.get("data");
    match json.get("event")?.as_str()? {
        "property-change" => match json.get("name").and_then(|v| v.as_str())? {
            "time-pos" => Some(IpcEvent::TimePos(data.and_then(|v| v.as_f64()))),
            "duration" => Some(IpcEvent::Duration(data.and_then(|v| v.as_f64()))),
            "pause" => Some(IpcEvent::Pause(
                data.and_then(|v| v.as_bool()).unwrap_or(false),
            )),
            _ => None,
        },
        "end-file" => {
            let reason = match json.get("reason").and_then(|v| v.as_str()) {
                Some("eof") => EndReason::Eof,
                Some("quit") => EndReason::Quit,
                Some("error") => EndReason::Error,
                Some("stop") => EndReason::Stop,
                _ => EndReason::Other,
            };
            Some(IpcEvent::EndFile(reason))
        }
        _ => None,
    }
}

/// Progress to write to the DB and emit to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProgressUpdate {
    pos: i64,
    watched: bool,
}

/// What the IPC loop should do after one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackerStep {
    persist: Option<ProgressUpdate>,
    /// Playback ended: leave the IPC loop.
    exit: bool,
}

/// What to do once the IPC loop has exited, for any reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrackerExit {
    /// Progress the throttle held back (quit, stop, error, killed mpv).
    flush: Option<ProgressUpdate>,
    /// Finished at EOF: look up the next episode and emit `play_next_episode`.
    /// Only acted on after the loop so the frontend never kills mpv while we
    /// still write to the socket (avoids Broken pipe).
    play_next: bool,
}

/// Playback progress state for one mpv session. Pure: it decides what to
/// persist, the caller does the DB writes and emits. Time is passed in.
struct ProgressTracker {
    pos: Option<f64>,
    duration: Option<f64>,
    // Latched for the whole session: seeking back below the threshold keeps it watched
    watched: bool,
    // Latest known progress vs. what was last written (throttled, see should_persist)
    current: Option<(i64, bool)>,
    persisted: Option<(i64, bool)>,
    persisted_at: Instant,
    finished: bool,
}

impl ProgressTracker {
    fn new(previously_watched: bool, now: Instant) -> Self {
        Self {
            pos: None,
            duration: None,
            watched: previously_watched,
            current: None,
            persisted: None,
            persisted_at: now,
            finished: false,
        }
    }

    fn mark_persisted(&mut self, now: Instant) -> Option<ProgressUpdate> {
        self.persisted = self.current;
        self.persisted_at = now;
        self.current
            .map(|(pos, watched)| ProgressUpdate { pos, watched })
    }

    fn handle(&mut self, event: IpcEvent, now: Instant) -> TrackerStep {
        let mut step = TrackerStep {
            persist: None,
            exit: false,
        };
        match event {
            // Pause is a flush point so the saved position is exact when the user stops watching
            IpcEvent::Pause(paused) => {
                if paused && self.current.is_some() && self.current != self.persisted {
                    step.persist = self.mark_persisted(now);
                }
            }
            IpcEvent::TimePos(pos) => {
                self.pos = pos;
                step.persist = self.on_progress(now);
            }
            IpcEvent::Duration(duration) => {
                self.duration = duration;
                step.persist = self.on_progress(now);
            }
            IpcEvent::EndFile(EndReason::Eof) => {
                // Finished: the next play starts from the beginning
                self.current = Some((0, true));
                step.persist = self.mark_persisted(now);
                self.finished = true;
                // keep-open=no exits after EOF; do not send quit here — it races with play_next
                step.exit = true;
            }
            IpcEvent::EndFile(EndReason::Quit | EndReason::Error | EndReason::Stop) => {
                // User quit manually or we quit via script
                step.exit = true;
            }
            IpcEvent::EndFile(EndReason::Other) => {}
        }
        step
    }

    fn on_progress(&mut self, now: Instant) -> Option<ProgressUpdate> {
        let (Some(pos), Some(duration)) = (self.pos, self.duration) else {
            return None;
        };
        self.watched = is_watched(self.watched, pos, duration);
        let pos = pos as i64;
        self.current = Some((pos, self.watched));
        let since_last = now.saturating_duration_since(self.persisted_at);
        if should_persist(self.persisted, pos, self.watched, since_last) {
            self.mark_persisted(now)
        } else {
            None
        }
    }

    /// Call once the IPC loop has exited, whatever the reason.
    fn finish(self) -> TrackerExit {
        let flush = match self.current {
            Some((pos, watched)) if self.current != self.persisted => {
                Some(ProgressUpdate { pos, watched })
            }
            _ => None,
        };
        TrackerExit {
            flush,
            play_next: self.finished,
        }
    }
}

/// Next episode in the same group after `file_path`, if any.
fn find_next_episode(conn: &Connection, file_path: &str) -> Option<String> {
    let (group_id, season, episode) = get_episode_meta(conn, file_path)?;
    get_next_episode(conn, group_id, season, episode)
}

/// The IPC loop: feed each mpv line through the tracker and `persist` what it
/// asks for, until playback ends, the socket errors, or input runs out. Then
/// `persist` the final flush. The caller acts on `play_next` afterwards.
fn drive_ipc(
    lines: impl IntoIterator<Item = std::io::Result<String>>,
    mut tracker: ProgressTracker,
    mut now: impl FnMut() -> Instant,
    mut persist: impl FnMut(ProgressUpdate),
) -> TrackerExit {
    for line in lines {
        let Ok(line) = line else {
            break; // Socket error or closed
        };
        let Some(event) = parse_ipc_line(&line) else {
            continue;
        };
        let step = tracker.handle(event, now());
        if let Some(update) = step.persist {
            persist(update);
        }
        if step.exit {
            break;
        }
    }

    let exit = tracker.finish();
    if let Some(update) = exit.flush {
        persist(update);
    }
    exit
}

fn track_progress(
    app: AppHandle,
    socket_path: String,
    file_path: String,
    previously_watched: bool,
) {
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
    let tracker = ProgressTracker::new(previously_watched, Instant::now());
    let exit = drive_ipc(reader.lines(), tracker, Instant::now, |update| {
        persist_progress(&app, conn.as_ref(), &file_path, update.pos, update.watched)
    });

    let play_next_after_close = if exit.play_next {
        println!("Episode finished → finding next");
        let next = conn
            .as_ref()
            .and_then(|conn| find_next_episode(conn, &file_path));
        match &next {
            Some(next_path) => println!("Next episode: {}", next_path),
            None => println!("End of series"),
        }
        next
    } else {
        None
    };

    // CLEANUP SOCKET (IMPORTANT)
    let _ = std::fs::remove_file(&socket_path);

    println!("Tracker ended for {}", file_path);
    let _ = app.emit("video_playback_stopped", file_path.clone());

    // Emit only after the IPC loop has exited (see TrackerExit::play_next)
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
        assert!(
            !is_watched(false, 0.0, 0.0),
            "unknown duration never counts as watched"
        );
    }

    #[test]
    fn resume_skips_the_credits_zone() {
        assert_eq!(resume_position(600, 1200), 600);
        assert_eq!(resume_position(1150, 1200), 0);
        assert_eq!(resume_position(0, 1200), 0);
        assert_eq!(
            resume_position(300, 0),
            300,
            "unknown duration resumes as saved"
        );
        assert_eq!(resume_position(-5, 1200), 0);
    }

    #[test]
    fn persist_throttles_position_ticks() {
        let now = Duration::ZERO;
        assert!(
            should_persist(None, 0, false, now),
            "first tick always writes"
        );
        assert!(
            !should_persist(Some((100, false)), 100, false, Duration::from_secs(60)),
            "no change, no write"
        );
        assert!(!should_persist(
            Some((100, false)),
            103,
            false,
            Duration::from_secs(3)
        ));
        assert!(
            should_persist(Some((100, false)), 105, false, now),
            "5 s of position"
        );
        assert!(
            should_persist(Some((100, false)), 40, false, now),
            "seeking back counts too"
        );
        assert!(
            should_persist(Some((100, false)), 101, false, Duration::from_secs(5)),
            "5 s of wall time"
        );
    }

    #[test]
    fn persist_immediately_when_watched_flips() {
        assert!(should_persist(
            Some((899, false)),
            900,
            true,
            Duration::ZERO
        ));
        assert!(!should_persist(
            Some((900, true)),
            901,
            true,
            Duration::ZERO
        ));
    }

    // Lines as mpv writes them on the IPC socket
    const TIME_POS_NULL: &str =
        r#"{"event":"property-change","id":1,"name":"time-pos","data":null}"#;
    const DURATION_1000: &str =
        r#"{"event":"property-change","id":2,"name":"duration","data":1000.0}"#;
    const PAUSED: &str = r#"{"event":"property-change","id":3,"name":"pause","data":true}"#;
    const UNPAUSED: &str = r#"{"event":"property-change","id":3,"name":"pause","data":false}"#;
    const END_EOF: &str = r#"{"event":"end-file","reason":"eof","playlist_entry_id":1}"#;
    const END_QUIT: &str = r#"{"event":"end-file","reason":"quit","playlist_entry_id":1}"#;
    const END_ERROR: &str =
        r#"{"event":"end-file","reason":"error","playlist_entry_id":1,"file_error":"x"}"#;
    const END_STOP: &str = r#"{"event":"end-file","reason":"stop","playlist_entry_id":1}"#;
    const END_REDIRECT: &str = r#"{"event":"end-file","reason":"redirect"}"#;

    fn time_pos(secs: f64) -> String {
        format!(r#"{{"event":"property-change","id":1,"name":"time-pos","data":{secs}}}"#)
    }

    fn update(pos: i64, watched: bool) -> Option<ProgressUpdate> {
        Some(ProgressUpdate { pos, watched })
    }

    /// Runs `drive_ipc` (the real `track_progress` loop) over `(wall-clock secs,
    /// read result)` with a fake clock. Returns the persisted updates in order
    /// (final flush included), the exit, and how many lines were read.
    fn run_reads(
        previously_watched: bool,
        reads: Vec<(u64, std::io::Result<String>)>,
    ) -> (Vec<ProgressUpdate>, TrackerExit, usize) {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let read = std::cell::Cell::new(0);
        let lines = reads.into_iter().map(|(secs, line)| {
            clock.set(start + Duration::from_secs(secs));
            read.set(read.get() + 1);
            line
        });
        let mut writes = Vec::new();
        let tracker = ProgressTracker::new(previously_watched, start);
        let exit = drive_ipc(lines, tracker, || clock.get(), |u| writes.push(u));
        (writes, exit, read.get())
    }

    /// `run_reads` over lines that all read fine, ending at end of input.
    fn run_session(
        previously_watched: bool,
        lines: &[(u64, &str)],
    ) -> (Vec<ProgressUpdate>, TrackerExit) {
        let reads = lines
            .iter()
            .map(|(secs, line)| (*secs, Ok(line.to_string())))
            .collect();
        let (writes, exit, _) = run_reads(previously_watched, reads);
        (writes, exit)
    }

    #[test]
    fn parse_property_changes() {
        assert_eq!(
            parse_ipc_line(&time_pos(12.5)),
            Some(IpcEvent::TimePos(Some(12.5)))
        );
        assert_eq!(
            parse_ipc_line(DURATION_1000),
            Some(IpcEvent::Duration(Some(1000.0)))
        );
        assert_eq!(parse_ipc_line(PAUSED), Some(IpcEvent::Pause(true)));
        assert_eq!(parse_ipc_line(UNPAUSED), Some(IpcEvent::Pause(false)));
    }

    #[test]
    fn parse_null_data() {
        assert_eq!(parse_ipc_line(TIME_POS_NULL), Some(IpcEvent::TimePos(None)));
        assert_eq!(
            parse_ipc_line(r#"{"event":"property-change","id":2,"name":"duration","data":null}"#),
            Some(IpcEvent::Duration(None))
        );
        assert_eq!(
            parse_ipc_line(r#"{"event":"property-change","id":3,"name":"pause"}"#),
            Some(IpcEvent::Pause(false)),
            "missing pause data counts as not paused"
        );
    }

    #[test]
    fn parse_end_file_reasons() {
        for (line, reason) in [
            (END_EOF, EndReason::Eof),
            (END_QUIT, EndReason::Quit),
            (END_ERROR, EndReason::Error),
            (END_STOP, EndReason::Stop),
            (END_REDIRECT, EndReason::Other),
            (r#"{"event":"end-file"}"#, EndReason::Other),
        ] {
            assert_eq!(
                parse_ipc_line(line),
                Some(IpcEvent::EndFile(reason)),
                "{line}"
            );
        }
    }

    #[test]
    fn parse_ignores_replies_other_events_and_garbage() {
        for line in [
            r#"{"data":null,"request_id":0,"error":"success"}"#,
            r#"{"event":"playback-restart"}"#,
            r#"{"event":"property-change","id":4,"name":"volume","data":50}"#,
            r#"{"event":"property-change","data":1.0}"#,
            r#"{"event":42}"#,
            r#"{"event":"property-change","name":"time-pos""#,
            "not json",
            "",
        ] {
            assert_eq!(parse_ipc_line(line), None, "{line}");
        }
    }

    #[test]
    fn nothing_persists_until_position_and_duration_are_known() {
        let t = time_pos(10.0);
        let (writes, exit) = run_session(false, &[(0, TIME_POS_NULL), (0, &t)]);
        assert!(writes.is_empty());
        assert_eq!(exit.flush, None);

        let (writes, _) = run_session(false, &[(0, &t), (0, DURATION_1000)]);
        assert_eq!(writes, vec![update(10, false).unwrap()]);
    }

    #[test]
    fn null_time_pos_pauses_tracking() {
        let start = Instant::now();
        let later = start + Duration::from_secs(60);
        let mut tracker = ProgressTracker::new(false, start);
        tracker.handle(IpcEvent::Duration(Some(1000.0)), start);
        let step = tracker.handle(IpcEvent::TimePos(Some(10.0)), start);
        assert_eq!(step.persist, update(10, false));
        let step = tracker.handle(IpcEvent::TimePos(None), start);
        assert_eq!(step.persist, None);
        let step = tracker.handle(IpcEvent::Duration(Some(1000.0)), later);
        assert_eq!(step.persist, None, "no position, nothing to write");
        let step = tracker.handle(IpcEvent::TimePos(Some(11.0)), later);
        assert_eq!(step.persist, update(11, false));
    }

    #[test]
    fn position_ticks_are_throttled() {
        // One tick per second of playback, as mpv roughly reports them
        let ticks: Vec<String> = (0..=12).map(|s| time_pos(100.0 + s as f64)).collect();
        let mut lines = vec![(0, DURATION_1000)];
        lines.extend(
            ticks
                .iter()
                .enumerate()
                .map(|(i, l)| (i as u64, l.as_str())),
        );
        let (writes, exit) = run_session(false, &lines);
        let positions: Vec<i64> = writes.iter().map(|u| u.pos).collect();
        // 112 is the final flush when the socket closes
        assert_eq!(positions, vec![100, 105, 110, 112]);
        assert!(writes.iter().all(|u| !u.watched));
        assert!(!exit.play_next);
    }

    #[test]
    fn small_moves_persist_after_five_seconds_of_wall_time() {
        let start = Instant::now();
        let at = |s| start + Duration::from_secs(s);
        let mut tracker = ProgressTracker::new(false, start);
        tracker.handle(IpcEvent::Duration(Some(1000.0)), at(0));
        let step = tracker.handle(IpcEvent::TimePos(Some(100.0)), at(0));
        assert_eq!(step.persist, update(100, false));
        let step = tracker.handle(IpcEvent::TimePos(Some(101.0)), at(4));
        assert_eq!(step.persist, None);
        let step = tracker.handle(IpcEvent::TimePos(Some(102.0)), at(5));
        assert_eq!(step.persist, update(102, false));
        let step = tracker.handle(IpcEvent::TimePos(Some(102.0)), at(60));
        assert_eq!(step.persist, None, "unchanged position is never rewritten");
    }

    #[test]
    fn pause_flushes_held_back_progress() {
        let (p100, p102) = (time_pos(100.0), time_pos(102.0));
        let (writes, exit) = run_session(
            false,
            &[
                (0, DURATION_1000),
                (0, &p100),
                (1, &p102),
                (1, UNPAUSED),
                (2, PAUSED),
                (3, PAUSED),
            ],
        );
        assert_eq!(
            writes,
            vec![update(100, false).unwrap(), update(102, false).unwrap()],
            "unpause writes nothing, a second pause has nothing new"
        );
        assert_eq!(exit.flush, None, "the pause already flushed");
    }

    #[test]
    fn pause_before_any_progress_writes_nothing() {
        let (writes, _) = run_session(false, &[(0, PAUSED), (0, DURATION_1000), (0, PAUSED)]);
        assert!(writes.is_empty());
    }

    #[test]
    fn pause_resets_the_wall_clock_throttle() {
        let start = Instant::now();
        let at = |s| start + Duration::from_secs(s);
        let mut tracker = ProgressTracker::new(false, start);
        tracker.handle(IpcEvent::Duration(Some(1000.0)), at(0));
        tracker.handle(IpcEvent::TimePos(Some(100.0)), at(0));
        tracker.handle(IpcEvent::TimePos(Some(101.0)), at(3));
        let step = tracker.handle(IpcEvent::Pause(true), at(4));
        assert_eq!(step.persist, update(101, false));
        let step = tracker.handle(IpcEvent::TimePos(Some(102.0)), at(6));
        assert_eq!(step.persist, None, "only 2 s since the pause write");
    }

    #[test]
    fn crossing_the_threshold_persists_immediately_and_latches() {
        let lines: Vec<String> = [898.0, 900.0, 100.0, 101.0]
            .into_iter()
            .map(time_pos)
            .collect();
        let (writes, exit) = run_session(
            false,
            &[
                (0, DURATION_1000),
                (0, &lines[0]),
                (1, &lines[1]),
                (2, &lines[2]),
                (3, &lines[3]),
            ],
        );
        assert_eq!(
            writes,
            vec![
                update(898, false).unwrap(),
                // 2 s of position, but watched flipped
                update(900, true).unwrap(),
                // seeking back is ≥ 5 s of position; the latch holds
                update(100, true).unwrap(),
                // final flush
                update(101, true).unwrap(),
            ]
        );
        assert!(!exit.play_next);
    }

    #[test]
    fn already_watched_stays_watched_while_rewatching() {
        let p = time_pos(10.0);
        let (writes, _) = run_session(true, &[(0, DURATION_1000), (0, &p), (1, END_QUIT)]);
        assert_eq!(writes, vec![update(10, true).unwrap()]);
    }

    #[test]
    fn eof_resets_position_and_plays_next_only_on_exit() {
        let start = Instant::now();
        let mut tracker = ProgressTracker::new(false, start);
        tracker.handle(IpcEvent::Duration(Some(1000.0)), start);
        tracker.handle(IpcEvent::TimePos(Some(995.0)), start);
        tracker.handle(IpcEvent::TimePos(Some(997.0)), start);
        let step = tracker.handle(IpcEvent::EndFile(EndReason::Eof), start);
        assert_eq!(
            step,
            TrackerStep {
                persist: update(0, true),
                exit: true,
            },
            "eof writes position 0, watched, and leaves the loop"
        );
        assert_eq!(
            tracker.finish(),
            TrackerExit {
                flush: None,
                play_next: true,
            },
            "play next is only decided once the loop has exited"
        );
    }

    #[test]
    fn eof_session_ignores_lines_after_exit() {
        let (p, late) = (time_pos(500.0), time_pos(42.0));
        let (writes, exit) = run_session(
            false,
            &[
                (0, DURATION_1000),
                (0, &p),
                (1, END_EOF),
                (2, &late),
                (2, END_QUIT),
            ],
        );
        assert_eq!(
            writes,
            vec![update(500, false).unwrap(), update(0, true).unwrap()]
        );
        assert!(exit.play_next);
    }

    #[test]
    fn eof_without_known_progress_still_marks_finished() {
        let (writes, exit) = run_session(false, &[(0, END_EOF)]);
        assert_eq!(writes, vec![update(0, true).unwrap()]);
        assert!(exit.play_next);
    }

    #[test]
    fn quit_error_and_stop_flush_and_never_play_next() {
        let (p100, p102) = (time_pos(100.0), time_pos(102.0));
        for end in [END_QUIT, END_ERROR, END_STOP] {
            let (writes, exit) = run_session(
                false,
                &[(0, DURATION_1000), (0, &p100), (1, &p102), (1, end)],
            );
            assert_eq!(
                writes,
                vec![update(100, false).unwrap(), update(102, false).unwrap()],
                "{end}"
            );
            assert_eq!(exit.flush, update(102, false), "{end}");
            assert!(!exit.play_next, "{end}");
        }
    }

    #[test]
    fn other_end_reasons_keep_tracking() {
        let (p, q) = (time_pos(100.0), time_pos(300.0));
        let (writes, exit) = run_session(
            false,
            &[(0, DURATION_1000), (0, &p), (1, END_REDIRECT), (2, &q)],
        );
        assert_eq!(
            writes,
            vec![update(100, false).unwrap(), update(300, false).unwrap()]
        );
        assert!(!exit.play_next);
    }

    #[test]
    fn read_error_stops_reading_and_flushes_held_back_progress() {
        let ok = |line: &str| Ok(line.to_string());
        let reads = vec![
            (0, ok(DURATION_1000)),
            (0, ok(&time_pos(100.0))),
            (1, ok(&time_pos(101.0))),
            (1, Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))),
            (2, ok(&time_pos(300.0))),
            (2, ok(END_EOF)),
        ];
        let (writes, exit, read) = run_reads(false, reads);
        assert_eq!(read, 4, "nothing is read after the error");
        assert_eq!(
            writes,
            vec![update(100, false).unwrap(), update(101, false).unwrap()]
        );
        assert_eq!(
            exit,
            TrackerExit {
                flush: update(101, false),
                play_next: false,
            }
        );
    }

    #[test]
    fn end_of_input_flushes_held_back_progress() {
        let (p100, p101) = (time_pos(100.0), time_pos(101.0));
        let (writes, exit) = run_session(
            false,
            &[(0, DURATION_1000), (0, &p100), (1, &p101), (1, "garbage")],
        );
        assert_eq!(
            exit,
            TrackerExit {
                flush: update(101, false),
                play_next: false,
            }
        );
        assert_eq!(writes.len(), 2);
    }
}
