# TODO — Restack

Project backlog and progress tracker (committed alongside the code). The maintenance rules are in `AGENT.md` §10.

**How to use**
- Each item has a stable ID so it can be referenced in commits ("fixes BUG-2"). Prefixes: `ARCH` architecture, `DES` code design, `BUG` bugs, `SEC` security & data integrity, `PERF` performance, `DEP` dependencies & config, `TEST` testing, `MAINT` maintainability, `FEAT` features, `CHORE` legacy chores (kept their IDs when re-filed into the sections below).
- Status: `[ ]` open · `[~]` in progress · `[x]` done (move to **Done** with the date).
- Priority: **Critical** (data loss / unusable) · **High** (breaks data or UX badly) · **Medium** (should fix soon) · **Low** (nice to have). Legacy P1/P2/P3 were mapped to High/Medium/Low.
- Type: **Confirmed** (verified by reading the code end to end, running it, or checking the local DB) · **Potential** (plausible, needs a repro) · **Improvement** (not a defect).
- Add new items at the bottom of their section with the next free ID for that prefix.
- Line numbers in **Location** fields written before 2026-11-05 predate the `cargo fmt`/Prettier reformat (CHORE-2) and may be off. Search by the named function or snippet, and correct the line when you touch an item.

---

## 🔥 Now (current focus)

_Nothing picked yet. Move items here when you start them._

## 🧭 Suggested order (from the 2026-10-28 audit)

1. ~~**Stop data loss**: SEC-1, SEC-2, BUG-1, BUG-3, BUG-8~~ (done 2026-11-01; follow-up BUG-35).
2. ~~**Fix silent state corruption**: SEC-3, BUG-9, BUG-10, BUG-11, BUG-12~~ (done 2026-11-01, plus BUG-2).
3. ~~**Make it responsive**: PERF-1, PERF-6, PERF-7, PERF-8~~ (done 2026-11-04).
4. **Safety net before refactors**: ~~CHORE-1~~ (done 2026-11-04), ~~TEST-1, TEST-2~~ (done 2026-11-01), ~~CHORE-2, CHORE-3~~ (done 2026-11-05).
5. **Then the structure**: ARCH-*, DES-*, MAINT-*.

---

## 🏛️ Architecture and Project Structure

- [ ] **ARCH-1 · Low · Confirmed** — Scan orchestration and raw SQL live in a command file.
  - **Location:** `src-tauri/src/commands/scanner.rs` (`sync_library`, `upsert_parsed`)
  - **Problem:** Mostly done on 2026-11-01: the SQL is in `db.rs`, the walk and missing-row decision are pure functions in `utils/scanner.rs`, and `sync_library` runs without an `AppHandle` (tested). What remains: `sync_library` still lives in a command file, and the probe is a closure rather than a named seam.
  - **Solution:** Move `sync_library` + `upsert_parsed` to a `services/scan.rs` when a second caller appears (e.g. FEAT-2 file watching). Low urgency now.

- [ ] **ARCH-2 · Medium · Confirmed** — `player.rs` mixes four concerns in one deeply nested loop.
  - **Location:** `src-tauri/src/commands/player.rs:151-284` (`track_progress`; IPC loop about 203-258)
  - **Problem:** Process lifecycle, mpv JSON-IPC protocol parsing, DB writes, and event emission are interleaved six levels deep. The watched/resume rules and the throttle decision are already pure fns (`is_watched`, `resume_position`, `should_persist`, tested), but the loop glue and the `eof → next` ordering are still untested and live in one block.
  - **Solution:** Extract `fn parse_ipc_line(&str) -> Option<IpcEvent>` (pure, testable) and a small `ProgressTracker` struct that owns the `current`/`persisted`/`persisted_at` state now held as locals in `track_progress` (calling `should_persist` and the watched latch), and keep the thread loop as glue. Preserve the "emit `play_next_episode` after the loop exits" invariant.

- [ ] **ARCH-3 · Medium · Improvement** — Media identity is the file path.
  - **Location:** `src-tauri/src/db.rs:42-56` (`file_path TEXT UNIQUE`), `commands/scanner.rs:166-178` (`upsert_parsed`)
  - **Problem:** Renaming or moving a file (e.g. tidying `~/Downloads` into `~/Videos/Show/`) is seen as delete + new file. Progress, watched state, `user_pinned`, and group assignment are lost, and ffprobe/ffmpeg rerun.
  - **Solution:** During a scan, before hard-deleting a missing row, try to match a new file by `(size, mtime)` or `(dev, inode)`. Store these in new additive columns. If matched, `UPDATE media SET file_path = new` instead of delete + insert.

- [ ] **ARCH-4 · Medium · Improvement** — Group identity has no alias concept, so merges and moves don't steer future scans.
  - **Location:** `src-tauri/src/db.rs:127-148` (`find_or_create_group`), `:450-468` (`merge_groups`)
  - **Problem:** Groups are keyed only by `normalized_name`. After a merge, the source name is gone, so the next file with that name creates a fresh group (see BUG-13). The same goes for any show whose release names vary in ways the parser doesn't normalize (a trailing year is stripped since BUG-17).
  - **Solution:** Add an additive `show_group_aliases(normalized_name TEXT PRIMARY KEY, group_id INTEGER)` table. `find_or_create_group` checks aliases first. `merge_groups` inserts the source's normalized name as an alias of the target.

- [ ] **ARCH-5 · Medium · Improvement** — No frontend API layer: `invoke()` strings and arg shapes are scattered across about 10 files.
  - **Location:** `src/App.jsx`, `src/layouts/Sidebar.jsx`, `src/pages/*.jsx`, `src/components/library/{MergeGroupModal,RenameGroupModal,ShowCard}.jsx`
  - **Problem:** Command names and camelCase arg shapes are repeated (e.g. `play_episode` with `startSeconds: null` appears in six places). A rename or signature change is a grep hunt, and the "mutate, then `get_library_grouped`, then `setLibrary`" refresh pattern is copy-pasted and sometimes done wrong (BUG-8).
  - **Solution:** Add `src/api.js` exporting typed-by-JSDoc wrappers (`playMedia(path)`, `reclassify(ids, type, groupId)`, `refreshLibrary()` which also calls `setLibrary`). Route all calls through it.


- [ ] **ARCH-7 · Low · Confirmed** — Stringly-typed media types across the boundary.
  - **Location:** `src-tauri/src/db.rs:483-530` (`reclassify_media(target_type: &str)`), `models.rs:95-104` (`ContinueWatchingItem.mode`, `up_next_type`)
  - **Problem:** An unknown `target_type` is silently a no-op that returns `Ok(())`. Typos in JS can't be caught.
  - **Solution:** Use `#[derive(Deserialize)] #[serde(rename_all = "lowercase")] enum MediaType { Series, Movie, Other }` for the command arg, and serialize enums for `mode`/`upNextType`.

