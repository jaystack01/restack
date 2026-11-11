# AGENT.md — Restack

Authoritative context for any AI agent or contributor working in this repo. It describes the code as it **is**; `README.md` is the original product spec and is partly outdated (see "Spec vs. reality").

---

## 1. What this is

**Restack** is a **Linux-only Tauri 2 desktop app** that indexes local video files, organizes them into **Series → Seasons → Episodes**, **Movies**, and **Others**, tracks watch progress, and hands playback to **mpv**.

It is *not* a player, streaming server, or media center. Keep it offline-first, lightweight, fast, and minimal.

- npm package / Cargo crate: `restack` (lib crate: `restack_lib`)
- Tauri identifier: `com.jay.restack`. Changing it moves the app data dir, which orphans the user's DB and thumbnails.
- Runtime deps on the host: `mpv`, `ffmpeg`, `ffprobe` (all must be on `PATH`)

---

## 2. Commands

```bash
npm install
npm run tauri dev        # full app (runs vite on :1420 + Rust). Script forces GDK_BACKEND=x11
npm run tauri build      # release bundle
npm run dev              # frontend only (vite, port 1420, strictPort) — invoke() calls will fail outside Tauri
npm run build            # frontend production build → dist/
npm run lint             # ESLint (react, react-hooks, no-shadow)
npm run format           # Prettier on the frontend + cargo fmt on the backend
npm run format:check     # same, check only

cd src-tauri && cargo check    # type-check backend
cd src-tauri && cargo test     # backend unit tests (in-memory SQLite + temp dirs)
cd src-tauri && cargo clippy --all-targets -- -D warnings   # lint backend (kept at 0 warnings)
```

- **Tests**: backend only, as `#[cfg(test)]` modules in `db.rs` (in-memory SQLite: upsert/pinning invariants, missing/soft-delete rows, migrations, aliases and merges, end-of-scan re-keying, group GC, live-only counts; plus WAL, busy-timeout and read-only-DB tests on temp-file DBs, and IMMEDIATE transactions against a concurrent writer on a temp WAL DB), `utils/scanner.rs` (walk + reconcile decisions, skip rules, username and default roots, `run_with_timeout`), `utils/thumbnail.rs` (seek position), `utils/parser.rs` (table-driven filename classification cases, including regression tests for BUG-15 to BUG-18 and BUG-38), `commands/scanner.rs` (`sync_library` end to end on temp dirs with a fake probe, including per-file upsert failures forced by test-only SQLite triggers a scan batch waiting on a concurrent writer, and probe retries for known files with a zero duration or a missing thumbnail), `commands/player.rs` (recorded mpv IPC lines through `drive_ipc` with a fake clock: throttle, watched latch, pause, eof → play next, read errors; the resume/watched rules; mpv argument order, socket dir, session-scoped reaping with stand-in `true`/`sleep` children, and the playback event payload shape), and `models.rs` / `commands/library.rs` (`ScanGuard` exclusivity and release, Trash ops rejected during a scan, `run_trash_batch` id validation with a fake file op, playback session ids strictly increase). They never touch the real DB, `$HOME`, or Trash. Tests for open bugs are `#[ignore = "BUG-x: …"]` and fail today; remove the `#[ignore]` when fixing that item (`cargo test -- --ignored` lists them).
- **No frontend tests yet.** CI (`.github/workflows/ci.yml`) runs lint, format check, build, clippy, and tests on every push to `develop`/`main` and on PRs. Verify locally before committing with `npm run build`, `npm run lint`, `npm run format:check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- **Formatting**: Prettier with default options for `src/` and the root JS/JSON/HTML/CSS (`.prettierignore` skips `src-tauri`, `dist`, and Markdown), and default `cargo fmt` for Rust. Run `npm run format` before committing; don't hand-format against it.

### Git workflow

- `main`: released code only. Each merge into it is a release, tagged `vX.Y.Z`. Never commit to it directly.
- `develop`: the working branch. Commit directly to it; no per-task branches. Test the combined app here before a release.
- One commit per task: each commit covers a single TODO item (or a tightly related set) together with its TODO.md/AGENT.md updates. Don't mix unrelated tasks in one commit.
- Release: `git checkout main && git merge --no-ff develop && git tag vX.Y.Z`.

### Commit messages

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <description>`.

