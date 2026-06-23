# ReelVault

# 🎬 Project Document: Local Media Indexer (Working Title: **ReelVault**)

---

# 1. 🧠 Vision

Build a **native desktop application for Linux** that scans local media files, intelligently groups them into series/seasons, tracks watch progress, and launches playback via an external player (mpv).

This is **not a media player**.

It is a **local media intelligence layer** that:

* Understands your files
* Organizes them automatically
* Tracks your consumption
* Stays lightweight and fast

---

# 2. 🎯 MVP Goal

A minimal but functional system that allows:

### Core Capabilities

* Scan directories for video files
* Parse filenames into structured metadata
* Group episodes into shows and seasons
* Display grouped media in a clean UI
* Launch files in mpv
* Track and persist watch progress

### MVP Success Criteria

* User can open app → instantly see organized library
* Clicking a video opens it in mpv
* Progress bar reflects last watched position
* Completed episodes show a visual indicator

---

# 3. 🧩 Core Features (MVP Scope)

## 3.1 File Scanning

* Recursive directory scanning
* Supported formats: `.mkv`, `.mp4`, `.avi`, `.webm`
* Ignore non-video files

## 3.2 Filename Parsing

Extract:

* Show Name
* Season Number
* Episode Number
* Episode Title (optional)

Example:

```
Haikyuu S02E01 Lets Go To Tokyo 1080p.mkv
```

Parsed as:

```
Show: Haikyuu
Season: 2
Episode: 1
Title: Lets Go To Tokyo
```

## 3.3 Grouping Logic

Group hierarchy:

```
Show → Season → Episodes
```

## 3.4 Playback Integration

* Launch mpv externally
* Resume playback from last saved position

## 3.5 Progress Tracking

* Store last watched timestamp
* Calculate percentage watched
* Mark episode as completed

## 3.6 UI Display

Each episode shows:

* Thumbnail
* Title
* Progress bar
* Completed tick

---

# 4. 🏗️ System Architecture

## 4.1 Overview

```
[ File System ]
       ↓
[ Scanner ]
       ↓
[ Parser ]
       ↓
[ SQLite Database ]
       ↓
[ UI Layer (Tauri + React) ]
       ↓
[ mpv Player ]
```

---

## 4.2 Components

### A. Scanner

* Walk directories using filesystem APIs
* Detect new/removed files

### B. Parser

* Regex-based extraction
* Normalize naming inconsistencies

### C. Database (SQLite)

Stores:

* Files
* Metadata
* Progress

### D. UI Layer

* Built with React
* Communicates with backend via Tauri

### E. Player Controller

* Spawns mpv process
* Passes resume time

---

# 5. 🗄️ Database Schema

## Table: `media`

| Column         | Type    | Description              |
| -------------- | ------- | ------------------------ |
| id             | INTEGER | Primary key              |
| file_path      | TEXT    | Absolute path            |
| show_name      | TEXT    | Parsed show name         |
| season         | INTEGER | Season number            |
| episode        | INTEGER | Episode number           |
| title          | TEXT    | Episode title            |
| duration       | INTEGER | Total duration (seconds) |
| last_position  | INTEGER | Last watched position    |
| watched        | BOOLEAN | Completed flag           |
| thumbnail_path | TEXT    | Cached thumbnail         |

---

# 6. 🔍 Filename Parsing Strategy

## Primary Regex

```
(.+?)\s*S(\d+)E(\d+)
```

## Fallback Patterns

* `(.+?)\s*(\d+)x(\d+)`
* `(.+?)\s*Season\s*(\d+)\s*Episode\s*(\d+)`

## Cleanup Rules

* Remove tags like:

  * 1080p
  * x264
  * Hindi
  * WEB-DL

---

# 7. ▶️ Playback Integration (mpv)

## Launch Command

```
mpv <file> --start=<seconds>
```

## Advanced (Optional)

Use IPC socket:

```
--input-ipc-server=/tmp/mpv-socket
```

Allows:

* Real-time progress tracking
* Sync with UI

---

# 8. 🖼️ Thumbnail Generation

## Tool: ffmpeg

Command:

```
ffmpeg -ss 00:00:10 -i input.mkv -frames:v 1 output.jpg
```

## Strategy

* Generate once
* Cache locally
* Store path in DB

---

# 9. 🎨 UI Design Principles

## Layout

* Sidebar (Shows)
* Main panel (Seasons → Episodes)

## Episode Card

* Thumbnail
* Title
* Progress bar
* Completed tick

## Design Style

* Dark mode
* Rounded cards
* Smooth animations

---

# 10. ⚙️ Technology Stack

## Frontend

* React
* Tailwind CSS

## Backend

* Tauri (Rust)

## Database

* SQLite

## Media Tools

* mpv
* ffmpeg

---

# 11. 🚧 Challenges & Risks

## 11.1 Filename Inconsistency

