# AGENT.md — Restack

Authoritative context for any AI agent or contributor working in this repo. It describes the code as it **is**; `README.md` is the original product spec and is partly outdated (see "Spec vs. reality").

---

## 1. What this is

**Restack** is a **Linux-only Tauri 2 desktop app** that indexes local video files, organizes them into **Series → Seasons → Episodes**, **Movies**, and **Others**, tracks watch progress, and hands playback to **mpv**.

It is *not* a player, streaming server, or media center. Keep it offline-first, lightweight, fast, and minimal.

- npm package / Cargo crate: `restack` (lib crate: `restack_lib`)
- Tauri identifier: `com.jay.restack`
- Runtime deps on the host: `mpv`, `ffmpeg`, `ffprobe` (all must be on `PATH`)

---

## 2. Commands

```bash
npm install
npm run tauri dev        # full app (runs vite on :1420 + Rust). Script forces GDK_BACKEND=x11
npm run tauri build      # release bundle
npm run dev              # frontend only (vite, port 1420, strictPort) — invoke() calls will fail outside Tauri
npm run build            # frontend production build → dist/

cd src-tauri && cargo check    # type-check backend
cd src-tauri && cargo test     # backend unit tests (in-memory SQLite + temp dirs)
cd src-tauri && cargo clippy   # lint backend
```

- **Tests**: backend only, as `#[cfg(test)]` modules in `db.rs` (in-memory SQLite: upsert/pinning invariants, missing/soft-delete rows, migrations), `utils/scanner.rs` (walk + reconcile decisions), and `commands/scanner.rs` (`sync_library` end to end on temp dirs with a fake probe). They never touch the real DB, `$HOME`, or Trash. Tests for open bugs are `#[ignore = "BUG-x: …"]` and fail today; remove the `#[ignore]` when fixing that item (`cargo test -- --ignored` lists them).
- **No frontend tests, no ESLint/Prettier config, no CI.** Verify changes with `npm run build`, `cargo check`, and `cargo test`.

### Git workflow

- `main`: released code only. Each merge into it is a release, tagged `vX.Y.Z`. Never commit to it directly.
- `develop`: the working branch. Commit directly to it; no per-task branches. Test the combined app here before a release.
- One commit per task: each commit covers a single TODO item (or a tightly related set) together with its TODO.md/AGENT.md updates, and references the IDs in the message ("fixes BUG-8"). Don't mix unrelated tasks in one commit.
- Release: `git checkout main && git merge --no-ff develop && git tag vX.Y.Z`.

### Multi-agent waves (how the TODO backlog is worked)

The backlog is worked in **waves** of parallel agents. The maintainer has approved pushing to `origin/develop` after each wave (never `main`).

- **Coordinator** (the main session) plans each wave so that no two workers touch the same file. It also owns the final review and the push.
- **Workers** each run in their own git worktree. Each makes one commit per TODO ID, adds tests, and passes `npm run build`, `cargo check`, and `cargo test` before reporting. Workers **never edit `TODO.md` or `AGENT.md`**; they report the doc changes they want instead.
- **Reviewers** read each worker's commits without the worker's reasoning. They check correctness and the invariants in §4/§9.
- **Integrator** is the only writer of `TODO.md`/`AGENT.md` during a wave. It cherry-picks approved commits onto `develop` one at a time, folds the doc updates into the same commit, reruns build + tests, and reverts anything that fails. Failed or rejected tasks are marked blocked in `TODO.md` with the reason, not retried indefinitely.
- After each wave the coordinator pauses so the maintainer can launch the app and check the UI. Agents never run the app against the real library (CLAUDE.md rule 7).
- **Ask the maintainer before starting** FEAT-2, FEAT-7, FEAT-8, FEAT-9, FEAT-10, ARCH-3, or major-version upgrades in DEP-4.

---

## 3. Architecture

```
React (src/)  ──invoke()──▶  #[tauri::command] (src-tauri/src/commands/*)
     ▲                              │
     │  emit() events               ▼
     └──────────────────────  db.rs (rusqlite)  ◀── utils/{scanner,parser,thumbnail}.rs
                                    │                         │
                              restack.db (SQLite)       walkdir / ffprobe / ffmpeg
                                                              
player.rs ── spawns mpv with --input-ipc-server=/tmp/mpv-<uuid>.sock
          └─ background thread reads IPC → writes progress to DB → emits events
```

The frontend **never** touches the filesystem or DB directly; everything goes through Tauri commands and events.

### File map