- **Types:** `feat`, `fix`, `refactor`, `perf`, `style`, `test`, `docs`, `chore`, `build`, `ci`, `revert`.
- **Scopes** (use these; omit the scope when none fits, and add a new one here before using it):

  | Scope | Covers |
  |---|---|
  | `scanner` | `commands/scanner.rs`, `utils/scanner.rs`, `utils/thumbnail.rs` |
  | `parser` | `utils/parser.rs` |
  | `player` | `commands/player.rs`, mpv IPC, playback events |
  | `db` | `db.rs` schema, migrations, queries |
  | `library` | `commands/library.rs`, delete/restore, `utils/trash.rs`, reclassify |
  | `groups` | `commands/groups.rs`, rename/merge/move |
  | `ui` | pages, layouts, components, styling |
  | `store` | `src/store/libraryStore.js` |
  | `deps` | npm/Cargo dependency changes |

- Subject: lowercase, imperative mood, under 72 characters, no trailing period, no emojis, specific (never "update files" / "fix bugs").
- Body only when it adds context, mainly *why*. TODO IDs go in a `Refs:` footer, not the subject, e.g. `Refs: PERF-6`. AI-assisted commits keep the `Co-Authored-By:` trailer after it.
- Example:

  ```
  perf(player): throttle progress writes and patch store in place

  mpv reports time-pos many times per second; persisting each tick cost
  an fsync and a full library re-render.

  Refs: PERF-6
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  ```

### Git hygiene

- Commits are organised around application changes. Don't make a separate commit for each edit to `CLAUDE.md`, `AGENT.md`, prompts, or agent workflow; put doc updates in the commit of the change they describe, or group related tooling/doc changes into one meaningful `docs:`/`chore:` commit.
- Never commit temporary AI notes, intermediate plans, debugging logs, worktrees (`.claude/worktrees/`), secrets, or environment files. Keep useful project documentation, even when it is about AI tooling.
- Stage specific paths, never blindly `git add -A`; check `git diff --cached` before committing; never include unrelated or pre-existing changes.
- No history rewrites, `reset --hard`, or force pushes without the maintainer's explicit approval.
- Commit and push only as authorized: the multi-agent wave workflow below may commit to `develop` and push it after a wave; anything else waits for the maintainer.

### Multi-agent waves (how the TODO backlog is worked)

The backlog is worked in **waves** of parallel agents. The maintainer has approved pushing to `origin/develop` after each wave (never `main`).

- **Coordinator** (the main session) plans each wave so that no two workers touch the same file. It also owns the final review and the push.
- **Workers** each run in their own git worktree. Each makes one commit per TODO ID (message per "Commit messages"), adds tests, and passes `npm run build`, `cargo check`, and `cargo test` before reporting. Workers **never edit `TODO.md` or `AGENT.md`**; they report the doc changes they want instead.
- **Reviewers** read each worker's commits without the worker's reasoning. They check correctness and the invariants in §4/§9.
- **Integrator** is the only writer of `TODO.md`/`AGENT.md` during a wave. It cherry-picks approved commits onto `develop` one at a time, folds the doc updates into the same commit, reruns build + tests, and commits with a message that follows "Commit messages". If a task fails, it sets the half-applied change aside with `git stash push` (never `reset --hard`) and keeps going. Failed or rejected tasks are marked blocked in `TODO.md` with the reason, not retried indefinitely.
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
                                                              
