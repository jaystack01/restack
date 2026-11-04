use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::process::Child;

pub struct AppState {
    pub player_process: Mutex<Option<Child>>,
    /// Set while a library scan runs; see `ScanGuard`.
    pub scan_running: AtomicBool,
}

/// Holds the "scan running" flag; clears it on drop, so the flag is released on
/// every exit path (Ok, Err via `?`, or a panic unwinding the scan).
pub struct ScanGuard<'a>(&'a AtomicBool);

impl<'a> ScanGuard<'a> {
    /// Claim the flag, or fail if another scan holds it.
    pub fn try_acquire(flag: &'a AtomicBool) -> Result<Self, String> {
        flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| ScanGuard(flag))
            .map_err(|_| "A scan is already running".to_string())
    }
}

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub id: i64,
    pub file_path: String,
    pub season: u32,
    pub episode: u32,
    pub title: Option<String>,
    pub last_position: i64,
    pub watched: bool,
    pub duration: i64,
    pub thumbnail_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonGroup {
    pub season: u32,
    pub episodes: Vec<Episode>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowGroup {
    pub group_id: i64,
    pub display_name: String,
    pub seasons: Vec<SeasonGroup>,
}

pub struct ParsedEpisode {
    pub show_name: String,
    pub season: u32,
    pub episode: u32,
    pub title: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub group_id: i64,
    pub display_name: String,
    pub episode_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeSuggestion {
    pub group_id_a: i64,
    pub display_name_a: String,
    pub episode_count_a: i64,
    pub group_id_b: i64,
    pub display_name_b: String,
    pub episode_count_b: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueWatchingItem {
    pub mode: String,
    pub up_next_type: Option<String>,
    pub display_name: String,
    pub file_path: String,
    pub season: u32,
    pub episode: u32,
    pub last_position: i64,
    pub watched: bool,
    pub duration: i64,
    pub thumbnail_path: Option<String>,
}

// ---------------------------------------------------------------------------
// New types for Movies / Others
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MovieItem {
    pub id: i64,
    pub file_path: String,
    pub title: String,
    pub year: Option<u32>,
    pub last_position: i64,
    pub watched: bool,
    pub duration: i64,
    pub thumbnail_path: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OtherItem {
    pub id: i64,
    pub file_path: String,
    pub file_name: String,
    pub last_position: i64,
    pub watched: bool,
    pub duration: i64,
    pub thumbnail_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryResponse {
    pub series: Vec<ShowGroup>,
    pub movies: Vec<MovieItem>,
    pub others: Vec<OtherItem>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    #[test]
    fn scan_guard_is_exclusive_and_released_on_drop() {
        let flag = AtomicBool::new(false);
        let guard = ScanGuard::try_acquire(&flag).unwrap();
        assert_eq!(
            ScanGuard::try_acquire(&flag).err().as_deref(),
            Some("A scan is already running")
        );
        drop(guard);
        assert!(!flag.load(Ordering::SeqCst));
        assert!(ScanGuard::try_acquire(&flag).is_ok());
    }

    #[test]
    fn scan_guard_released_on_error_and_panic() {
        let flag = AtomicBool::new(false);
        let failing_scan = || -> Result<(), String> {
            let _guard = ScanGuard::try_acquire(&flag)?;
            Err("Failed to scan: boom".to_string())
        };
        assert!(failing_scan().is_err());
        assert!(!flag.load(Ordering::SeqCst));

        let panicked = catch_unwind(AssertUnwindSafe(|| {
            let _guard = ScanGuard::try_acquire(&flag).unwrap();
            panic!("scan panicked");
        }));
        assert!(panicked.is_err());
        assert!(!flag.load(Ordering::SeqCst));
    }
}
