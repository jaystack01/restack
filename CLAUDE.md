# CLAUDE.md

Guidance for Claude Code in this repo. The full project context (architecture, schema, command/event contract, conventions, known issues) is in **AGENT.md**, imported below. Keep AGENT.md as the single source of truth, and put only Claude-specific workflow notes here.

@AGENT.md

---

## Quick reference

- **App**: Restack, a Linux-only Tauri 2 (Rust) + React 19 (JSX) + Tailwind v4 local media indexer. Playback goes to `mpv`; thumbnails/duration come from `ffmpeg`/`ffprobe`.
- **Run**: `npm run tauri dev`
- **Verify** (no tests exist): `npm run build` and `cd src-tauri && cargo check`
- **Hot spots**: `src-tauri/src/db.rs` (all SQL), `src/pages/SeriesDetail.jsx` (largest UI), `src/App.jsx` (event listeners + startup scan), `src-tauri/src/commands/player.rs` (mpv IPC thread).

## Working rules

1. **Read before editing** `db.rs` upserts and `player.rs` tracking. Both encode subtle invariants: `user_pinned`, `COALESCE`/`CASE` on rescan, and emitting `play_next_episode` only after the IPC loop exits.
2. **New Tauri command** means four edits: a `db.rs` fn, a `commands/*.rs` wrapper, `lib.rs` `generate_handler![]`, and a `models.rs` struct with `#[serde(rename_all = "camelCase")]`. JS calls it with camelCase args.
3. **Schema change** means an additive `ALTER TABLE ... ADD COLUMN` guarded by the `PRAGMA table_info` check in `init_schema()`. Never drop or rename columns, because user DBs persist in `~/.local/share/com.jay.restack/`.
4. **Frontend data flow**: mutate via `invoke`, then `setLibrary(await invoke("get_library_grouped"))`. Use `libraryStore.js`; don't add a state library.
5. **Styling**: use the `@theme` tokens from `src/index.css` and `cn()`. No `tailwind.config.js`.
6. **Match the surrounding style**: `Result<_, String>` with `format!("Failed to …: {e}")` in Rust, and function components + hooks in JSX. Keep comment density similar.
7. **Don't touch the user's real library** while testing: `scan_user_storage` scans all of `$HOME`, and `delete_episodes_cmd` really moves files to Trash. Never run the app's delete flows against real files.
8. Don't change the Tauri `identifier` (`com.jay.restack`) without also updating `assetProtocol.scope` in `tauri.conf.json`. Thumbnails depend on it.
9. When you change commands, events, schema, or conventions, **update AGENT.md** in the same change.
10. Don't commit unless asked.