player.rs ── spawns mpv with --input-ipc-server=$XDG_RUNTIME_DIR/mpv-<uuid>.sock (or /tmp)
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
  api.js                     One wrapper per Tauri command, plus refreshLibrary() (get_library_grouped → setLibrary)
  store/libraryStore.js      Hand-rolled pub/sub store (library + currently playing path and session)
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
  lib.rs                     Builder setup, no plugins (open DB, init_schema, migrate_existing_groups), command registry, `run_blocking`
  models.rs                  Serde structs returned to the frontend (camelCase), `AppState`, `PlayerSession`, `next_playback_session`, `ScanGuard`
  db.rs                      ALL SQL: schema/migrations, upserts, queries, groups, progress, delete/restore
  commands/scanner.rs        scan_user_storage, scan_and_sync, get_scan_roots
  commands/library.rs        get_library_grouped, continue watching, delete/restore, reclassify
  commands/groups.rs         rename/merge/move groups, group list, merge suggestions
  commands/player.rs         play_episode (mpv + IPC tracker), set_watched_cmd; pure parse_ipc_line +
                             ProgressTracker, drive_ipc loop, track_progress I/O glue
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
show_group_aliases (normalized_name TEXT PK, group_id INTEGER NOT NULL)  -- extra names that resolve to a group (merged-away sources, re-keyed old names)
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

**Connection model**: every command calls `open_db(&app)` + `init_schema(&conn)` fresh. There is no pool or shared connection in `AppState` (which holds the current mpv as a `PlayerSession` and the `scan_running` flag). `open_db` calls `configure_connection` on every connection: an explicit 5 s `BUSY_TIMEOUT` and `PRAGMA journal_mode=WAL`. WAL is stored in the file, so old DBs convert on their first open; if the switch fails (read-only file or directory) it is logged and the connection is kept in its old journal mode. Keep that pattern unless you're deliberately refactoring it everywhere. Transactions that read and then write begin with `db::begin_write` (`BEGIN IMMEDIATE`), never `unchecked_transaction()`: in WAL a deferred transaction that has read fails at once with `SQLITE_BUSY_SNAPSHOT` if another connection committed in between, while IMMEDIATE takes the write lock up front so the busy timeout applies. Savepoints nest inside it.

**Errors**: all DB/command functions return `Result<T, String>` with `map_err(|e| format!("Failed to …: {e}"))`. Match that style.

### Grouping

- `normalize_name()` = lowercase, alphanumerics only. `find_or_create_group()` keys on it, so `Show.Name` and `show name` collapse into one group.
- `find_group()` (lookup only) checks `show_group_aliases` first (joined to live groups), then `show_groups.normalized_name`; `find_or_create_group()` creates a group only when both miss.
- `merge_groups` runs in one transaction and errors on unknown ids: it moves the rows, re-points the source's aliases to the target, records the source's normalized name as an alias of the target (`INSERT OR IGNORE`), and deletes the source. Chains collapse onto the final target, so a merged-away name never recreates its group on rescan.
- The scan upsert resolves (or creates) a group only for new rows or rows with a NULL `group_id`. Existing rows keep their group; the write uses a compare-and-set on the group read just before (`media.group_id IS ?`), so a concurrent move or merge on another connection is never overwritten.
- At the end of every scan, `sync_missing_media` runs `rekey_renamed_shows` (in a savepoint; errors are logged, never fail the scan) and then `delete_empty_groups`. Re-keying follows show names changed by parser fixes (e.g. the BUG-17 year strip, "Doctor Who 2005" → "Doctor Who"). A group with no pinned rows whose live rows parse to a strict prefix of its key, and whose file names contain the full old key, is re-keyed **in place**: same id, old key kept as an alias, a user-renamed `display_name` kept. A group that already owns the new name is absorbed only if it holds nothing but that show's own newer episodes (every file name contains the old key) and isn't pinned or renamed. Two distinct existing shows are **never** merged automatically (`The.Flash.1990` / `2014` stay separate and a note is logged). All decisions are made before any change, so walk order doesn't matter.
- `delete_empty_groups` removes groups no `media` row references and aliases of deleted groups. Soft-deleted and missing rows keep their group, so Undo and a remounted drive return to it. It also runs inside `merge_groups`, `move_episodes`, and `reclassify_media` (all transactional).
- `rename_group` changes only `display_name`, never `normalized_name`.
- Merge suggestions = pairs of groups with Levenshtein distance 1–2 on normalized names (`strsim`).
- Upserts honour `user_pinned`: once a user moves/reclassifies a file, rescans keep its group/type/season/episode.
- Only series rows have a `group_id`; movies/others are always `NULL`. `migrate_existing_groups` (startup) backfills series rows once, guarded by `app_state.migration_groups_v1`, and on that run also cleared phantom groups that older builds gave movies/others.

