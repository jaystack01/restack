# TODO — Restack

Project backlog and progress tracker (committed alongside the code). The maintenance rules are in `AGENT.md` §10.

**How to use**
- Each item has a stable ID so it can be referenced in commits ("fixes BUG-2"). Prefixes: `ARCH` architecture, `DES` code design, `BUG` bugs, `SEC` security & data integrity, `PERF` performance, `DEP` dependencies & config, `TEST` testing, `MAINT` maintainability, `FEAT` features, `CHORE` legacy chores (kept their IDs when re-filed into the sections below).
- Status: `[ ]` open · `[~]` in progress · `[x]` done (move to **Done** with the date).
- Priority: **Critical** (data loss / unusable) · **High** (breaks data or UX badly) · **Medium** (should fix soon) · **Low** (nice to have). Legacy P1/P2/P3 were mapped to High/Medium/Low.
- Type: **Confirmed** (verified by reading the code end to end, running it, or checking the local DB) · **Potential** (plausible, needs a repro) · **Improvement** (not a defect).
- Add new items at the bottom of their section with the next free ID for that prefix.

---

## 🔥 Now (current focus)

_Nothing picked yet. Move items here when you start them._

## 🧭 Suggested order (from the 2026-10-28 audit)

1. **Stop data loss**: SEC-1, SEC-2, BUG-1, BUG-3, BUG-8 (wrong files can be trashed).
2. **Fix silent state corruption**: SEC-3, BUG-9, BUG-10, BUG-11, BUG-12.
3. **Make it responsive**: PERF-1, PERF-6, PERF-7, PERF-8.
4. **Safety net before refactors**: CHORE-1, TEST-1, TEST-2, CHORE-2, CHORE-3.
5. **Then the structure**: ARCH-*, DES-*, MAINT-*.

---

## 🏛️ Architecture and Project Structure

- [ ] **ARCH-1 · Medium · Confirmed** — Scan orchestration and raw SQL live in a command file.
  - **Location:** `src-tauri/src/commands/scanner.rs:23-123` (`scan_roots_and_sync`, SQL at :32 and :115)
  - **Problem:** AGENT.md says command files stay thin and all SQL lives in `db.rs`. The scanner command walks the filesystem, decides what is new, runs ffprobe/ffmpeg, emits events, and runs its own `SELECT`/`DELETE`. That logic can't be tested without an `AppHandle`, and it is where BUG-1 and SEC-1/SEC-2 hide.
  - **Solution:** Add a `services/scan.rs` (or `utils/scan.rs`) with `fn sync(conn: &mut Connection, files: &[String], probe: &dyn MediaProbe, on_progress: impl FnMut(..))`. Move `SELECT file_path` and the delete loop into `db.rs` (`get_all_file_paths`, `remove_missing_media`). Keep the command as open DB → call service → emit.

- [ ] **ARCH-2 · Medium · Confirmed** — `player.rs` mixes four concerns in one deeply nested loop.
  - **Location:** `src-tauri/src/commands/player.rs:94-194` (`track_progress`)
  - **Problem:** Process lifecycle, mpv JSON-IPC protocol parsing, DB writes, and event emission are interleaved six levels deep. Throttling (PERF-6), rewatch semantics (BUG-12), and testing the `eof → next` ordering all require editing the same block.
  - **Solution:** Extract `fn parse_ipc_line(&str) -> Option<IpcEvent>` (pure, testable), a small `ProgressTracker` struct that decides when to persist (throttle and watched latch), and keep the thread loop as glue. Preserve the "emit `play_next_episode` after the loop exits" invariant.

- [ ] **ARCH-3 · Medium · Improvement** — Media identity is the file path.
  - **Location:** `src-tauri/src/db.rs:42-56` (`file_path TEXT UNIQUE`), `commands/scanner.rs:112-118`
  - **Problem:** Renaming or moving a file (e.g. tidying `~/Downloads` into `~/Videos/Show/`) is seen as delete + new file. Progress, watched state, `user_pinned`, and group assignment are lost, and ffprobe/ffmpeg rerun.
  - **Solution:** During a scan, before hard-deleting a missing row, try to match a new file by `(size, mtime)` or `(dev, inode)`. Store these in new additive columns. If matched, `UPDATE media SET file_path = new` instead of delete + insert.

- [ ] **ARCH-4 · Medium · Improvement** — Group identity has no alias concept, so merges and moves don't steer future scans.
  - **Location:** `src-tauri/src/db.rs:120-141` (`find_or_create_group`), `:443-461` (`merge_groups`)
  - **Problem:** Groups are keyed only by `normalized_name`. After a merge, the source name is gone, so the next file with that name creates a fresh group (see BUG-13). The same goes for any show whose release names vary ("Show 2005" vs "Show", BUG-17).
  - **Solution:** Add an additive `show_group_aliases(normalized_name TEXT PRIMARY KEY, group_id INTEGER)` table. `find_or_create_group` checks aliases first. `merge_groups` inserts the source's normalized name as an alias of the target.

- [ ] **ARCH-5 · Medium · Improvement** — No frontend API layer: `invoke()` strings and arg shapes are scattered across about 10 files.
  - **Location:** `src/App.jsx`, `src/layouts/Sidebar.jsx`, `src/pages/*.jsx`, `src/components/library/{MergeGroupModal,RenameGroupModal,ShowCard}.jsx`
  - **Problem:** Command names and camelCase arg shapes are repeated (e.g. `play_episode` with `startSeconds: null` appears in six places). A rename or signature change is a grep hunt, and the "mutate, then `get_library_grouped`, then `setLibrary`" refresh pattern is copy-pasted and sometimes done wrong (BUG-8).
  - **Solution:** Add `src/api.js` exporting typed-by-JSDoc wrappers (`playMedia(path)`, `reclassify(ids, type, groupId)`, `refreshLibrary()` which also calls `setLibrary`). Route all calls through it.

- [ ] **ARCH-6 · Low · Confirmed** — The DB layer performs filesystem and Trash I/O.
  - **Location:** `src-tauri/src/db.rs:828-882` (`delete_episodes`, `restore_episodes`)
  - **Problem:** `db.rs` is documented as "all SQL", but these functions trash and restore files, parse `.trashinfo`, and rename across directories. That mixes failure domains and makes the SQL half untestable without real files.
  - **Solution:** Move the trash/restore logic to `utils/trash.rs`. Keep `db::mark_deleted(paths)` / `db::mark_restored(paths)` as pure SQL. Have the command orchestrate both, per file (see BUG-7).

- [ ] **ARCH-7 · Low · Confirmed** — Stringly-typed media types across the boundary.
  - **Location:** `src-tauri/src/db.rs:476-523` (`reclassify_media(target_type: &str)`), `models.rs:73-82` (`ContinueWatchingItem.mode`, `up_next_type`)
  - **Problem:** An unknown `target_type` is silently a no-op that returns `Ok(())`. Typos in JS can't be caught.
  - **Solution:** Use `#[derive(Deserialize)] #[serde(rename_all = "lowercase")] enum MediaType { Series, Movie, Other }` for the command arg, and serialize enums for `mode`/`upNextType`.

- [ ] **ARCH-8 · Low · Improvement** — Mobile/Windows template scaffolding in a Linux-only app.
  - **Location:** `src-tauri/src/main.rs:2` (`windows_subsystem`), `src-tauri/src/lib.rs:17` (`mobile_entry_point`), `src-tauri/Cargo.toml:15` (`staticlib`, `cdylib`), `tauri.conf.json:33-40` (`.icns`, `.ico`)
  - **Problem:** These contradict the "Linux only, no Windows/macOS branches" rule and add build output for nothing.
  - **Solution:** Drop `staticlib`/`cdylib` (keep `rlib` + bin), remove the cfg attrs, and trim the bundle icons to PNG sizes.

