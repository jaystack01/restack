# Restack

Restack is a Linux desktop app that indexes the video files already on your disks, sorts them into series, movies, and everything else, and remembers how far you got in each one. Playback happens in [mpv](https://mpv.io/); Restack is the library around it, not a player.

## Why

Local video collections tend to end up as folders full of release-style filenames (`Show.Name.S02E05.1080p.WEB-DL.mkv`). File managers can't tell you which episode comes next or where you stopped. Full media centers can, but they bring servers, accounts, and metadata downloads with them.

Restack does the smaller job: read the filenames you already have, group them, and keep watch progress, entirely offline and on your machine. The guiding principles are to stay lightweight, fast, offline-first, and minimal, and not to grow into a streaming server or a media center.

## Features

### Available now

- **Automatic library.** Scans your home folder and mounted drives for `.mkv`, `.mp4`, `.avi`, and `.webm` files and classifies each one as a series episode, a movie, or "other" from its filename.
- **Series, seasons, episodes.** Episodes of the same show are grouped even when filenames differ in punctuation or case (`Show.Name` and `show name` land together), then split by season.
- **Playback in mpv** with resume from where you stopped. Restarting from the credits starts the episode over.
- **Watch progress.** An item counts as watched at 90%, and progress bars show where you are.
- **Continue Watching / Up Next** on the dashboard, and the next episode starts automatically when one finishes.
- **Thumbnails** taken 15 seconds into each video, generated once and cached. Clips shorter than that currently get none.
- **Library management.**
  - Rename a series.
  - Move episodes to another series.
  - Mark episodes watched or unwatched.
  - Drag files between Series, Movies, and Others to fix a wrong classification.

  Manual corrections survive later scans.
- **Safe deletes.** Episodes are moved to the system Trash, with an Undo option for 10 seconds.
- **Offline drives are handled gently.** If an external drive or network share is missing during a scan, its files are hidden rather than deleted, and they come back with their progress when the drive returns.
- Search on each library page, plus grid and list views, sorting, multi-select, and keyboard navigation in the series view.

### Planned

These are tracked in [TODO.md](TODO.md) and are **not** available yet:

- Choosing which folders to scan (today the whole home folder and mounted drives are scanned).
- Merging duplicate series from the UI. The backend support exists, but no screen uses it yet.
- Delete and mark-watched for movies and "other" files (currently series only).
- Error messages in the UI when mpv or FFmpeg is missing.
- Filesystem watching instead of full rescans.
- Optional extras: subtitles, online metadata (opt-in), and watch statistics.

## Requirements

- **Linux.** Restack relies on Linux-specific pieces (freedesktop Trash, Unix sockets for mpv) and is not built for Windows or macOS.
- **mpv**, **ffmpeg**, and **ffprobe** available on your `PATH` at runtime.

To build from source you also need:

- Node.js `^20.19.0` or `>=22.12.0` (required by Vite 7), with npm.
- A Rust toolchain, 1.80 or newer.
- The [Tauri 2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux) (WebKitGTK and related system libraries).

## Getting started

```bash
git clone https://github.com/jaystack01/restack.git
cd restack
npm install
npm run tauri dev
```

On first launch Restack scans your home folder and any drives under `/media/<user>` and `/mnt`, which can take a while on a large disk. Later launches show the saved library immediately and rescan in the background. Press <kbd>Ctrl</kbd>+<kbd>R</kbd> to rescan manually.

Other commands:

| Command | What it does |
|---|---|
| `npm run tauri build` | Builds a release bundle (no release has been published yet). |
| `npm run build` | Builds only the web frontend into `dist/`. |
| `cd src-tauri && cargo test` | Runs the backend test suite. |

Restack keeps its database and thumbnail cache in `~/.local/share/com.jay.restack/`. It holds only Restack's own data, not your videos; deleting it resets the library and all watch progress.

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri 2](https://v2.tauri.app/) |
| Backend | Rust, SQLite via `rusqlite` |
| Frontend | React 19, Vite 7, Tailwind CSS 4, Framer Motion, Lucide icons |
| Media tools | mpv (playback), ffprobe (duration), ffmpeg (thumbnails) |

A React UI talks to a Rust backend through Tauri commands. The backend scans the disk, parses filenames, stores everything in a local SQLite database, and launches mpv, following playback over mpv's IPC socket to record progress. See [docs/architecture.md](docs/architecture.md) for how each part works.

## Project status

Restack is in early development (version 0.1.0) and has no published release. The core loop works: scan, browse, play, resume. Scans run in the background without freezing the window. Remaining work includes the screens listed under [Planned](#planned), and grouping and thumbnail edge cases. The full backlog lives in [TODO.md](TODO.md).

There is no automated frontend test suite or CI yet; the Rust backend has unit tests.

## License

No license has been chosen yet.