### Scanning (`commands/scanner.rs`)

- Default roots (`discover_default_scan_roots`): **all of `$HOME`**, `/media/<user>`, `/run/media/<user>` (udisks2), and every dir in `/mnt`. `<user>` comes from `$USER`, then `$LOGNAME`, then the `$HOME` basename (`current_username`).
- Skip rules (`skip_kind`) apply only below a root (a root itself is always walked, even a hidden one): hidden dirs, `node_modules`, `target`, `dist` (`SkipKind::Excluded`), and `Trash` / `.Trash-*` (`SkipKind::Trash`). Extensions: `mkv mp4 avi webm`.
- Two passes: collect files → emit `scan:start {total_files}` → per-file `scan:progress {current,total,file_name}`.
- ffprobe and ffmpeg are expensive, so `sync_library` decides per file what to run (`ProbeNeed`, from `db::get_known_media`): new files get both; a known file gets ffprobe only when its duration is 0 and ffmpeg only when `thumbnail_path` is NULL or its file is gone from disk. Complete rows are never re-probed, and a failed retry keeps the existing values (`CASE`/`COALESCE`). A file whose probe always fails is retried on every scan, bounded by the 30/60 s timeouts (DEP-8). The open batch is committed before any probe.
- `commands/scanner.rs::sync_library` does the upserts and reconciliation without an `AppHandle`; the command passes a closure that emits progress and runs the probes its `ProbeNeed` asks for.
- `sync_library` commits upserts in batches (`UpsertBatch`: 500 rows or 1 s) and commits the open batch before probing each new file, so SQLite's write lock is never held across ffprobe/ffmpeg (the mpv tracker writes progress on its own connection during scans). The reconcile step runs afterwards in `sync_missing_media`'s own transaction, so don't call `sync_missing_media` while a batch is open: `begin_write` would nest.
- Each upsert runs in a savepoint inside the open batch. A file whose upsert fails is rolled back on its own, logged (`Skipping <path>: …`) and skipped; the rest of the batch commits. It still counts as seen, so reconciliation never deletes or hides its existing row. Only batch-level failures (begin/commit, or SQLite abandoning the whole transaction, detected via `is_autocommit()`) abort the scan, and so do `MAX_CONSECUTIVE_FAILURES` (20) per-file failures in a row with no success between them, so a read-only or broken DB still returns `Err`.
- ffprobe and ffmpeg run through `utils/scanner.rs::run_with_timeout` (std only: stdout drained on a thread, `try_wait` polling). `FFPROBE_TIMEOUT` is 30 s and `THUMBNAIL_TIMEOUT` 60 s. On timeout the child is killed and reaped on a background thread, and the file gets duration 0 / no thumbnail (a per-file failure, logged, retried on the next scan).
- After the scan, rows whose `file_path` wasn't seen are reconciled (`utils/scanner.rs::reconcile_unseen`, then `db::sync_missing_media`):
  - Only rows under a root that existed and was walked **this** scan are considered, minus subtrees that had walk errors and skipped hidden/build dirs (`ScanWalk.skipped_dirs`; rows there are left untouched). Trash dirs are not in `skipped_dirs`, so stale rows indexed from them by older builds are still reconciled. `scan_and_sync(root)` never touches rows outside `root`.
  - Soft-deleted rows (`deleted_at` set) are never touched, so Undo keeps working.
  - If the file's folder still exists and has other content, the file was really removed → **hard-delete**.
  - Otherwise (unmounted drive, offline NAS, removed folder) → set `missing_since`; the row is hidden but keeps progress/pinning. Upserts clear `missing_since` when the file reappears.