## 🧩 Code Design and Quality

- [ ] **DES-1 · Medium · Confirmed** — Multi-statement mutations aren't wrapped in transactions.
  - **Location:** `src-tauri/src/db.rs:443-461` (`merge_groups`), `:464-473` (`move_episodes`), `:476-523` (`reclassify_media`), `:828-882`, `commands/scanner.rs:70-118`
  - **Problem:** A failure halfway leaves partial state (some episodes moved, group not deleted, and so on). Loops issue one autocommit statement per row (see PERF-7 for the cost).
  - **Solution:** Take `&mut Connection`, use `let tx = conn.transaction()?; …; tx.commit()?`. Use `UPDATE … WHERE id IN (…)` for the id loops.

- [ ] **DES-2 · Low · Confirmed** — Errors are silently swallowed or dropped in the DB layer.
  - **Location:** `src-tauri/src/db.rs:386,415` (`filter_map(|r| r.ok())` drops movie/other rows), `:496,504` (`unwrap_or((1,0))`, `unwrap_or(0)`), `:637-647` (`get_last_position` maps every error to 0), `:649-681` (`.ok()?` everywhere)
  - **Problem:** A bad row disappears from the library without a trace. A DB lock or error during reclassify silently numbers the episode as S01E01.
  - **Solution:** Propagate errors with `?` and `format!("Failed to …: {e}")`. Use `.optional()` (rusqlite `OptionalExtension`) for "no row" instead of catch-all `Err(_)`.

- [ ] **DES-3 · Low · Confirmed** — Error-message style is inconsistent with the project convention.
  - **Location:** `commands/scanner.rs:32-33,116` (`e.to_string()`), `commands/player.rs:74`, `utils/scanner.rs:23,30`, `utils/thumbnail.rs:10,12` (`format!("…{}", e)`)
  - **Problem:** CLAUDE.md requires `format!("Failed to …: {e}")`. Bare `e.to_string()` gives the user/log no context.
  - **Solution:** Normalize these when touching the files.

- [ ] **DES-4 · Low · Confirmed** — Three near-duplicate upserts and hand-rolled row tuples.
  - **Location:** `src-tauri/src/db.rs:174-273` (`upsert_episode/movie/other`), `:303-325`, `:719-794` (10- and 8-element tuples)
  - **Problem:** The `user_pinned` / `COALESCE` / `CASE` invariants are repeated three times and must be kept in sync. Positional tuples are easy to misorder.
  - **Solution:** Use one `upsert_media(conn, path, MediaKind, duration, thumb)` with a shared `ON CONFLICT` clause. Add `fn episode_from_row(&Row)` helpers.

- [ ] **DES-5 · Low · Confirmed** — Duplicated store-subscription boilerplate in every page.
  - **Location:** `src/layouts/Sidebar.jsx:17-33`, `src/pages/Dashboard.jsx:12-27`, `SeriesIndex.jsx:11-17`, `MoviesPage.jsx:10-22`, `OthersPage.jsx:18-26`, `SeriesDetail.jsx:31-62`
  - **Problem:** Six copies of `useState(getX()) + useEffect(subscribe)`. One of them shadows the store's `setLibrary` (BUG-8).
  - **Solution:** Add `useLibrary()` and `usePlayingFilePath()` hooks (built on `useSyncExternalStore`) in `src/store/libraryStore.js`.

- [ ] **DES-6 · Low · Confirmed** — Duplicated helpers.
  - **Location:** `formatTime` in `components/library/ContinueWatchingRow.jsx:6` and `EpisodeList.jsx:5`; `formatDuration` in `pages/OthersPage.jsx:8`; `getFileName` in `ContinueWatchingRow.jsx:15` and `SeriesDetail.jsx:23`; progress-percent calc in 5 components
  - **Solution:** Move them to `src/utils/format.js`.

- [ ] **DES-7 · Low · Confirmed** — Magic numbers are spread across layers.
  - **Location:** watched threshold `0.9` (`player.rs:141`), thumbnail seek `00:00:15` (`thumbnail.rs:31`), socket wait `15×200ms` (`player.rs:98-103`), `EPISODE_CLICK_DELAY_MS` (`SeriesDetail.jsx:22`), undo window `10000` (`SeriesDetail.jsx:372`), `MIN_SCAN_DISPLAY_MS` (`App.jsx:20`)
  - **Problem:** FEAT-1 (settings) will need several of these. Today they're unnamed literals.
  - **Solution:** Use named `const`s at module top (Rust) or a `src/config.js`, and later the settings table.

- [ ] **DES-8 · Low · Confirmed** — Selection identity is mixed (file path vs id).
  - **Location:** `src/pages/SeriesDetail.jsx:38` (Set of `filePath`), drag payloads use `id` (`EpisodeGrid.jsx:27-29`), move uses ids (`:346`), delete/restore use paths (`:358`)
  - **Problem:** Two keys for the same entity make bugs like BUG-8 more likely and complicate a future id-based backend.
  - **Solution:** Key selection by `id` and map to paths only at the delete/restore call.

## 🐛 Bugs and Logical Errors

- [ ] **BUG-1 · High · Confirmed** — Rescan wipes soft-deleted rows.
  - **Location:** `src-tauri/src/commands/scanner.rs:30-39,112-118`
  - **Problem:** `scan_roots_and_sync` hard-deletes any row whose file wasn't seen, including trashed episodes. Their progress is lost, and undo/restore can't clear `deleted_at`. Related: SEC-1.
  - **Solution:** Skip rows with `deleted_at IS NOT NULL` in the "remove deleted files" loop. Purge them only after the undo window or on Trash empty.

- [ ] **BUG-2 · Medium · Confirmed** — Auto-play next can pick deleted or non-series files.
  - **Location:** `src-tauri/src/db.rs:663-681` (`get_next_episode`); the same gap exists in the Up Next query at `:760-794` (no `media_type = 'series'`)
  - **Solution:** Add `AND deleted_at IS NULL AND media_type = 'series'` to both.

- [ ] **BUG-3 · High · Confirmed** — Restore only works for home-partition files with unique names.
  - **Location:** `src-tauri/src/db.rs:843-882` (`restore_episodes`)
  - **Problem:**
    1. It assumes the trashed name equals the original basename. With two same-named files the freedesktop spec renames to `name.2.mkv`, so restore fails or restores the wrong one.
    2. It hardcodes `~/.local/share/Trash`. Files on `/media/<user>/…` or `/mnt/…` (both default scan roots) are trashed by the `trash` crate into `$topdir/.Trash-$UID`, so **Undo always fails for external drives**, and that trash dir then gets indexed (BUG-20).
    3. It ignores `$XDG_DATA_HOME`.
    4. If the 2nd of N files fails, the 1st is restored on disk but `deleted_at` is never cleared.
  - **Solution:** Use the `trash` crate's Linux API (`trash::os_limited::list()` then filter by `original_parent.join(name) == path`, then `trash::os_limited::restore_all`). That handles topdir trash, name collisions, and XDG paths. Update `deleted_at` per file as each restore succeeds. Drop the `urlencoding` dependency afterwards.

- [ ] **BUG-4 · Low · Confirmed** — `get_all_groups` / merge-suggestion episode counts include soft-deleted and non-series rows.
  - **Location:** `src-tauri/src/db.rs:526-552`, `:555-602`
  - **Solution:** `LEFT JOIN media m ON m.group_id = sg.id AND m.deleted_at IS NULL AND m.media_type = 'series'`.

