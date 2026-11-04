# Restack architecture

This document describes how Restack is implemented today. For what the app does and how to run it, see the [README](../README.md). The open backlog, including every known bug referenced below by ID, lives in [TODO.md](../TODO.md).

Status markers used throughout:

- **Implemented**: in the code and reachable from the UI.
- **Partial**: exists in the code but is incomplete, or exists in the backend with no UI entry point.
- **Planned**: not in the code; tracked in TODO.md.

## Contents

1. [Overview](#1-overview)
2. [Frontend](#2-frontend)
3. [Backend commands and events](#3-backend-commands-and-events)
4. [Scanning](#4-scanning)
5. [Filename parsing](#5-filename-parsing)
6. [Library organization and grouping](#6-library-organization-and-grouping)
7. [Database](#7-database)
8. [Playback and progress tracking](#8-playback-and-progress-tracking)
9. [Thumbnails](#9-thumbnails)
10. [Delete and restore](#10-delete-and-restore)
11. [Concurrency](#11-concurrency)
12. [Error handling](#12-error-handling)
13. [Testing](#13-testing)
14. [Implementation status](#14-implementation-status)

## 1. Overview

Restack is a Tauri 2 application: a Rust backend and a React frontend rendered in the system WebView (WebKitGTK on Linux). The frontend never touches the filesystem or the database. Everything goes through Tauri commands (request/response) and events (backend → frontend).

```
┌──────────────────────────── Restack process ─────────────────────────────┐
│                                                                          │
│  React UI (src/)                         Rust backend (src-tauri/src/)   │
│  ─────────────────                       ──────────────────────────────  │
│  pages, layouts ──── invoke(command) ───▶ commands/*.rs                  │
│  libraryStore.js ◀── listen(event) ───── │  (run on blocking pool)       │
│                                          ▼                               │
│                                         db.rs ──────▶ restack.db (SQLite)│
│                                          ▲                               │
│                                         utils/: scanner, parser,         │
│                                                 thumbnail, trash         │
└──────────────────────────────────────────┬───────────────────────────────┘
                                           │ spawns / talks to
              ┌──────────────┬─────────────┴───────┬───────────────────┐
              ▼              ▼                     ▼                   ▼
            mpv          ffprobe               ffmpeg         freedesktop Trash
   (JSON IPC over a   (duration of        (one thumbnail     (via the `trash`
    Unix socket)       new files)          per new file)      crate)
```

### Source layout

| Path | Responsibility |
|---|---|
| `src/App.jsx` | Routes, startup sequence, global event listeners, <kbd>Ctrl</kbd>+<kbd>R</kbd> rescan |
| `src/store/libraryStore.js` | In-memory library and "currently playing" state with a small pub/sub API |
| `src/pages/`, `src/layouts/`, `src/components/` | Screens and UI components |
| `src-tauri/src/lib.rs` | App setup, managed state, command registry, `run_blocking` helper |
| `src-tauri/src/commands/` | Tauri command wrappers: `scanner`, `library`, `groups`, `player` |
| `src-tauri/src/db.rs` | All SQL: schema, migrations, upserts, queries |
| `src-tauri/src/models.rs` | Serialized response types, `AppState`, `ScanGuard` |
| `src-tauri/src/utils/` | `scanner` (walk, ffprobe), `parser`, `thumbnail` (ffmpeg), `trash` |

## 2. Frontend

React 19 with React Router 7 (`HashRouter`), styled with Tailwind CSS 4. There are five routes:

| Route | Page | Purpose |
|---|---|---|
| `/` | `Dashboard` | One Continue Watching / Up Next card |
| `/series` | `SeriesIndex` | Grid of all series, with search |
| `/series/:groupId` | `SeriesDetail` | Seasons and episodes of one series, and all series-level actions |
| `/movies` | `MoviesPage` | Movie grid, with search |
| `/others` | `OthersPage` | Unclassified files, with search |

### State

`libraryStore.js` holds the last `LibraryResponse` and the path of the file currently playing. Components read it with `getLibrary()`/`getSeries()`/…, subscribe in a `useEffect`, and unsubscribe on cleanup. There is no other state library.

After any mutating command the frontend refetches the whole library with `get_library_grouped` and calls `setLibrary`. The one exception is playback progress: `video_progress_updated` patches a single item in place (`updateEpisodeProgress`). That rebuilds only the path from the library root to the changed item and skips notifying subscribers when nothing changed.

### Startup sequence

```
mount App
 ├─ listen("scan:start"), listen("scan:progress")
 ├─ invoke("get_library_grouped")      ← saved library from SQLite
 │    └─ non-empty? show it immediately; otherwise keep the first-scan overlay
 ├─ doSync() → invoke("scan_user_storage")   ← full rescan in the background
 └─ listen(playback events)
```

A returning user sees the cached library at once while a thin progress bar tracks the rescan. On an empty database, a full-screen overlay shows live scan progress until the first scan finishes. `doSync` keeps the scanning state visible for at least one second so fast scans don't flicker.

Because Tauri's `listen()` is asynchronous, the effect tracks a `cancelled` flag. Listeners that resolve after cleanup (React StrictMode mounts effects twice in development) are removed immediately, so events are never handled twice.

## 3. Backend commands and events

### Command layer

Every command is an `async fn` that runs its body on Tauri's blocking thread pool through `run_blocking` (`lib.rs`). This keeps scans, ffprobe/ffmpeg, Trash operations, and SQLite I/O off the main thread, which also drives the window. Commands are thin: open a connection, ensure the schema, call a `db.rs` function. Arguments are camelCase in JavaScript and snake_case in Rust; Tauri converts between them.

| Command | Purpose | Used by the UI |
|---|---|---|
| `scan_user_storage` | Scan the default roots, return the library | Yes (startup, <kbd>Ctrl</kbd>+<kbd>R</kbd>, sidebar sync button) |
| `scan_and_sync { rootPath }` | Scan a single root | No |
| `get_scan_roots` | List default roots | No |
| `get_library_grouped` | Return the library without scanning | Yes |
| `get_continue_watching_single_cmd` | Continue Watching / Up Next item | Yes (Dashboard) |
| `play_episode { filePath, startSeconds }` | Launch mpv and start tracking | Yes |
| `set_watched_cmd { mediaIds, watched }` | Manual mark watched/unwatched | Yes (SeriesDetail) |
| `rename_group_cmd { groupId, newName }` | Rename a series | Yes (SeriesDetail) |
| `move_episodes_cmd { mediaIds, targetGroupId }` | Move episodes to another series | Yes (SeriesDetail) |
| `reclassify_media_cmd { mediaIds, targetType, targetGroupId }` | Series ↔ movie ↔ other | Yes (sidebar drag and drop) |
| `delete_episodes_cmd { filePaths }` | Move files to Trash | Yes (SeriesDetail) |
| `restore_episodes_cmd { filePaths }` | Undo a delete | Yes (SeriesDetail) |
| `merge_groups_cmd { sourceGroupId, targetGroupId }` | Merge two series | **No**: UI components exist but are not mounted |
| `get_all_groups_cmd` | Groups with episode counts | **No** (only the unmounted merge dialog) |
| `get_merge_suggestions_cmd` | Similar group names | **No** (only the unmounted banner) |

All return `Result<T, String>`; the error string reaches JavaScript as a rejected promise.

### Events

| Event | Payload | Emitted when |
|---|---|---|
| `scan:start` | `{ total_files }` | After the directory walk, before files are processed |
| `scan:progress` | `{ current, total, file_name }` | Once per file |
| `video_playback_started` | file path | mpv was spawned |
| `video_progress_updated` | `{ file_path, last_position, watched }` | Throttled during playback (see §8) |
| `video_playback_stopped` | file path | The tracker for that file ended |
| `play_next_episode` | next file path | An episode reached its end and a next episode exists |

Event payloads use snake_case field names; command return types use camelCase (`#[serde(rename_all = "camelCase")]`).

## 4. Scanning

**Implemented**, with fixed roots. Choosing folders is planned (FEAT-1).

### Roots and discovery

`discover_default_scan_roots` returns:

- `$HOME` (all of it),
- `/media/<name>`, where `<name>` is the last component of `$HOME`,
- every directory in `/mnt`.

`/run/media/$USER`, where udisks2 mounts removable drives on many distributions, is not included (BUG-20).

`collect_video_files` walks each existing root with `walkdir` (symlinks are not followed) and keeps files whose extension is `mkv`, `mp4`, `avi`, or `webm`, case-insensitively. It skips directories named `.cache`, `.git`, `node_modules`, `target`, `dist`, `Trash`, and `.Trash-*`, wherever they appear. Other hidden directories are still walked (BUG-20). Unreadable directories are recorded as *incomplete* rather than aborting the walk.

### Processing

A scan has two passes:

1. **Walk** all roots and collect paths, then emit `scan:start` with the total.
2. **Process** each file: emit `scan:progress`, then parse and upsert it (§5, §7).

Only files not already in the database are probed. Those get a duration from `ffprobe` and a thumbnail from `ffmpeg`. Known files pass a duration of 0 and no thumbnail, and the upserts keep the stored values (`CASE WHEN excluded.duration > 0 …`, `COALESCE(excluded.thumbnail_path, …)`). A failed probe is therefore never retried (BUG-19).

Upserts are grouped into short write transactions (`UpsertBatch`): a batch commits after 500 rows or 1 second, and always before probing a new file. A rescan of unchanged files then costs one fsync per batch instead of one per file, and the write lock is never held while ffprobe/ffmpeg run, so the player can save progress during a long scan.

### Reconciling files that were not seen

After processing, rows whose files were not seen are classified by `reconcile_unseen` and applied by `sync_missing_media` in one transaction:

| Row | Outcome |
|---|---|
| Outside every root walked this scan, or under an unreadable directory | Untouched |
| Soft-deleted (`deleted_at` set, i.e. in the Trash) | Untouched, so Undo keeps working |
| Its folder still exists and has other content | **Hard-deleted**: the file was really removed |
| Its folder is missing or empty | **Hidden** (`missing_since` set): probably an unmounted drive |

Hidden rows keep their progress and manual corrections, and are revived by the next upsert when the file reappears. Nothing purges hidden or trashed rows yet (BUG-35).

### One scan at a time

Commands run concurrently now that they are off the main thread, so `ScanGuard` (an `AtomicBool` in `AppState`, released on drop) allows one scan at a time. A second scan, for example <kbd>Ctrl</kbd>+<kbd>R</kbd> during the startup scan, fails with "A scan is already running". The frontend ignores that error. Delete and restore take the same guard (§10).

## 5. Filename parsing

`utils/parser.rs` classifies a file from its name without the extension, trying in order: **series → movie → other**. Every file ends up in one of the three. Regexes are compiled once (`LazyLock`).

### Cleaning

`clean_text` removes `[...]` and `(...)` groups, turns runs of `.`, `_`, and `-` into spaces, and strips quality, codec, source, and language tags (`1080p`, `x265`, `hevc`, `web-dl`, `bluray`, `hindi`, …) as well as `WxH` resolutions such as `1920x1080`.

### Series patterns

Tried on the cleaned name, first match wins:

| # | Pattern | Example | Result |
|---|---|---|---|
| 1 | `SxxEyy`, also `S01 E02` and multi-episode `S01E01E02` | `Show.Name.S02E05.1080p` | Show Name, S2E5 |
| 2 | `NxNN` (season 1–2 digits, episode 1–3) | `Show 3x07` | Show, S3E7 |
| 3 | `Season N Episode M` | `Show Season 1 Episode 2` | Show, S1E2 |
| 4 | `E` + episode only | `Show E04` | Show, S1E4 |
| 5 | Anime dash, on a lightly cleaned name | `One_Piece_-_1071` | One Piece, S1E1071 |

Text after the episode number becomes the episode title. It is stored but not shown in the UI (FEAT-13). The dash pattern (5) needs a space or non-digit before the dash, rejects 19xx/20xx as episode numbers, and is skipped entirely when the name contains an ISO date, so screen recordings such as `Screencast from 2024-05-14 12-30-45` are not treated as episodes. A trailing year is dropped from series names (`Doctor.Who.2005.S01E01` → "Doctor Who"), unless the name is only a year.

### Movies and other

If no series pattern matches and the name has no ISO date:

1. A parenthesized year in the original name is the release year: `Blade Runner 2049 (2017)` → "Blade Runner 2049", 2017.
2. Otherwise the last `19xx`/`20xx` token after a title: `Inception.2010.1080p` → "Inception", 2010.

Anything else is **other**, stored under its raw file name.

Known parser limitations: show names that genuinely end in a year lose it (BUG-39), and a bare 4-digit resolution after a dash can read as an episode (BUG-40).

## 6. Library organization and grouping

Series episodes belong to a **group** (`show_groups`). Movies and others never have one.

- **Grouping key.** `normalize_name` lowercases a show name and keeps only alphanumeric characters. `find_or_create_group` looks groups up by that key, so `Show.Name`, `show name`, and `SHOW-NAME` share one group. The group's `display_name` is the first spelling seen.
- **Rename** (implemented) changes `display_name` only. The key stays, so later scans still land in the renamed group.
- **Move episodes** and **reclassify** (implemented) set `user_pinned = 1` on the affected rows. Scans then keep their group, type, season, and episode. Reclassifying a movie or other file into a series gives it season 1 and the next free episode number if it had none.
- **Merge** and **merge suggestions** (partial: backend only). `merge_groups` moves every row from one group to another and deletes the source. Suggestions pair groups whose keys are 1–2 edits apart (Levenshtein). No screen calls either. After a merge, the next scan recreates the deleted group for files that still parse to its name (BUG-13).
- **Group stickiness.** An existing series row keeps its group on rescan, even if its parsed show name changes; only new rows and rows without a group are assigned from the parse. This keeps renames and merges stable, but it also means a parser change can put new episodes in a different group from older ones (BUG-41).
- Groups that become empty are not removed (BUG-5).

## 7. Database

SQLite through `rusqlite` (bundled SQLite), stored at `~/.local/share/com.jay.restack/restack.db` (Tauri's app data directory for the identifier `com.jay.restack`).

### Schema

```
show_groups                         media
──────────────────────              ─────────────────────────────────────────────
id              INTEGER PK  ◀────── group_id        INTEGER   (series only; NULL otherwise)
display_name    TEXT                id              INTEGER PK
normalized_name TEXT UNIQUE         file_path       TEXT UNIQUE NOT NULL
                                    media_type      TEXT  'series' | 'movie' | 'other'
app_state                           show_name       TEXT  show / movie title / raw name
──────────────────────              season, episode INTEGER  (0 / 0 for movies and others)
key        TEXT PK                  title           TEXT
value_text TEXT                     year            INTEGER  (movies)
updated_at INTEGER                  duration        INTEGER  seconds
                                    last_position   INTEGER  seconds
                                    watched         INTEGER  0 / 1
                                    thumbnail_path  TEXT
                                    user_pinned     INTEGER  1 = manual correction, scans keep it
                                    deleted_at      INTEGER  unix time; file is in the Trash
                                    missing_since   INTEGER  unix time; file not found, storage may be offline
```

`group_id` is a plain integer column; there is no foreign-key constraint. `app_state` holds `last_watched_episode_id` (Continue Watching) and `migration_groups_v1` (a one-time migration marker).

Indexes: unique `normalized_name`; `media(show_name, season, episode)`; `media(group_id, season, episode)`; and `media(media_type, deleted_at, missing_since)` for the live-row filter.

### Live rows

Every library query filters on `deleted_at IS NULL AND missing_since IS NULL`. Trashed and hidden rows stay in the table but are invisible.

### Upserts

Rows are keyed by `file_path`. `upsert_episode`, `upsert_movie`, and `upsert_other` insert new rows and update existing ones while:

- preserving `last_position` and `watched` (never part of an upsert),
- keeping the stored duration and thumbnail when the scan supplies none,
- keeping group, type, season, and episode when `user_pinned = 1`,
- clearing `deleted_at` and `missing_since`, since the file was just seen.

### Migrations

`init_schema` runs on startup and in every command. It creates missing tables with `CREATE … IF NOT EXISTS`, then adds missing columns with `ALTER TABLE … ADD COLUMN` after checking `PRAGMA table_info(media)`, then creates indexes that depend on those columns. Migrations are additive only; columns are never dropped or renamed, so older databases keep opening. `migrate_existing_groups` runs once at startup (guarded by `app_state`) to assign groups to series rows from builds that predate grouping.

### Connections

There is no connection pool. Each command opens its own connection and runs `init_schema`. Each playback tracker holds one connection for the length of the playback. The database uses SQLite's default rollback journal and rusqlite's default 5-second busy timeout; neither is configured explicitly (PERF-4, PERF-15).

## 8. Playback and progress tracking

**Implemented.** Restack does not decode video. `play_episode` launches mpv and follows it over mpv's JSON IPC.

### Launch

1. Read `last_position`, `duration`, and `watched` for the file.
2. Choose the start: an explicit `startSeconds`, otherwise the saved position. If the saved position is in the last 10% of the file (the credits), start from 0.
3. Kill and reap any mpv that Restack started earlier. One player runs at a time.
4. Spawn:

   ```
   mpv <file> --input-ipc-server=/tmp/mpv-<uuid>.sock --keep-open=no
       --no-resume-playback --script-opts=autoload-disabled=yes
       --force-window=no [--start=N]
   ```

   mpv's own resume (`watch-later`) is disabled so the database is the single source of truth.
5. Emit `video_playback_started` and start a tracker thread.

### Tracker

The tracker waits up to 3 seconds for the socket, connects, and observes `time-pos`, `duration`, and `pause`. If the socket never appears, the video still plays but no progress is recorded (BUG-31).

- **Watched** becomes true once the position reaches 90% of the duration and stays true for the rest of the session, even after seeking back. An item already marked watched stays watched when rewatched.
- **Throttling.** Progress is written to the database and emitted as `video_progress_updated` when the watched flag flips, or when the position has moved and at least 5 seconds of position or wall time have passed since the last write. Pausing writes immediately.
- **End of file.** The position is reset to 0 with `watched = true`, so the next play starts from the beginning, and the next episode is looked up (same group, later season/episode, live series rows only).
- **Any other exit** (quit, stop, error, or mpv killed by a new playback) writes whatever the throttle held back.

When the loop ends, the tracker removes the socket and emits `video_playback_stopped`. Then, only if there is a next episode, it emits `play_next_episode`, which the frontend answers with `play_episode`. Emitting it after the IPC loop has closed avoids a race in which the frontend kills mpv while the tracker is still writing to its socket.

### Continue Watching and Up Next

Each progress write on a **series** row stores its id as `last_watched_episode_id`. Manual mark watched/unwatched does not move it. `get_continue_watching_single_cmd` returns:

- **Continue Watching**: that episode, if it is not watched;
- **Up Next**: the next live episode in the same group, if it is watched (`next_episode` or `next_season`);
- nothing, otherwise.

If the episode is temporarily hidden (`missing_since`), the pointer is kept for when the file returns; if the row is gone, the pointer is cleared. The Dashboard fetches this once on mount and does not refresh while it stays open (BUG-27).

## 9. Thumbnails

**Implemented**, with gaps. For each new file, `generate_thumbnail` runs:

```
ffmpeg -ss 00:00:15 -i <file> -frames:v 1 -q:v 2 -y <thumbnails>/<uuid>.jpg
```

The file name is a UUID v5 of the video path, so each path maps to a stable thumbnail. An existing thumbnail is reused. Thumbnails live in `~/.local/share/com.jay.restack/thumbnails/`, and the WebView loads them through Tauri's asset protocol (`convertFileSrc`). `tauri.conf.json` scopes that protocol to the thumbnail directory and allows `asset:` in the Content Security Policy.

Gaps: videos shorter than 15 seconds get no thumbnail, and failures are not retried (BUG-19); the asset scope hard-codes `$HOME/.local/share/…` (SEC-6); thumbnails of removed files are never cleaned up (PERF-14).

## 10. Delete and restore

**Implemented for series episodes.** Movies and others have no delete yet (FEAT-12).

- `delete_episodes_cmd` moves each file to the freedesktop Trash with the `trash` crate, then sets `deleted_at` for that file before moving to the next, so a failure partway through leaves the database consistent with the disk.
- `restore_episodes_cmd` finds the most recently trashed item whose original path matches, restores it, and clears `deleted_at`. This works for trash directories on other drives (`.Trash-$UID`) and for renamed duplicates.
- The SeriesDetail page offers Undo for 10 seconds after a delete.
- Both commands hold the scan guard. While a scan runs they fail with "Failed to delete: a library scan is running" instead of racing it, because a scan that walked a path before it was trashed would otherwise mark it live again. Rejected rather than queued operations are tracked as PERF-16.

## 11. Concurrency

| Work | Where it runs |
|---|---|
| Window, WebView, event delivery | Main thread |
| Every command body | Tauri's blocking thread pool (`run_blocking`) |
| Playback tracking | One `std::thread` per playback |
| mpv, ffprobe, ffmpeg | Child processes; ffprobe and ffmpeg are run synchronously inside the scan |

Shared state is small: `AppState` holds the current mpv `Child` (behind a `Mutex`, held only while swapping processes) and the `scan_running` flag. Everything else is coordinated through SQLite: short scan batches, rusqlite's 5-second busy timeout, and per-command connections. ffprobe and ffmpeg have no timeout, so a hung probe stalls the scan, and with it the scan guard, until restart (PERF-10).

## 12. Error handling

- Backend functions return `Result<T, String>` with messages in the form `Failed to …: <cause>`. Commands pass them to JavaScript unchanged.
- One file failing to upsert aborts the rest of that scan; batches already committed stay committed (BUG-14).
- Background errors (the tracker, IPC) are printed to stdout with `println!`; there is no logging framework (CHORE-8).
- Most frontend calls swallow errors (`catch {}`), including the startup scan. SeriesDetail shows an error banner, mostly with fixed messages such as "Failed to delete episodes." rather than the backend's text. Missing `mpv`/`ffmpeg`/`ffprobe` is not reported in the UI (FEAT-3, FEAT-4).

## 13. Testing

The Rust backend has unit tests under `#[cfg(test)]` in `db.rs` (in-memory SQLite: upsert invariants, pinning, hidden and trashed rows, migrations from older schemas), `utils/scanner.rs` (walk and reconcile decisions on temporary directories), `utils/parser.rs` (table-driven filename cases), `commands/scanner.rs` (end-to-end sync with a fake probe), `commands/player.rs` (resume, watched, and throttle rules), `commands/library.rs`, and `models.rs` (scan guard). Tests never touch the real database, `$HOME`, or the Trash. Tests that reproduce open bugs are marked `#[ignore = "BUG-…"]`.

There are no frontend tests and no CI.

```bash
cd src-tauri && cargo test
```

## 14. Implementation status

| Area | Status | Notes |
|---|---|---|
| Scan home and mounted drives | Implemented | Fixed roots; `/run/media` not scanned (BUG-20) |
| Choose scan folders, settings page | Planned | FEAT-1; `scan_and_sync` and `get_scan_roots` exist without UI |
| Filesystem watching | Planned | FEAT-2 |
| Series / movie / other classification | Implemented | Known gaps BUG-39, BUG-40 |
| Grouping, rename, move, reclassify | Implemented | |
| Merge series, merge suggestions | Partial | Backend only (MAINT-1); BUG-13 |
| Playback, resume, watched, auto-play next | Implemented | |
| Continue Watching / Up Next | Implemented | Single item; stale until remount (BUG-27) |
| Thumbnails | Partial | No thumbnail under 15 s, no retry (BUG-19) |
| Delete to Trash with Undo | Partial | Series episodes only (FEAT-12) |
| Mark watched / unwatched | Partial | Series episodes only (FEAT-12) |
| Error reporting in the UI | Planned | FEAT-3, FEAT-4 |
| Subtitles, online metadata, statistics, profiles | Planned | FEAT-7 to FEAT-10 |