- Every live-row query filters `deleted_at IS NULL AND missing_since IS NULL`.
- Only one scan runs at a time. `scan_roots_and_sync` takes a `ScanGuard` (models.rs, RAII over `AppState.scan_running`), and a second `scan_user_storage`/`scan_and_sync` returns `Err("A scan is already running")` instead of waiting. The guard is released on every path, including errors and panics. Delete/restore hold the same flag, so a scan also fails with that error while a Trash batch runs (see PERF-16).
- Returns the full `LibraryResponse`.

### Parsing (`utils/parser.rs`)

Order: series → movie → other. `clean_text()` strips `[...]`/`(...)`, turns `._-` into spaces, and removes quality/codec/language tags (`1080p`, `x265`, `web-dl`, `hindi`, `bluray`, …) and WxH resolutions (`1920x1080`). Series show names lose a trailing `19xx|20xx` (`Doctor.Who.2005` → "Doctor Who") unless the year is the whole name (`1923`).

1. `Name S01E02 [title]`, also `S01 E02` and multi-episode `S01E01E02` (episode = first)
2. `Name 1x02 [title]` (season 1–2 digits, episode 1–3 digits, so resolutions never match)
3. `Name Season 1 Episode 2 [title]`
4. `Name E04 [title]` → season 1
5. `Name - 04` (anime style, checked on the lightly cleaned stem) → season 1. Up to 4 digits, but never 19xx/20xx. The dash must follow a space or a non-digit, and a stem with an ISO date (`2024-05-14`) skips this rule.
6. Movie: a parenthesised `(19xx|20xx)` in the raw stem wins, with the title taken from the text before it. Otherwise the **last** `19xx|20xx` token is the year. A year is required. Stems with an ISO date are never movies.
7. Else Other (raw stem)

When adding a pattern, insert it at the right priority and think about false positives (e.g. a year looking like an episode number).

### Playback (`commands/player.rs`)

- `play_episode(filePath, startSeconds?)`: start = `startSeconds ??` the DB `last_position`, except that a position in the last 10% (credits) restarts from 0 (`resume_position`). Kills any previous mpv child, spawns
  `mpv --input-ipc-server=<sock> --keep-open=no --no-resume-playback --script-opts=autoload-disabled=yes --force-window=no [--start=N] -- <file>`, built by the pure `mpv_args`. The file always comes last, after `--`, so a path can never be parsed as an mpv option. `<sock>` is `$XDG_RUNTIME_DIR/mpv-<uuid>.sock` when `$XDG_RUNTIME_DIR` is an absolute path to an existing dir (per-user, mode 0700), else `/tmp/mpv-<uuid>.sock` (`mpv_socket_dir`).