- [ ] **BUG-5 · Low · Confirmed** — Empty `show_groups` are never cleaned up.
  - **Location:** `src-tauri/src/db.rs` (no GC anywhere); amplified by SEC-3 and BUG-13
  - **Problem:** Evidence: the local DB has 43 groups and **0** of them have a live series episode. They still appear in `get_all_groups` pickers and merge suggestions.
  - **Solution:** Run `DELETE FROM show_groups WHERE id NOT IN (SELECT DISTINCT group_id FROM media WHERE group_id IS NOT NULL)` at the end of each scan and after move/reclassify/merge. Keep alias rows (ARCH-4) pointing to live groups only.

- [ ] **BUG-7 · Medium · Confirmed** — Delete trashes files one by one and only marks the DB at the end.
  - **Location:** `src-tauri/src/db.rs:828-841`
  - **Problem:** A mid-batch failure leaves earlier files trashed but still visible in the library. Their rows then get hard-deleted at the next scan (BUG-1).
  - **Solution:** Mark `deleted_at` per file right after each successful `trash::delete`. Return a partial-failure list to the UI.

- [ ] **BUG-8 · High · Confirmed** — Delete trashes episodes that aren't on screen.
  - **Location:** `src/pages/SeriesDetail.jsx:38` (selection state), `:357-362` (`deleteSelectedEpisodes` uses `Array.from(selectedEpisodes)`)
  - **Problem:** The route `/series/:groupId` reuses the same component instance, so the `selectedEpisodes` Set survives navigating to another series. It also survives the search filter and collapsed seasons. Mark watched/unwatched/move filter by the current group, but **delete sends every selected path**. So the user can trash files from a series they're no longer looking at. The confirm dialog only shows a count.
  - **Solution:** Clear selection when `groupId` changes (`useEffect(() => setSelectedEpisodes(new Set()), [groupId])`, or `key={groupId}` on the route element). In `deleteSelectedEpisodes`, intersect with `selectedGroupData.episodes`. List the file names in the confirm modal.

- [ ] **BUG-9 · Medium · Confirmed** — Sidebar drag-to-reclassify doesn't update the global store.
  - **Location:** `src/layouts/Sidebar.jsx:12` (imports store `setLibrary`), `:17` (local `const [library, setLibrary] = useState(…)` shadows it), `:56`
  - **Problem:** After a drop, `setLibrary(updated)` calls the component's state setter, not the store. The Sidebar counts update, but the Movies/Others/Series pages keep showing the item in its old place until another refresh.
  - **Solution:** Rename the local state (e.g. `[counts, setCounts]`) or adopt DES-5's hook. Call the store's `setLibrary`.

