use crate::models::{
    ContinueWatchingItem, Episode, GroupInfo, LibraryResponse, MergeSuggestion, MovieItem,
    OtherItem, ParsedEpisode, SeasonGroup, ShowGroup,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;
    Ok(app_dir.join("restack.db"))
}

/// How long a connection waits for another connection's write lock before failing with
/// `SQLITE_BUSY`. Scan batches hold the lock for at most ~1 s, which leaves a wide margin
/// for the mpv tracker and UI commands writing at the same time.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub fn open_db(app: &AppHandle) -> Result<Connection, String> {
    let path = db_path(app)?;
    let conn =
        Connection::open(path).map_err(|e| format!("Failed to open SQLite database: {e}"))?;
    configure_connection(&conn)?;
    Ok(conn)
}

/// Settings every app connection needs: an explicit busy timeout, and WAL so readers never
/// block on the writer and batch commits skip the rollback-journal fsync. WAL is stored in
/// the file, so setting it again on each open is a cheap no-op.
fn configure_connection(conn: &Connection) -> Result<(), String> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| format!("Failed to set SQLite busy timeout: {e}"))?;
    // WAL is an optimisation, never a reason to refuse a DB: on a read-only file or
    // directory the pragma errors, but reads in the old journal mode still work.
    // SQLite also keeps the old mode where WAL isn't supported; in-memory databases
    // always report "memory"
    match conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0)) {
        Ok(mode) if mode.eq_ignore_ascii_case("wal") || mode.eq_ignore_ascii_case("memory") => {}
        Ok(mode) => eprintln!("SQLite journal mode is {mode}, not wal"),
        Err(e) => eprintln!("Failed to enable WAL journal mode, keeping the current one: {e}"),
    }
    Ok(())
}

/// Begin a write transaction that takes the write lock up front (`BEGIN IMMEDIATE`). Use it
/// for every transaction that reads and then writes. A deferred transaction starts as a
/// reader; in WAL, if another connection commits before its first write, the upgrade fails
/// at once with `SQLITE_BUSY(_SNAPSHOT)` and the busy timeout never applies. An immediate
/// one waits for the lock (up to the busy timeout) and then reads current data.
/// Like `unchecked_transaction`, it takes `&Connection`; savepoints nest inside it.
pub fn begin_write(conn: &Connection) -> rusqlite::Result<Transaction<'_>> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
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

        -- Extra normalized names that resolve to a group (e.g. a merged-away source)
        CREATE TABLE IF NOT EXISTS show_group_aliases (
            normalized_name TEXT PRIMARY KEY,
            group_id INTEGER NOT NULL
        );

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
        conn.execute_batch(
            "ALTER TABLE media ADD COLUMN media_type TEXT NOT NULL DEFAULT 'series';",
        )
        .map_err(|e| format!("Failed to add media_type column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "year") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN year INTEGER;")
            .map_err(|e| format!("Failed to add year column: {e}"))?;
    }

    if !existing_cols.iter().any(|c| c == "missing_since") {
        conn.execute_batch("ALTER TABLE media ADD COLUMN missing_since INTEGER;")
            .map_err(|e| format!("Failed to add missing_since column: {e}"))?;
    }

    // Created after the migrations so pre-`group_id` databases can open
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_media_group_season_episode ON media(group_id, season, episode);",
    )
    .map_err(|e| format!("Failed to create group index: {e}"))?;

    // Live-row filter used by every library query (`media_type = ? AND deleted_at IS NULL
    // AND missing_since IS NULL`); also after the migrations, since all three are added columns
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_media_type_live ON media(media_type, deleted_at, missing_since);",
    )
    .map_err(|e| format!("Failed to create media type index: {e}"))?;

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