```
src/
  main.jsx                   HashRouter + StrictMode
  App.jsx                    Routes, cached library on startup then background scan, all global
                             Tauri event listeners, Ctrl/Cmd+R = rescan
  index.css                  Tailwind v4 `@theme` design tokens (colors, fonts, shadows)
  store/libraryStore.js      Hand-rolled pub/sub store (library + currently playing path)
  layouts/MainLayout.jsx     Shell: Sidebar + main + TopSyncBar + FirstScanOverlay
  layouts/Sidebar.jsx        Nav, series list, drag-and-drop reclassify targets
  pages/Dashboard.jsx        Continue Watching / Up Next
  pages/SeriesIndex.jsx      Grid of all series
  pages/SeriesDetail.jsx     Largest page (~720 lines): seasons, grid/list, multi-select,
                             mark watched, move, delete/undo, rename, merge suggestions
  pages/MoviesPage.jsx       Movies grid
  pages/OthersPage.jsx       Unclassified files
  components/library/*       Domain components (ShowCard, EpisodeGrid/List, modals, banners…)
  components/ui/*            Primitives (Button, Checkbox, Input, Modal, Select, Tab, FirstScanOverlay)
  utils/cn.js                clsx + tailwind-merge

src-tauri/src/
  main.rs                    calls restack_lib::run()
  lib.rs                     Plugins, setup (open DB, init_schema, migrate_existing_groups), command registry
  models.rs                  Serde structs returned to the frontend (camelCase)
  db.rs                      ALL SQL: schema/migrations, upserts, queries, groups, progress, delete/restore
  commands/scanner.rs        scan_user_storage, scan_and_sync, get_scan_roots
  commands/library.rs        get_library_grouped, continue watching, delete/restore, reclassify
  commands/groups.rs         rename/merge/move groups, group list, merge suggestions
  commands/player.rs         play_episode (mpv + IPC tracker), set_watched_cmd
  utils/parser.rs            Filename → Series / Movie / Other
  utils/scanner.rs           Video ext filter, skip dirs, default roots, ffprobe duration,
                             walk (collect_video_files) + missing-row decision (reconcile_unseen)
  utils/thumbnail.rs         ffmpeg thumbnail generation + cache
  utils/trash.rs             freedesktop Trash: move to trash, restore by original path
```

---

## 4. Backend details

### Database (`$APPDATA/restack.db`, i.e. `~/.local/share/com.jay.restack/restack.db`)

```sql
show_groups (id PK AUTOINCREMENT, display_name TEXT, normalized_name TEXT UNIQUE)
app_state   (key TEXT PK, value_text TEXT, updated_at INTEGER)   -- e.g. 'last_watched_episode_id'
media (
  id INTEGER PK, file_path TEXT UNIQUE NOT NULL,
  show_name TEXT NOT NULL,          -- series: parsed show; movie: title; other: raw stem
  season INTEGER, episode INTEGER,  -- 0/0 for movies & others
  title TEXT, duration INTEGER, last_position INTEGER, watched INTEGER(0/1),
  thumbnail_path TEXT,
  group_id INTEGER,                 -- → show_groups.id; NULL for movie/other
  user_pinned INTEGER DEFAULT 0,    -- 1 = user manually moved/reclassified; scans must not override
  deleted_at INTEGER,               -- soft delete (unix secs)
  media_type TEXT DEFAULT 'series', -- 'series' | 'movie' | 'other'
  year INTEGER,                     -- movies
  missing_since INTEGER             -- file not found by a scan but storage may be offline (unix secs); row hidden
)
```

**Migrations** are additive and live in `init_schema()`: check `PRAGMA table_info(media)` and `ALTER TABLE ... ADD COLUMN` if missing. Follow that pattern; there is no migration framework and existing user DBs must keep working. Indexes on migrated columns (`idx_media_group_season_episode`, `idx_media_type_live`) must be created after the `ADD COLUMN` migrations in `init_schema()`, or older DBs fail to open.

**Connection model**: every command calls `open_db(&app)` + `init_schema(&conn)` fresh. There is no pool or shared connection in `AppState` (which only holds the mpv `Child`). Keep that pattern unless you're deliberately refactoring it everywhere.

**Errors**: all DB/command functions return `Result<T, String>` with `map_err(|e| format!("Failed to …: {e}"))`. Match that style.

### Grouping