- A thread observes `time-pos`, `duration` and `pause` over IPC on one DB connection per playback. It writes and emits progress at most every 5 s of position or wall time (`should_persist`), right away when watched flips, and on pause and when the loop exits for any reason. **Watched latches** at ≥ 90% (`WATCHED_THRESHOLD`): an already-watched item stays watched while rewatching (`is_watched`). Watched also latches within a session, so seeking back below 90% does not un-mark it. On `eof` the position resets to 0 (watched = true).
- Structure: `parse_ipc_line` turns an mpv JSON line into an `IpcEvent`. `ProgressTracker` (pure, time is passed in) owns the position/watched/throttle state and returns what to persist and when to exit. `drive_ipc` is the loop (read, parse, handle, persist, break on exit or read error, final flush) and is shared by `track_progress` and the tests. `track_progress` only does the I/O: socket setup, DB writes, events.
- On `end-file` with reason `eof`, `TrackerExit::play_next` is set. Only after `drive_ipc` returns does `track_progress` look up the next episode in the group, remove the socket, emit `video_playback_stopped`, and then emit `play_next_episode`. That ordering avoids a Broken-pipe race, so don't move it.
- `update_progress` also sets `app_state.last_watched_episode_id` for **series** rows only, which drives Continue Watching. Manual mark watched/unwatched goes through `set_watched_cmd` (one transaction, position reset to 0) and never moves that pointer.
- Auto-play next (`get_next_episode`) and Up Next skip trashed, missing, and non-series rows.
- Each `play_episode` takes a session id from `models::next_playback_session()` (a process-wide, strictly increasing static `AtomicU64`). `AppState.player_process` holds `Option<PlayerSession { session, child }>`.
- `play_episode` (body in `start_playback`) only swaps the `PlayerSession` while holding the `player_process` lock. Killing the old mpv and spawning the new one happen outside it, and if two plays race, the newest mpv is kept and the replaced one is killed.
- After the post-loop events, the tracker calls `reap_session`: under the lock it `try_wait`s only its own session's child and clears the slot once mpv has exited, sleeping outside the lock between checks (at most 20 × 100 ms). It never kills or reaps a newer session's mpv, so no zombie mpv lingers until the next play.

### Thumbnails (`utils/thumbnail.rs`)

- Saved to `$APPDATA/thumbnails/<uuid-v5(file_path)>.jpg`, frame at `min(15 s, 10% of duration)`, or 1 s when the duration is unknown (`thumbnail_seek`). ffmpeg (60 s timeout) writes `<uuid>.jpg.part`, which is renamed into place only on success and removed otherwise, so an existing `<uuid>.jpg` is always complete.
- Served to the webview via the asset protocol (`convertFileSrc`). `tauri.conf.json` scopes it to `$APPDATA/thumbnails/**`, which Tauri resolves through `app_data_dir()` (the dir `utils/thumbnail.rs` writes to), so it follows the identifier and `$XDG_DATA_HOME`. The CSP allows `asset:`/`http://asset.localhost`.

### Delete / restore

- The webview never supplies paths: both commands take media ids, and `db::resolve_trash_paths(conn, ids, TrashAction::Delete|Restore)` resolves them in request order (duplicates dropped). Delete accepts only live rows (`deleted_at IS NULL AND missing_since IS NULL`), restore only trashed rows (`deleted_at` set). If any id is unknown or in the wrong state, the whole batch is rejected before any Trash move.
- `commands/library.rs::run_trash_batch` then runs the file op per path (trash or restore) and marks that row before the next one. Tests drive it with a fake file op.
- `delete_episodes_cmd` moves each file to the freedesktop Trash (`utils/trash.rs`, `trash` crate), then sets `deleted_at` for that file (`db::mark_deleted`) before the next one, so a mid-batch failure leaves the DB consistent.
- `restore_episodes_cmd` finds the most recent Trash item whose original path matches (`trash::os_limited::list`), restores it, then clears `deleted_at` (`db::mark_restored`), per file. This works for external drives (`$topdir/.Trash-$UID`), renamed duplicates, and `$XDG_DATA_HOME`.
- Both commands run inside `exclusive_with_scan` (commands/library.rs), which holds the `ScanGuard` for the whole batch and fails with `Failed to delete|restore: a library scan is running` during a scan. Otherwise a scan that collected the path earlier would upsert the trashed row back to live (`deleted_at = NULL`), or hard-delete a just-restored one. Two delete/restore batches also exclude each other.

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
| `delete_episodes_cmd` | `{ mediaIds }` | — |
| `restore_episodes_cmd` | `{ mediaIds }` | — |
| `reclassify_media_cmd` | `{ mediaIds, targetType: 'series'\|'movie'\|'other', targetGroupId }` | — |