* Different naming conventions
* Solution: multiple regex + fallback

## 11.2 Progress Sync Issues

* Watching outside app breaks tracking

## 11.3 Performance

* Large libraries → slow scan

Solution:

* Initial scan + incremental updates

---

# 12. 🗺️ Development Roadmap

## Phase 1: Core Foundation

* File scanning
* Basic parsing
* Console output grouping

## Phase 2: Database

* Store parsed data
* Query grouped structure

## Phase 3: UI MVP

* Display shows and episodes
* Basic layout

## Phase 4: Playback

* Launch mpv
* Resume support

## Phase 5: Progress Tracking

* Save last position
* Show progress bars

## Phase 6: Thumbnails

* Generate + cache

## Phase 7: Polish

* Animations
* UX improvements

---

# 13. 🚀 Future Enhancements

* Subtitle management
* Auto metadata (optional TMDB integration)
* Multi-user profiles
* Watch history analytics
* Mobile companion app

---

# 14. 🧠 Philosophy

This project should remain:

* Lightweight
* Offline-first
* Fast
* Minimal

Avoid turning it into:

* A streaming server
* A bloated media center

---

# 15. 📌 Final Summary

You are building:

> A smart, local-first media organizer that feels like a personal Netflix but runs entirely on your machine and respects your control.

---

If expanded correctly, this can evolve into a full product.



## Project requirement analysis (from `README.md`)

### What ReelVault is (and isn’t)
- **Is**: a **native Linux desktop app** that indexes local video files, auto-organizes them, tracks watch progress, and launches playback in **mpv**.
- **Is not**: a media player (playback is delegated to mpv).

### MVP goal
- **User flow**: open app → instantly see organized library → click episode → opens in mpv → **resumes** where left off → UI shows **progress bar** and **completed indicator**.

### Core MVP features (functional requirements)
- **File scanning**
  - Recursive scan of chosen directories
  - Only video formats: **`.mkv`, `.mp4`, `.avi`, `.webm`**
  - Ignore non-video files
  - Detect **new/removed files** (implies incremental refresh, not only full re-scan)

- **Filename parsing → structured metadata**
  - Extract: **show name**, **season #**, **episode #**, optional **episode title**
  - Primary regex: `(.+?)\s*S(\d+)E(\d+)`
  - Fallback patterns:
    - `(.+?)\s*(\d+)x(\d+)`
    - `(.+?)\s*Season\s*(\d+)\s*Episode\s*(\d+)`
  - Cleanup rules: strip common tags like **1080p, x264, Hindi, WEB-DL** (and presumably similar)

- **Grouping logic**
  - Hierarchy: **Show → Season → Episodes**

- **Playback integration (mpv)**
  - Launch external command: `mpv <file> --start=<seconds>`
  - Optional “advanced” path via mpv IPC socket for real-time sync (not required for MVP, but mentioned)

- **Progress tracking**
  - Persist **last watched timestamp**
  - Compute **percentage watched**
  - Mark episode as **completed** (boolean)

- **UI display**
  - Present shows/seasons/episodes in a clean UI
  - Each episode card shows: **thumbnail**, **title**, **progress bar**, **completed tick**
  - Layout expectation: **sidebar (shows)** + **main panel (seasons → episodes)**
  - Style direction: **dark mode**, rounded cards, smooth animations

### Data/storage requirements
- **SQLite database** with a `media` table containing:
  - `file_path` (absolute), `show_name`, `season`, `episode`, `title`
  - `duration` (seconds), `last_position` (seconds), `watched` (boolean)
  - `thumbnail_path` (cached thumbnail location)

### Thumbnail generation requirement
- Use **ffmpeg** to generate a still (example at 10s):
  - `ffmpeg -ss 00:00:10 -i input.mkv -frames:v 1 output.jpg`
- **Generate once, cache locally**, store path in DB

### Technology constraints / target stack
- **Frontend**: React + Tailwind CSS  
- **Backend**: Tauri (Rust)  
- **DB**: SQLite  
- **External tools**: mpv, ffmpeg  
- **Platform**: Linux desktop (native feel)

### Non-functional requirements (explicit philosophy + implied)
- **Offline-first**
- **Lightweight / minimal**
- **Fast**, including for large libraries (explicit risk)
- Avoid evolving into a streaming server / “media center” bloat

### Risks called out (requirements implications)
- **Filename inconsistency** → parser must be robust and extensible
- **Progress sync**: watching outside the app breaks tracking (acceptable limitation unless IPC is implemented)
- **Performance with large libraries** → needs **initial scan + incremental updates**

### Roadmap expectations (sequencing)
- Build in phases: scanning → parsing → DB → UI → playback/resume → progress → thumbnails → polish.

If you want, I can convert this into a concrete implementation checklist (modules, APIs between Tauri/Rust and React, and a minimal DB migration plan) that matches this MVP scope exactly.