- `normalize_name()` = lowercase, alphanumerics only. `find_or_create_group()` keys on it, so `Show.Name` and `show name` collapse into one group.
- `rename_group` changes only `display_name`, never `normalized_name`.
- Merge suggestions = pairs of groups with Levenshtein distance 1–2 on normalized names (`strsim`).
- Upserts honour `user_pinned`: once a user moves/reclassifies a file, rescans keep its group/type/season/episode.
- Only series rows have a `group_id`; movies/others are always `NULL`. `migrate_existing_groups` (startup) backfills series rows once, guarded by `app_state.migration_groups_v1`, and on that run also cleared phantom groups that older builds gave movies/others.

### Scanning (`commands/scanner.rs`)

- Default roots (`discover_default_scan_roots`): **all of `$HOME`**, `/media/<username>`, every dir in `/mnt`.
- Skips dirs named `.cache .git node_modules target dist Trash` and `.Trash-*`. Extensions: `mkv mp4 avi webm`.
- Two passes: collect files → emit `scan:start {total_files}` → per-file `scan:progress {current,total,file_name}`.
- ffprobe duration + ffmpeg thumbnail run **only for files not already in the DB** (expensive).
- `commands/scanner.rs::sync_library` does the upserts and reconciliation without an `AppHandle`; the command passes a closure that emits progress and probes new files.
- After the scan, rows whose `file_path` wasn't seen are reconciled (`utils/scanner.rs::reconcile_unseen`, then `db::sync_missing_media`):
  - Only rows under a root that existed and was walked **this** scan are considered, minus subtrees that had walk errors. `scan_and_sync(root)` never touches rows outside `root`.
  - Soft-deleted rows (`deleted_at` set) are never touched, so Undo keeps working.
  - If the file's folder still exists and has other content, the file was really removed → **hard-delete**.
  - Otherwise (unmounted drive, offline NAS, removed folder) → set `missing_since`; the row is hidden but keeps progress/pinning. Upserts clear `missing_since` when the file reappears.
- Every live-row query filters `deleted_at IS NULL AND missing_since IS NULL`.
- Returns the full `LibraryResponse`.

### Parsing (`utils/parser.rs`)

Order: series → movie → other. `clean_text()` strips `[...]`/`(...)`, turns `._-` into spaces, removes quality/codec/language tags (`1080p`, `x265`, `web-dl`, `hindi`, `bluray`, …).

1. `Name S01E02 [title]`
2. `Name 1x02 [title]`
3. `Name Season 1 Episode 2 [title]`
4. `Name E04 [title]` → season 1
5. `Name - 04` (anime style, checked on the lightly cleaned stem) → season 1
6. Movie: `Title 19xx|20xx ...` (year required)
7. Else Other (raw stem)

When adding a pattern, insert it at the right priority and think about false positives (e.g. a year looking like an episode number).

### Playback (`commands/player.rs`)

- `play_episode(filePath, startSeconds?)`: start = `startSeconds ??` the DB `last_position`, except that a position in the last 10% (credits) restarts from 0 (`resume_position`). Kills any previous mpv child, spawns
  `mpv <file> --input-ipc-server=/tmp/mpv-<uuid>.sock --keep-open=no --no-resume-playback --script-opts=autoload-disabled=yes --force-window=no [--start=N]`.
- A thread observes `time-pos`, `duration` and `pause` over IPC on one DB connection per playback. It writes and emits progress at most every 5 s of position or wall time (`should_persist`), right away when watched flips, and on pause and when the loop exits for any reason. **Watched latches** at ≥ 90% (`WATCHED_THRESHOLD`): an already-watched item stays watched while rewatching (`is_watched`). Watched also latches within a session, so seeking back below 90% does not un-mark it. On `eof` the position resets to 0 (watched = true).
- On `end-file` with reason `eof` it looks up the next episode in the group and emits `play_next_episode` **after** the IPC loop closes. That ordering avoids a Broken-pipe race, so don't move it.
- `update_progress` also sets `app_state.last_watched_episode_id` for **series** rows only, which drives Continue Watching. Manual mark watched/unwatched goes through `set_watched_cmd` (one transaction, position reset to 0) and never moves that pointer.
- Auto-play next (`get_next_episode`) and Up Next skip trashed, missing, and non-series rows.

### Thumbnails (`utils/thumbnail.rs`)

- Saved to `$APPDATA/thumbnails/<uuid-v5(file_path)>.jpg`, frame at **00:00:15**.
- Served to the webview via the asset protocol (`convertFileSrc`). `tauri.conf.json` scopes it to `$HOME/.local/share/com.jay.restack/thumbnails/**` and the CSP allows `asset:`/`http://asset.localhost`. **Changing the app identifier breaks thumbnails** unless you update that scope too.