Tauri maps camelCase JS args to snake_case Rust params. Return structs use `#[serde(rename_all = "camelCase")]`. All commands are `async` and return `Result` (§7.1); `delete_episodes_cmd`/`restore_episodes_cmd` return an error while a scan is running.

`LibraryResponse = { series: ShowGroup[], movies: MovieItem[], others: OtherItem[] }`, `ShowGroup = { groupId, displayName, seasons: [{ season, episodes: Episode[] }] }`.

The frontend calls commands only through `src/api.js` (one exported function per command, JSDoc'd args), never `invoke()` directly.

**Adding a command** takes five steps: write the fn in `db.rs`, add the `#[tauri::command]` wrapper in `commands/*.rs`, import it and add it to `generate_handler![]` in `lib.rs`, add any new return type to `models.rs` with camelCase serde, then add its JS wrapper to `src/api.js`. The wrapper is an `async fn` that runs its body through `crate::run_blocking` (§7.1).

### Events (backend → frontend, all listened to in `App.jsx`)

| Event | Payload | Note |
|---|---|---|
| `scan:start` | `{ total_files }` | snake_case payload |
| `scan:progress` | `{ current, total, file_name }` | snake_case payload |
| `video_playback_started` | `{ file_path, session }` | snake_case payload; `session` from `next_playback_session()` |
| `video_progress_updated` | `{ file_path, last_position, watched }` | snake_case payload |
| `video_playback_stopped` | `{ file_path, session }` | snake_case payload; frontend clears Playing only on a matching session (`clearPlayingSession`) |
| `play_next_episode` | next `filePath` string | frontend calls `play_episode` |

Event payload structs are **not** camelCase-renamed, unlike command return types. Keep the JS destructuring consistent with that.

### Drag and drop

Episode/movie/other cards set `dataTransfer` `text/plain` to `JSON.stringify({ mediaIds: [...] })`. Sidebar drop targets (Movies, Others, each series) call `reclassify_media_cmd` and then refresh via `get_library_grouped`.

---

## 6. Frontend conventions

- **React 19, JSX (no TypeScript)**, function components + hooks, React Router v7 with **HashRouter** (`vite base: "./"`).
- **State**: `libraryStore.js` only, with `getX()` / `setLibrary()` / `subscribe()` / `subscribePlaying()`. Components subscribe in `useEffect` and return the unsubscribe function. Don't add Redux/Zustand/Context for library data. `updateEpisodeProgress` copies only the path to the matching item and does not notify listeners when nothing changed. The store also tracks the playing session: `setPlayingFilePath(path, session)` on start, `clearPlayingSession(session)` on stop (a no-op unless it matches), so a replaced mpv's late stop event can't clear a newer play. Components still read `getPlayingFilePath()` / `subscribePlaying()`.
- Call the backend through `src/api.js`, never `invoke()` directly (`convertFileSrc` and `listen` are fine to import). After a mutating command, refresh with `await refreshLibrary()` (fetches `get_library_grouped` → `setLibrary`). Don't rescan unless that's the intent.
- **Styling**: Tailwind v4 via `@tailwindcss/vite` (no `tailwind.config.js`). Use the tokens in `index.css` `@theme` (`bg-bg`, `bg-surface`, `bg-surface-card`, `text-text-secondary`, `text-accent`, `border-border`, `bg-progress`, `font-display`, …) over raw colors. Compose classes with `cn()`.
- **Motion**: `framer-motion` (`motion`, `AnimatePresence`). **Icons**: `lucide-react`.
- Dark-only UI, rounded cards, `select-none` app shell.
- `components/ui/Modal` is an accessible dialog: `role=dialog`, focus trap, Escape closes it, and focus returns to the previous element on close. Mark the child that should get initial focus with `data-autofocus` (prefer Cancel for destructive confirms). Page-level `window` key handlers must ignore events while a dialog is open or whose target is inside `[role=dialog]`.
- Thumbnails: always `convertFileSrc(thumbnailPath)`, and `draggable="false"` on `<img>`.

---

## 7. Known issues / sharp edges

Check these before you assume a behaviour is intended. This list only covers sharp edges an agent must know. The full backlog of bugs and improvements lives in `TODO.md` (see §10).

1. **Commands run on the blocking pool.** Every `#[tauri::command]` is `async fn` and runs its sync body through `crate::run_blocking` (lib.rs: `tauri::async_runtime::spawn_blocking`, JoinError mapped to `Failed to run background task: …`). New commands must follow this pattern, because a plain sync `fn` runs on the main thread and freezes the window. Commands take owned args and use `app.state::<AppState>()` inside the closure instead of a `State<'_>` parameter. As a result, UI commands can now run while a scan is open, and DB writes rely on the explicit 5 s busy timeout set in `open_db` (the DB runs in WAL, so readers don't block on the writer; read-then-write transactions begin IMMEDIATE via `db::begin_write`, §4). Anything that changes `deleted_at` or moves files (delete/restore) must be exclusive with scans (see §4 Delete / restore).
2. **Scanning all of `$HOME`** is slow on large homes. Hidden dirs below a root are skipped (BUG-20), but large non-hidden trees are still walked until FEAT-1 adds configurable roots (PERF-2). The narrower `Videos/Downloads/Movies` roots are commented out in `utils/scanner.rs`.
3. **Missing vs soft-deleted rows**: a rescan never purges soft-deleted rows, and hides (rather than deletes) rows whose storage may be offline (`missing_since`). Neither kind is purged automatically yet (see BUG-35).
4. `get_all_groups` and merge-suggestion counts include only live series rows; merge suggestions skip groups with no live episodes. Groups whose only rows are trashed or missing are kept (for Undo/remount) and show 0 episodes in pickers.
5. Unchanged files that aren't re-probed (and failed retries) get `duration = 0` / `thumb = None` on rescan. The upserts handle this via `CASE`/`COALESCE`, so preserve that logic if you edit upserts.
6. Many `invoke` calls swallow errors (`catch {}`). The App-level scan is intentionally silent.
7. `index.css` imports Google Fonts. That conflicts with offline-first, and the CSP (`default-src 'self'`) likely blocks it in the webview, so the system font fallback is what renders.
8. mpv sockets live in `$XDG_RUNTIME_DIR/mpv-<uuid>.sock` (fallback `/tmp/mpv-<uuid>.sock`, see `mpv_socket_dir`) and are cleaned up when the tracker ends.

---

## 8. Spec vs. reality

`README.md` describes the MVP. The code has since grown:

- The mpv IPC progress sync described as "advanced/optional" **is implemented**, along with auto-play of the next episode.
- Added beyond the spec: `show_groups` with rename/merge/move/merge-suggestions, Movies/Others classification, drag-to-reclassify, soft delete to Trash with restore, Continue Watching/Up Next, first-scan overlay with live progress.
- Thumbnail frame is at `min(15 s, 10% of duration)` (spec says 10s), and ffprobe is used for duration.

Still backlog: subtitle management, optional TMDB metadata, multi-user profiles, watch-history analytics, user-configurable scan roots / settings page, incremental filesystem watching.

---

## 9. Rules for agents

- Linux only. Don't add Windows/macOS branches.
- Keep all SQL in `db.rs`. Command files stay thin (open DB → init schema → call db fn).
- Schema changes must be additive and backward compatible via the `PRAGMA table_info` pattern.
- Respect `user_pinned`, `deleted_at`, and `missing_since` in any new query or upsert.
- Don't introduce new heavy dependencies, background daemons, servers, or network calls without a clear reason. The product is offline-first and minimal.
- Verify with `npm run build`, `npm run lint`, `npm run format:check`, and `cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test` before calling work done. Add tests next to the code you change, and keep them off the real DB/`$HOME`/Trash.
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

*Last updated: 2026-11-11*