- [ ] **ARCH-8 · Low · Improvement** — Mobile/Windows template scaffolding in a Linux-only app.
  - **Location:** `src-tauri/src/main.rs:2` (`windows_subsystem`), `src-tauri/src/lib.rs:31` (`mobile_entry_point`), `src-tauri/Cargo.toml:15` (`staticlib`, `cdylib`), `tauri.conf.json:33-40` (`.icns`, `.ico`)
  - **Problem:** These contradict the "Linux only, no Windows/macOS branches" rule and add build output for nothing.
  - **Solution:** Drop `staticlib`/`cdylib` (keep `rlib` + bin), remove the cfg attrs, and trim the bundle icons to PNG sizes.

## 🧩 Code Design and Quality

- [ ] **DES-1 · Medium · Confirmed** — Multi-statement mutations aren't wrapped in transactions.
  - **Location:** `src-tauri/src/db.rs` (`merge_groups`, `move_episodes`, `reclassify_media`), `commands/scanner.rs` upsert loop (scan upserts are batched into transactions since PERF-7)
  - **Problem:** A failure halfway leaves partial state (some episodes moved, group not deleted, and so on). Loops issue one autocommit statement per row, which costs one fsync each.
  - **Solution:** Take `&mut Connection`, use `let tx = conn.transaction()?; …; tx.commit()?`. Use `UPDATE … WHERE id IN (…)` for the id loops.

- [ ] **DES-2 · Low · Confirmed** — Errors are silently swallowed or dropped in the DB layer.
  - **Location:** `src-tauri/src/db.rs:430,459` (`filter_map(|r| r.ok())` drops movie/other rows), `:540,548` (`unwrap_or((1,0))`, `unwrap_or(0)`), `get_episode_meta` / `get_next_episode` (`.ok()?` everywhere). (`get_last_position` was replaced by `get_playback_state`, which uses `.optional()`.)
  - **Problem:** A bad row disappears from the library without a trace. A DB lock or error during reclassify silently numbers the episode as S01E01.
  - **Solution:** Propagate errors with `?` and `format!("Failed to …: {e}")`. Use `.optional()` (rusqlite `OptionalExtension`) for "no row" instead of catch-all `Err(_)`.

- [ ] **DES-3 · Low · Confirmed** — Error-message style is inconsistent with the project convention.
  - **Location:** `commands/player.rs:131`, `utils/scanner.rs:23,30`, `utils/thumbnail.rs:10,12` (`format!("…{}", e)`)
  - **Problem:** CLAUDE.md requires `format!("Failed to …: {e}")`. Bare `e.to_string()` gives the user/log no context.
  - **Solution:** Normalize these when touching the files.

- [ ] **DES-4 · Low · Confirmed** — Three near-duplicate upserts and hand-rolled row tuples.
  - **Location:** `src-tauri/src/db.rs:181-280` (`upsert_episode/movie/other`), `:310-332`, `:726-801` (10- and 8-element tuples)
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
  - **Location:** thumbnail seek `00:00:15` (`thumbnail.rs:31`), socket wait `15×200ms` (`player.rs:105-110`), `EPISODE_CLICK_DELAY_MS` (`SeriesDetail.jsx:22`), undo window `10000` (`SeriesDetail.jsx:372`), `MIN_SCAN_DISPLAY_MS` (`App.jsx:20`)
  - **Problem:** FEAT-1 (settings) will need several of these. Today they're unnamed literals. (The watched threshold is now `WATCHED_THRESHOLD` in `player.rs`.)
  - **Solution:** Use named `const`s at module top (Rust) or a `src/config.js`, and later the settings table.

- [ ] **DES-8 · Low · Confirmed** — Selection identity is mixed (file path vs id).
  - **Location:** `src/pages/SeriesDetail.jsx:38` (Set of `filePath`), drag payloads use `id` (`EpisodeGrid.jsx:27-29`), move uses ids (`:346`), delete/restore use paths (`:358`)
  - **Problem:** Two keys for the same entity make bugs like BUG-8 more likely and complicate a future id-based backend.
  - **Solution:** Key selection by `id` and map to paths only at the delete/restore call.

## 🐛 Bugs and Logical Errors



- [ ] **BUG-4 · Low · Confirmed** — `get_all_groups` / merge-suggestion episode counts include soft-deleted and non-series rows.
  - **Location:** `src-tauri/src/db.rs` (`get_all_groups`, `get_merge_suggestions`)
  - **Solution:** `LEFT JOIN media m ON m.group_id = sg.id AND m.deleted_at IS NULL AND m.missing_since IS NULL AND m.media_type = 'series'`.

- [ ] **BUG-5 · Medium · Confirmed** — Empty `show_groups` are never cleaned up.
  - **Location:** `src-tauri/src/db.rs` (only merge and the one-time `migrate_existing_groups` run delete groups); amplified by BUG-13
  - **Problem:** The one-time migration (2026-11-01) clears the backlog once, but nothing collects groups emptied later by moves, reclassify, or deleted files. Evidence before that: the local DB has 43 groups and **0** of them have a live series episode. They still appear in `get_all_groups` pickers and merge suggestions. The 2026-11-04 parser fixes (BUG-15 to BUG-17) make this worse: on the next rescan unpinned rows leave junk groups such as "Inception 2010" (S1920), "Screencast from 2024 05" and "Doctor Who 2005" empty, one per old misparse.
  - **Solution:** Run `DELETE FROM show_groups WHERE id NOT IN (SELECT DISTINCT group_id FROM media WHERE group_id IS NOT NULL)` at the end of each scan and after move/reclassify/merge. Keep alias rows (ARCH-4) pointing to live groups only.







- [ ] **BUG-13 · Medium · Confirmed (latent: merge UI is unreachable, see MAINT-1)** — Merged groups resurrect and split new episodes.
  - **Location:** `src-tauri/src/db.rs:188` (`find_or_create_group` runs unconditionally before upsert), `:201-205`, `:450-468`
  - **Problem:** After merging A into B, group A is deleted. The next scan recreates A for every A-named file (an empty group, re-suggested for merge). Any **newly downloaded** A episode gets `group_id = A`, splitting the show again.
  - **Solution:** ARCH-4 (aliases). Also only call `find_or_create_group` when the row is new or `group_id IS NULL`.
  - **Test:** `db::tests::merge_then_rescan_does_not_split` is `#[ignore]`d and fails today (verified 2026-11-01). Remove the `#[ignore]` as part of the fix.