### Delete / restore

- `delete_episodes_cmd` moves each file to the freedesktop Trash (`utils/trash.rs`, `trash` crate), then sets `deleted_at` for that file (`db::mark_deleted`) before the next one, so a mid-batch failure leaves the DB consistent.
- `restore_episodes_cmd` finds the most recent Trash item whose original path matches (`trash::os_limited::list`), restores it, then clears `deleted_at` (`db::mark_restored`), per file. This works for external drives (`$topdir/.Trash-$UID`), renamed duplicates, and `$XDG_DATA_HOME`.

---

## 5. Frontend ↔ backend contract

### Commands (registered in `lib.rs`)

| Command | JS args | Returns |
|---|---|---|
| `scan_user_storage` | — | `LibraryResponse` |
| `scan_and_sync` | `{ rootPath }` | `LibraryResponse` |
| `get_scan_roots` | — | `string[]` |
| `get_library_grouped` | — | `LibraryResponse` (no scan) |
| `get_continue_watching_single_cmd` | — | `ContinueWatchingItem \| null` |
| `play_episode` | `{ filePath, startSeconds }` | — |
| `set_watched_cmd` | `{ mediaIds, watched }` | — |
| `rename_group_cmd` | `{ groupId, newName }` | — |
| `merge_groups_cmd` | `{ sourceGroupId, targetGroupId }` | — |
| `move_episodes_cmd` | `{ mediaIds, targetGroupId }` | — |
| `get_all_groups_cmd` | — | `GroupInfo[]` |
| `get_merge_suggestions_cmd` | — | `MergeSuggestion[]` |
| `delete_episodes_cmd` | `{ filePaths }` | — |
| `restore_episodes_cmd` | `{ filePaths }` | — |
| `reclassify_media_cmd` | `{ mediaIds, targetType: 'series'\|'movie'\|'other', targetGroupId }` | — |

Tauri maps camelCase JS args to snake_case Rust params. Return structs use `#[serde(rename_all = "camelCase")]`.

`LibraryResponse = { series: ShowGroup[], movies: MovieItem[], others: OtherItem[] }`, `ShowGroup = { groupId, displayName, seasons: [{ season, episodes: Episode[] }] }`.

**Adding a command** takes four steps: write the fn in `db.rs`, add the `#[tauri::command]` wrapper in `commands/*.rs`, import it and add it to `generate_handler![]` in `lib.rs`, then add any new return type to `models.rs` with camelCase serde.

### Events (backend → frontend, all listened to in `App.jsx`)

| Event | Payload | Note |
|---|---|---|
| `scan:start` | `{ total_files }` | snake_case payload |
| `scan:progress` | `{ current, total, file_name }` | snake_case payload |
| `video_playback_started` | `filePath` string | |
| `video_progress_updated` | `{ file_path, last_position, watched }` | snake_case payload |
| `video_playback_stopped` | `filePath` string | |
| `play_next_episode` | next `filePath` string | frontend calls `play_episode` |

Event payload structs are **not** camelCase-renamed, unlike command return types. Keep the JS destructuring consistent with that.

### Drag and drop

Episode/movie/other cards set `dataTransfer` `text/plain` to `JSON.stringify({ mediaIds: [...] })`. Sidebar drop targets (Movies, Others, each series) call `reclassify_media_cmd` and then refresh via `get_library_grouped`.

---

## 6. Frontend conventions

- **React 19, JSX (no TypeScript)**, function components + hooks, React Router v7 with **HashRouter** (`vite base: "./"`).
- **State**: `libraryStore.js` only, with `getX()` / `setLibrary()` / `subscribe()` / `subscribePlaying()`. Components subscribe in `useEffect` and return the unsubscribe function. Don't add Redux/Zustand/Context for library data. `updateEpisodeProgress` copies only the path to the matching item and does not notify listeners when nothing changed.
- After a mutating command, refresh with `invoke("get_library_grouped")` → `setLibrary(data)`. Don't rescan unless that's the intent.
- **Styling**: Tailwind v4 via `@tailwindcss/vite` (no `tailwind.config.js`). Use the tokens in `index.css` `@theme` (`bg-bg`, `bg-surface`, `bg-surface-card`, `text-text-secondary`, `text-accent`, `border-border`, `bg-progress`, `font-display`, …) over raw colors. Compose classes with `cn()`.
- **Motion**: `framer-motion` (`motion`, `AnimatePresence`). **Icons**: `lucide-react`.
- Dark-only UI, rounded cards, `select-none` app shell.
- Thumbnails: always `convertFileSrc(thumbnailPath)`, and `draggable="false"` on `<img>`.

