use serde::{Serialize, Deserialize};
use std::sync::Mutex;
use std::process::Child;

pub struct AppState {
    pub player_process: Mutex<Option<Child>>,
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

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProgressUpdatePayload {
    pub file_path: String,
    pub last_position: i64,
    pub watched: bool,
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