- [ ] **BUG-14 · Medium · Confirmed** — One bad file aborts the whole scan, silently.
  - **Location:** `src/App.jsx:50-54`, `src-tauri/src/commands/scanner.rs:166-172` (`upsert_parsed`)
  - **Problem:** Any single upsert error aborts `scan_user_storage` via `?`, and the frontend swallows the error. Since PERF-8 the cached library stays on screen when the DB has data, but new and changed files are never picked up, and nobody is told. With an empty DB the overlay is dismissed onto an empty library. This repeats on every launch. Since PERF-7 a failing upsert rolls back only the current uncommitted batch (up to 500 rows); earlier batches stay committed, and the scan still aborts.
  - **Solution:** In the scanner, log and skip per-file failures instead of aborting the whole scan. Show a non-blocking error when the scan fails (FEAT-3).

- [ ] **BUG-19 · Medium · Confirmed** — Thumbnails are never generated for clips shorter than 15 s, and never retried.
  - **Location:** `src-tauri/src/utils/thumbnail.rs:30-31` (`-ss 00:00:15`), `commands/scanner.rs:65-71` (probe closure, only for `is_new`), `:97-100` (commit + `on_file` call)
  - **Problem:** Evidence: in the local DB all 24 thumbnail-less rows have duration 1–8 s. A failed ffprobe/ffmpeg (or a deleted thumbnails dir) is also permanent, because unchanged files are never re-probed.
  - **Solution:** Seek to `min(15, duration * 0.1)` (duration is known first). Regenerate when `thumbnail_path IS NULL`, the file at `thumbnail_path` no longer exists, or `duration = 0`, not only for new files.

- [ ] **BUG-20 · Medium · Confirmed** — The scanner walks hidden dirs and misses udisks2 mounts.
  - **Location:** `src-tauri/src/utils/scanner.rs:45-51` (`should_skip_dir`), `:53-96` (`discover_default_scan_roots`)
  - **Problem:** (`.Trash-*` dirs are skipped since 2026-11-01; what remains:)
    1. Other hidden dirs (`~/.var`, `~/.local/share/Steam`, …) are still walked.
    2. On Arch/Fedora (including this CachyOS machine), udisks2 mounts removable drives at `/run/media/$USER`, which isn't a root, so they're never scanned.
    3. `/media/<name>` uses the basename of `$HOME` instead of the actual username.
  - **Solution:** Skip hidden dirs generally (with an allowlist if needed). Add `/run/media/$USER`. Get the username from `$USER` / `users::get_current_username`.

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

- [ ] **BUG-26 · Low · Confirmed** — "Recent" sort sorts by playback position, not recency.
  - **Location:** `src/pages/SeriesDetail.jsx:127-134`, options `:585-586`
  - **Problem:** `b.lastPosition - a.lastPosition` orders by seconds watched. The third "boost" branch is unreachable (only two options exist).
  - **Solution:** Add a `last_played_at` column (additive) set in `update_progress` and sort by it, or rename the option to "Progress". Remove the dead branch.

- [ ] **BUG-27 · Low · Confirmed** — Dashboard "Continue Watching" goes stale.
  - **Location:** `src/pages/Dashboard.jsx:29-33`
  - **Problem:** It's fetched once on mount, so it doesn't refresh after playback stops, after auto-play next, or after a scan while the Dashboard is visible. `onResume` (`:76-81`) has no `.catch`, so it throws an unhandled rejection if mpv is missing.
  - **Solution:** Refetch on `video_playback_stopped` / library changes (subscribe), and add error handling.

- [ ] **BUG-28 · Low · Confirmed** — A stale stop event clears "Playing" when the same file is replayed.
  - **Location:** `src/App.jsx:144-148`, `src-tauri/src/commands/player.rs:138,279` (started/stopped emits)
  - **Problem:** Replaying the same file kills the old mpv. The old tracker's `video_playback_stopped(path)` arrives after the new `video_playback_started(path)`, and the path compare passes, so the UI shows nothing playing.
  - **Solution:** Include a playback session id (the socket uuid) in the start/stop payloads and compare that.

- [ ] **BUG-29 · Low · Confirmed** — Reclassify-to-series edge cases.
  - **Location:** `src-tauri/src/db.rs:497-526`
  - **Problem:**
    1. Season-0 specials (`S00E01`) are treated as "unnumbered" and renumbered to S01E(max+1).
    2. Moving S01E03 into a group that already has S01E03 creates duplicate episode numbers with undefined order and auto-play next.
    3. An unknown `target_type` returns `Ok`.
  - **Solution:** Use `season IS NULL`-style sentinel logic (or check `media_type != 'series'`) instead of `season == 0`. Detect collisions and append. Return an error on an unknown type (ARCH-7).

- [ ] **BUG-30 · Low · Confirmed** — mpv is never reaped after it exits on its own.
  - **Location:** `src-tauri/src/commands/player.rs:134-136` (`start_playback` stores the `Child`), `models.rs:6-10`
  - **Problem:** The `Child` stays in `AppState` until the next `play_episode`, so a zombie `mpv` process lingers.
  - **Solution:** Have the tracker thread `try_wait()`/`wait()` and clear `AppState.player_process` when the IPC loop ends (pass the `AppHandle` and use `app.state::<AppState>()`).

- [ ] **BUG-31 · Low · Potential** — Progress tracking silently gives up on slow mpv startup.
  - **Location:** `src-tauri/src/commands/player.rs:154-168`
  - **Problem:** It waits at most 3 s for the socket. On a cold network/USB mount mpv can take longer, so playback works but no progress is saved, with no signal to the user. A `expect()` on `try_clone` at `:191` can also panic the thread.
  - **Solution:** Retry `connect` until the child exits (check `try_wait`) rather than a fixed count. Replace `expect` with an early return and a log.

- [ ] **BUG-33 · Low · Confirmed** — Undefined Tailwind classes silently do nothing.
  - **Location:** `animate-in`/`fade-in`/`slide-in-from-top-2` (`SeriesDetail.jsx:484`, `MultiSelectBar.jsx:29`, `GroupActionsMenu.jsx:41`, `RenameGroupModal.jsx:42`, `MergeSuggestionBanner.jsx:23`) need the `tailwindcss-animate` plugin, which isn't installed. `bg-surface-2` (`RenameGroupModal.jsx:62`) isn't a theme token.
  - **Solution:** Replace these with `framer-motion` (already used) or define the keyframes/tokens in `index.css`.

- [ ] **BUG-34 · Low · Confirmed** — List-view hover styles never apply.
  - **Location:** `src/components/library/EpisodeList.jsx:74,78` (`group-hover:*`), button at `:39-62` has no `group` class
  - **Solution:** Add `group` to the button's className.