---

## 7. Known issues / sharp edges

Check these before you assume a behaviour is intended. This list only covers sharp edges an agent must know. The full backlog of bugs and improvements lives in `TODO.md` (see §10).

1. **Sync commands run on the main thread.** All commands are non-`async` `fn`. Tauri 2 runs those on the main thread, so a long `scan_user_storage` (ffprobe + ffmpeg per new file) can stall the app. Prefer `async` commands or `spawn_blocking` for heavy work.
2. **Scanning all of `$HOME`** is slow on large homes. The narrower `Videos/Downloads/Movies` roots are commented out in `utils/scanner.rs`.
3. **Missing vs soft-deleted rows**: a rescan never purges soft-deleted rows, and hides (rather than deletes) rows whose storage may be offline (`missing_since`). Neither kind is purged automatically yet (see BUG-35).
4. `get_all_groups` / merge-suggestion counts include soft-deleted and missing rows, and empty groups are only garbage-collected by merge and by the one-time startup migration.
5. Unchanged files get `duration = 0` / `thumb = None` on rescan. The upserts handle this via `CASE`/`COALESCE`, so preserve that logic if you edit upserts.
6. Many `invoke` calls swallow errors (`catch {}`). The App-level scan is intentionally silent.
7. `index.css` imports Google Fonts. That conflicts with offline-first, and the CSP (`default-src 'self'`) likely blocks it in the webview, so the system font fallback is what renders.
8. mpv sockets live in `/tmp/mpv-<uuid>.sock` and are cleaned up when the tracker ends.

---

## 8. Spec vs. reality

`README.md` describes the MVP. The code has since grown:

- The mpv IPC progress sync described as "advanced/optional" **is implemented**, along with auto-play of the next episode.
- Added beyond the spec: `show_groups` with rename/merge/move/merge-suggestions, Movies/Others classification, drag-to-reclassify, soft delete to Trash with restore, Continue Watching/Up Next, first-scan overlay with live progress.
- Thumbnail frame is 15s (spec says 10s), and ffprobe is used for duration.

Still backlog: subtitle management, optional TMDB metadata, multi-user profiles, watch-history analytics, user-configurable scan roots / settings page, incremental filesystem watching.

---

## 9. Rules for agents

- Linux only. Don't add Windows/macOS branches.
- Keep all SQL in `db.rs`. Command files stay thin (open DB → init schema → call db fn).
- Schema changes must be additive and backward compatible via the `PRAGMA table_info` pattern.
- Respect `user_pinned`, `deleted_at`, and `missing_since` in any new query or upsert.
- Don't introduce new heavy dependencies, background daemons, servers, or network calls without a clear reason. The product is offline-first and minimal.
- Verify with `npm run build` and `cd src-tauri && cargo check && cargo test` before calling work done. Add tests next to the code you change, and keep them off the real DB/`$HOME`/Trash.
- Update this file when you change architecture, commands, events, schema, or conventions.
- Keep `TODO.md` in sync with every change, following §10.

---

## 10. Task tracking (`TODO.md`)

`TODO.md` at the repo root is the project backlog and implementation roadmap. It is committed. Treat TODO updates as part of the change and commit them with the code they describe. A stale TODO is worse than none, because the next session will trust it.

**Before you start work**
- Read `TODO.md`. Check whether the task is already tracked, and read related items. Many items reference each other (e.g. SEC-1 and BUG-1 share a fix).
- Move the item you're working on into **🔥 Now** and mark it `[~]`.

**While you work**
- If you find a new bug, risk, or debt, add it to the matching section with the next free ID for that prefix. Don't fix unrelated things silently, and don't leave them unrecorded.
- Every item needs: **Location** (file:line or function), **Problem**, **Solution**, plus **Priority** (Critical/High/Medium/Low) and **Type** (Confirmed/Potential/Improvement) in the title line. Only mark it Confirmed if you verified it.

**When you finish**
- Mark finished items `[x]` and move them to **✅ Done** with the date and a one-line summary.
- If you fixed only part of an item, leave it open and rewrite it to describe what remains.
- Re-check items that touch the code you changed. Update line numbers, priorities, or wording that your change made wrong, and close items your change made obsolete.
- Never renumber or reuse IDs. Commits may reference them ("fixes BUG-2").
- If an item changes a documented invariant, sharp edge, command, or schema, update the relevant section of this file in the same change.

*Last updated: 2026-11-04*