- [ ] **BUG-10 · Medium · Confirmed** — Playing a movie or "other" file erases Continue Watching.
  - **Location:** `src-tauri/src/db.rs:608-635` (`update_progress` sets `last_watched_episode_id` for any media), `:701-740` (`JOIN show_groups` then deletes the pointer when there's no row)
  - **Problem:** Movies/others have `group_id = NULL`, so the JOIN finds nothing and the pointer is deleted. After a restart, SEC-3 gives them a phantom group, and the Dashboard then shows the movie as "S00E00" of a fake series.
  - **Solution:** Only set the pointer for `media_type = 'series'` (or track a separate pointer per type). Add `AND m.media_type = 'series'` to the lookup.

- [ ] **BUG-11 · Medium · Confirmed** — Bulk mark watched/unwatched hijacks Continue Watching.
  - **Location:** `src/pages/SeriesDetail.jsx:303-321` (N parallel `update_watch_progress`), `src-tauri/src/db.rs:619-632`
  - **Problem:** Every manual mark also rewrites `last_watched_episode_id`. Whichever parallel call finishes last becomes "Continue Watching", which is non-deterministic.
  - **Solution:** Split the API: `set_watched_cmd({ mediaIds, watched })` (one transaction, no pointer update) vs. the player's progress path, which updates the pointer.

- [ ] **BUG-12 · Medium · Confirmed** — Rewatching un-marks "watched" and resumes near the end.
  - **Location:** `src-tauri/src/commands/player.rs:140-143` (`watched = pos >= duration * 0.9` written every tick), `:46-47` (resume from DB position)
  - **Problem:**
    1. Opening a watched episode writes `watched = false` on the first tick.
    2. After finishing, `last_position` stays at about 90–100%, so the next play (and "Up Next" for a re-watched show) starts at the credits.
  - **Solution:** Latch watched (`watched = previously_watched || pos >= 0.9*dur`). On `eof`, or when crossing the threshold, reset `last_position` to 0. When starting a watched item with `startSeconds == null`, start from 0.

- [ ] **BUG-13 · Medium · Confirmed (latent: merge UI is unreachable, see MAINT-1)** — Merged groups resurrect and split new episodes.
  - **Location:** `src-tauri/src/db.rs:181` (`find_or_create_group` runs unconditionally before upsert), `:194-198`, `:443-461`
  - **Problem:** After merging A into B, group A is deleted. The next scan recreates A for every A-named file (an empty group, re-suggested for merge). Any **newly downloaded** A episode gets `group_id = A`, splitting the show again.
  - **Solution:** ARCH-4 (aliases). Also only call `find_or_create_group` when the row is new or `group_id IS NULL`.

- [ ] **BUG-14 · Medium · Confirmed** — A failed startup scan shows an empty library even though the DB has data.
  - **Location:** `src/App.jsx:45-49,60-63`
  - **Problem:** If `scan_user_storage` throws (any single upsert error aborts it via `?`, `commands/scanner.rs:101-107`), `libraryData` stays null, the overlay is dismissed, and every page renders empty with no error. This repeats on every launch.
  - **Solution:** On error, fall back to `invoke("get_library_grouped")` and show a non-blocking error (FEAT-3). In the scanner, log and skip per-file failures instead of aborting the whole scan.

- [ ] **BUG-15 · Medium · Confirmed** — Parser classifies resolution strings as episodes.
  - **Location:** `src-tauri/src/utils/parser.rs:64` (`(\d+)x(\d+)`)
  - **Problem:** `Inception.2010.1920x1080` becomes Series "Inception 2010" **S1920E1080** (verified with a harness against the real parser).
  - **Solution:** Bound the NxNN pattern (`\b(\d{1,2})x(\d{2,3})\b`) and/or strip `\d{3,4}x\d{3,4}` in `clean_text`. Add a test.

- [ ] **BUG-16 · Medium · Confirmed** — Date/time-stamped recordings parse as series.
  - **Location:** `src-tauri/src/utils/parser.rs:105-106` (dash-episode pattern)
  - **Problem:** `Screencast from 2024-05-14 12-30-45` becomes Series "Screencast from 2024 05" S1E14. `OBS 2024-05-14 10-30-12` does the same. Each recording session spawns a junk group.
  - **Solution:** Reject the dash pattern when the stem contains a `\d{4}-\d{2}-\d{2}` date. Require whitespace or a bracket before the dash when the left side ends in digits.

- [ ] **BUG-17 · Medium · Confirmed** — Multi-episode and year-suffixed releases are misgrouped.
  - **Location:** `src-tauri/src/utils/parser.rs:62-66`
  - **Problem:**
    1. `Breaking.Bad.S01E01E02` becomes **Other** (the `$` anchor fails on the trailing `E02`).
    2. `Doctor.Who.2005.S01E01` gives show "Doctor Who 2005", a different group from "Doctor Who" (Levenshtein 4, so no merge suggestion either).
    3. `show.s01.e02` gives show "show s01".
  - **Solution:**
    1. Allow `(?:E\d+)*` after the first episode.
    2. Strip a trailing `(19|20)\d{2}` from `show_name`.
    3. Support `S(\d+)\s*E(\d+)`.

- [ ] **BUG-18 · Low · Confirmed** — Movie year picks the first year-like number.
  - **Location:** `src-tauri/src/utils/parser.rs:45`
  - **Problem:** `Blade Runner 2049 (2017)` becomes title "Blade Runner", year 2049. The real year was in parentheses, which `clean_text` strips first.
  - **Solution:** Extract a parenthesised `(19|20)\d{2}` before bracket stripping and prefer it. Otherwise use the **last** year-like token.

- [ ] **BUG-19 · Medium · Confirmed** — Thumbnails are never generated for clips shorter than 15 s, and never retried.
  - **Location:** `src-tauri/src/utils/thumbnail.rs:30-31` (`-ss 00:00:15`), `commands/scanner.rs:92-96` (only for `is_new`)
  - **Problem:** Evidence: in the local DB all 24 thumbnail-less rows have duration 1–8 s. A failed ffprobe/ffmpeg (or a deleted thumbnails dir) is also permanent, because unchanged files are never re-probed.
  - **Solution:** Seek to `min(15, duration * 0.1)` (duration is known first). Regenerate when `thumbnail_path IS NULL`, the file at `thumbnail_path` no longer exists, or `duration = 0`, not only for new files.

- [ ] **BUG-20 · Medium · Confirmed** — The scanner indexes external-drive trash and misses udisks2 mounts.
  - **Location:** `src-tauri/src/utils/scanner.rs:45-51` (`should_skip_dir`), `:53-96` (`discover_default_scan_roots`)
  - **Problem:**
    1. Only a dir literally named `Trash` is skipped, so `/media/<user>/<disk>/.Trash-1000/files/*.mkv` is indexed as new media.
    2. On Arch/Fedora (including this CachyOS machine), udisks2 mounts removable drives at `/run/media/$USER`, which isn't a root, so they're never scanned.
    3. `/media/<name>` uses the basename of `$HOME` instead of the actual username.
  - **Solution:** Skip `.Trash-*` and, generally, hidden dirs (with an allowlist if needed). Add `/run/media/$USER`. Get the username from `$USER` / `users::get_current_username`.

- [ ] **BUG-21 · Low · Confirmed** — Skip-dir matches by name anywhere in the tree.
  - **Location:** `src-tauri/src/utils/scanner.rs:50`
  - **Problem:** A user folder called `dist` or `target` (e.g. `~/Videos/target/`) is silently skipped.
  - **Solution:** Match `target`/`dist`/`node_modules` only when the parent contains `Cargo.toml`/`package.json`, or make the exclusion list user-configurable (FEAT-1).

- [ ] **BUG-22 · Low · Confirmed** — Global keyboard handler hijacks Enter/Escape while the delete modal is open.
  - **Location:** `src/pages/SeriesDetail.jsx:411-457`, `src/components/ui/Modal.jsx` (no key handling)
  - **Problem:** With the confirm modal open, focus stays on the trash button behind it. Enter then **plays the focused episode** (and `preventDefault` blocks the button). Escape clears the selection but leaves the modal open.
  - **Solution:** Give Modal its own Escape handler, initial focus, and focus trap. Skip the page shortcuts while `deleteConfirmOpen` is true (or when `e.target` is inside `[role=dialog]`).

- [ ] **BUG-23 · Low · Confirmed** — A second delete within 10 s makes the Undo banner disappear early.
  - **Location:** `src/pages/SeriesDetail.jsx:372-373`
  - **Problem:** The previous `timeoutId` is overwritten without `clearTimeout`, so the first timer clears the second banner. Undo also covers only the last batch, and the timer isn't cleared on unmount.
  - **Solution:** `clearTimeout(recentlyDeleted?.timeoutId)` before setting a new one, and clear it in an unmount effect.

- [ ] **BUG-24 · Low · Confirmed** — Scan "done" timer can reset an in-progress scan's state.
  - **Location:** `src/App.jsx:68-69`
  - **Problem:** Pressing Ctrl+R within 2 s of a finished sync starts a new scan, then the old `setTimeout` sets `scanState` to idle mid-scan.
  - **Solution:** Keep the timer id in a ref and clear it at the start of `doSync`.

- [ ] **BUG-25 · Medium · Confirmed** — Event listeners leak and double-fire under StrictMode.
  - **Location:** `src/App.jsx:72-136`, `src/main.jsx` (StrictMode)
  - **Problem:** `listen()` is async. StrictMode's mount/unmount/mount runs the cleanup before the promises resolve, so the first set of unlisteners is never called. In `npm run tauri dev`, every event is handled twice: `play_next_episode` calls `play_episode` twice (killing the just-spawned mpv), and progress updates double.
  - **Solution:** Use `let cancelled = false;` and, after each `await listen(...)`, `if (cancelled) unlisten(); else cleanupFns.push(unlisten)`. Set `cancelled = true` in cleanup.

- [ ] **BUG-26 · Low · Confirmed** — "Recent" sort sorts by playback position, not recency.
  - **Location:** `src/pages/SeriesDetail.jsx:127-134`, options `:585-586`
  - **Problem:** `b.lastPosition - a.lastPosition` orders by seconds watched. The third "boost" branch is unreachable (only two options exist).
  - **Solution:** Add a `last_played_at` column (additive) set in `update_progress` and sort by it, or rename the option to "Progress". Remove the dead branch.

- [ ] **BUG-27 · Low · Confirmed** — Dashboard "Continue Watching" goes stale.
  - **Location:** `src/pages/Dashboard.jsx:29-33`
  - **Problem:** It's fetched once on mount, so it doesn't refresh after playback stops, after auto-play next, or after a scan while the Dashboard is visible. `onResume` (`:76-81`) has no `.catch`, so it throws an unhandled rejection if mpv is missing.
  - **Solution:** Refetch on `video_playback_stopped` / library changes (subscribe), and add error handling.

- [ ] **BUG-28 · Low · Confirmed** — A stale stop event clears "Playing" when the same file is replayed.
  - **Location:** `src/App.jsx:113-117`, `src-tauri/src/commands/player.rs:65-71,189`
  - **Problem:** Replaying the same file kills the old mpv. The old tracker's `video_playback_stopped(path)` arrives after the new `video_playback_started(path)`, and the path compare passes, so the UI shows nothing playing.
  - **Solution:** Include a playback session id (the socket uuid) in the start/stop payloads and compare that.

- [ ] **BUG-29 · Low · Confirmed** — Reclassify-to-series edge cases.
  - **Location:** `src-tauri/src/db.rs:490-519`
  - **Problem:**
    1. Season-0 specials (`S00E01`) are treated as "unnumbered" and renumbered to S01E(max+1).
    2. Moving S01E03 into a group that already has S01E03 creates duplicate episode numbers with undefined order and auto-play next.
    3. An unknown `target_type` returns `Ok`.
  - **Solution:** Use `season IS NULL`-style sentinel logic (or check `media_type != 'series'`) instead of `season == 0`. Detect collisions and append. Return an error on an unknown type (ARCH-7).

- [ ] **BUG-30 · Low · Confirmed** — mpv is never reaped after it exits on its own.
  - **Location:** `src-tauri/src/commands/player.rs:76-78`, `models.rs:5-7`
  - **Problem:** The `Child` stays in `AppState` until the next `play_episode`, so a zombie `mpv` process lingers.
  - **Solution:** Have the tracker thread `try_wait()`/`wait()` and clear `AppState.player_process` when the IPC loop ends (pass the `AppHandle` and use `app.state::<AppState>()`).

- [ ] **BUG-31 · Low · Potential** — Progress tracking silently gives up on slow mpv startup.
  - **Location:** `src-tauri/src/commands/player.rs:98-111`
  - **Problem:** It waits at most 3 s for the socket. On a cold network/USB mount mpv can take longer, so playback works but no progress is saved, with no signal to the user. A `expect()` on `try_clone` at `:121` can also panic the thread.
  - **Solution:** Retry `connect` until the child exits (check `try_wait`) rather than a fixed count. Replace `expect` with an early return and a log.

- [ ] **BUG-32 · Low · Confirmed** — Latent runtime error in `MergeGroupModal`.
  - **Location:** `src/components/library/MergeGroupModal.jsx:3,106`
  - **Problem:** `AlertCircle` is used but `AlertTriangle` is imported, so a ReferenceError fires whenever the error state renders. The component is currently unreachable (MAINT-1).
  - **Solution:** Fix the import when reviving the merge UI, or delete the file.

- [ ] **BUG-33 · Low · Confirmed** — Undefined Tailwind classes silently do nothing.
  - **Location:** `animate-in`/`fade-in`/`slide-in-from-top-2` (`SeriesDetail.jsx:484`, `MultiSelectBar.jsx:29`, `GroupActionsMenu.jsx:41`, `RenameGroupModal.jsx:42`, `MergeSuggestionBanner.jsx:23`) need the `tailwindcss-animate` plugin, which isn't installed. `bg-surface-2` (`RenameGroupModal.jsx:62`) isn't a theme token.
  - **Solution:** Replace these with `framer-motion` (already used) or define the keyframes/tokens in `index.css`.

- [ ] **BUG-34 · Low · Confirmed** — List-view hover styles never apply.
  - **Location:** `src/components/library/EpisodeList.jsx:74,78` (`group-hover:*`), button at `:39-62` has no `group` class
  - **Solution:** Add `group` to the button's className.

## 🔐 Security and Data Integrity

- [ ] **SEC-1 · High · Confirmed** — Unmounted drive plus rescan hard-deletes everything on that drive.
  - **Location:** `src-tauri/src/commands/scanner.rs:45-49` (missing roots silently skipped), `:112-118` (delete every unseen row)
  - **Problem:** `/media/<user>` and `/mnt/*` are default roots. If an external drive is unplugged (or a NAS is offline) at startup, the automatic startup scan deletes all its rows: progress, watched state, manual `user_pinned` organisation, and moves. Plugging it back in re-imports everything as new.
  - **Solution:** Only purge rows whose path is under a root that **exists and was walked** this scan. Better: add an additive `missing_since INTEGER` column, hide missing rows, and purge after N days or on user action. Walk errors (`filter_map(Result::ok)` at `:54`) should mark that subtree "not fully scanned" so its rows are kept.

- [ ] **SEC-2 · High · Confirmed (latent)** — `scan_and_sync(rootPath)` deletes the whole library outside `rootPath`.
  - **Location:** `src-tauri/src/commands/scanner.rs:131-135` then `:112-118`
  - **Problem:** `existing_files` is every row in the DB, but `seen_files` only covers `rootPath`. Calling it on `~/Videos` hard-deletes every other series/movie row. It isn't called from the UI today, but it's a registered command and FEAT-1 plans to use it.
  - **Solution:** Restrict the delete set to `file_path LIKE root || '/%'` for the roots actually scanned (shared fix with SEC-1).

- [ ] **SEC-3 · Medium · Confirmed** — Startup migration assigns phantom groups to movies and others.
  - **Location:** `src-tauri/src/lib.rs:25` (runs on every launch), `src-tauri/src/db.rs:144-168` (`WHERE group_id IS NULL`, no `media_type` filter)
  - **Problem:** Movies/others intentionally have `group_id = NULL`. Every launch gives each one a `show_groups` row named after its title. The next scan resets unpinned rows to NULL, but **pinned** rows keep it. Evidence: local DB row 27 (`media_type='other'`, `user_pinned=1`, `group_id=25`). The phantom groups pollute `get_all_groups` and merge suggestions and feed BUG-10.
  - **Solution:** Add `AND media_type = 'series'` to the backfill query. Run it once, guarded by an `app_state` key such as `migration_groups_v1`. Clean up existing phantom groups (BUG-5).

- [ ] **SEC-4 · Medium · Potential** — Destructive commands trust arbitrary paths from the webview.
  - **Location:** `src-tauri/src/commands/library.rs:20-24` (`delete_episodes_cmd`), `:27-31` (`restore_episodes_cmd`), `commands/player.rs:36-92` (`play_episode`)
  - **Problem:** Any string sent from JS is trashed or handed to `mpv` as an argument. The CSP is strict and there's no remote content, so exploitation needs an XSS first. But this is the app's only file-destroying path and it does no validation.
  - **Solution:** Have the commands accept `media_ids`, resolve paths from the DB, and refuse paths not in `media`. For mpv, pass `--` before the file argument so a path can never be parsed as an option.

- [ ] **SEC-5 · Low · Confirmed** — The mpv IPC socket is in world-readable `/tmp`.
  - **Location:** `src-tauri/src/commands/player.rs:50`
  - **Problem:** Other local users can see the socket name. mpv creates it with the default umask, so they could connect and send commands to mpv (which can run `run` subprocess commands).
  - **Solution:** Use `$XDG_RUNTIME_DIR` (per-user, mode 0700), falling back to `/tmp`.

- [ ] **SEC-6 · Medium · Confirmed** — The asset-protocol scope hardcodes the data dir.
  - **Location:** `src-tauri/tauri.conf.json:25` (`$HOME/.local/share/com.jay.restack/thumbnails/**`), versus `utils/thumbnail.rs:6-14` (`app_data_dir()`)
  - **Problem:** If `$XDG_DATA_HOME` is set, `app_data_dir()` resolves elsewhere and every thumbnail breaks. The identifier is also duplicated in two places (CLAUDE.md rule 8).
  - **Solution:** Use `"$APPDATA/thumbnails/**"` (a Tauri path variable).

- [ ] **SEC-7 · Low · Confirmed** — Unused plugin permissions are granted.
  - **Location:** `src-tauri/capabilities/default.json:6-10` (`opener:default`, `dialog:default`), `src-tauri/src/lib.rs:20-21`
  - **Problem:** Neither plugin is used by any frontend code. They widen the IPC surface for nothing.
  - **Solution:** Remove them (see DEP-2) until FEAT-1 needs the dialog.

- [ ] **SEC-8 · Low · Potential** — Non-UTF-8 paths are stored lossily.
  - **Location:** `src-tauri/src/commands/scanner.rs:57` (`to_string_lossy`)
  - **Problem:** A filename with invalid UTF-8 is stored with U+FFFD. mpv, trash, and restore then target a path that doesn't exist.
  - **Solution:** Skip and log such files (simplest), or store paths as BLOB/`OsString` bytes.

## ⚡ Performance

- [ ] **PERF-1 · High · Confirmed** — All Tauri commands are sync `fn`, so they run on the main thread.
  - **Location:** every `#[tauri::command]` in `src-tauri/src/commands/*.rs`
  - **Problem:** A long `scan_user_storage` (ffprobe + ffmpeg per new file) can freeze the window. Potential knock-on: WebKitGTK needs the main loop to deliver `emit()`s, so the live scan-progress UI may not repaint until the scan returns. Verify on a large library.
  - **Solution:** Make scan/thumbnail commands `async` or use `tauri::async_runtime::spawn_blocking`.

- [ ] **PERF-2 · Medium · Confirmed** — The default scan walks **all of `$HOME`**.
  - **Location:** `src-tauri/src/utils/scanner.rs:53-80`
  - **Problem:** That includes `~/.local/share/Steam`, `~/.var`, browser profiles, and so on.
  - **Solution:** User-configurable scan roots (FEAT-1). Until then consider `~/Videos`, `~/Downloads`, `~/Movies`, and skip hidden dirs (BUG-20).

- [ ] **PERF-3 · Medium · Improvement** — Thumbnail and duration generation is serial.
  - **Location:** `src-tauri/src/commands/scanner.rs:94-96`
  - **Solution:** Parallelise with a bounded worker pool, or generate thumbnails in the background after the library is shown.

- [ ] **PERF-4 · Low · Improvement** — Every command reopens the SQLite connection and re-runs `init_schema` (about 6 statements plus `PRAGMA table_info`).
  - **Location:** all command wrappers; also `player.rs:142,160` per progress tick
  - **Solution:** Hold one `Mutex<Connection>` in `AppState`. Run schema init once at startup. Enable `PRAGMA journal_mode=WAL` (currently `delete`) so the player thread doesn't contend with scans.

- [ ] **PERF-5 · Low · Improvement** — Merge suggestions are O(n²) Levenshtein over all groups.
  - **Location:** `src-tauri/src/db.rs:582-599`
  - **Problem:** Fine for now; revisit with large libraries. Also, an absolute distance ≤ 2 on short names ("Dark" vs "Dare") gives false positives.
  - **Solution:** Use a relative threshold (`distance <= max(1, len/8)`) and skip groups with 0 live episodes.

- [ ] **PERF-6 · High · Confirmed** — The player writes to the DB and emits an event on every mpv `time-pos` tick.
  - **Location:** `src-tauri/src/commands/player.rs:131-154`; frontend `src/store/libraryStore.js:28-60`
  - **Problem:** mpv reports `time-pos` many times per second. Each tick opens a **new** SQLite connection, runs two writes (each an fsync'd autocommit), and emits `video_progress_updated`. The frontend then deep-clones the whole library and re-renders every subscribed page, including the Sidebar and the O(n²) drag-id computation (PERF-11). This runs for the entire playback.
  - **Solution:** Persist at most every ~5 s, or when the integer second changes by ≥ 5, plus on pause/end-file/quit. Reuse one connection in the thread. Emit at the same throttled rate. In the store, update only the matching episode's season array.

- [ ] **PERF-7 · High · Confirmed** — Scan upserts run without a transaction.
  - **Location:** `src-tauri/src/commands/scanner.rs:70-118`
  - **Problem:** Each `find_or_create_group` + upsert + delete is its own autocommit transaction (fsync each). A rescan of N unchanged files costs at least N fsyncs, which dominates rescan time once the ffprobe/ffmpeg work is skipped.
  - **Solution:** Wrap the processing loop and the removal loop in one transaction, or commit every ~500 files so progress isn't lost on a crash.

- [ ] **PERF-8 · High · Confirmed** — The UI is blocked behind a full `$HOME` scan on every launch.
  - **Location:** `src/App.jsx:102` then `doSync`; `src/layouts/MainLayout.jsx:62-67` (overlay until `hasLibraryLoaded`)
  - **Problem:** The cached library in SQLite is never shown first, so every launch waits for a full filesystem walk.
  - **Solution:** On mount, `setLibrary(await invoke("get_library_grouped"))` and set `hasLibraryLoaded`. Then run the scan in the background with the TopSyncBar. Keep the full overlay only when the DB is empty.

- [ ] **PERF-9 · Medium · Confirmed** — Regexes are recompiled on every call.
  - **Location:** `src-tauri/src/utils/parser.rs:5-11` (`clean_text`, 4 regexes, called 2–4× per file), `:45`, `:62-66`, `:93`, `:105-106`
  - **Problem:** About 10–15 regex compilations per scanned file.
  - **Solution:** Use `static RE: LazyLock<Regex>` (std, Rust 1.80+).

- [ ] **PERF-10 · Medium · Confirmed** — ffprobe/ffmpeg have no timeout.
  - **Location:** `src-tauri/src/utils/scanner.rs:15-23`, `utils/thumbnail.rs:29-41`
  - **Problem:** A truncated file or a hung network mount stalls the scan forever, and with PERF-1 it also freezes the app.
  - **Solution:** Spawn with `wait_timeout` (the `wait-timeout` crate), or pass ffmpeg `-rw_timeout` plus kill-after-N-seconds logic.

- [ ] **PERF-11 · Low · Confirmed** — Episode grid/list work is quadratic and memo is defeated.
  - **Location:** `src/components/library/EpisodeGrid.jsx:27-29`, `EpisodeList.jsx:34-36` (per-card `episodes.filter(...)` every render), `src/pages/SeriesDetail.jsx:184-189,241,253` (new callbacks every render defeat `memo`)
  - **Solution:** Compute drag ids lazily inside `onDragStart`. Wrap the handlers in `useCallback`, and make `bindEpisodeRef` stable (a single ref callback reading `data-id`).

- [ ] **PERF-12 · Low · Improvement** — One `scan:progress` IPC event and one React state update per file.
  - **Location:** `src-tauri/src/commands/scanner.rs:79-86`, `src/App.jsx:89-97`
  - **Solution:** Throttle to about 10 events/s (time-based) and always send the final one.

- [ ] **PERF-13 · Low · Improvement** — Missing indexes for the hot filters.
  - **Location:** `src-tauri/src/db.rs:58-61`
  - **Problem:** Library queries filter on `media_type` and `deleted_at`, which aren't indexed.
  - **Solution:** `CREATE INDEX IF NOT EXISTS idx_media_type_deleted ON media(media_type, deleted_at)`.

- [ ] **PERF-14 · Low · Confirmed** — Orphaned thumbnails are never removed.
  - **Location:** `src-tauri/src/commands/scanner.rs:112-118` (row deleted, jpg kept)
  - **Problem:** Evidence: the local thumbnails dir has 48 files while 2 rows reference a thumbnail.
  - **Solution:** Delete the jpg when its row is purged, and/or sweep files not referenced by any row at the end of a scan.

## 📦 Dependencies and Configuration

- [ ] **DEP-1 · Medium · Confirmed** — `trash = "*"` is an unbounded wildcard.
  - **Location:** `src-tauri/Cargo.toml:31`
  - **Problem:** It resolves to 5.x today. A major bump changes the API silently on `cargo update`. crates.io also disallows publishing wildcards, a sign this is bad practice.
  - **Solution:** Pin to `trash = "5"`.

- [ ] **DEP-2 · Low · Confirmed** — Unused plugins are shipped.
  - **Location:** `package.json:15-16`, `src-tauri/Cargo.toml:22-23`, `lib.rs:20-21`, `capabilities/default.json`
  - **Problem:** `@tauri-apps/plugin-dialog`/`plugin-opener` and the Rust crates are never used.
  - **Solution:** Remove them now and re-add the dialog when FEAT-1 is built.

- [ ] **DEP-3 · Low · Confirmed** — Build-only packages are listed as runtime dependencies.
  - **Location:** `package.json:13,24` (`@tailwindcss/vite`, `tailwindcss`)
  - **Solution:** Move them to `devDependencies`.

- [ ] **DEP-4 · Low · Confirmed** — `npm audit` reports 9 advisories (6 high) and packages are behind.
  - **Location:** `package-lock.json`
  - **Problem:** The advisories are in vite, postcss, nanoid, browserslist, source-map-js, and react-router. They are dev/build-time or RSC-specific, so the shipped app risk is low. Several minors are behind (`@tauri-apps/api` 2.11 → 2.12, react 19.2 → 19.3, react-router 7.16 → 7.18). `@vitejs/plugin-react` 4 → 6 and vite 7 → 8 are majors.
  - **Solution:** Run `npm update` within ranges and `npm audit fix`. Keep `@tauri-apps/api` and `@tauri-apps/cli` at the same minor as the Rust `tauri` crate. Plan the vite 8 / plugin-react 6 upgrade separately.

- [ ] **DEP-5 · Low · Potential** — `allowScripts` pins an exact esbuild version.
  - **Location:** `package.json:31-33`
  - **Problem:** The entry goes stale on the next esbuild bump, and whichever tool reads this field will then block its install script.
  - **Solution:** Document which tool consumes it, or use a version range.

- [ ] **DEP-6 · Low · Confirmed** — `GDK_BACKEND=x11` is hardcoded into the `tauri` npm script.
  - **Location:** `package.json:10`
  - **Problem:** It forces XWayland for dev **and** build invocations, with no explanation. The installed app doesn't get it at all, so dev and release behave differently on Wayland.
  - **Solution:** Add a comment in AGENT.md explaining why (a WebKitGTK/Wayland issue?). If it's needed at runtime, set it in `main.rs` before `run()` (`std::env::set_var` when unset), so dev and prod match.

- [ ] **DEP-7 · Low · Potential** — An SVG is listed in the bundle icon list.
  - **Location:** `src-tauri/tauri.conf.json:34`
  - **Problem:** Tauri's bundler expects PNG/ICNS/ICO. Unverified whether `tauri build` warns, ignores it, or fails.
  - **Solution:** Run `npm run tauri build` once. Generate the icon set with `tauri icon public/restack-icon.svg` (current PNGs are the default Tauri logo, dated Jul 5).

- [ ] **DEP-8 · Low · Improvement** — Runtime tools aren't validated.
  - **Location:** `utils/scanner.rs:15`, `utils/thumbnail.rs:29`, `commands/player.rs:52`
  - **Problem:** A missing `ffprobe`/`ffmpeg` silently yields duration 0 and no thumbnails forever (BUG-19). A missing `mpv` errors, but the UI swallows it.
  - **Solution:** See FEAT-4. Also cache the result in `AppState` and skip probing when the tools are missing, so rows are retried once they're installed.

- [ ] **CHORE-6 · Low · Confirmed** — Fill in metadata and the window defaults.
  - **Location:** `src-tauri/Cargo.toml:4-5`, `tauri.conf.json:13-19`
  - **Problem:** `Cargo.toml` still has `description = "A Tauri App"`, `authors = ["you"]`. The default window of 800×600 is small.
  - **Solution:** Set a larger default plus `minWidth`/`minHeight`.

- [ ] **CHORE-2 · Medium · Improvement** — No lint/format tooling.
  - **Location:** repo root
  - **Problem:** ESLint would have caught BUG-8 (`no-shadow`), BUG-32 (`no-undef`), and missing hook deps (`react-hooks/exhaustive-deps`, e.g. `SeriesDetail.jsx:457`). `cargo clippy` currently reports 4 warnings (`Iterator::last` on `DoubleEndedIterator` ×2 at `db.rs:401-402`, a needless `if let` at `commands/scanner.rs:35`, a needless borrow at `utils/scanner.rs:16`).
  - **Solution:** Add ESLint (with `react-hooks`) and Prettier for the frontend, and run `cargo clippy` / `cargo fmt` for the backend.

## 🧪 Testing and Reliability

_Current state: no tests of any kind, no CI. `npm run build`, `cargo check`, and `cargo clippy` pass (audit run 2026-10-28)._

- [ ] **CHORE-1 · High · Improvement** — Unit tests for `utils/parser.rs`.
  - **Location:** `src-tauri/src/utils/parser.rs`
  - **Problem:** These are the highest-value first tests. BUG-15 through BUG-18 were all found with a 20-line harness.
  - **Solution:** Write table-driven `#[cfg(test)]` cases covering SxxExx, NxNN, Season/Episode, E-only, anime dash, movies, Other, and the known false positives (resolutions, dates, multi-episode, year-suffixed shows, years in titles like `1917`/`2012`).

- [ ] **TEST-1 · High · Improvement** — DB invariant tests on in-memory SQLite.
  - **Location:** `src-tauri/src/db.rs`
  - **Problem:** These functions already take `&Connection`, so `Connection::open_in_memory()` + `init_schema` works with no `AppHandle`.
  - **Solution:** Cover:
    1. Rescan preserves `user_pinned` group/type/season/episode.
    2. `duration = 0` / `thumb = None` don't overwrite existing values.
    3. Movies/others are never backfilled a group (SEC-3).
    4. `get_next_episode` skips deleted and non-series rows (BUG-2).
    5. The Continue Watching pointer only follows series (BUG-10).
    6. Merge, then rescan, doesn't split (BUG-13).
    7. Migrations run against an old-schema DB fixture (create a table without the newer columns, then run `init_schema`).

- [ ] **TEST-2 · High · Improvement** — Scan-sync tests with an injected filesystem and probe.
  - **Location:** `src-tauri/src/commands/scanner.rs` (after ARCH-1)
  - **Problem:** These are the data-loss paths (SEC-1, SEC-2, BUG-1).
  - **Solution:** Feed a file list plus a fake `MediaProbe`. Assert that missing roots don't delete rows, `scan_and_sync(root)` only touches `root`, and soft-deleted rows survive.

- [ ] **TEST-3 · Medium · Improvement** — mpv IPC parser and tracker tests.
  - **Location:** `src-tauri/src/commands/player.rs` (after ARCH-2)
  - **Solution:** Feed recorded mpv JSON lines (property-change, end-file `eof`/`quit`/`error`, null `data`). Assert the throttled persist calls, the watched latch (BUG-12), and that `play_next_episode` is produced only after the loop ends.

- [ ] **TEST-4 · Medium · Improvement** — Frontend unit tests (Vitest).
  - **Location:** `src/store/libraryStore.js`, selection logic in `src/pages/SeriesDetail.jsx`
  - **Solution:** Mock `@tauri-apps/api/core`. Test `updateEpisodeProgress` immutability, that selection resets on group change (BUG-8 regression), and that delete only sends visible-group paths.

- [ ] **TEST-5 · Low · Improvement** — Manual QA checklist in AGENT.md for flows that can't be unit-tested.
  - **Problem:** Covers delete/undo on home and on an external drive, auto-play next, drag-reclassify, and first run with an empty DB.
  - **Solution:** Write the checklist using a throwaway `$HOME` (e.g. `HOME=/tmp/restack-qa npm run tauri dev`) so the real library is never touched (CLAUDE.md rule 7).

- [ ] **CHORE-3 · Medium · Improvement** — Simple CI.
  - **Solution:** GitHub Actions running `npm ci && npm run build`, `cargo check`, `cargo clippy -- -D warnings`, `cargo test`. Add `vitest run` once TEST-4 exists. Linux runner only; it needs the `libwebkit2gtk-4.1-dev` system deps.

## 🧹 Maintainability and Technical Debt

- [ ] **MAINT-1 · Medium · Confirmed** — About 600 lines of dead UI, and AGENT.md describes features that aren't reachable.
  - **Location:** `src/components/library/{MergeSuggestionBanner,MergeGroupModal,RenameGroupModal,GroupTabs,GroupActionsMenu,ShowCard}.jsx`, `src/components/ui/Tab.jsx` (imported nowhere, or only by other dead files)
  - **Problem:** Merge, merge suggestions, and the group picker exist in the backend (`merge_groups_cmd`, `get_merge_suggestions_cmd`, `get_all_groups_cmd`) but have **no entry point** in the UI. AGENT.md §3 lists "merge suggestions" as part of `SeriesDetail.jsx`.
  - **Solution:** Decide per feature. Either wire `MergeSuggestionBanner` into `SeriesIndex` and the actions menu into the `SeriesDetail` header (after fixing BUG-13 and BUG-32), or delete the files and the unused commands. Update AGENT.md either way.

- [ ] **MAINT-2 · Low · Confirmed** — Small dead code paths.
  - **Location:** `src/pages/SeriesDetail.jsx:292-301` (`fetchLibrary(scan=true)` branch never used), `:130-133` (unreachable sort branch), `:1` (`React` default import unused), `commands/scanner.rs:137-140` (`get_scan_roots` unused, keep for FEAT-1), `db.rs:7` (`use trash;` redundant), `db.rs:402` (Windows `\\` fallback in a Linux-only app, and `.or_else` can never trigger)
  - **Solution:** Remove these during the next touch of each file.

- [ ] **MAINT-3 · Low · Confirmed** — SeriesDetail error banner is sticky and errors are generic.
  - **Location:** `src/pages/SeriesDetail.jsx:32,476-481`
  - **Problem:** `error` is never cleared, so one failure keeps the red banner up for the rest of the session. Messages drop the backend's string ("Failed to delete episodes." instead of the actual `Failed to trash file …`).
  - **Solution:** Auto-dismiss or clear on the next successful action, and include the backend message. Fold into FEAT-3.

- [ ] **MAINT-4 · Low · Confirmed** — Docs drift between AGENT.md and this TODO.
  - **Location:** `AGENT.md` §3 file map, §7 "Known issues"
  - **Problem:** The known-issues list duplicates items here and will diverge.
  - **Solution:** Keep AGENT.md §7 to *invariants and sharp edges an agent must know* and link here for the backlog. Update it when SEC-1, SEC-3, MAINT-1, and PERF-6 land.

- [ ] **CHORE-4 · Low · Improvement** — Split `pages/SeriesDetail.jsx` (724 lines).
  - **Solution:** Extract `useEpisodeSelection` (selection, range, keyboard), `useUndoDelete`, `SeasonSection`, and `SeriesHeader` (inline rename). Do this after BUG-8 and BUG-22 so the fixes stay small.

- [ ] **CHORE-5 · Low · Confirmed** — Raw colors instead of `@theme` tokens.
  - **Location:** 142 raw `zinc-*`/`red-*`/`amber-*`/hex usages across `src/`. Worst: `FirstScanOverlay.jsx`, `Sidebar.jsx`, `GroupTabs.jsx:58,71` (`#E0B0FF`), `index.css:56-62` (scrollbar)
  - **Solution:** Add `--color-danger`, `--color-warning`, and muted-text tokens, then replace the raw values.

- [ ] **CHORE-7 · Low · Improvement** — Rewrite `README.md` from the original spec into a real project README.
  - **Solution:** Cover install, runtime deps (`mpv`, `ffmpeg`, `ffprobe`), usage, and screenshots.

- [ ] **CHORE-8 · Low · Improvement** — Replace `println!` debugging with proper logging.
  - **Location:** `commands/player.rs:67,95,108,113,158,163,166,188`, `commands/scanner.rs:120`
  - **Solution:** Use `log` + `tauri-plugin-log`.

- [ ] **CHORE-9 · Low · Confirmed** — Remove unused Vite template leftovers.
  - **Location:** `src/assets/react.svg`, `public/vite.svg`, `public/tauri.svg`

- [ ] **BUG-6 · Low · Confirmed** — The Google Fonts `@import` needs network access and the CSP blocks it.
  - **Location:** `src/index.css:1`, `tauri.conf.json:21`
  - **Problem:** The system font fallback is what actually renders, so the design never matches the intent.
  - **Solution:** Bundle the fonts locally (e.g. `@fontsource/*`) or drop to the system stack.

## ✨ Features / backlog

- [ ] **FEAT-1 (Medium)** Settings page: choose scan roots (uses existing `scan_and_sync` / `get_scan_roots`; **blocked on SEC-2**), excluded dirs, watched threshold.
- [ ] **FEAT-2 (Medium)** Incremental updates via filesystem watching (`notify` crate) instead of full rescans.
- [ ] **FEAT-3 (Medium)** Surface errors in the UI (toast) instead of `catch {}`, especially for mpv/ffmpeg missing on `PATH`. Silent catches today: `App.jsx:47,125`, `MoviesPage.jsx:31`, `OthersPage.jsx:39`, `Dashboard.jsx:32`, `Sidebar.jsx:57`.
- [ ] **FEAT-4 (Medium)** Startup dependency check for `mpv`, `ffmpeg`, `ffprobe` with a friendly message.
- [ ] **FEAT-5 (Low)** Search / filter across the library.
- [ ] **FEAT-6 (Low)** Manually edit season/episode numbers for a file (sets `user_pinned`).
- [ ] **FEAT-7 (Low)** Subtitle management.
- [ ] **FEAT-8 (Low)** Optional TMDB metadata/posters (opt-in, offline-first).
- [ ] **FEAT-9 (Low)** Watch-history analytics.
- [ ] **FEAT-10 (Low)** Multi-user profiles.
- [ ] **FEAT-11 (Low)** Regenerate thumbnail for an item / pick a different frame.
- [ ] **FEAT-12 (Low)** Movies/Others parity: delete-to-Trash, mark watched/unwatched, multi-select (today only series have these; `MoviesPage.jsx`, `OthersPage.jsx`).
- [ ] **FEAT-13 (Low)** Show the parsed episode `title` on cards (parsed and stored, but cards show the raw file name; `EpisodeGrid.jsx:91-93`).

---

## 📝 Audit log

**2026-10-28: full project audit** (no application code changed).
- **Read line by line:** all of `src-tauri/src/**` (lib, main, models, db, commands/{scanner,library,groups,player}, utils/{parser,scanner,thumbnail}), `Cargo.toml`, `tauri.conf.json`, `capabilities/default.json`, `build.rs`; all of `src/**` (App, main, store, utils, index.css, layouts, 6 pages, 9 library components, 7 ui components); `package.json`, `vite.config.js`, `index.html`, `.gitignore`, `.vscode/`; `AGENT.md`, `CLAUDE.md`; `README.md` headings only (the spec, see CHORE-7).
- **Skipped (generated/vendored):** `package-lock.json`, `Cargo.lock` (only versions checked), `src-tauri/icons/*`, `src-tauri/gen/`, `dist/`, `node_modules/`, `target/`.
- **Tools run:** `npm run build` ✅, `cargo check` ✅ (0 warnings), `cargo clippy` (4 warnings, see CHORE-2), `npm outdated`, `npm audit` (see DEP-4). A scratch harness compiled the real `parser.rs` against 22 filenames (BUG-15 to BUG-18). Read-only queries ran against a **copy** of `~/.local/share/com.jay.restack/restack.db` (evidence in BUG-5, BUG-19, SEC-3, PERF-14).
- **Not verified at runtime:** the app wasn't launched (CLAUDE.md rule 7). The UI findings (BUG-8, BUG-22, BUG-25, BUG-28) come from reading the code, and PERF-1's progress-repaint effect is marked as potential.

---

## ✅ Done

- [x] 2026-10-27 Rebrand ReelVault → Restack (package, crate, identifier, DB file, UI, docs, GitHub repo).
- [x] 2026-10-27 Added `AGENT.md` + `CLAUDE.md` project docs.
- [x] 2026-10-27 Cleared stale `target/` cache from the old project path.