/// Resolve a show name to an existing group without creating one:
/// an alias (e.g. a merged-away name) wins, then the group's own normalized name.
pub fn find_group(conn: &Connection, show_name: &str) -> Result<Option<i64>, String> {
    let normalized = normalize_name(show_name);

    // Join so an alias left pointing at a deleted group is ignored
    let aliased: Option<i64> = conn
        .query_row(
            "SELECT a.group_id FROM show_group_aliases a
             JOIN show_groups sg ON sg.id = a.group_id
             WHERE a.normalized_name = ?1",
            params![normalized],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to query show_group_aliases: {e}"))?;
    if aliased.is_some() {
        return Ok(aliased);
    }

    conn.query_row(
        "SELECT id FROM show_groups WHERE normalized_name = ?1",
        params![normalized],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("Failed to query show_groups: {e}"))
}

/// Look up an existing show group (aliases first, then normalized name), or create a new one.
pub fn find_or_create_group(conn: &Connection, show_name: &str) -> Result<i64, String> {
    if let Some(id) = find_group(conn, show_name)? {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO show_groups (display_name, normalized_name) VALUES (?1, ?2)",
        params![show_name, normalize_name(show_name)],
    )
    .map_err(|e| format!("Failed to insert show group: {e}"))?;
    Ok(conn.last_insert_rowid())
}

/// One-time startup migration: backfill `group_id` for series rows that lack one,
/// and undo earlier runs that gave movies/others phantom groups.
pub fn migrate_existing_groups(conn: &Connection) -> Result<(), String> {
    let done: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE key = 'migration_groups_v1')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to read migration state: {e}"))?;
    if done {
        return Ok(());
    }

    let tx = begin_write(conn).map_err(|e| format!("Failed to start group migration: {e}"))?;

    let rows: Vec<(i64, String)> = {
        let mut stmt = tx
            .prepare(
                "SELECT id, show_name FROM media WHERE group_id IS NULL AND media_type = 'series'",
            )
            .map_err(|e| format!("Failed to prepare migration query: {e}"))?;
        let collected = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Failed to read migration rows: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read migration rows: {e}"))?;
        collected
    };

    for (id, show_name) in rows {
        let group_id = find_or_create_group(&tx, &show_name)?;
        tx.execute(
            "UPDATE media SET group_id = ?1 WHERE id = ?2",
            params![group_id, id],
        )
        .map_err(|e| format!("Failed to update group_id for media id {id}: {e}"))?;
    }

    tx.execute_batch(
        "
        UPDATE media SET group_id = NULL WHERE media_type != 'series' AND group_id IS NOT NULL;
        DELETE FROM show_groups
            WHERE id NOT IN (SELECT DISTINCT group_id FROM media WHERE group_id IS NOT NULL);
        INSERT INTO app_state (key, value_text, updated_at)
            VALUES ('migration_groups_v1', 'done', CAST(strftime('%s','now') AS INTEGER));
        ",
    )
    .map_err(|e| format!("Failed to clean up phantom groups: {e}"))?;

    tx.commit()
        .map_err(|e| format!("Failed to commit group migration: {e}"))
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
    // Existing row's (group_id, user_pinned), if any
    let existing: Option<(Option<i64>, bool)> = conn
        .query_row(
            "SELECT group_id, user_pinned != 0 FROM media WHERE file_path = ?1",
            params![file_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Failed to read existing media row: {e}"))?;

    // Resolve (or create) a group only for rows that need one. Doing it for every
    // rescanned file would recreate groups that were merged away (BUG-13). Rows whose
    // parsed show changed keep their group here; `rekey_renamed_shows` deals with them
    // once the whole scan is upserted (BUG-41).
    let seen_group = existing.and_then(|(group_id, _)| group_id);
    let group_id = match existing {
        None | Some((None, false)) => Some(find_or_create_group(conn, &parsed.show_name)?),
        Some((group_id, _)) => group_id,
    };

    write_episode(
        conn,
        file_path,
        parsed,
        duration,
        thumbnail_path,
        group_id,
        seen_group,
    )
}

/// The upsert itself. `seen_group` (`?9`) is the row's group when it was read: the row
/// only takes `group_id` if nothing changed its group in between, and pinned rows
/// always keep theirs.
fn write_episode(
    conn: &Connection,
    file_path: &str,
    parsed: &ParsedEpisode,
    duration: i64,
    thumbnail_path: Option<String>,
    group_id: Option<i64>,
    seen_group: Option<i64>,
) -> Result<(), String> {
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
                             WHEN media.group_id IS ?9 THEN COALESCE(excluded.group_id, media.group_id)
                             ELSE media.group_id
                           END,
          media_type     = CASE WHEN media.user_pinned = 1 THEN media.media_type ELSE 'series' END,
          deleted_at     = NULL,
          missing_since  = NULL
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
            seen_group,
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
          deleted_at     = NULL,
          missing_since  = NULL
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
          deleted_at     = NULL,
          missing_since  = NULL
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
            WHERE m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'
            ORDER BY sg.display_name COLLATE NOCASE, m.season ASC, m.episode ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare library query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,             // id
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
        let (
            id,
            file_path,
            group_id,
            display_name,
            season,
            episode,
            title,
            last_position,
            watched,
            duration,
            thumbnail_path,
        ) = row.map_err(|e| format!("Failed to map query row: {e}"))?;

        let season_u32 = u32::try_from(season).map_err(|_| "Invalid season value".to_string())?;
        let episode_u32 =
            u32::try_from(episode).map_err(|_| "Invalid episode value".to_string())?;

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
        response.push(ShowGroup {
            group_id,
            display_name,
            seasons,
        });
    }

    Ok(response)
}

pub fn get_full_library(conn: &Connection) -> Result<LibraryResponse, String> {
    let series = get_grouped_library(conn)?;

    // Movies
    let movies: Vec<MovieItem> = {
        let mut stmt = conn.prepare(
            "SELECT id, file_path, show_name, year, last_position, watched, duration, thumbnail_path
             FROM media WHERE media_type = 'movie' AND deleted_at IS NULL AND missing_since IS NULL
             ORDER BY show_name COLLATE NOCASE"
        ).map_err(|e| format!("Failed to prepare movies query: {e}"))?;

        let collected: Vec<MovieItem> = stmt
            .query_map([], |row| {
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
    let others: Vec<OtherItem> =
        {
            let mut stmt = conn.prepare(
            "SELECT id, file_path, show_name, last_position, watched, duration, thumbnail_path
             FROM media WHERE media_type = 'other' AND deleted_at IS NULL AND missing_since IS NULL
             ORDER BY show_name COLLATE NOCASE"
        ).map_err(|e| format!("Failed to prepare others query: {e}"))?;

            let collected: Vec<OtherItem> = stmt
                .query_map([], |row| {
                    let file_path: String = row.get(1)?;
                    let file_name = file_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file_path)
                        .to_string();
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

    Ok(LibraryResponse {
        series,
        movies,
        others,
    })
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
/// The source's normalized name (and any alias that pointed at it) becomes an alias
/// of the target, so later scans file those names under the target. Aliases follow
/// chains: merging A into B and then B into C leaves both "a" and "b" pointing at C.
/// An alias name never matches an existing group's key (lookups resolve it first),
/// so the source-name insert can't collide; OR IGNORE keeps any alias that already exists.
pub fn merge_groups(conn: &Connection, source_id: i64, target_id: i64) -> Result<(), String> {
    if source_id == target_id {
        return Err("Source and target groups must be different".to_string());
    }

    let tx = begin_write(conn).map_err(|e| format!("Failed to start merge: {e}"))?;

    let source_name: String = tx
        .query_row(
            "SELECT normalized_name FROM show_groups WHERE id = ?1",
            params![source_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to read source group: {e}"))?
        .ok_or_else(|| format!("No group found with id {source_id}"))?;
    let target_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM show_groups WHERE id = ?1)",
            params![target_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to read target group: {e}"))?;
    if !target_exists {
        return Err(format!("No group found with id {target_id}"));
    }

    tx.execute(
        "UPDATE media SET group_id = ?1 WHERE group_id = ?2",
        params![target_id, source_id],
    )
    .map_err(|e| format!("Failed to move episodes during merge: {e}"))?;

    tx.execute(
        "UPDATE show_group_aliases SET group_id = ?1 WHERE group_id = ?2",
        params![target_id, source_id],
    )
    .map_err(|e| format!("Failed to re-point aliases during merge: {e}"))?;
    tx.execute(
        "INSERT OR IGNORE INTO show_group_aliases (normalized_name, group_id) VALUES (?1, ?2)",
        params![source_name, target_id],
    )
    .map_err(|e| format!("Failed to record merge alias: {e}"))?;

    tx.execute("DELETE FROM show_groups WHERE id = ?1", params![source_id])
        .map_err(|e| format!("Failed to delete source group: {e}"))?;
    delete_empty_groups(&tx)?;

    tx.commit()
        .map_err(|e| format!("Failed to commit merge: {e}"))
}

/// Move specific episodes (by media.id) to a target group and mark them user_pinned.
pub fn move_episodes(
    conn: &Connection,
    media_ids: &[i64],
    target_group_id: i64,
) -> Result<(), String> {
    let tx = begin_write(conn).map_err(|e| format!("Failed to start move: {e}"))?;
    for &media_id in media_ids {
        tx.execute(
            "UPDATE media SET group_id = ?1, user_pinned = 1 WHERE id = ?2",
            params![target_group_id, media_id],
        )
        .map_err(|e| format!("Failed to move episode id {media_id}: {e}"))?;
    }
    delete_empty_groups(&tx)?;
    tx.commit()
        .map_err(|e| format!("Failed to commit move: {e}"))
}

/// Reclassify media files between 'series', 'movie', and 'other'.
pub fn reclassify_media(
    conn: &Connection,
    media_ids: &[i64],
    target_type: &str,
    target_group_id: Option<i64>,
) -> Result<(), String> {
    let tx = begin_write(conn).map_err(|e| format!("Failed to start reclassify: {e}"))?;
    for &media_id in media_ids {
        if target_type == "movie" {
            tx.execute(
                "UPDATE media SET media_type = 'movie', group_id = NULL, season = 0, episode = 0, user_pinned = 1 WHERE id = ?1",
                params![media_id],
            )
            .map_err(|e| format!("Failed to reclassify to movie: {e}"))?;
        } else if target_type == "other" {
            tx.execute(
                "UPDATE media SET media_type = 'other', group_id = NULL, season = 0, episode = 0, user_pinned = 1 WHERE id = ?1",
                params![media_id],
            )
            .map_err(|e| format!("Failed to reclassify to other: {e}"))?;
        } else if target_type == "series" {
            if let Some(group_id) = target_group_id {
                let current: (i64, i64) = tx
                    .query_row(
                        "SELECT season, episode FROM media WHERE id = ?1",
                        params![media_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap_or((1, 0));

                let (season, episode) = current;
                if season == 0 || episode == 0 {
                    let max_ep: i64 = tx.query_row(
                        "SELECT COALESCE(MAX(episode), 0) FROM media WHERE group_id = ?1 AND season = 1 AND deleted_at IS NULL",
                        params![group_id],
                        |row| row.get(0)
                    ).unwrap_or(0);
                    tx.execute(
                        "UPDATE media SET media_type = 'series', group_id = ?1, season = 1, episode = ?2, user_pinned = 1 WHERE id = ?3",
                        params![group_id, max_ep + 1, media_id],
                    )
                    .map_err(|e| format!("Failed to reclassify to series: {e}"))?;
                } else {
                    tx.execute(
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
    delete_empty_groups(&tx)?;
    tx.commit()
        .map_err(|e| format!("Failed to commit reclassify: {e}"))
}

/// Delete groups that no `media` row references, and aliases left pointing at them.
/// Counts every row, including soft-deleted and missing ones: those must keep their
/// group so Undo or a remounted drive brings them back where they were.
pub fn delete_empty_groups(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        DELETE FROM show_groups
            WHERE NOT EXISTS (SELECT 1 FROM media WHERE media.group_id = show_groups.id);
        DELETE FROM show_group_aliases
            WHERE group_id NOT IN (SELECT id FROM show_groups);
        ",
    )
    .map_err(|e| format!("Failed to delete empty groups: {e}"))
}

/// Return all show groups with their episode counts. Used for the merge/move picker.
pub fn get_all_groups(conn: &Connection) -> Result<Vec<GroupInfo>, String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT sg.id, sg.display_name, COUNT(m.id) AS episode_count
            FROM show_groups sg
            LEFT JOIN media m ON m.group_id = sg.id
                AND m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'
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
                AND m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'
            GROUP BY sg.id
            HAVING COUNT(m.id) > 0
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

pub fn update_progress(
    conn: &Connection,
    file_path: &str,
    last_position: i64,
    watched: bool,
) -> Result<(), String> {
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
        WHERE file_path = ?1 AND media_type = 'series'
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

/// `(last_position, duration, watched)` for a file, or `None` if it isn't indexed.
pub fn get_playback_state(
    conn: &Connection,
    file_path: &str,
) -> Result<Option<(i64, i64, bool)>, String> {
    conn.query_row(
        "SELECT last_position, duration, watched FROM media WHERE file_path = ?1",
        params![file_path],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()
    .map_err(|e| format!("Failed to read playback state: {e}"))
}

/// Manual mark watched/unwatched. Resets the position so the next play starts
/// from the beginning, and doesn't move the Continue Watching pointer.
pub fn set_watched(conn: &Connection, media_ids: &[i64], watched: bool) -> Result<(), String> {
    let tx = begin_write(conn).map_err(|e| format!("Failed to start watched update: {e}"))?;
    for &media_id in media_ids {
        tx.execute(
            "UPDATE media SET watched = ?1, last_position = 0 WHERE id = ?2",
            params![watched, media_id],
        )
        .map_err(|e| format!("Failed to update watched state for media id {media_id}: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("Failed to commit watched update: {e}"))
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

pub fn get_next_episode(
    conn: &Connection,
    group_id: i64,
    season: u32,
    episode: u32,
) -> Option<String> {
    let mut stmt = conn
        .prepare(
            "
            SELECT file_path FROM media
            WHERE group_id = ?1
            AND deleted_at IS NULL AND missing_since IS NULL AND media_type = 'series'
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

pub fn get_continue_watching_single(
    conn: &Connection,
) -> Result<Option<ContinueWatchingItem>, String> {
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
        let _ = conn.execute(
            "DELETE FROM app_state WHERE key = 'last_watched_episode_id'",
            [],
        );
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
        WHERE m.id = ?1 AND m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'
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

    let Ok((
        _,
        display_name,
        file_path,
        season,
        episode,
        last_position,
        watched,
        duration,
        thumbnail_path,
        group_id,
    )) = current
    else {
        // Keep the pointer while the file is only temporarily missing (e.g. unplugged drive)
        let is_missing = conn
            .query_row(
                "SELECT missing_since IS NOT NULL FROM media WHERE id = ?1",
                params![last_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        if !is_missing {
            let _ = conn.execute(
                "DELETE FROM app_state WHERE key = 'last_watched_episode_id'",
                [],
            );
        }
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
        WHERE m.group_id = ?1 AND m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'
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

    let Ok((
        next_display_name,
        next_file_path,
        next_season,
        next_episode,
        next_last_position,
        next_watched,
        next_duration,
        next_thumbnail_path,
    )) = next
    else {
        return Ok(None);
    };

    let next_season_u32 =
        u32::try_from(next_season).map_err(|_| "Invalid next season value".to_string())?;
    let next_episode_u32 =
        u32::try_from(next_episode).map_err(|_| "Invalid next episode value".to_string())?;
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

pub fn mark_deleted(conn: &Connection, file_path: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE media SET deleted_at = strftime('%s', 'now') WHERE file_path = ?1",
        params![file_path],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to mark episode as deleted: {e}"))
}

pub fn mark_restored(conn: &Connection, file_path: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE media SET deleted_at = NULL WHERE file_path = ?1",
        params![file_path],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to mark episode as restored: {e}"))
}

/// A Trash operation requested by the frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrashAction {
    /// Move live rows' files to the Trash.
    Delete,
    /// Bring soft-deleted rows' files back from the Trash.
    Restore,
}

impl TrashAction {
    pub fn verb(self) -> &'static str {
        match self {
            TrashAction::Delete => "delete",
            TrashAction::Restore => "restore",
        }
    }
}

/// Resolve media ids to the file paths a Trash operation may touch, in request order with
/// duplicates dropped. The webview only ever names ids, so nothing outside `media` can be
/// trashed. Delete accepts live rows only, restore soft-deleted rows only; if any id is
/// unknown or in the wrong state the whole request is refused before a file is moved.
pub fn resolve_trash_paths(
    conn: &Connection,
    media_ids: &[i64],
    action: TrashAction,
) -> Result<Vec<String>, String> {
    let sql = match action {
        TrashAction::Delete => {
            "SELECT file_path FROM media
             WHERE id = ?1 AND deleted_at IS NULL AND missing_since IS NULL"
        }
        TrashAction::Restore => {
            "SELECT file_path FROM media WHERE id = ?1 AND deleted_at IS NOT NULL"
        }
    };
    let verb = action.verb();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to prepare {verb} lookup: {e}"))?;

    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    let mut rejected = Vec::new();
    for &id in media_ids {
        if !seen.insert(id) {
            continue;
        }
        let path: Option<String> = stmt
            .query_row(params![id], |r| r.get(0))
            .optional()
            .map_err(|e| format!("Failed to look up media {id}: {e}"))?;
        match path {
            Some(p) => paths.push(p),
            None => rejected.push(id),
        }
    }

    if !rejected.is_empty() {
        let state = match action {
            TrashAction::Delete => "in the library",
            TrashAction::Restore => "in the Trash",
        };
        return Err(format!(
            "Failed to {verb}: media {rejected:?} not found or not {state}"
        ));
    }
    Ok(paths)
}

// ---------------------------------------------------------------------------
// Scan sync helpers
// ---------------------------------------------------------------------------

/// Every known path, plus whether the row is soft-deleted (trashed by the user).
pub fn get_media_paths(conn: &Connection) -> Result<Vec<(String, bool)>, String> {
    let mut stmt = conn
        .prepare("SELECT file_path, deleted_at IS NOT NULL FROM media")
        .map_err(|e| format!("Failed to prepare media paths query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .map_err(|e| format!("Failed to query media paths: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read media paths: {e}"))
}

/// A group whose rows now parse to a shorter show name than its key (BUG-41).
struct Rekey {
    group_id: i64,
    old_key: String,
    display_name: String,
    new_key: String,
    new_display: String,
}

fn normalized_stem(file_path: &str) -> String {
    let stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_path);
    normalize_name(stem)
}

/// Whether a group's display name differs from its key, i.e. the user renamed it.
fn is_renamed(display_name: &str, key: &str) -> bool {
    normalize_name(display_name) != key
}

/// The parser-change signature for group `id` (key `old_key`): live rows whose parsed
/// name N is a strict prefix of the key (the parser now strips a trailing year or
/// `s01`) and whose own file name contains the key, so the key came from that file.
/// Rows under other names (e.g. a merge from before aliases) are ignored. All
/// signature rows must agree on N.
fn rekey_candidate(
    conn: &Connection,
    id: i64,
    old_key: &str,
    display_name: &str,
) -> Result<Option<Rekey>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT file_path, show_name FROM media
             WHERE group_id = ?1 AND media_type = 'series'
               AND deleted_at IS NULL AND missing_since IS NULL
             ORDER BY id",
        )
        .map_err(|e| format!("Failed to prepare group rows query: {e}"))?;
    let rows = stmt
        .query_map(params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("Failed to read group rows: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read group rows: {e}"))?;

    let mut new_name: Option<(String, String)> = None;
    for (file_path, show_name) in rows {
        let key = normalize_name(&show_name);
        if key.is_empty()
            || key == old_key
            || !old_key.starts_with(&key)
            || !normalized_stem(&file_path).contains(old_key)
        {
            continue;
        }
        match &new_name {
            None => new_name = Some((key, show_name)),
            Some((k, _)) if *k == key => {}
            Some(_) => return Ok(None),
        }
    }

    Ok(new_name.map(|(new_key, new_display)| Rekey {
        group_id: id,
        old_key: old_key.to_string(),
        display_name: display_name.to_string(),
        new_key,
        new_display,
    }))
}

/// Whether existing group `h` holds only the newer episodes of `c`'s show: no pinned
/// rows, not renamed by the user, and every row's file (any state) carries `c`'s old key.
fn can_absorb(conn: &Connection, h: i64, c: &Rekey) -> Result<bool, String> {
    let (key, display, pinned): (String, String, bool) = conn
        .query_row(
            "SELECT normalized_name, display_name,
                    EXISTS(SELECT 1 FROM media WHERE group_id = sg.id AND user_pinned = 1)
             FROM show_groups sg WHERE id = ?1",
            params![h],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| format!("Failed to read group {h}: {e}"))?;
    if pinned || is_renamed(&display, &key) {
        return Ok(false);
    }
    let mut stmt = conn
        .prepare("SELECT file_path FROM media WHERE group_id = ?1")
        .map_err(|e| format!("Failed to prepare group paths query: {e}"))?;
    let paths = stmt
        .query_map(params![h], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to read group paths: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read group paths: {e}"))?;
    Ok(paths
        .iter()
        .all(|p| normalized_stem(p).contains(&c.old_key)))
}

/// Re-key group `c` to its new name in place (same id, so routes and progress stay),
/// after absorbing `absorb` if given. The old key becomes an alias of the group, and
/// a user-chosen display name is kept.
fn apply_rekey(conn: &Connection, c: &Rekey, absorb: Option<i64>) -> Result<(), String> {
    let g = c.group_id;
    if let Some(h) = absorb {
        let h_key: String = conn
            .query_row(
                "SELECT normalized_name FROM show_groups WHERE id = ?1",
                params![h],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to read group {h}: {e}"))?;
        conn.execute(
            "UPDATE media SET group_id = ?1 WHERE group_id = ?2",
            params![g, h],
        )
        .map_err(|e| format!("Failed to move episodes into re-keyed group: {e}"))?;
        conn.execute(
            "UPDATE show_group_aliases SET group_id = ?1 WHERE group_id = ?2",
            params![g, h],
        )
        .map_err(|e| format!("Failed to re-point aliases: {e}"))?;
        conn.execute("DELETE FROM show_groups WHERE id = ?1", params![h])
            .map_err(|e| format!("Failed to delete absorbed group: {e}"))?;
        if h_key != c.new_key {
            conn.execute(
                "INSERT OR IGNORE INTO show_group_aliases (normalized_name, group_id) VALUES (?1, ?2)",
                params![h_key, g],
            )
            .map_err(|e| format!("Failed to alias absorbed group: {e}"))?;
        }
    }

    let display = if is_renamed(&c.display_name, &c.old_key) {
        c.display_name.as_str()
    } else {
        c.new_display.as_str()
    };
    conn.execute(
        "DELETE FROM show_group_aliases WHERE normalized_name = ?1",
        params![c.new_key],
    )
    .map_err(|e| format!("Failed to clear alias: {e}"))?;
    conn.execute(
        "UPDATE show_groups SET normalized_name = ?1, display_name = ?2 WHERE id = ?3",
        params![c.new_key, display, g],
    )
    .map_err(|e| format!("Failed to re-key group: {e}"))?;
    conn.execute(
        "INSERT OR REPLACE INTO show_group_aliases (normalized_name, group_id) VALUES (?1, ?2)",
        params![c.old_key, g],
    )
    .map_err(|e| format!("Failed to alias old group key: {e}"))?;
    Ok(())
}

/// End-of-scan pass for shows whose parsed name changed under a parser fix (BUG-41),
/// e.g. "Doctor.Who.2005.S01E01" once parsed as "Doctor Who 2005", now "Doctor Who".
/// Runs on the rows' freshly upserted `show_name`s, so the outcome doesn't depend on
/// walk order or batch boundaries. It never moves rows between two distinct shows:
///
/// - Only unpinned groups showing the parser-change signature (`rekey_candidate`).
/// - New name unknown: re-key the group in place, if no other group claims that name.
/// - New name is an existing group H: absorb H only if exactly one candidate can
///   (`can_absorb`), so "Doctor Who 1963" and "Doctor Who 2005" stay separate.
fn rekey_renamed_shows(conn: &Connection) -> Result<(), String> {
    let groups: Vec<(i64, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, normalized_name, display_name FROM show_groups sg
                 WHERE NOT EXISTS (SELECT 1 FROM media WHERE group_id = sg.id AND user_pinned = 1)
                 ORDER BY id",
            )
            .map_err(|e| format!("Failed to prepare re-key query: {e}"))?;
        let collected = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| format!("Failed to read groups for re-key: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read groups for re-key: {e}"))?;
        collected
    };

    let mut by_key: BTreeMap<String, Vec<Rekey>> = BTreeMap::new();
    for (id, key, display) in &groups {
        if let Some(c) = rekey_candidate(conn, *id, key, display)? {
            by_key.entry(c.new_key.clone()).or_default().push(c);
        }
    }
    let candidate_ids: HashSet<i64> = by_key.values().flatten().map(|c| c.group_id).collect();

    // Decide everything before changing anything
    let mut plans: Vec<(&Rekey, Option<i64>)> = Vec::new();
    let mut absorbed: HashSet<i64> = HashSet::new();
    for (new_key, claims) in &by_key {
        match find_group(conn, new_key)? {
            None if claims.len() == 1 => plans.push((&claims[0], None)),
            None => eprintln!(
                "Not regrouping {} groups that now all parse as \"{}\"",
                claims.len(),
                claims[0].new_display
            ),
            Some(h) if candidate_ids.contains(&h) || absorbed.contains(&h) => {}
            Some(h) => {
                let mut eligible = Vec::new();
                for c in claims {
                    if can_absorb(conn, h, c)? {
                        eligible.push(c);
                    }
                }
                if let [c] = eligible.as_slice() {
                    absorbed.insert(h);
                    plans.push((c, Some(h)));
                } else {
                    eprintln!(
                        "Not regrouping \"{}\": \"{}\" is already a separate show",
                        claims[0].display_name, claims[0].new_display
                    );
                }
            }
        }
    }

    for (c, absorb) in plans {
        apply_rekey(conn, c, absorb)?;
    }
    Ok(())
}

/// Hard-delete rows whose files are confirmed gone, and hide rows whose storage
/// may only be unavailable (keeps progress / pinning until the file reappears).
pub fn sync_missing_media(
    conn: &Connection,
    removed: &[String],
    missing: &[String],
) -> Result<(), String> {
    let mut tx =
        begin_write(conn).map_err(|e| format!("Failed to start missing-media transaction: {e}"))?;
    for file_path in removed {
        tx.execute("DELETE FROM media WHERE file_path = ?1", params![file_path])
            .map_err(|e| format!("Failed to remove missing media row: {e}"))?;
    }
    for file_path in missing {
        tx.execute(
            "UPDATE media SET missing_since = COALESCE(missing_since, strftime('%s', 'now')) WHERE file_path = ?1",
            params![file_path],
        )
        .map_err(|e| format!("Failed to mark media as missing: {e}"))?;
    }
    // Last steps of every scan: follow show names changed by parser fixes, then collect
    // groups emptied by removed rows, absorbed groups or rows that now parse as movies/others.
    // Re-keying runs in a savepoint and only logs its errors: it is a cosmetic repair, and a
    // bug in it must not roll back the reconcile above or make every future scan fail.
    {
        let sp = tx
            .savepoint()
            .map_err(|e| format!("Failed to start re-key savepoint: {e}"))?;
        match rekey_renamed_shows(&sp) {
            Ok(()) => sp
                .commit()
                .map_err(|e| format!("Failed to commit show re-keying: {e}"))?,
            Err(e) => eprintln!("Skipping show re-keying: {e}"), // dropping sp rolls it back
        }
    }
    delete_empty_groups(&tx)?;
    tx.commit()
        .map_err(|e| format!("Failed to commit missing-media changes: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        init_schema(&conn).expect("schema");
        conn
    }

    fn ep(show: &str, season: u32, episode: u32) -> ParsedEpisode {
        ParsedEpisode {
            show_name: show.to_string(),
            season,
            episode,
            title: None,
        }
    }

    fn row<T: rusqlite::types::FromSql>(conn: &Connection, file_path: &str, col: &str) -> T {
        conn.query_row(
            &format!("SELECT {col} FROM media WHERE file_path = ?1"),
            params![file_path],
            |r| r.get(0),
        )
        .expect("media row")
    }

    fn id_of(conn: &Connection, file_path: &str) -> i64 {
        row(conn, file_path, "id")
    }

    fn library_paths(conn: &Connection) -> Vec<String> {
        let lib = get_full_library(conn).expect("library");
        let mut paths: Vec<String> = lib
            .series
            .iter()
            .flat_map(|g| {
                g.seasons
                    .iter()
                    .flat_map(|s| s.episodes.iter().map(|e| e.file_path.clone()))
            })
            .chain(lib.movies.iter().map(|m| m.file_path.clone()))
            .chain(lib.others.iter().map(|o| o.file_path.clone()))
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn rescan_preserves_user_pinned_fields() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e02.mkv", &ep("Show A", 1, 2), 100, None).unwrap();
        let target = find_or_create_group(&conn, "Other Show").unwrap();
        move_episodes(&conn, &[id_of(&conn, "/v/a.s01e02.mkv")], target).unwrap();
        conn.execute(
            "UPDATE media SET season = 3, episode = 7 WHERE file_path = '/v/a.s01e02.mkv'",
            [],
        )
        .unwrap();

        upsert_episode(&conn, "/v/a.s01e02.mkv", &ep("Show A", 1, 2), 0, None).unwrap();

        assert_eq!(row::<i64>(&conn, "/v/a.s01e02.mkv", "group_id"), target);
        assert_eq!(row::<i64>(&conn, "/v/a.s01e02.mkv", "season"), 3);
        assert_eq!(row::<i64>(&conn, "/v/a.s01e02.mkv", "episode"), 7);
    }

    #[test]
    fn rescan_preserves_pinned_media_type() {
        let conn = test_db();
        upsert_episode(&conn, "/v/x.s01e01.mkv", &ep("X", 1, 1), 100, None).unwrap();
        reclassify_media(&conn, &[id_of(&conn, "/v/x.s01e01.mkv")], "movie", None).unwrap();

        upsert_episode(&conn, "/v/x.s01e01.mkv", &ep("X", 1, 1), 0, None).unwrap();

        assert_eq!(
            row::<String>(&conn, "/v/x.s01e01.mkv", "media_type"),
            "movie"
        );
    }

    #[test]
    fn rescan_does_not_clobber_duration_or_thumbnail() {
        let conn = test_db();
        upsert_episode(
            &conn,
            "/v/a.s01e01.mkv",
            &ep("A", 1, 1),
            1200,
            Some("/t/a.jpg".into()),
        )
        .unwrap();
        upsert_movie(
            &conn,
            "/v/m.2010.mkv",
            "M",
            Some(2010),
            5400,
            Some("/t/m.jpg".into()),
        )
        .unwrap();
        upsert_other(&conn, "/v/o.mkv", "o", 30, Some("/t/o.jpg".into())).unwrap();

        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 0, None).unwrap();
        upsert_movie(&conn, "/v/m.2010.mkv", "M", Some(2010), 0, None).unwrap();
        upsert_other(&conn, "/v/o.mkv", "o", 0, None).unwrap();

        for (path, duration, thumb) in [
            ("/v/a.s01e01.mkv", 1200, "/t/a.jpg"),
            ("/v/m.2010.mkv", 5400, "/t/m.jpg"),
            ("/v/o.mkv", 30, "/t/o.jpg"),
        ] {
            assert_eq!(row::<i64>(&conn, path, "duration"), duration, "{path}");
            assert_eq!(
                row::<String>(&conn, path, "thumbnail_path"),
                thumb,
                "{path}"
            );
        }
    }

    #[test]
    fn rescan_keeps_watch_progress() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();
        update_progress(&conn, "/v/a.s01e01.mkv", 600, false).unwrap();

        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 0, None).unwrap();

        assert_eq!(row::<i64>(&conn, "/v/a.s01e01.mkv", "last_position"), 600);
    }

    #[test]
    fn missing_rows_are_hidden_and_revived_by_upsert() {
        let conn = test_db();
        upsert_episode(&conn, "/drive/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();
        upsert_movie(&conn, "/drive/m.2010.mkv", "M", Some(2010), 5400, None).unwrap();
        update_progress(&conn, "/drive/a.s01e01.mkv", 600, false).unwrap();

        sync_missing_media(
            &conn,
            &[],
            &["/drive/a.s01e01.mkv".into(), "/drive/m.2010.mkv".into()],
        )
        .unwrap();
        assert!(library_paths(&conn).is_empty());

        upsert_episode(&conn, "/drive/a.s01e01.mkv", &ep("A", 1, 1), 0, None).unwrap();
        upsert_movie(&conn, "/drive/m.2010.mkv", "M", Some(2010), 0, None).unwrap();
        assert_eq!(
            library_paths(&conn),
            vec!["/drive/a.s01e01.mkv", "/drive/m.2010.mkv"]
        );
        assert_eq!(
            row::<i64>(&conn, "/drive/a.s01e01.mkv", "last_position"),
            600
        );
    }

    #[test]
    fn missing_since_keeps_first_timestamp() {
        let conn = test_db();
        upsert_other(&conn, "/drive/o.mkv", "o", 10, None).unwrap();
        sync_missing_media(&conn, &[], &["/drive/o.mkv".into()]).unwrap();
        conn.execute(
            "UPDATE media SET missing_since = 1 WHERE file_path = '/drive/o.mkv'",
            [],
        )
        .unwrap();

        sync_missing_media(&conn, &[], &["/drive/o.mkv".into()]).unwrap();

        assert_eq!(row::<i64>(&conn, "/drive/o.mkv", "missing_since"), 1);
    }

    #[test]
    fn sync_missing_media_removes_confirmed_gone_rows() {
        let conn = test_db();
        upsert_other(&conn, "/v/gone.mkv", "gone", 10, None).unwrap();
        upsert_other(&conn, "/v/kept.mkv", "kept", 10, None).unwrap();

        sync_missing_media(&conn, &["/v/gone.mkv".into()], &[]).unwrap();

        let paths: Vec<String> = get_media_paths(&conn)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(paths, vec!["/v/kept.mkv"]);
    }

    #[test]
    fn soft_delete_hides_row_and_restore_brings_it_back() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();

        mark_deleted(&conn, "/v/a.s01e01.mkv").unwrap();
        assert!(library_paths(&conn).is_empty());
        assert_eq!(
            get_media_paths(&conn).unwrap(),
            vec![("/v/a.s01e01.mkv".to_string(), true)]
        );

        mark_restored(&conn, "/v/a.s01e01.mkv").unwrap();
        assert_eq!(library_paths(&conn), vec!["/v/a.s01e01.mkv"]);
        assert_eq!(
            get_media_paths(&conn).unwrap(),
            vec![("/v/a.s01e01.mkv".to_string(), false)]
        );
    }

    #[test]
    fn trash_paths_resolve_only_eligible_rows() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 0, None).unwrap();
        upsert_episode(&conn, "/v/a.s01e02.mkv", &ep("A", 1, 2), 0, None).unwrap();
        upsert_movie(&conn, "/v/film.2001.mkv", "Film", Some(2001), 0, None).unwrap();
        let (e1, e2) = (
            id_of(&conn, "/v/a.s01e01.mkv"),
            id_of(&conn, "/v/a.s01e02.mkv"),
        );
        let film = id_of(&conn, "/v/film.2001.mkv");

        assert_eq!(
            resolve_trash_paths(&conn, &[film, e1, film], TrashAction::Delete).unwrap(),
            vec!["/v/film.2001.mkv", "/v/a.s01e01.mkv"]
        );
        assert_eq!(
            resolve_trash_paths(&conn, &[], TrashAction::Delete).unwrap(),
            Vec::<String>::new()
        );
        assert!(resolve_trash_paths(&conn, &[e1, -1], TrashAction::Delete).is_err());
        assert!(resolve_trash_paths(&conn, &[e1], TrashAction::Restore).is_err());

        mark_deleted(&conn, "/v/a.s01e01.mkv").unwrap();
        assert!(resolve_trash_paths(&conn, &[e1], TrashAction::Delete).is_err());
        assert_eq!(
            resolve_trash_paths(&conn, &[e1], TrashAction::Restore).unwrap(),
            vec!["/v/a.s01e01.mkv"]
        );

        // Hidden rows whose storage is offline can't be trashed
        sync_missing_media(&conn, &[], &["/v/a.s01e02.mkv".into()]).unwrap();
        assert!(resolve_trash_paths(&conn, &[e2], TrashAction::Delete).is_err());
    }

    #[test]
    fn continue_watching_pointer_survives_missing_file() {
        let conn = test_db();
        upsert_episode(&conn, "/drive/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();
        update_progress(&conn, "/drive/a.s01e01.mkv", 600, false).unwrap();

        sync_missing_media(&conn, &[], &["/drive/a.s01e01.mkv".into()]).unwrap();
        assert!(get_continue_watching_single(&conn).unwrap().is_none());

        upsert_episode(&conn, "/drive/a.s01e01.mkv", &ep("A", 1, 1), 0, None).unwrap();
        let item = get_continue_watching_single(&conn)
            .unwrap()
            .expect("pointer kept");
        assert_eq!(item.file_path, "/drive/a.s01e01.mkv");
        assert_eq!(item.last_position, 600);
    }

    #[test]
    fn group_lookup_is_normalized() {
        let conn = test_db();
        let a = find_or_create_group(&conn, "Show.Name").unwrap();
        let b = find_or_create_group(&conn, "show name").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn init_schema_migrates_old_database() {
        let conn = Connection::open_in_memory().unwrap();
        // Pre-group, pre-classification schema
        conn.execute_batch(
            "CREATE TABLE media (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL UNIQUE,
                show_name TEXT NOT NULL,
                season INTEGER NOT NULL,
                episode INTEGER NOT NULL,
                title TEXT,
                duration INTEGER NOT NULL DEFAULT 0,
                last_position INTEGER NOT NULL DEFAULT 0,
                watched INTEGER NOT NULL DEFAULT 0,
                thumbnail_path TEXT
            );
            INSERT INTO media (file_path, show_name, season, episode, last_position)
            VALUES ('/v/old.s01e01.mkv', 'Old Show', 1, 1, 42);",
        )
        .unwrap();

        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap(); // idempotent
        migrate_existing_groups(&conn).unwrap();

        assert_eq!(row::<i64>(&conn, "/v/old.s01e01.mkv", "last_position"), 42);
        assert_eq!(
            row::<String>(&conn, "/v/old.s01e01.mkv", "media_type"),
            "series"
        );
        assert_eq!(row::<i64>(&conn, "/v/old.s01e01.mkv", "user_pinned"), 0);
        assert!(row::<Option<i64>>(&conn, "/v/old.s01e01.mkv", "missing_since").is_none());
        assert!(row::<Option<i64>>(&conn, "/v/old.s01e01.mkv", "group_id").is_some());
        assert_eq!(library_paths(&conn), vec!["/v/old.s01e01.mkv"]);
    }

    fn index_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
            params![name],
            |r| r.get(0),
        )
        .expect("sqlite_master")
    }

    #[test]
    fn live_row_index_is_created_on_old_database_and_used() {
        let conn = Connection::open_in_memory().unwrap();
        // Pre-`deleted_at`/`media_type`/`missing_since` schema
        conn.execute_batch(
            "CREATE TABLE media (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL UNIQUE,
                show_name TEXT NOT NULL,
                season INTEGER NOT NULL,
                episode INTEGER NOT NULL,
                title TEXT,
                duration INTEGER NOT NULL DEFAULT 0,
                last_position INTEGER NOT NULL DEFAULT 0,
                watched INTEGER NOT NULL DEFAULT 0,
                thumbnail_path TEXT
            );",
        )
        .unwrap();

        init_schema(&conn).unwrap();
        assert!(index_exists(&conn, "idx_media_type_live"));
        init_schema(&conn).unwrap(); // idempotent
        assert!(index_exists(&conn, "idx_media_type_live"));

        let plan: Vec<String> = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT id FROM media
                 WHERE media_type = 'movie' AND deleted_at IS NULL AND missing_since IS NULL",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            plan.iter().any(|d| d.contains("idx_media_type_live")),
            "plan: {plan:?}"
        );
    }

    #[test]
    fn group_migration_runs_once_and_drops_phantom_groups() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 100, None).unwrap();
        upsert_other(&conn, "/v/o.mkv", "o", 30, None).unwrap();
        // State left by the old migration: a pinned "other" with a phantom group
        let phantom = find_or_create_group(&conn, "o").unwrap();
        conn.execute(
            "UPDATE media SET group_id = ?1, user_pinned = 1 WHERE file_path = '/v/o.mkv'",
            [phantom],
        )
        .unwrap();
        conn.execute(
            "UPDATE media SET group_id = NULL WHERE file_path = '/v/a.s01e01.mkv'",
            [],
        )
        .unwrap();

        migrate_existing_groups(&conn).unwrap();

        assert!(row::<Option<i64>>(&conn, "/v/o.mkv", "group_id").is_none());
        assert!(row::<Option<i64>>(&conn, "/v/a.s01e01.mkv", "group_id").is_some());
        let groups: i64 = conn
            .query_row("SELECT COUNT(*) FROM show_groups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(groups, 1);

        // Second launch: a NULL series group stays NULL because the migration is done
        conn.execute(
            "UPDATE media SET group_id = NULL WHERE file_path = '/v/a.s01e01.mkv'",
            [],
        )
        .unwrap();
        migrate_existing_groups(&conn).unwrap();
        assert!(row::<Option<i64>>(&conn, "/v/a.s01e01.mkv", "group_id").is_none());
    }

    #[test]
    fn set_watched_is_bulk_and_keeps_continue_watching() {
        let conn = test_db();
        for e in 1..=3 {
            upsert_episode(
                &conn,
                &format!("/v/a.s01e0{e}.mkv"),
                &ep("A", 1, e),
                1200,
                None,
            )
            .unwrap();
        }
        update_progress(&conn, "/v/a.s01e02.mkv", 600, false).unwrap();
        let ids = [
            id_of(&conn, "/v/a.s01e01.mkv"),
            id_of(&conn, "/v/a.s01e03.mkv"),
        ];

        set_watched(&conn, &ids, true).unwrap();

        assert!(row::<bool>(&conn, "/v/a.s01e01.mkv", "watched"));
        assert!(row::<bool>(&conn, "/v/a.s01e03.mkv", "watched"));
        assert_eq!(row::<i64>(&conn, "/v/a.s01e01.mkv", "last_position"), 0);
        let item = get_continue_watching_single(&conn)
            .unwrap()
            .expect("pointer");
        assert_eq!(item.file_path, "/v/a.s01e02.mkv");

        set_watched(&conn, &ids[..1], false).unwrap();
        assert!(!row::<bool>(&conn, "/v/a.s01e01.mkv", "watched"));
    }

    #[test]
    fn playback_state_reads_position_duration_and_watched() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();
        update_progress(&conn, "/v/a.s01e01.mkv", 1150, true).unwrap();

        assert_eq!(
            get_playback_state(&conn, "/v/a.s01e01.mkv").unwrap(),
            Some((1150, 1200, true))
        );
        assert_eq!(get_playback_state(&conn, "/v/unknown.mkv").unwrap(), None);
    }

    #[test]
    fn movies_and_others_never_get_a_group() {
        let conn = test_db();
        upsert_movie(&conn, "/v/m.2010.mkv", "M", Some(2010), 5400, None).unwrap();
        upsert_other(&conn, "/v/o.mkv", "o", 30, None).unwrap();

        migrate_existing_groups(&conn).unwrap();

        assert!(row::<Option<i64>>(&conn, "/v/m.2010.mkv", "group_id").is_none());
        assert!(row::<Option<i64>>(&conn, "/v/o.mkv", "group_id").is_none());
    }

    #[test]
    fn next_episode_skips_unavailable_rows() {
        let conn = test_db();
        for e in 1..=4 {
            upsert_episode(
                &conn,
                &format!("/v/a.s01e0{e}.mkv"),
                &ep("A", 1, e),
                100,
                None,
            )
            .unwrap();
        }
        let group: i64 = row(&conn, "/v/a.s01e01.mkv", "group_id");
        mark_deleted(&conn, "/v/a.s01e02.mkv").unwrap();
        sync_missing_media(&conn, &[], &["/v/a.s01e03.mkv".into()]).unwrap();

        assert_eq!(
            get_next_episode(&conn, group, 1, 1).as_deref(),
            Some("/v/a.s01e04.mkv")
        );
    }

    #[test]
    fn continue_watching_only_follows_series() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 1200, None).unwrap();
        upsert_movie(&conn, "/v/m.2010.mkv", "M", Some(2010), 5400, None).unwrap();
        update_progress(&conn, "/v/a.s01e01.mkv", 600, false).unwrap();

        update_progress(&conn, "/v/m.2010.mkv", 100, false).unwrap();

        let item = get_continue_watching_single(&conn)
            .unwrap()
            .expect("series pointer kept");
        assert_eq!(item.file_path, "/v/a.s01e01.mkv");
    }

    #[test]
    fn merge_then_rescan_does_not_split() {
        let conn = test_db();
        upsert_episode(&conn, "/v/showa.s01e01.mkv", &ep("Show A", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/show.s01e02.mkv", &ep("Show", 1, 2), 100, None).unwrap();
        let source: i64 = row(&conn, "/v/showa.s01e01.mkv", "group_id");
        let target: i64 = row(&conn, "/v/show.s01e02.mkv", "group_id");
        merge_groups(&conn, source, target).unwrap();

        upsert_episode(&conn, "/v/showa.s01e01.mkv", &ep("Show A", 1, 1), 0, None).unwrap();
        upsert_episode(&conn, "/v/showa.s01e03.mkv", &ep("Show A", 1, 3), 100, None).unwrap();

        assert_eq!(row::<i64>(&conn, "/v/showa.s01e03.mkv", "group_id"), target);
        let groups: i64 = conn
            .query_row("SELECT COUNT(*) FROM show_groups", [], |r| r.get(0))
            .unwrap();
        assert_eq!(groups, 1);
    }

    fn group_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM show_groups", [], |r| r.get(0))
            .unwrap()
    }

    fn alias_target(conn: &Connection, normalized: &str) -> Option<i64> {
        conn.query_row(
            "SELECT group_id FROM show_group_aliases WHERE normalized_name = ?1",
            params![normalized],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
    }

    #[test]
    fn alias_resolves_before_normalized_name() {
        let conn = test_db();
        let target = find_or_create_group(&conn, "The Show").unwrap();
        conn.execute(
            "INSERT INTO show_group_aliases (normalized_name, group_id) VALUES ('theshowus', ?1)",
            params![target],
        )
        .unwrap();

        assert_eq!(find_group(&conn, "The.Show.US").unwrap(), Some(target));
        assert_eq!(find_or_create_group(&conn, "the show us").unwrap(), target);
        assert_eq!(find_group(&conn, "Unknown").unwrap(), None);
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn merge_records_and_repoints_aliases() {
        let conn = test_db();
        for show in ["a", "b", "c"] {
            let path = format!("/v/{show}.s01e01.mkv");
            upsert_episode(&conn, &path, &ep(&format!("Show {show}"), 1, 1), 100, None).unwrap();
        }
        let a: i64 = row(&conn, "/v/a.s01e01.mkv", "group_id");
        let b: i64 = row(&conn, "/v/b.s01e01.mkv", "group_id");
        let c: i64 = row(&conn, "/v/c.s01e01.mkv", "group_id");

        merge_groups(&conn, a, b).unwrap();
        assert_eq!(alias_target(&conn, "showa"), Some(b));

        // Merging B on into C carries A's alias along
        merge_groups(&conn, b, c).unwrap();
        assert_eq!(alias_target(&conn, "showa"), Some(c));
        assert_eq!(alias_target(&conn, "showb"), Some(c));
        assert_eq!(find_or_create_group(&conn, "Show A").unwrap(), c);
        assert_eq!(group_count(&conn), 1);

        assert!(merge_groups(&conn, a, c).is_err(), "source is gone");
        assert!(merge_groups(&conn, c, 999).is_err(), "unknown target");
    }

    fn dw(year: u32, e: u32) -> String {
        format!("/v/Doctor.Who.{year}.S01E0{e}.mkv")
    }

    /// Pre-BUG-17 library: "Doctor.Who.<year>.*" parsed as show "Doctor Who <year>".
    fn seed_yeared_doctor_who(conn: &Connection, year: u32) -> i64 {
        for e in 1..=2 {
            let name = format!("Doctor Who {year}");
            upsert_episode(conn, &dw(year, e), &ep(&name, 1, e), 100, None).unwrap();
        }
        row(conn, &dw(year, 1), "group_id")
    }

    /// One scan: upsert each (path, parsed show, episode), then the scan's reconcile step.
    fn scan(conn: &Connection, files: &[(String, &str, u32)]) {
        for (path, show, e) in files {
            upsert_episode(conn, path, &ep(show, 1, *e), 0, None).unwrap();
        }
        sync_missing_media(conn, &[], &[]).unwrap();
    }

    /// Rescan of the seeded episodes under the fixed parser, plus `extra` new files.
    fn rescan_as_doctor_who(
        conn: &Connection,
        year: u32,
        extra: &[(String, &str, u32)],
        new_first: bool,
    ) {
        let old: Vec<(String, &str, u32)> =
            (1..=2).map(|e| (dw(year, e), "Doctor Who", e)).collect();
        let files: Vec<_> = if new_first {
            extra.iter().cloned().chain(old).collect()
        } else {
            old.into_iter().chain(extra.iter().cloned()).collect()
        };
        scan(conn, &files);
    }

    fn group_key(conn: &Connection, id: i64) -> (String, String) {
        conn.query_row(
            "SELECT normalized_name, display_name FROM show_groups WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn failing_rekey_keeps_the_reconcile_and_the_scan() {
        let conn = test_db();
        let g = seed_yeared_doctor_who(&conn, 2005);
        conn.execute_batch(
            "CREATE TRIGGER fail_rekey BEFORE UPDATE ON show_groups
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
        upsert_episode(&conn, &dw(2005, 1), &ep("Doctor Who", 1, 1), 0, None).unwrap();

        // Episode 2 is gone; the re-key of the group fails on the trigger
        sync_missing_media(&conn, &[dw(2005, 2)], &[]).unwrap();

        assert!(conn.is_autocommit());
        let gone: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM media WHERE file_path = ?1",
                params![dw(2005, 2)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gone, 0);
        assert_eq!(group_key(&conn, g).0, "doctorwho2005");
        assert_eq!(alias_target(&conn, "doctorwho2005"), None);
    }

    #[test]
    fn rescan_rekeys_group_in_place() {
        let conn = test_db();
        let g = seed_yeared_doctor_who(&conn, 2005);
        update_progress(&conn, &dw(2005, 1), 50, false).unwrap();

        rescan_as_doctor_who(&conn, 2005, &[], false);

        assert_eq!(
            group_key(&conn, g),
            ("doctorwho".into(), "Doctor Who".into())
        );
        assert_eq!(alias_target(&conn, "doctorwho2005"), Some(g));
        assert_eq!(row::<i64>(&conn, &dw(2005, 1), "last_position"), 50);

        // A later episode files straight into the same group
        scan(&conn, &[(dw(2005, 9), "Doctor Who", 9)]);
        assert_eq!(row::<i64>(&conn, &dw(2005, 9), "group_id"), g);
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn renamed_group_keeps_id_and_title_after_rescan() {
        for new_first in [true, false] {
            let conn = test_db();
            let g = seed_yeared_doctor_who(&conn, 2005);
            rename_group(&conn, g, "Doctor Who (New Who)").unwrap();

            rescan_as_doctor_who(&conn, 2005, &[(dw(2005, 9), "Doctor Who", 9)], new_first);

            let lib = get_grouped_library(&conn).unwrap();
            assert_eq!(lib.len(), 1, "new_first = {new_first}");
            assert_eq!(lib[0].group_id, g);
            assert_eq!(lib[0].display_name, "Doctor Who (New Who)");
            assert_eq!(lib[0].seasons[0].episodes.len(), 3);
            assert_eq!(group_count(&conn), 1);
        }
    }

    #[test]
    fn new_episode_joins_old_group_in_any_walk_order() {
        for new_first in [true, false] {
            let conn = test_db();
            let g = seed_yeared_doctor_who(&conn, 2005);
            update_progress(&conn, &dw(2005, 1), 50, false).unwrap();

            rescan_as_doctor_who(&conn, 2005, &[(dw(2005, 9), "Doctor Who", 9)], new_first);

            let lib = get_grouped_library(&conn).unwrap();
            assert_eq!(lib.len(), 1, "new_first = {new_first}");
            assert_eq!(lib[0].group_id, g);
            assert_eq!(lib[0].display_name, "Doctor Who");
            assert_eq!(lib[0].seasons[0].episodes.len(), 3);
            assert_eq!(row::<i64>(&conn, &dw(2005, 1), "last_position"), 50);
        }
    }

    #[test]
    fn split_left_by_an_earlier_scan_is_healed() {
        let conn = test_db();
        let g = seed_yeared_doctor_who(&conn, 2005);
        // An older build already rescanned: the rows' show_name is "Doctor Who", they
        // stayed in the yeared group, and the new episode got its own "Doctor Who" group
        for e in 1..=2 {
            upsert_episode(&conn, &dw(2005, e), &ep("Doctor Who", 1, e), 0, None).unwrap();
        }
        upsert_episode(&conn, &dw(2005, 9), &ep("Doctor Who", 1, 9), 100, None).unwrap();
        let h: i64 = row(&conn, &dw(2005, 9), "group_id");
        assert_ne!(h, g);

        rescan_as_doctor_who(&conn, 2005, &[(dw(2005, 9), "Doctor Who", 9)], true);

        assert_eq!(row::<i64>(&conn, &dw(2005, 9), "group_id"), g);
        assert_eq!(
            group_key(&conn, g),
            ("doctorwho".into(), "Doctor Who".into())
        );
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn distinct_shows_with_the_same_new_name_stay_separate() {
        for classic_first in [true, false] {
            let conn = test_db();
            let (classic, modern) = if classic_first {
                let c = seed_yeared_doctor_who(&conn, 1963);
                (c, seed_yeared_doctor_who(&conn, 2005))
            } else {
                let m = seed_yeared_doctor_who(&conn, 2005);
                (seed_yeared_doctor_who(&conn, 1963), m)
            };
            let mut files: Vec<(String, &str, u32)> = Vec::new();
            for year in [1963, 2005] {
                files.extend((1..=2).map(|e| (dw(year, e), "Doctor Who", e)));
            }
            if !classic_first {
                files.reverse();
            }

            scan(&conn, &files);
            assert_eq!(group_key(&conn, classic).0, "doctorwho1963");
            assert_eq!(group_key(&conn, modern).0, "doctorwho2005");

            // A new 2005 episode gets its own group first; only the 2005 show can absorb it
            scan(&conn, &[(dw(2005, 9), "Doctor Who", 9)]);
            assert_eq!(row::<i64>(&conn, &dw(2005, 9), "group_id"), modern);
            assert_eq!(group_key(&conn, modern).1, "Doctor Who");
            assert_eq!(group_key(&conn, classic).1, "Doctor Who 1963");
            for e in 1..=2 {
                assert_eq!(row::<i64>(&conn, &dw(1963, e), "group_id"), classic);
            }
            assert_eq!(group_count(&conn), 2);
        }
    }

    #[test]
    fn group_mixing_two_old_shows_is_not_absorbed() {
        let conn = test_db();
        let classic = seed_yeared_doctor_who(&conn, 1963);
        let modern = seed_yeared_doctor_who(&conn, 2005);
        // A "Doctor Who" group holding episodes of both shows
        scan(
            &conn,
            &[
                (dw(1963, 9), "Doctor Who", 9),
                (dw(2005, 9), "Doctor Who", 9),
            ],
        );
        let h: i64 = row(&conn, &dw(1963, 9), "group_id");

        let mut files: Vec<(String, &str, u32)> = Vec::new();
        for year in [1963, 2005] {
            files.extend((1..=2).map(|e| (dw(year, e), "Doctor Who", e)));
        }
        scan(&conn, &files);

        assert_eq!(row::<i64>(&conn, &dw(2005, 9), "group_id"), h);
        assert_eq!(row::<i64>(&conn, &dw(2005, 1), "group_id"), modern);
        assert_eq!(row::<i64>(&conn, &dw(1963, 1), "group_id"), classic);
        assert_eq!(group_count(&conn), 3);
    }

    #[test]
    fn renamed_new_name_group_is_not_absorbed() {
        let conn = test_db();
        let g = seed_yeared_doctor_who(&conn, 2005);
        scan(&conn, &[(dw(2005, 9), "Doctor Who", 9)]);
        let h: i64 = row(&conn, &dw(2005, 9), "group_id");
        rename_group(&conn, h, "Specials").unwrap();

        rescan_as_doctor_who(&conn, 2005, &[], false);

        assert_eq!(row::<i64>(&conn, &dw(2005, 9), "group_id"), h);
        assert_eq!(group_key(&conn, g).0, "doctorwho2005");
        assert_eq!(group_count(&conn), 2);
    }

    #[test]
    fn rescan_never_undoes_a_merge() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("Show A", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/a.s01e02.mkv", &ep("Show A", 1, 2), 100, None).unwrap();
        upsert_episode(&conn, "/v/b.s01e01.mkv", &ep("Show B", 1, 1), 100, None).unwrap();
        let a: i64 = row(&conn, "/v/a.s01e01.mkv", "group_id");
        let b: i64 = row(&conn, "/v/b.s01e01.mkv", "group_id");
        merge_groups(&conn, a, b).unwrap();

        scan(
            &conn,
            &[
                ("/v/a.s01e01.mkv".into(), "Show A", 1),
                ("/v/a.s01e02.mkv".into(), "Show A", 2),
                ("/v/b.s01e01.mkv".into(), "Show B", 1),
            ],
        );

        for path in ["/v/a.s01e01.mkv", "/v/a.s01e02.mkv", "/v/b.s01e01.mkv"] {
            assert_eq!(row::<i64>(&conn, path, "group_id"), b, "{path}");
        }
        assert_eq!(group_key(&conn, b), ("showb".into(), "Show B".into()));
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn pre_alias_merge_is_not_reshuffled() {
        let conn = test_db();
        upsert_episode(&conn, "/v/Show.S01E01.mkv", &ep("Show", 1, 1), 100, None).unwrap();
        upsert_episode(
            &conn,
            "/v/Show.2005.S01E01.mkv",
            &ep("Show 2005", 1, 1),
            100,
            None,
        )
        .unwrap();
        let plain: i64 = row(&conn, "/v/Show.S01E01.mkv", "group_id");
        let yeared: i64 = row(&conn, "/v/Show.2005.S01E01.mkv", "group_id");
        // "Show" merged into "Show 2005" by a build without aliases: no alias row left
        conn.execute(
            "UPDATE media SET group_id = ?1 WHERE group_id = ?2",
            params![yeared, plain],
        )
        .unwrap();
        conn.execute("DELETE FROM show_groups WHERE id = ?1", params![plain])
            .unwrap();

        scan(&conn, &[("/v/Show.S01E01.mkv".into(), "Show", 1)]);

        assert_eq!(row::<i64>(&conn, "/v/Show.S01E01.mkv", "group_id"), yeared);
        assert_eq!(
            group_key(&conn, yeared),
            ("show2005".into(), "Show 2005".into())
        );
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn pinned_rows_freeze_their_group() {
        let conn = test_db();
        let g = seed_yeared_doctor_who(&conn, 2005);
        upsert_episode(&conn, "/v/extra.s01e05.mkv", &ep("Extras", 1, 5), 100, None).unwrap();
        // The user moved an episode into the yeared group by hand
        move_episodes(&conn, &[id_of(&conn, "/v/extra.s01e05.mkv")], g).unwrap();

        rescan_as_doctor_who(&conn, 2005, &[(dw(2005, 9), "Doctor Who", 9)], false);

        assert_eq!(
            group_key(&conn, g),
            ("doctorwho2005".into(), "Doctor Who 2005".into())
        );
        assert_eq!(row::<i64>(&conn, &dw(2005, 1), "group_id"), g);
        assert_eq!(row::<i64>(&conn, "/v/extra.s01e05.mkv", "group_id"), g);
        assert_ne!(row::<i64>(&conn, &dw(2005, 9), "group_id"), g);
    }

    #[test]
    fn upsert_keeps_a_group_changed_after_it_was_read() {
        let conn = test_db();
        upsert_other(&conn, "/v/x.s01e01.mkv", "x", 100, None).unwrap();
        let resolved = find_or_create_group(&conn, "X").unwrap();
        let concurrent = find_or_create_group(&conn, "Elsewhere").unwrap();
        // upsert_episode read group NULL; another connection sets one before the write
        conn.execute(
            "UPDATE media SET group_id = ?1 WHERE file_path = '/v/x.s01e01.mkv'",
            params![concurrent],
        )
        .unwrap();

        write_episode(
            &conn,
            "/v/x.s01e01.mkv",
            &ep("X", 1, 1),
            0,
            None,
            Some(resolved),
            None,
        )
        .unwrap();
        assert_eq!(row::<i64>(&conn, "/v/x.s01e01.mkv", "group_id"), concurrent);

        // Unchanged since the read: the resolved group is taken
        write_episode(
            &conn,
            "/v/x.s01e01.mkv",
            &ep("X", 1, 1),
            0,
            None,
            Some(resolved),
            Some(concurrent),
        )
        .unwrap();
        assert_eq!(row::<i64>(&conn, "/v/x.s01e01.mkv", "group_id"), resolved);
    }

    #[test]
    fn merge_suggestions_skip_groups_without_live_episodes() {
        let conn = test_db();
        upsert_episode(&conn, "/v/show.s01e01.mkv", &ep("Show", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/shows.s01e01.mkv", &ep("Shows", 1, 1), 100, None).unwrap();
        assert_eq!(get_merge_suggestions(&conn).unwrap().len(), 1);

        mark_deleted(&conn, "/v/shows.s01e01.mkv").unwrap();
        assert!(get_merge_suggestions(&conn).unwrap().is_empty());
    }

    #[test]
    fn old_database_without_alias_table_opens() {
        let conn = Connection::open_in_memory().unwrap();
        // Schema from before show_group_aliases
        conn.execute_batch(
            "CREATE TABLE show_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                normalized_name TEXT NOT NULL
            );
            CREATE UNIQUE INDEX idx_show_groups_normalized ON show_groups(normalized_name);
            CREATE TABLE media (
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
                deleted_at INTEGER,
                media_type TEXT NOT NULL DEFAULT 'series',
                year INTEGER,
                missing_since INTEGER
            );
            INSERT INTO show_groups (id, display_name, normalized_name) VALUES (7, 'Old', 'old');
            INSERT INTO media (file_path, show_name, season, episode, group_id)
            VALUES ('/v/old.s01e01.mkv', 'Old', 1, 1, 7);",
        )
        .unwrap();

        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap(); // idempotent

        assert_eq!(find_or_create_group(&conn, "Old").unwrap(), 7);
        upsert_episode(&conn, "/v/old.s01e02.mkv", &ep("Old", 1, 2), 0, None).unwrap();
        assert_eq!(row::<i64>(&conn, "/v/old.s01e02.mkv", "group_id"), 7);
        assert_eq!(library_paths(&conn).len(), 2);
    }

    #[test]
    fn empty_groups_are_collected_after_scan_reconcile() {
        let conn = test_db();
        seed_yeared_doctor_who(&conn, 2005);
        upsert_episode(&conn, "/v/gone.s01e01.mkv", &ep("Gone", 1, 1), 100, None).unwrap();
        for e in 1..=2 {
            let path = format!("/v/Doctor.Who.2005.S01E0{e}.mkv");
            upsert_episode(&conn, &path, &ep("Doctor Who", 1, e), 0, None).unwrap();
        }

        sync_missing_media(&conn, &["/v/gone.s01e01.mkv".into()], &[]).unwrap();

        let names: Vec<String> = get_all_groups(&conn)
            .unwrap()
            .into_iter()
            .map(|g| g.display_name)
            .collect();
        assert_eq!(names, vec!["Doctor Who"]);
    }

    #[test]
    fn gc_keeps_groups_of_soft_deleted_and_missing_rows() {
        let conn = test_db();
        upsert_episode(
            &conn,
            "/v/trashed.s01e01.mkv",
            &ep("Trashed", 1, 1),
            100,
            None,
        )
        .unwrap();
        upsert_episode(
            &conn,
            "/v/offline.s01e01.mkv",
            &ep("Offline", 1, 1),
            100,
            None,
        )
        .unwrap();
        let trashed: i64 = row(&conn, "/v/trashed.s01e01.mkv", "group_id");
        mark_deleted(&conn, "/v/trashed.s01e01.mkv").unwrap();

        sync_missing_media(&conn, &[], &["/v/offline.s01e01.mkv".into()]).unwrap();
        assert_eq!(group_count(&conn), 2);

        // Undo and remount bring the rows back into their old groups
        mark_restored(&conn, "/v/trashed.s01e01.mkv").unwrap();
        upsert_episode(
            &conn,
            "/v/offline.s01e01.mkv",
            &ep("Offline", 1, 1),
            0,
            None,
        )
        .unwrap();
        assert_eq!(
            row::<i64>(&conn, "/v/trashed.s01e01.mkv", "group_id"),
            trashed
        );
        assert_eq!(get_grouped_library(&conn).unwrap().len(), 2);
    }

    #[test]
    fn move_reclassify_and_merge_drop_emptied_groups_and_aliases() {
        let conn = test_db();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("Show A", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/b.s01e01.mkv", &ep("Show B", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/c.s01e01.mkv", &ep("Show C", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/d.s01e01.mkv", &ep("Show D", 1, 1), 100, None).unwrap();
        let a: i64 = row(&conn, "/v/a.s01e01.mkv", "group_id");
        let b: i64 = row(&conn, "/v/b.s01e01.mkv", "group_id");
        let c: i64 = row(&conn, "/v/c.s01e01.mkv", "group_id");

        merge_groups(&conn, a, b).unwrap();
        assert_eq!(alias_target(&conn, "showa"), Some(b));
        assert_eq!(group_count(&conn), 3);

        // Moving every episode out of B empties it: B and its alias go
        let ids = [
            id_of(&conn, "/v/a.s01e01.mkv"),
            id_of(&conn, "/v/b.s01e01.mkv"),
        ];
        move_episodes(&conn, &ids, c).unwrap();
        assert_eq!(group_count(&conn), 2);
        assert_eq!(alias_target(&conn, "showa"), None);

        reclassify_media(&conn, &[id_of(&conn, "/v/d.s01e01.mkv")], "movie", None).unwrap();
        assert_eq!(group_count(&conn), 1);
    }

    #[test]
    fn group_counts_only_live_series_episodes() {
        let conn = test_db();
        for e in 1..=4 {
            let path = format!("/v/show.s01e0{e}.mkv");
            upsert_episode(&conn, &path, &ep("Show", 1, e), 100, None).unwrap();
        }
        upsert_episode(&conn, "/v/shows.s01e01.mkv", &ep("Shows", 1, 1), 100, None).unwrap();
        mark_deleted(&conn, "/v/show.s01e02.mkv").unwrap();
        sync_missing_media(&conn, &[], &["/v/show.s01e03.mkv".into()]).unwrap();
        // A row that is in the group but isn't a series (normal code clears group_id)
        conn.execute(
            "UPDATE media SET media_type = 'other' WHERE file_path = '/v/show.s01e04.mkv'",
            [],
        )
        .unwrap();

        let show = get_all_groups(&conn)
            .unwrap()
            .into_iter()
            .find(|g| g.display_name == "Show")
            .unwrap();
        assert_eq!(show.episode_count, 1);

        let suggestions = get_merge_suggestions(&conn).unwrap();
        assert_eq!(suggestions.len(), 1);
        let s = &suggestions[0];
        let counts = if s.display_name_a == "Show" {
            (s.episode_count_a, s.episode_count_b)
        } else {
            (s.episode_count_b, s.episode_count_a)
        };
        assert_eq!(counts, (1, 1));
    }

    /// A database file in a fresh temp dir (WAL needs a real file), removed on drop.
    struct TempDb(PathBuf);

    impl TempDb {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("restack-db-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDb(dir)
        }

        fn path(&self) -> PathBuf {
            self.0.join("restack.db")
        }

        /// Open the way `open_db` does, without an `AppHandle`.
        fn open(&self) -> Connection {
            let conn = Connection::open(self.path()).unwrap();
            configure_connection(&conn).unwrap();
            conn
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn connections_use_wal_and_an_explicit_busy_timeout() {
        let db = TempDb::new("wal");
        let conn = db.open();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, BUSY_TIMEOUT.as_millis() as i64);

        // In-memory databases can't use WAL, which must not be an error
        configure_connection(&Connection::open_in_memory().unwrap()).unwrap();
    }

    #[test]
    fn old_rollback_journal_file_db_opens_in_wal() {
        let db = TempDb::new("old-file");
        {
            // A DB written by an older build: rollback journal, pre-group schema
            let old = Connection::open(db.path()).unwrap();
            old.execute_batch(
                "CREATE TABLE media (
                    id INTEGER PRIMARY KEY,
                    file_path TEXT NOT NULL UNIQUE,
                    show_name TEXT NOT NULL,
                    season INTEGER NOT NULL,
                    episode INTEGER NOT NULL,
                    title TEXT,
                    duration INTEGER NOT NULL DEFAULT 0,
                    last_position INTEGER NOT NULL DEFAULT 0,
                    watched INTEGER NOT NULL DEFAULT 0,
                    thumbnail_path TEXT
                );
                INSERT INTO media (file_path, show_name, season, episode, last_position)
                VALUES ('/v/old.s01e01.mkv', 'Old Show', 1, 1, 42);",
            )
            .unwrap();
            let mode: String = old
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "delete");
        }

        let conn = db.open();
        init_schema(&conn).unwrap();
        migrate_existing_groups(&conn).unwrap();
        assert_eq!(row::<i64>(&conn, "/v/old.s01e01.mkv", "last_position"), 42);
        assert_eq!(library_paths(&conn), vec!["/v/old.s01e01.mkv"]);
        drop(conn);

        // WAL sticks to the file, and reopening it works
        let again = db.open();
        init_schema(&again).unwrap();
        let mode: String = again
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(library_paths(&again), vec!["/v/old.s01e01.mkv"]);
    }

    #[test]
    fn readers_and_the_writer_do_not_block_each_other() {
        let db = TempDb::new("concurrent");
        let writer = db.open();
        init_schema(&writer).unwrap();
        upsert_episode(&writer, "/v/a.s01e01.mkv", &ep("A", 1, 1), 100, None).unwrap();
        let reader = db.open();

        // A reader sees the last committed state while a write transaction is open
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        update_progress(&writer, "/v/a.s01e01.mkv", 50, false).unwrap();
        assert_eq!(row::<i64>(&reader, "/v/a.s01e01.mkv", "last_position"), 0);
        writer.execute_batch("COMMIT").unwrap();
        assert_eq!(row::<i64>(&reader, "/v/a.s01e01.mkv", "last_position"), 50);

        // And an open read transaction doesn't stop the writer from committing
        reader.execute_batch("BEGIN").unwrap();
        assert_eq!(row::<i64>(&reader, "/v/a.s01e01.mkv", "last_position"), 50);
        let started = std::time::Instant::now();
        update_progress(&writer, "/v/a.s01e01.mkv", 70, false).unwrap();
        assert!(started.elapsed() < BUSY_TIMEOUT);
        assert_eq!(row::<i64>(&reader, "/v/a.s01e01.mkv", "last_position"), 50);
        reader.execute_batch("COMMIT").unwrap();
        assert_eq!(row::<i64>(&reader, "/v/a.s01e01.mkv", "last_position"), 70);
    }

    /// How long `with_concurrent_writer` holds the write lock.
    const WRITER_HOLD: Duration = Duration::from_millis(200);

    /// Run `f` while another connection holds the write lock: it opens an immediate
    /// transaction, writes, and commits `WRITER_HOLD` later, the way a scan batch does.
    fn with_concurrent_writer<T>(db: &TempDb, f: impl FnOnce() -> T) -> T {
        let path = db.path();
        let (locked_tx, locked) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let conn = Connection::open(path).unwrap();
            configure_connection(&conn).unwrap();
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 INSERT OR REPLACE INTO app_state (key, value_text) VALUES ('writer', 'done');",
            )
            .unwrap();
            locked_tx.send(()).unwrap();
            std::thread::sleep(WRITER_HOLD);
            conn.execute_batch("COMMIT").unwrap();
        });
        locked.recv().unwrap();
        let result = f();
        writer.join().unwrap();
        result
    }

    fn writer_committed(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE key = 'writer')",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn deferred_read_then_write_fails_without_waiting() {
        // The problem `begin_write` avoids: a deferred transaction that has read can't wait
        // for the lock, so its first write fails at once instead of using the busy timeout.
        let db = TempDb::new("deferred");
        let conn = db.open();
        init_schema(&conn).unwrap();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 100, None).unwrap();

        let result = with_concurrent_writer(&db, || {
            let tx = conn.unchecked_transaction().unwrap();
            let _: i64 = row(&tx, "/v/a.s01e01.mkv", "id");
            tx.execute("UPDATE media SET watched = 1", [])
        });

        let err = result.unwrap_err();
        assert_eq!(
            err.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy),
            "{err}"
        );
    }

    #[test]
    fn write_transactions_wait_for_a_concurrent_writer() {
        let db = TempDb::new("immediate");
        let conn = db.open();
        init_schema(&conn).unwrap();
        upsert_episode(&conn, "/v/a.s01e01.mkv", &ep("A", 1, 1), 100, None).unwrap();
        upsert_episode(&conn, "/v/a.s01e02.mkv", &ep("A", 1, 2), 100, None).unwrap();
        upsert_episode(&conn, "/v/b.s01e01.mkv", &ep("B", 1, 1), 100, None).unwrap();
        upsert_other(&conn, "/v/clip.mkv", "clip", 10, None).unwrap();
        let a = row::<i64>(&conn, "/v/a.s01e01.mkv", "group_id");
        let b = row::<i64>(&conn, "/v/b.s01e01.mkv", "group_id");
        let a1 = id_of(&conn, "/v/a.s01e01.mkv");
        let a2 = id_of(&conn, "/v/a.s01e02.mkv");
        let clip = id_of(&conn, "/v/clip.mkv");

        // Each of these reads before it writes; all must wait for the lock, not fail
        type Op<'a> = Box<dyn FnOnce() -> Result<(), String> + 'a>;
        let ops: Vec<(&str, Op)> = vec![
            ("set_watched", Box::new(|| set_watched(&conn, &[a1], true))),
            ("move_episodes", Box::new(|| move_episodes(&conn, &[a2], b))),
            (
                "reclassify_media",
                Box::new(|| reclassify_media(&conn, &[clip], "series", Some(a))),
            ),
            ("merge_groups", Box::new(|| merge_groups(&conn, b, a))),
            (
                "sync_missing_media",
                Box::new(|| sync_missing_media(&conn, &[], &["/v/a.s01e01.mkv".into()])),
            ),
            (
                "migrate_existing_groups",
                Box::new(|| migrate_existing_groups(&conn)),
            ),
        ];
        for (name, op) in ops {
            conn.execute("DELETE FROM app_state WHERE key = 'writer'", [])
                .unwrap();
            with_concurrent_writer(&db, op).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(writer_committed(&conn), "{name}");
        }

        assert_eq!(row::<i64>(&conn, "/v/a.s01e01.mkv", "watched"), 1);
        assert_eq!(row::<i64>(&conn, "/v/b.s01e01.mkv", "group_id"), a);
        assert_eq!(row::<i64>(&conn, "/v/clip.mkv", "group_id"), a);
        assert!(row::<Option<i64>>(&conn, "/v/a.s01e01.mkv", "missing_since").is_some());
    }

    /// Sets a path's mode and puts the old one back on drop, so a failing test still lets
    /// `TempDb` clean up.
    struct ModeGuard(PathBuf, u32);

    impl ModeGuard {
        fn set(path: PathBuf, mode: u32) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let old = fs::metadata(&path).unwrap().permissions().mode();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            ModeGuard(path, old)
        }
    }

    impl Drop for ModeGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1));
        }
    }

    #[test]
    fn read_only_database_still_opens_when_wal_fails() {
        let db = TempDb::new("read-only");
        {
            // Written by an older build in the rollback journal, so the switch to WAL
            // would need a write
            let old = Connection::open(db.path()).unwrap();
            init_schema(&old).unwrap();
            upsert_episode(&old, "/v/a.s01e01.mkv", &ep("A", 1, 1), 100, None).unwrap();
        }

        // Read-only file in a read-only directory
        let _file = ModeGuard::set(db.path(), 0o444);
        let _dir = ModeGuard::set(db.0.clone(), 0o555);

        let conn = db.open();
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, BUSY_TIMEOUT.as_millis() as i64);
        assert_eq!(library_paths(&conn), vec!["/v/a.s01e01.mkv"]);
    }
}
