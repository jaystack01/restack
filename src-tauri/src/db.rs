use crate::models::{ContinueWatchingItem, Episode, GroupInfo, LibraryResponse, MergeSuggestion, MovieItem, OtherItem, ParsedEpisode, SeasonGroup, ShowGroup};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use trash;

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;
    Ok(app_dir.join("restack.db"))
}

pub fn open_db(app: &AppHandle) -> Result<Connection, String> {
    let path = db_path(app)?;
    Connection::open(path).map_err(|e| format!("Failed to open SQLite database: {e}"))
}

pub fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS show_groups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            display_name TEXT NOT NULL,
            normalized_name TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_show_groups_normalized
            ON show_groups(normalized_name);

        CREATE TABLE IF NOT EXISTS app_state (
            key TEXT PRIMARY KEY,
            value_text TEXT,
            updated_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS media (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL UNIQUE,
            show_name TEXT NOT NULL,
            season INTEGER NOT NULL,
            episode INTEGER NOT NULL,
            title TEXT,
            duration INTEGER NOT NULL DEFAULT 0,
            last_position INTEGER NOT NULL DEFAULT 0,
            watched INTEGER NOT NULL DEFAULT 0,
            thumbnail_path TEXT,
            group_id INTEGER,
            user_pinned INTEGER NOT NULL DEFAULT 0,
            deleted_at INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_media_show_season_episode
            ON media(show_name, season, episode);
        CREATE INDEX IF NOT EXISTS idx_media_group_season_episode
            ON media(group_id, season, episode);
        ",
    )
    .map_err(|e| format!("Failed to initialize database schema: {e}"))?;

    // Safe column migrations for pre-existing databases
    let existing_cols: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(media)")
            .map_err(|e| format!("Failed to inspect media table: {e}"))?;
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("Failed to read table_info rows: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        cols
    };

    if !existing_cols.iter().any(|c| c == "group_id") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN group_id INTEGER;")
            .map_err(|e| format!("Failed to add group_id column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "user_pinned") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN user_pinned INTEGER NOT NULL DEFAULT 0;")
            .map_err(|e| format!("Failed to add user_pinned column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "deleted_at") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN deleted_at INTEGER;")
            .map_err(|e| format!("Failed to add deleted_at column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "media_type") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN media_type TEXT NOT NULL DEFAULT 'series';")
            .map_err(|e| format!("Failed to add media_type column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "year") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN year INTEGER;")
            .map_err(|e| format!("Failed to add year column: {e}"))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Normalization & group helpers
// ---------------------------------------------------------------------------

/// Lowercase + strip all non-alphanumeric characters.
pub fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

/// Look up an existing show group by normalized name, or create a new one.
pub fn find_or_create_group(conn: &Connection, show_name: &str) -> Result<i64, String> {
    let normalized = normalize_name(show_name);

    let existing: rusqlite::Result<i64> = conn.query_row(
        "SELECT id FROM show_groups WHERE normalized_name = ?1",
        params![normalized],
        |row| row.get(0),
    );

    match existing {
        Ok(id) => Ok(id),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            conn.execute(
                "INSERT INTO show_groups (display_name, normalized_name) VALUES (?1, ?2)",
                params![show_name, normalized],
            )
            .map_err(|e| format!("Failed to insert show group: {e}"))?;
            Ok(conn.last_insert_rowid())
        }
        Err(e) => Err(format!("Failed to query show_groups: {e}")),
    }
}

/// Backfill `group_id` for all `media` rows that don't have one yet.
pub fn migrate_existing_groups(conn: &Connection) -> Result<(), String> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, show_name FROM media WHERE group_id IS NULL")
            .map_err(|e| format!("Failed to prepare migration query: {e}"))?;

        let collected: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("Failed to read migration rows: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        collected
    };

    for (id, show_name) in rows {
        let group_id = find_or_create_group(conn, &show_name)?;
        conn.execute(
            "UPDATE media SET group_id = ?1 WHERE id = ?2",
            params![group_id, id],
        )
        .map_err(|e| format!("Failed to update group_id for media id {id}: {e}"))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Episode upsert
// ---------------------------------------------------------------------------

pub fn upsert_episode(
    conn: &Connection,
    file_path: &str,
    parsed: &ParsedEpisode,
    duration: i64,
    thumbnail_path: Option<String>,
) -> Result<(), String> {
    let group_id = find_or_create_group(conn, &parsed.show_name)?;

    conn.execute(
        "
        INSERT INTO media (file_path, show_name, season, episode, title, duration, thumbnail_path, group_id, media_type)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'series')
        ON CONFLICT(file_path) DO UPDATE SET
          show_name      = excluded.show_name,
          season         = CASE WHEN media.user_pinned = 1 THEN media.season ELSE excluded.season END,
          episode        = CASE WHEN media.user_pinned = 1 THEN media.episode ELSE excluded.episode END,
          title          = excluded.title,
          duration       = CASE WHEN excluded.duration > 0 THEN excluded.duration ELSE media.duration END,
          thumbnail_path = COALESCE(excluded.thumbnail_path, media.thumbnail_path),
          group_id       = CASE
                             WHEN media.user_pinned = 1 THEN media.group_id
                             WHEN media.group_id IS NULL THEN excluded.group_id
                             ELSE media.group_id
                           END,
          media_type     = CASE WHEN media.user_pinned = 1 THEN media.media_type ELSE 'series' END,
          deleted_at     = NULL
        ",
        params![
            file_path,
            parsed.show_name,
            i64::from(parsed.season),
            i64::from(parsed.episode),
            parsed.title,
            duration,
            thumbnail_path,
            group_id,
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to upsert media row: {e}"))
}

pub fn upsert_movie(
    conn: &Connection,
    file_path: &str,
    title: &str,
    year: Option<u32>,
    duration: i64,
    thumbnail_path: Option<String>,
) -> Result<(), String> {
    conn.execute(
        "
        INSERT INTO media (file_path, show_name, season, episode, title, duration, thumbnail_path, media_type, year)
        VALUES (?1, ?2, 0, 0, ?2, ?3, ?4, 'movie', ?5)
        ON CONFLICT(file_path) DO UPDATE SET
          show_name      = excluded.show_name,
          title          = excluded.title,
          duration       = CASE WHEN excluded.duration > 0 THEN excluded.duration ELSE media.duration END,
          thumbnail_path = COALESCE(excluded.thumbnail_path, media.thumbnail_path),
          media_type     = CASE WHEN media.user_pinned = 1 THEN media.media_type ELSE 'movie' END,
          group_id       = CASE WHEN media.user_pinned = 1 THEN media.group_id ELSE NULL END,
          season         = CASE WHEN media.user_pinned = 1 THEN media.season ELSE 0 END,
          episode        = CASE WHEN media.user_pinned = 1 THEN media.episode ELSE 0 END,
          year           = excluded.year,
          deleted_at     = NULL
        ",
        params![file_path, title, duration, thumbnail_path, year.map(|y| y as i64)],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to upsert movie row: {e}"))
}

pub fn upsert_other(
    conn: &Connection,
    file_path: &str,
    raw_name: &str,
    duration: i64,
    thumbnail_path: Option<String>,
) -> Result<(), String> {
    conn.execute(
        "
        INSERT INTO media (file_path, show_name, season, episode, title, duration, thumbnail_path, media_type)
        VALUES (?1, ?2, 0, 0, ?2, ?3, ?4, 'other')
        ON CONFLICT(file_path) DO UPDATE SET
          show_name      = excluded.show_name,
          title          = excluded.title,
          duration       = CASE WHEN excluded.duration > 0 THEN excluded.duration ELSE media.duration END,
          thumbnail_path = COALESCE(excluded.thumbnail_path, media.thumbnail_path),
          media_type     = CASE WHEN media.user_pinned = 1 THEN media.media_type ELSE 'other' END,
          group_id       = CASE WHEN media.user_pinned = 1 THEN media.group_id ELSE NULL END,
          season         = CASE WHEN media.user_pinned = 1 THEN media.season ELSE 0 END,
          episode        = CASE WHEN media.user_pinned = 1 THEN media.episode ELSE 0 END,
          deleted_at     = NULL
        ",
        params![file_path, raw_name, duration, thumbnail_path],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to upsert other row: {e}"))
}

// ---------------------------------------------------------------------------
// Library query
// ---------------------------------------------------------------------------

pub fn get_grouped_library(conn: &Connection) -> Result<Vec<ShowGroup>, String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT
                m.id,
                m.file_path,
                sg.id          AS group_id,
                sg.display_name,
                m.season,
                m.episode,
                m.title,
                m.last_position,
                m.watched,
                m.duration,
                m.thumbnail_path
            FROM media m
            JOIN show_groups sg ON m.group_id = sg.id
            WHERE m.deleted_at IS NULL AND m.media_type = 'series'
            ORDER BY sg.display_name COLLATE NOCASE, m.season ASC, m.episode ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare library query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,            // id
                row.get::<_, String>(1)?,          // file_path
                row.get::<_, i64>(2)?,             // group_id
                row.get::<_, String>(3)?,          // display_name
                row.get::<_, i64>(4)?,             // season
                row.get::<_, i64>(5)?,             // episode
                row.get::<_, Option<String>>(6)?,  // title
                row.get::<_, i64>(7)?,             // last_position
                row.get::<_, i64>(8)?,             // watched
                row.get::<_, i64>(9)?,             // duration
                row.get::<_, Option<String>>(10)?, // thumbnail_path
            ))
        })
        .map_err(|e| format!("Failed to read library query rows: {e}"))?;

    let mut grouped: BTreeMap<(i64, String), BTreeMap<u32, Vec<Episode>>> = BTreeMap::new();

    for row in rows {
        let (id, file_path, group_id, display_name, season, episode, title, last_position, watched, duration, thumbnail_path) =
            row.map_err(|e| format!("Failed to map query row: {e}"))?;

        let season_u32 = u32::try_from(season).map_err(|_| "Invalid season value".to_string())?;
        let episode_u32 = u32::try_from(episode).map_err(|_| "Invalid episode value".to_string())?;

        let item = Episode {
            id,
            file_path,
            season: season_u32,
            episode: episode_u32,
            title,
            last_position,
            watched: watched != 0,
            duration,
            thumbnail_path,
        };

        grouped
            .entry((group_id, display_name))
            .or_default()
            .entry(season_u32)
            .or_default()
            .push(item);
    }

    let mut response = Vec::new();
    for ((group_id, display_name), seasons_map) in grouped {
        let mut seasons = Vec::new();
        for (season, episodes) in seasons_map {
            seasons.push(SeasonGroup { season, episodes });
        }
        response.push(ShowGroup { group_id, display_name, seasons });
    }

    Ok(response)
}

pub fn get_full_library(conn: &Connection) -> Result<LibraryResponse, String> {
    let series = get_grouped_library(conn)?;

    // Movies
    let movies: Vec<MovieItem> = {
        let mut stmt = conn.prepare(
            "SELECT id, file_path, show_name, year, last_position, watched, duration, thumbnail_path
             FROM media WHERE media_type = 'movie' AND deleted_at IS NULL
             ORDER BY show_name COLLATE NOCASE"
        ).map_err(|e| format!("Failed to prepare movies query: {e}"))?;

        let collected: Vec<MovieItem> = stmt.query_map([], |row| {
            Ok(MovieItem {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get(2)?,
                year: row.get::<_, Option<i64>>(3)?.map(|y| y as u32),
                last_position: row.get(4)?,
                watched: row.get::<_, i64>(5)? != 0,
                duration: row.get(6)?,
                thumbnail_path: row.get(7)?,
            })
        })
        .map_err(|e| format!("Failed to read movie rows: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
        collected
    };

    // Others
    let others: Vec<OtherItem> = {
        let mut stmt = conn.prepare(
            "SELECT id, file_path, show_name, last_position, watched, duration, thumbnail_path
             FROM media WHERE media_type = 'other' AND deleted_at IS NULL
             ORDER BY show_name COLLATE NOCASE"
        ).map_err(|e| format!("Failed to prepare others query: {e}"))?;

        let collected: Vec<OtherItem> = stmt.query_map([], |row| {
            let file_path: String = row.get(1)?;
            let file_name = file_path.split('/').last()
                .or_else(|| file_path.split('\\').last())
                .unwrap_or(&file_path).to_string();
            Ok(OtherItem {
                id: row.get(0)?,
                file_path,
                file_name,
                last_position: row.get(3)?,
                watched: row.get::<_, i64>(4)? != 0,
                duration: row.get(5)?,
                thumbnail_path: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to read other rows: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
        collected
    };

    Ok(LibraryResponse { series, movies, others })
}

// ---------------------------------------------------------------------------
// Group management
// ---------------------------------------------------------------------------

/// Rename a group's display_name only. normalized_name is never changed.
pub fn rename_group(conn: &Connection, group_id: i64, new_name: &str) -> Result<(), String> {
    let changed = conn
        .execute(
            "UPDATE show_groups SET display_name = ?1 WHERE id = ?2",
            params![new_name, group_id],
        )
        .map_err(|e| format!("Failed to rename group: {e}"))?;

    if changed == 0 {
        return Err(format!("No group found with id {group_id}"));
    }
    Ok(())
}

/// Move all episodes from source group → target group, then delete source.
pub fn merge_groups(conn: &Connection, source_id: i64, target_id: i64) -> Result<(), String> {
    if source_id == target_id {
        return Err("Source and target groups must be different".to_string());
    }

    conn.execute(
        "UPDATE media SET group_id = ?1 WHERE group_id = ?2",
        params![target_id, source_id],
    )
    .map_err(|e| format!("Failed to move episodes during merge: {e}"))?;

    conn.execute(
        "DELETE FROM show_groups WHERE id = ?1",
        params![source_id],
    )
    .map_err(|e| format!("Failed to delete source group: {e}"))?;

    Ok(())
}

/// Move specific episodes (by media.id) to a target group and mark them user_pinned.
pub fn move_episodes(conn: &Connection, media_ids: &[i64], target_group_id: i64) -> Result<(), String> {
    for &media_id in media_ids {
        conn.execute(
            "UPDATE media SET group_id = ?1, user_pinned = 1 WHERE id = ?2",
            params![target_group_id, media_id],
        )
        .map_err(|e| format!("Failed to move episode id {media_id}: {e}"))?;
    }
    Ok(())
}
 
/// Reclassify media files between 'series', 'movie', and 'other'.
pub fn reclassify_media(conn: &Connection, media_ids: &[i64], target_type: &str, target_group_id: Option<i64>) -> Result<(), String> {
    for &media_id in media_ids {
        if target_type == "movie" {
            conn.execute(
                "UPDATE media SET media_type = 'movie', group_id = NULL, season = 0, episode = 0, user_pinned = 1 WHERE id = ?1",
                params![media_id],
            )
            .map_err(|e| format!("Failed to reclassify to movie: {e}"))?;
        } else if target_type == "other" {
            conn.execute(
                "UPDATE media SET media_type = 'other', group_id = NULL, season = 0, episode = 0, user_pinned = 1 WHERE id = ?1",
                params![media_id],
            )
            .map_err(|e| format!("Failed to reclassify to other: {e}"))?;
        } else if target_type == "series" {
            if let Some(group_id) = target_group_id {
                let current: (i64, i64) = conn.query_row(
                    "SELECT season, episode FROM media WHERE id = ?1",
                    params![media_id],
                    |row| Ok((row.get(0)?, row.get(1)?))
                ).unwrap_or((1, 0));

                let (season, episode) = current;
                if season == 0 || episode == 0 {
                    let max_ep: i64 = conn.query_row(
                        "SELECT COALESCE(MAX(episode), 0) FROM media WHERE group_id = ?1 AND season = 1 AND deleted_at IS NULL",
                        params![group_id],
                        |row| row.get(0)
                    ).unwrap_or(0);
                    conn.execute(
                        "UPDATE media SET media_type = 'series', group_id = ?1, season = 1, episode = ?2, user_pinned = 1 WHERE id = ?3",
                        params![group_id, max_ep + 1, media_id],
                    )
                    .map_err(|e| format!("Failed to reclassify to series: {e}"))?;
                } else {
                    conn.execute(
                        "UPDATE media SET media_type = 'series', group_id = ?1, user_pinned = 1 WHERE id = ?2",
                        params![group_id, media_id],
                    )
                    .map_err(|e| format!("Failed to reclassify to series: {e}"))?;
                }
            } else {
                return Err("Missing target_group_id for series reclassification".to_string());
            }
        }
    }
    Ok(())
}

/// Return all show groups with their episode counts. Used for the merge/move picker.
pub fn get_all_groups(conn: &Connection) -> Result<Vec<GroupInfo>, String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT sg.id, sg.display_name, COUNT(m.id) AS episode_count
            FROM show_groups sg
            LEFT JOIN media m ON m.group_id = sg.id
            GROUP BY sg.id
            ORDER BY sg.display_name COLLATE NOCASE
            ",
        )
        .map_err(|e| format!("Failed to prepare get_all_groups query: {e}"))?;

    let collected: Vec<GroupInfo> = stmt
        .query_map([], |row| {
            Ok(GroupInfo {
                group_id: row.get(0)?,
                display_name: row.get(1)?,
                episode_count: row.get(2)?,
            })
        })
        .map_err(|e| format!("Failed to read group rows: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(collected)
}

/// Return pairs of groups whose normalized names are within edit distance ≤ 2.
pub fn get_merge_suggestions(conn: &Connection) -> Result<Vec<MergeSuggestion>, String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT sg.id, sg.normalized_name, sg.display_name, COUNT(m.id) AS ep_count
            FROM show_groups sg
            LEFT JOIN media m ON m.group_id = sg.id
            GROUP BY sg.id
            ",
        )
        .map_err(|e| format!("Failed to prepare suggestions query: {e}"))?;

    let groups: Vec<(i64, String, String, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| format!("Failed to read groups for suggestions: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let mut suggestions = Vec::new();

    for i in 0..groups.len() {
        for j in (i + 1)..groups.len() {
            let (id_a, ref norm_a, ref name_a, count_a) = groups[i];
            let (id_b, ref norm_b, ref name_b, count_b) = groups[j];

            let distance = strsim::levenshtein(norm_a, norm_b);
            if distance > 0 && distance <= 2 {
                suggestions.push(MergeSuggestion {
                    group_id_a: id_a,
                    display_name_a: name_a.clone(),
                    episode_count_a: count_a,
                    group_id_b: id_b,
                    display_name_b: name_b.clone(),
                    episode_count_b: count_b,
                });
            }
        }
    }

    Ok(suggestions)
}

// ---------------------------------------------------------------------------
// Progress helpers
// ---------------------------------------------------------------------------

pub fn update_progress(conn: &Connection, file_path: &str, last_position: i64, watched: bool) -> Result<(), String> {
    conn.execute(
        "
        UPDATE media
        SET last_position = ?1, watched = ?2
        WHERE file_path = ?3
        ",
        params![last_position, if watched { 1 } else { 0 }, file_path],
    )
    .map_err(|e| format!("Failed to update progress: {e}"))?;

    conn.execute(
        "
        INSERT INTO app_state (key, value_text, updated_at)
        SELECT 'last_watched_episode_id', CAST(id AS TEXT), CAST(strftime('%s','now') AS INTEGER)
        FROM media
        WHERE file_path = ?1
        LIMIT 1
        ON CONFLICT(key) DO UPDATE SET
            value_text = excluded.value_text,
            updated_at = excluded.updated_at
        ",
        params![file_path],
    )
    .map_err(|e| format!("Failed to set last watched pointer: {e}"))?;

    Ok(())
}

pub fn get_last_position(conn: &Connection, file_path: &str) -> Result<i64, String> {
    let mut stmt = conn
        .prepare("SELECT last_position FROM media WHERE file_path = ?1")
        .map_err(|e| format!("Failed to prepare last_position query: {e}"))?;

    let result: Result<i64, _> = stmt.query_row(params![file_path], |row| row.get(0));
    match result {
        Ok(pos) => Ok(pos),
        Err(_) => Ok(0),
    }
}

pub fn get_episode_meta(conn: &Connection, file_path: &str) -> Option<(i64, u32, u32)> {
    let mut stmt = conn
        .prepare("SELECT group_id, season, episode FROM media WHERE file_path = ?1")
        .ok()?;

    stmt.query_row([file_path], |row| {
        let group_id: i64 = row.get(0)?;
        let season: i64 = row.get(1)?;
        let episode: i64 = row.get(2)?;
        Ok((group_id, season as u32, episode as u32))
    })
    .ok()
}

pub fn get_next_episode(conn: &Connection, group_id: i64, season: u32, episode: u32) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT file_path FROM media
            WHERE group_id = ?1
            AND (
                (season = ?2 AND episode > ?3)
                OR (season > ?2)
            )
            ORDER BY season ASC, episode ASC
            LIMIT 1
            ",
        )
        .ok()?;

    stmt.query_row(params![group_id, season, episode], |row| row.get(0))
        .ok()
}

pub fn get_continue_watching_single(conn: &Connection) -> Result<Option<ContinueWatchingItem>, String> {
    let last_id_text: Option<String> = conn
        .query_row(
            "SELECT value_text FROM app_state WHERE key = 'last_watched_episode_id'",
            [],
            |row| row.get(0),
        )
        .ok();

    let Some(last_id_text) = last_id_text else {
        return Ok(None);
    };

    let Ok(last_id) = last_id_text.parse::<i64>() else {
        let _ = conn.execute("DELETE FROM app_state WHERE key = 'last_watched_episode_id'", []);
        return Ok(None);
    };

    let current = conn.query_row(
        "
        SELECT
            m.id,
            sg.display_name,
            m.file_path,
            m.season,
            m.episode,
            m.last_position,
            m.watched,
            m.duration,
            m.thumbnail_path,
            m.group_id
        FROM media m
        JOIN show_groups sg ON sg.id = m.group_id
        WHERE m.id = ?1 AND m.deleted_at IS NULL
        ",
        params![last_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
            ))
        },
    );

    let Ok((_, display_name, file_path, season, episode, last_position, watched, duration, thumbnail_path, group_id)) =
        current
    else {
        let _ = conn.execute("DELETE FROM app_state WHERE key = 'last_watched_episode_id'", []);
        return Ok(None);
    };

    let season_u32 = u32::try_from(season).map_err(|_| "Invalid season value".to_string())?;
    let episode_u32 = u32::try_from(episode).map_err(|_| "Invalid episode value".to_string())?;

    if watched == 0 {
        return Ok(Some(ContinueWatchingItem {
            mode: "continue".to_string(),
            up_next_type: None,
            display_name,
            file_path,
            season: season_u32,
            episode: episode_u32,
            last_position,
            watched: false,
            duration,
            thumbnail_path,
        }));
    }

    let next = conn.query_row(
        "
        SELECT
            sg.display_name,
            m.file_path,
            m.season,
            m.episode,
            m.last_position,
            m.watched,
            m.duration,
            m.thumbnail_path
        FROM media m
        JOIN show_groups sg ON sg.id = m.group_id
        WHERE m.group_id = ?1 AND m.deleted_at IS NULL
          AND (
            (m.season = ?2 AND m.episode > ?3)
            OR (m.season > ?2)
          )
        ORDER BY m.season ASC, m.episode ASC
        LIMIT 1
        ",
        params![group_id, season, episode],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        },
    );

    let Ok((next_display_name, next_file_path, next_season, next_episode, next_last_position, next_watched, next_duration, next_thumbnail_path)) =
        next
    else {
        return Ok(None);
    };

    let next_season_u32 = u32::try_from(next_season).map_err(|_| "Invalid next season value".to_string())?;
    let next_episode_u32 = u32::try_from(next_episode).map_err(|_| "Invalid next episode value".to_string())?;
    let up_next_type = if next_season == season {
        "next_episode"
    } else {
        "next_season"
    };

    Ok(Some(ContinueWatchingItem {
        mode: "up_next".to_string(),
        up_next_type: Some(up_next_type.to_string()),
        display_name: next_display_name,
        file_path: next_file_path,
        season: next_season_u32,
        episode: next_episode_u32,
        last_position: next_last_position,
        watched: next_watched != 0,
        duration: next_duration,
        thumbnail_path: next_thumbnail_path,
    }))
}

// ---------------------------------------------------------------------------
// Delete and restore episodes
// ---------------------------------------------------------------------------

pub fn delete_episodes(conn: &Connection, file_paths: &[String]) -> Result<(), String> {
    // First, trash the files
    for file_path in file_paths {
        trash::delete(file_path).map_err(|e| format!("Failed to trash file {file_path}: {e}"))?;
    }

    // Then mark as deleted in DB
    let placeholders = file_paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE media SET deleted_at = strftime('%s', 'now') WHERE file_path IN ({})", placeholders);
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("Failed to prepare delete query: {e}"))?;
    let params: Vec<&dyn rusqlite::ToSql> = file_paths.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    stmt.execute(&params[..]).map_err(|e| format!("Failed to mark episodes as deleted: {e}"))?;
    Ok(())
}

pub fn restore_episodes(conn: &Connection, file_paths: &[String]) -> Result<(), String> {
    // Restore from trash by moving back to original location
    let trash_info_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".local/share/Trash/info");
    let trash_files_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".local/share/Trash/files");

    for file_path in file_paths {
        let path = std::path::Path::new(file_path);
        let file_name = path.file_name().ok_or("Invalid file path")?;
        let info_file = trash_info_dir.join(format!("{}.trashinfo", file_name.to_string_lossy()));

        if info_file.exists() {
            // Parse the .trashinfo to get original path
            let content = std::fs::read_to_string(&info_file).map_err(|e| format!("Failed to read trashinfo: {e}"))?;
            let original_path = content.lines()
                .find(|line| line.starts_with("Path="))
                .and_then(|line| line.strip_prefix("Path="))
                .ok_or("Invalid trashinfo format")?;
            let original_path = urlencoding::decode(original_path).map_err(|e| format!("Failed to decode path: {e}"))?;
            let original_path = std::path::Path::new(&*original_path);

            let trashed_file = trash_files_dir.join(file_name);
            std::fs::rename(&trashed_file, original_path).map_err(|e| format!("Failed to restore file: {e}"))?;
            std::fs::remove_file(&info_file).ok(); // Remove the info file
        } else {
            return Err(format!("Trash info not found for {file_path}"));
        }
    }

    // Then mark as not deleted in DB
    let placeholders = file_paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE media SET deleted_at = NULL WHERE file_path IN ({})", placeholders);
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("Failed to prepare restore query: {e}"))?;
    let params: Vec<&dyn rusqlite::ToSql> = file_paths.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    stmt.execute(&params[..]).map_err(|e| format!("Failed to restore episodes: {e}"))?;
    Ok(())
}