- [ ] **BUG-35 · Low · Confirmed** — Hidden rows (`missing_since`) and trashed rows (`deleted_at`) are never purged.
  - **Location:** `src-tauri/src/commands/scanner.rs` (reconcile loop), `src-tauri/src/db.rs` (`sync_missing_media`)
  - **Problem:** The 2026-11-01 data-loss fix keeps a row whenever deletion isn't certain. A deleted show folder, a drive that never returns, or a trashed episode leave hidden rows forever. They're invisible in the library but still hold `group_id` (BUG-5 GC can't drop those groups), count toward BUG-4 totals until that's fixed, and keep orphan thumbnails (PERF-14).
  - **Solution:** Purge `deleted_at` rows when their item is no longer in the Trash (`trash::os_limited::list`) or after N days. Offer a "Missing files" view (or a setting) to forget `missing_since` rows, and/or auto-purge after N days once FEAT-1 adds settings.

- [ ] **BUG-37 · Low · Potential** — An old tracker's final flush can move Continue Watching back to the previous episode.
  - **Location:** `src-tauri/src/commands/player.rs` `track_progress` (final flush after the IPC loop) and `start_playback` (kills the old mpv, outside the `player_process` lock since PERF-1)
  - **Problem:** Starting a different episode kills the old mpv; the old tracker's final flush calls `update_progress`, which also sets `app_state.last_watched_episode_id`. If that flush lands after the new tracker's first write, Continue Watching points at the old episode. The new tracker polls for its socket in 200 ms steps, so the old flush almost always lands first.
  - **Related (since PERF-1):** the kill and spawn now happen outside the `player_process` lock. Two near-simultaneous `play_episode` calls keep the newest mpv and reap the loser, but their `video_playback_started` events can reach the frontend out of order.
  - **Solution:** Have the final flush skip the pointer update when the tracker was superseded (e.g. a playback generation counter in `AppState`, which could also tag the start/stop events), or only set the pointer from the most recently started playback.

- [ ] **BUG-39 · Low · Improvement** — Show names that really end in a year lose it.
  - **Location:** `src-tauri/src/utils/parser.rs` `series_show_name` / `SHOW_YEAR_RE`
  - **Problem:** The BUG-17 trailing-year strip turns `Space.1999.S01E01` into "Space", and different shows that share a base name merge, e.g. `The.Flash.2014` and `The.Flash.1990` both become "The Flash". Grouping stays consistent and `rename_group` fixes the display name, but the two series end up in one group. Cosmetic variant: `Show 1999-2000 - 05` gives "Show 1999", because only one trailing year is stripped after `clean_text`.
  - **Solution:** If this shows up in practice, keep the year when there are both a yeared and an unyeared group for the same base name. Or record the stripped year on the row (additive column) and include it in the group key only when two years collide.

- [ ] **BUG-40 · Low · Potential** — A bare resolution after a dash parses as an anime episode.
  - **Location:** `src-tauri/src/utils/parser.rs` `DASH_EP_RE` (4-digit episodes since BUG-38)
  - **Problem:** 4-digit dash episodes accept 1000-1899 and 2100-9999, so `Show - 1080` or `Show - 2160` (no `p`) becomes S1E1080 / S1E2160. With the `p` suffix it is stripped and falls through correctly. Found in review, not seen in a real library.
  - **Solution:** Reject common vertical resolutions (`1080`, `1440`, `2160`, `4320`) as 4-digit dash episodes, or require a 4-digit episode to sit next to a show whose other files also use 3+ digit numbering.

- [ ] **BUG-41 · Medium · Confirmed** — Parser fixes that change a show name split existing shows when new episodes arrive.
  - **Location:** `src-tauri/src/db.rs` `upsert_episode` (`group_id = CASE … WHEN media.group_id IS NULL THEN excluded.group_id ELSE media.group_id END`), `find_or_create_group`; triggered by BUG-17 (2026-11-04)
  - **Problem:** An existing unpinned series row keeps its `group_id` on rescan; only `show_name` is updated. After BUG-17 strips trailing years and fixes `S01.E02`, old `Doctor.Who.2005.S01E01` rows stay in group "Doctor Who 2005", while a newly added `Doctor.Who.2005.S01E09` parses as "Doctor Who" and lands in a different (new or existing) group, splitting the show. Same for `show s01`-style groups. Rows that change *type* (series → movie/other) are unaffected, because type changes clear `group_id`. Verified by reading the upsert; not reproduced against a real library.
  - **Solution:** Decide with ARCH-4/BUG-13 (aliases), since naively re-deriving `group_id` from `show_name` would undo merges and moves. A cheap interim option: when a rescan finds an unpinned row whose group's `normalized_name` differs from `normalize_name(show_name)` *and* that group has no pinned rows, move it to the parsed group and garbage-collect the emptied group (BUG-5). Add a db test with a pre-BUG-17 group.

- [ ] **BUG-42 · Low · Potential** — The SeriesDetail keyboard handler can call stale `markSelectedWatched`/`markSelectedUnwatched`.
  - **Location:** `src/pages/SeriesDetail.jsx` (the `keydown` `useEffect` that handles `w`/`u`), reported by `react-hooks/exhaustive-deps`
  - **Problem:** The effect's dependency array omits `markSelectedWatched` and `markSelectedUnwatched`. They are recreated every render, so W/U can run a closure from an older render if anything they read changes without a listed dependency changing too.
  - **Solution:** Wrap both in `useCallback` with their real deps and list them, or read them through a ref. Do it with BUG-22 or CHORE-4, which touch the same handler. Then the lint warning goes away.

## 🔐 Security and Data Integrity




- [ ] **SEC-4 · Medium · Potential** — Destructive commands trust arbitrary paths from the webview.
  - **Location:** `src-tauri/src/commands/library.rs` (`delete_episodes_cmd`, `restore_episodes_cmd`), `commands/player.rs:84-149` (`play_episode` / `start_playback`)
  - **Problem:** Any string sent from JS is trashed or handed to `mpv` as an argument. The CSP is strict and there's no remote content, so exploitation needs an XSS first. But this is the app's only file-destroying path and it does no validation.
  - **Solution:** Have the commands accept `media_ids`, resolve paths from the DB, and refuse paths not in `media`. For mpv, pass `--` before the file argument so a path can never be parsed as an option.

- [ ] **SEC-5 · Low · Confirmed** — The mpv IPC socket is in world-readable `/tmp`.
  - **Location:** `src-tauri/src/commands/player.rs:111`
  - **Problem:** Other local users can see the socket name. mpv creates it with the default umask, so they could connect and send commands to mpv (which can run `run` subprocess commands).
  - **Solution:** Use `$XDG_RUNTIME_DIR` (per-user, mode 0700), falling back to `/tmp`.

- [ ] **SEC-6 · Medium · Confirmed** — The asset-protocol scope hardcodes the data dir.
  - **Location:** `src-tauri/tauri.conf.json:25` (`$HOME/.local/share/com.jay.restack/thumbnails/**`), versus `utils/thumbnail.rs:6-14` (`app_data_dir()`)
  - **Problem:** If `$XDG_DATA_HOME` is set, `app_data_dir()` resolves elsewhere and every thumbnail breaks. The identifier is also duplicated in two places (CLAUDE.md rule 8).
  - **Solution:** Use `"$APPDATA/thumbnails/**"` (a Tauri path variable).

- [ ] **SEC-7 · Low · Confirmed** — Unused plugin permissions are granted.
  - **Location:** `src-tauri/capabilities/default.json:6-10` (`opener:default`, `dialog:default`), `src-tauri/src/lib.rs:34-35`
  - **Problem:** Neither plugin is used by any frontend code. They widen the IPC surface for nothing.
  - **Solution:** Remove them (see DEP-2) until FEAT-1 needs the dialog.

- [ ] **SEC-8 · Low · Potential** — Non-UTF-8 paths are stored lossily.
  - **Location:** `src-tauri/src/utils/scanner.rs` (`collect_video_files`, where stored paths are built), `commands/scanner.rs:52-54` (progress file name, `to_string_lossy`)
  - **Problem:** A filename with invalid UTF-8 is stored with U+FFFD. mpv, trash, and restore then target a path that doesn't exist.
  - **Solution:** Skip and log such files (simplest), or store paths as BLOB/`OsString` bytes.

## ⚡ Performance

- [ ] **PERF-2 · Medium · Confirmed** — The default scan walks **all of `$HOME`**.
  - **Location:** `src-tauri/src/utils/scanner.rs:53-80`
  - **Problem:** That includes `~/.local/share/Steam`, `~/.var`, browser profiles, and so on.
  - **Solution:** User-configurable scan roots (FEAT-1). Until then consider `~/Videos`, `~/Downloads`, `~/Movies`, and skip hidden dirs (BUG-20).

- [ ] **PERF-3 · Medium · Improvement** — Thumbnail and duration generation is serial.
  - **Location:** `src-tauri/src/commands/scanner.rs:66-71` (probe closure)
  - **Solution:** Parallelise with a bounded worker pool, or generate thumbnails in the background after the library is shown.

- [ ] **PERF-4 · Low · Improvement** — Every command reopens the SQLite connection and re-runs `init_schema` (about 6 statements plus `PRAGMA table_info`).
  - **Location:** all command wrappers (the player tracker already reuses one connection per playback since PERF-6)
  - **Solution:** Hold one `Mutex<Connection>` in `AppState`. Run schema init once at startup. Enable `PRAGMA journal_mode=WAL` (currently `delete`) so the player thread doesn't contend with scans.

- [ ] **PERF-5 · Low · Improvement** — Merge suggestions are O(n²) Levenshtein over all groups.
  - **Location:** `src-tauri/src/db.rs:589-606`
  - **Problem:** Fine for now; revisit with large libraries. Also, an absolute distance ≤ 2 on short names ("Dark" vs "Dare") gives false positives.
  - **Solution:** Use a relative threshold (`distance <= max(1, len/8)`) and skip groups with 0 live episodes.


- [ ] **PERF-10 · Medium · Confirmed** — ffprobe/ffmpeg have no timeout.
  - **Location:** `src-tauri/src/utils/scanner.rs:15-23`, `utils/thumbnail.rs:29-41`
  - **Problem:** A truncated file or a hung network mount stalls the scan forever. Since PERF-1 the window stays responsive, but the scan never finishes and holds the scan guard, so every later scan and every delete/restore fails with "a scan is running" until restart.
  - **Solution:** Spawn with `wait_timeout` (the `wait-timeout` crate), or pass ffmpeg `-rw_timeout` plus kill-after-N-seconds logic.

- [ ] **PERF-11 · Low · Confirmed** — Episode grid/list work is quadratic and memo is defeated.
  - **Location:** `src/components/library/EpisodeGrid.jsx:27-29`, `EpisodeList.jsx:34-36` (per-card `episodes.filter(...)` every render), `src/pages/SeriesDetail.jsx:184-189,241,253` (new callbacks every render defeat `memo`)
  - **Solution:** Compute drag ids lazily inside `onDragStart`. Wrap the handlers in `useCallback`, and make `bindEpisodeRef` stable (a single ref callback reading `data-id`).

- [ ] **PERF-12 · Low · Improvement** — One `scan:progress` IPC event and one React state update per file.
  - **Location:** `src-tauri/src/commands/scanner.rs:50-64`, `src/App.jsx:104-112`
  - **Solution:** Throttle to about 10 events/s (time-based) and always send the final one.

- [ ] **PERF-14 · Low · Confirmed** — Orphaned thumbnails are never removed.
  - **Location:** `src-tauri/src/commands/scanner.rs:111-113` → `db::sync_missing_media` (row deleted, jpg kept)
  - **Problem:** Evidence: the local thumbnails dir has 48 files while 2 rows reference a thumbnail.
  - **Solution:** Delete the jpg when its row is purged, and/or sweep files not referenced by any row at the end of a scan.

- [ ] **PERF-15 · Medium · Potential** — App connections rely on the implicit busy timeout and the rollback journal.
  - **Location:** `src-tauri/src/db.rs:18-21` (`open_db`); `src-tauri/src/commands/player.rs` `track_progress`; every command in `src-tauri/src/commands/*.rs` (on the blocking pool since PERF-1)
  - **Problem:** The scanner and the mpv tracker write through separate connections in the default rollback-journal mode. Since PERF-7 scans hold short write transactions (at most 500 rows or 1 s, committed before every probe). Only rusqlite's implicit 5 s default busy timeout stops a progress write from failing with `SQLITE_BUSY` while a batch commits; nothing in the code states that. Readers (`get_library_grouped` during a scan) also block on the writer, and each batch commit pays a full rollback-journal fsync. Since PERF-1, UI commands (rename, merge, move, set watched, reclassify, `get_library_grouped`) run at the same time as a scan instead of queueing behind it on the main thread, so each one also depends on that 5 s timeout while a batch (up to 1 s or 500 rows) is open. Delete/restore are excluded: they are mutually exclusive with scans.
  - **Solution:** In `open_db`, set an explicit `busy_timeout` and `PRAGMA journal_mode=WAL` (see PERF-4). Verify with existing user DBs.

- [ ] **PERF-16 · Low · Potential** — Concurrent scans and Trash operations are rejected, not queued.
  - **Location:** `src-tauri/src/models.rs` `ScanGuard`, `src-tauri/src/commands/scanner.rs` `scan_roots_and_sync`, `src-tauri/src/commands/library.rs` `exclusive_with_scan`; `src/pages/SeriesDetail.jsx` (delete/Undo error handling)
  - **Problem:** A `scan_and_sync(root)` requested during a full scan fails with "A scan is already running". So does a scan that starts while a delete/restore batch runs, where that message is misleading. Delete/Undo during a scan fail with "Failed to delete/restore: a library scan is running". SeriesDetail shows only its generic "Failed to delete/restore episodes." message, and the 10 s Undo banner can expire during a long scan, leaving the files only in the system Trash. Before PERF-1 these calls waited and ran after the scan. Two concurrent delete/restore batches also exclude each other through the same guard, which the name doesn't suggest. No frontend calls `scan_and_sync` yet, and App.jsx already dedupes `scan_user_storage` via `isSyncingRef`.
  - **Solution:** Give the guard a wait/queue mode (or a separate lock with a timeout) so short Trash batches wait for the scan, or have the scan skip rows soft-deleted at write time (upsert `WHERE deleted_at IS NULL` on conflict in `db.rs`). At minimum, show the backend's error text, disable delete/Undo while a scan runs (or keep Undo open until it ends), and add a doc comment on `exclusive_with_scan` noting it also serialises Trash batches.

## 📦 Dependencies and Configuration


- [ ] **DEP-2 · Low · Confirmed** — Unused plugins are shipped.
  - **Location:** `package.json:15-16`, `src-tauri/Cargo.toml:22-23`, `lib.rs:34-35`, `capabilities/default.json`
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
  - **Solution:** Run `npm run tauri build` once. The icon set was regenerated from the new logo on 2026-11-05 (`tauri icon` on a 1024px render of `src-tauri/icons/restack-icon.svg`), so dropping the SVG entry should only need that check.

- [ ] **DEP-8 · Low · Improvement** — Runtime tools aren't validated.
  - **Location:** `utils/scanner.rs:15`, `utils/thumbnail.rs:29`, `commands/player.rs:52`
  - **Problem:** A missing `ffprobe`/`ffmpeg` silently yields duration 0 and no thumbnails forever (BUG-19). A missing `mpv` errors, but the UI swallows it.
  - **Solution:** See FEAT-4. Also cache the result in `AppState` and skip probing when the tools are missing, so rows are retried once they're installed.

- [ ] **CHORE-6 · Low · Confirmed** — Fill in metadata and the window defaults.
  - **Location:** `src-tauri/Cargo.toml:4-5`, `tauri.conf.json:13-19`
  - **Problem:** `Cargo.toml` still has `description = "A Tauri App"`, `authors = ["you"]`. The default window of 800×600 is small.
  - **Solution:** Set a larger default plus `minWidth`/`minHeight`.

## 🧪 Testing and Reliability

_Current state (2026-11-04): `cargo test` runs 54 backend tests (`db.rs`, `utils/scanner.rs`, `utils/parser.rs`, `commands/scanner.rs`, `commands/player.rs`, `commands/library.rs`, `models.rs`) plus 1 `#[ignore]`d test that reproduces open bug BUG-13. No frontend tests. CI (`.github/workflows/ci.yml`) runs on every push to `develop`/`main` and on PRs. `npm run lint` (ESLint) and `cargo clippy --all-targets -- -D warnings` are clean apart from one `exhaustive-deps` warning (BUG-42)._



- [ ] **TEST-3 · Medium · Improvement** — mpv IPC parser and tracker tests.
  - **Location:** `src-tauri/src/commands/player.rs` (after ARCH-2)
  - **Solution:** Feed recorded mpv JSON lines (property-change, end-file `eof`/`quit`/`error`, null `data`). Assert the throttled persist calls, the `eof` position reset, and that `play_next_episode` is produced only after the loop ends.

- [ ] **TEST-4 · Medium · Improvement** — Frontend unit tests (Vitest).
  - **Location:** `src/store/libraryStore.js`, selection logic in `src/pages/SeriesDetail.jsx`
  - **Solution:** Mock `@tauri-apps/api/core`. Test `updateEpisodeProgress` immutability, that selection resets on group change (BUG-8 regression), and that delete only sends visible-group paths. Add a `vitest run` step to `.github/workflows/ci.yml`.

- [ ] **TEST-5 · Low · Improvement** — Manual QA checklist in AGENT.md for flows that can't be unit-tested.
  - **Problem:** Covers delete/undo on home and on an external drive, auto-play next, drag-reclassify, and first run with an empty DB.
  - **Solution:** Write the checklist using a throwaway `$HOME` (e.g. `HOME=/tmp/restack-qa npm run tauri dev`) so the real library is never touched (CLAUDE.md rule 7).

## 🧹 Maintainability and Technical Debt

- [ ] **MAINT-1 · Medium · Confirmed** — About 600 lines of dead UI, and AGENT.md describes features that aren't reachable.
  - **Location:** `src/components/library/{MergeSuggestionBanner,MergeGroupModal,RenameGroupModal,GroupTabs,GroupActionsMenu,ShowCard}.jsx`, `src/components/ui/Tab.jsx` (imported nowhere, or only by other dead files)
  - **Problem:** Merge, merge suggestions, and the group picker exist in the backend (`merge_groups_cmd`, `get_merge_suggestions_cmd`, `get_all_groups_cmd`) but have **no entry point** in the UI. AGENT.md §3 lists "merge suggestions" as part of `SeriesDetail.jsx`.
  - **Solution:** Decide per feature. Either wire `MergeSuggestionBanner` into `SeriesIndex` and the actions menu into the `SeriesDetail` header (after fixing BUG-13; BUG-32 is fixed), or delete the files and the unused commands. Update AGENT.md either way.

- [ ] **MAINT-2 · Low · Confirmed** — Small dead code paths.
  - **Location:** `src/pages/SeriesDetail.jsx:292-301` (`fetchLibrary(scan=true)` branch never used), `:130-133` (unreachable sort branch), `commands/scanner.rs:199-202` (`get_scan_roots` unused, keep for FEAT-1)
  - **Solution:** Remove these during the next touch of each file.

- [ ] **MAINT-3 · Low · Confirmed** — SeriesDetail error banner is sticky and errors are generic.
  - **Location:** `src/pages/SeriesDetail.jsx:32,476-481`
  - **Problem:** `error` is never cleared, so one failure keeps the red banner up for the rest of the session. Messages drop the backend's string ("Failed to delete episodes." instead of the actual `Failed to trash file …`).
  - **Solution:** Auto-dismiss or clear on the next successful action, and include the backend message. Fold into FEAT-3.

- [ ] **MAINT-4 · Low · Confirmed** — Docs drift between AGENT.md and this TODO.
  - **Location:** `AGENT.md` §3 file map, §7 "Known issues"
  - **Problem:** The known-issues list duplicates items here and will diverge.
  - **Solution:** Keep AGENT.md §7 to *invariants and sharp edges an agent must know* and link here for the backlog. Update it when MAINT-1 lands (SEC-1, SEC-3, BUG-2, and PERF-6 already updated AGENT.md).

- [ ] **CHORE-4 · Low · Improvement** — Split `pages/SeriesDetail.jsx` (724 lines).
  - **Solution:** Extract `useEpisodeSelection` (selection, range, keyboard), `useUndoDelete`, `SeasonSection`, and `SeriesHeader` (inline rename). Do this after BUG-8 and BUG-22 so the fixes stay small.

- [ ] **CHORE-5 · Low · Confirmed** — Raw colors instead of `@theme` tokens.
  - **Location:** 142 raw `zinc-*`/`red-*`/`amber-*`/hex usages across `src/`. Worst: `FirstScanOverlay.jsx`, `Sidebar.jsx`, `GroupTabs.jsx:58,71` (`#E0B0FF`), `index.css:56-62` (scrollbar)
  - **Solution:** Add `--color-danger`, `--color-warning`, and muted-text tokens, then replace the raw values.

- [ ] **CHORE-7 · Low · Improvement** — Rewrite `README.md` from the original spec into a real project README.
  - **Solution:** Cover install, runtime deps (`mpv`, `ffmpeg`, `ffprobe`), usage, and screenshots.

- [ ] **CHORE-8 · Low · Improvement** — Replace `println!` debugging with proper logging.
  - **Location:** `commands/player.rs:70,95,152,165,170,186,237,246,249,278`, `commands/scanner.rs:75`
  - **Solution:** Use `log` + `tauri-plugin-log`.

- [ ] **CHORE-9 · Low · Confirmed** — Remove unused Vite template leftovers.
  - **Location:** `src/assets/react.svg`, `public/vite.svg`, `public/tauri.svg`

- [ ] **BUG-6 · Low · Confirmed** — The Google Fonts `@import` needs network access and the CSP blocks it.
  - **Location:** `src/index.css:1`, `tauri.conf.json:21`
  - **Problem:** The system font fallback is what actually renders, so the design never matches the intent.
  - **Solution:** Bundle the fonts locally (e.g. `@fontsource/*`) or drop to the system stack.

## ✨ Features / backlog

- [ ] **FEAT-1 (Medium)** Settings page: choose scan roots (uses existing `scan_and_sync` / `get_scan_roots`; SEC-2 fixed 2026-11-01, so it is safe to call per root), excluded dirs, watched threshold.
- [ ] **FEAT-2 (Medium)** Incremental updates via filesystem watching (`notify` crate) instead of full rescans.
- [ ] **FEAT-3 (Medium)** Surface errors in the UI (toast) instead of `catch {}`, especially for mpv/ffmpeg missing on `PATH`. Silent catches today: `App.jsx:52,126,156`, `MoviesPage.jsx:31`, `OthersPage.jsx:39`, `Dashboard.jsx:32`, `Sidebar.jsx:59`.
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

- [x] 2026-11-05 **CHORE-3**: GitHub Actions CI (`.github/workflows/ci.yml`, ubuntu-24.04, Node 24, stable Rust) runs `npm ci`, `npm run lint`, `npm run format:check`, `npm run build`, `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked` on pushes to `develop`/`main` and on PRs. Add `vitest run` when TEST-4 lands.
- [x] 2026-11-05 **CHORE-2**: Added ESLint 9 (flat config `eslint.config.js`: recommended + `react`, `react-hooks`, `no-shadow`) and Prettier 3 (default options, `.prettierignore` skips `src-tauri`, `dist` and Markdown). New scripts are `npm run lint`, `npm run format`, and `npm run format:check` (Prettier + `cargo fmt --check`). Fixed the 3 clippy warnings and removed unused imports. The whole tree was then reformatted in a separate style-only commit.
- [x] 2026-11-05 **BUG-32**: `MergeGroupModal` imports `AlertCircle`, which it renders, instead of the unused `AlertTriangle`. ESLint caught it (`react/jsx-no-undef`).
- [x] 2026-11-04 **PERF-1**: every Tauri command is now `async` and runs its existing sync body on the blocking pool (`run_blocking` in lib.rs: `spawn_blocking`, JoinError mapped to `Failed to run background task: …`). Scans are serialized by an RAII `ScanGuard` over `AppState.scan_running`; a second scan gets `Err("A scan is already running")` (tested: exclusive, released on Err and on panic). `delete_episodes_cmd`/`restore_episodes_cmd` take the same guard (`exclusive_with_scan` in commands/library.rs), so a Trash batch and a scan never overlap and a scan can't revive a trashed row (tested). `play_episode` no longer holds the player mutex across kill/wait/spawn; if two plays race, the newest mpv is kept. JS contract unchanged. Follow-ups: PERF-15, PERF-16.
- [x] 2026-11-04 **BUG-38**: Dash episodes can have 4 digits except 19xx/20xx. `One_Piece_-_1071` is S1E1071, and `Show - 2019` is still Movie "Show" (2019).
- [x] 2026-11-04 **BUG-18**: A parenthesised release year from the raw name is used first. Otherwise the last year-like number wins. `Blade Runner 2049 (2017)` and `The.Dark.Knight.(2008)` parse correctly.
- [x] 2026-11-04 **BUG-17**: SxxExx accepts multi-episode tags (`S01E01E02` is episode 1) and `S01 E02`. Series show names drop a trailing 19xx/20xx year (`Doctor.Who.2005.S01E01` is "Doctor Who"); a name that is only a year (`1923`) keeps it. Unpinned rows regroup on the next rescan and leave their old yeared groups empty (see BUG-5).
- [x] 2026-11-04 **BUG-16**: The dash-episode pattern needs a space or a non-digit before the dash, and an ISO date in the name turns off the dash fallback. Date-stamped recordings (Screencast/OBS) now stay Other instead of becoming a series or a Movie titled by their prefix.
- [x] 2026-11-04 **BUG-15**: NxNN is now bounded to a 1-2 digit season and 1-3 digit episode at a word boundary, and `clean_text` strips WxH resolution tags. `Inception.2010.1920x1080` is now Movie "Inception" (2010).
- [x] 2026-11-04 **CHORE-1**: table-driven `#[cfg(test)]` module in `utils/parser.rs` covers SxxExx, NxNN, Season/Episode, E-only, anime dash, movies with year, years as titles (1917, 2012, 1923) and the Other fallback. BUG-15 to BUG-18 each have an `#[ignore = "BUG-xx: …"]` test asserting the correct behaviour (`bug15_resolution_is_not_an_episode`, `bug16_timestamped_recordings_are_not_series`, `bug17_multi_episode_and_year_suffixed_series`, `bug18_movie_year_prefers_release_year`); remove the ignore when fixing each.
- [x] 2026-11-04 **PERF-9**: every parser regex in `utils/parser.rs` is a `std::sync::LazyLock<Regex>` static compiled once (no new crate). Patterns and matching order are unchanged.
- [x] 2026-11-04 **PERF-7**: `sync_library` commits upserts in batches (`UpsertBatch`: 500 rows or 1 s) and commits before probing each new file, so the write lock is never held during ffprobe/ffmpeg. Removal was already one transaction in `sync_missing_media`. Tests cover batch boundaries and a concurrent writer during probes.
- [x] 2026-11-04 **PERF-13**: `init_schema` creates `idx_media_type_live ON media(media_type, deleted_at, missing_since)` after the ADD COLUMN migrations, matching the live-row filter used by every library query (wider than the planned `(media_type, deleted_at)` index, since `missing_since` is part of every live-row filter). A test covers an old-schema DB, idempotency, and that the query planner uses the index.
- [x] 2026-11-04 **PERF-6**: the mpv tracker reuses one DB connection and writes and emits progress at most every 5 s (position or wall time), right away when watched flips, and on pause or any loop exit (pure `should_persist`, tested). Watched now latches for the whole session. The store's `updateEpisodeProgress` patches only the path to the matching item and skips no-op notifications.
- [x] 2026-11-04 **BUG-25**: App.jsx's startup effect uses a `cancelled` flag. Tauri listeners that resolve after cleanup are unlistened immediately, and the discarded StrictMode mount skips the cached library load and doSync, so events no longer fire twice.
- [x] 2026-11-04 **BUG-24**: the scan "done" → idle timer is kept in a ref and cleared when a new doSync starts and on unmount, so a quick rescan isn't reset to idle mid-scan.
- [x] 2026-11-04 **PERF-8**: startup shows the library saved in the DB (`get_library_grouped`) before the scan, which then runs in the background behind the TopSyncBar. The full-screen overlay only appears when the DB is empty. A failed scan no longer blanks a cached library (rest of BUG-14 still open).
- [x] 2026-11-01 **SEC-3**: `migrate_existing_groups` only backfills series, runs once (guarded by `app_state.migration_groups_v1`), and on that run clears phantom groups from movies/others and deletes groups with no rows.
- [x] 2026-11-01 **BUG-10**: the Continue Watching pointer is only set for, and only resolves to, series rows.
- [x] 2026-11-01 **BUG-11**: replaced `update_watch_progress` with `set_watched_cmd({ mediaIds, watched })`: one transaction, position reset to 0, Continue Watching pointer untouched. SeriesDetail makes one call instead of N parallel ones.
- [x] 2026-11-01 **BUG-12**: watched latches once reached (`is_watched`), `eof` resets the position to 0, and resume restarts from 0 when the saved position is in the last 10% (`resume_position`). `get_last_position` replaced by `get_playback_state`.
- [x] 2026-11-01 **BUG-2**: auto-play next and Up Next skip trashed, missing, and non-series rows.
- [x] 2026-11-01 **BUG-9**: Sidebar's local state renamed to `setLibrarySnapshot`, so drag-to-reclassify updates the global store.
- Tests: the SEC-3, BUG-2, and BUG-10 regression tests are no longer `#[ignore]`d. Added tests for the one-time migration, bulk `set_watched`, `get_playback_state`, and the player's watched/resume rules.

- [x] 2026-11-01 **TEST-1**: 15 DB tests on in-memory SQLite in `db.rs`: pinned group/type/season/episode survive rescans, duration/thumbnail/progress aren't clobbered, missing rows hide and revive, `missing_since` keeps its first timestamp, soft delete/restore, the Continue Watching pointer survives a missing file, normalized group lookup, and migration of a pre-group schema. SEC-3, BUG-2, BUG-10, and BUG-13 have `#[ignore]`d tests that fail today.
- [x] 2026-11-01 **TEST-2**: extracted `collect_video_files` / `reconcile_unseen` / `is_confirmed_gone` (`utils/scanner.rs`) and `sync_library` (`commands/scanner.rs`, probe passed as a closure). 13 tests cover the decision logic, real temp-dir walks (missing roots, `.Trash-*`, unreadable dirs), and end-to-end sync (unplug/replug keeps progress, missing root, single-root scan, deleted file, trashed row kept for Undo, only new files probed). A mutation check (restoring the old "delete every unseen row") fails 8 of them.
- [x] 2026-11-01 **BUG-36 · Low · Confirmed (latent)**: `init_schema` created `idx_media_group_season_episode` before the `ADD COLUMN group_id` migration, so a pre-`group_id` database couldn't open. Found by TEST-1's migration test. The index is now created after the migrations. Every committed schema already had the column, so no known user was affected.

- [x] 2026-11-01 **SEC-1, SEC-2, BUG-1**: rescan only reconciles rows under roots walked this scan (minus walk-error subtrees), never touches soft-deleted rows, hard-deletes only when the file's folder still exists with other content, and otherwise hides the row via the new additive `missing_since` column (revived on reappearance). Continue Watching keeps its pointer while a file is missing.
- [x] 2026-11-01 **BUG-3, BUG-7, ARCH-6**: trash I/O moved to `utils/trash.rs`. Restore uses `trash::os_limited::{list, restore_all}` (external-drive `.Trash-$UID`, renamed duplicates, `$XDG_DATA_HOME`, symlinked parents). Delete/restore update the DB per file. Verified with a scratch crate against an isolated `XDG_DATA_HOME`. Dropped the `urlencoding` and `dirs` dependencies.
- [x] 2026-11-01 **BUG-8**: selection clears when the series changes, delete only sends selected episodes of the series on screen, and the confirm modal lists file names.
- [x] 2026-11-01 **DEP-1**: pinned `trash = "5"`. Also BUG-20 part 1: the scanner skips `.Trash-*` dirs.

- [x] 2026-10-27 Rebrand ReelVault → Restack (package, crate, identifier, DB file, UI, docs, GitHub repo).
- [x] 2026-10-27 Added `AGENT.md` + `CLAUDE.md` project docs.
- [x] 2026-10-27 Cleared stale `target/` cache from the old project path.
