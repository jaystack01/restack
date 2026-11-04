import { Navigate, Route, Routes } from "react-router-dom";
import MainLayout from "./layouts/MainLayout";
import Dashboard from "./pages/Dashboard";
import SeriesIndex from "./pages/SeriesIndex";
import SeriesDetail from "./pages/SeriesDetail";
import MoviesPage from "./pages/MoviesPage";
import OthersPage from "./pages/OthersPage";
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  setLibrary,
  updateEpisodeProgress,
  setPlayingFilePath,
  getPlayingFilePath,
} from "./store/libraryStore";

// How long the scanning state must stay visible (ms).
// Prevents flickering when the scan completes in <100ms.
const MIN_SCAN_DISPLAY_MS = 1000;

const IDLE_SCAN_STATE = {
  phase: "idle",
  current: 0,
  total: 0,
  currentFile: "",
};

function App() {
  const [isSyncing, setIsSyncing] = useState(false);
  const [scanState, setScanState] = useState(IDLE_SCAN_STATE);
  const [hasLibraryLoaded, setHasLibraryLoaded] = useState(false);
  const isSyncingRef = useRef(false);
  // Pending "done" → idle timer, cleared when a new scan starts so it can't
  // reset the new scan's state.
  const doneTimerRef = useRef(null);

  const doSync = useCallback(async () => {
    if (isSyncingRef.current) return;
    isSyncingRef.current = true;
    clearTimeout(doneTimerRef.current);
    doneTimerRef.current = null;

    const startedAt = Date.now();

    setIsSyncing(true);
    setScanState({ phase: "scanning", current: 0, total: 0, currentFile: "" });

    let libraryData = null;
    try {
      libraryData = await invoke("scan_user_storage");
    } catch {
      // silent
    }

    // ── Minimum display time ────────────────────────────────────────────
    // If the scan was fast (< MIN_SCAN_DISPLAY_MS), wait the remainder
    // so the progress UI doesn't flicker away instantly.
    const elapsed = Date.now() - startedAt;
    if (elapsed < MIN_SCAN_DISPLAY_MS) {
      await new Promise((r) => setTimeout(r, MIN_SCAN_DISPLAY_MS - elapsed));
    }

    // ── Commit results ──────────────────────────────────────────────────
    if (libraryData) {
      setLibrary(libraryData);
    }
    setHasLibraryLoaded(true);
    isSyncingRef.current = false;
    setIsSyncing(false);

    // Show "done" flash briefly
    setScanState((prev) => ({ ...prev, phase: "done" }));
    doneTimerRef.current = setTimeout(() => {
      doneTimerRef.current = null;
      setScanState(IDLE_SCAN_STATE);
    }, 2000);
  }, []);

  useEffect(() => {
    const cleanupFns = [];
    // listen() is async: StrictMode (and a real unmount) can run the cleanup
    // before it resolves, so late unlisteners are called right away instead.
    let cancelled = false;
    const track = (unlisteners) => {
      if (cancelled) unlisteners.forEach((f) => f());
      else cleanupFns.push(...unlisteners);
    };

    const handleKeyDown = (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R")) {
        e.preventDefault();
        doSync();
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    const init = async () => {
      // Register scan listeners first (parallel), then start the scan
      const [unlistenStart, unlistenProgress] = await Promise.all([
        listen("scan:start", (event) => {
          setScanState((prev) => ({ ...prev, total: event.payload.total_files }));
        }),
        listen("scan:progress", (event) => {
          const { current, total, file_name } = event.payload;
          setScanState((prev) => ({
            ...prev,
            current,
            total,
            currentFile: file_name,
          }));
        }),
      ]);
      track([unlistenStart, unlistenProgress]);
      if (cancelled) return;

      // Show the library saved in the DB right away, so returning users don't
      // wait for the walk. An empty DB keeps the first-scan overlay up.
      try {
        const cached = await invoke("get_library_grouped");
        if (cancelled) return;
        if (cached.series.length || cached.movies.length || cached.others.length) {
          setLibrary(cached);
          setHasLibraryLoaded(true);
        }
      } catch {
        // fall through to the scan
      }

      // Now safe to scan — listeners are live. Skip if this mount was
      // discarded while the cache loaded (StrictMode's first mount).
      if (cancelled) return;
      doSync();

      // Register playback listeners in background (don't block scan start)
      Promise.all([
        listen("video_progress_updated", (event) => {
          const { file_path, last_position, watched } = event.payload;
          updateEpisodeProgress(file_path, last_position, watched);
        }),
        listen("video_playback_started", (event) => {
          setPlayingFilePath(event.payload);
        }),
        listen("video_playback_stopped", (event) => {
          if (getPlayingFilePath() === event.payload) {
            setPlayingFilePath(null);
          }
        }),
        listen("play_next_episode", (event) => {
          const nextPath =
            typeof event.payload === "string" ? event.payload : null;
          if (!nextPath) return;
          invoke("play_episode", {
            filePath: nextPath,
            startSeconds: null,
          }).catch(() => {});
        }),
      ]).then(track);
    };

    init();

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", handleKeyDown);
      clearTimeout(doneTimerRef.current);
      doneTimerRef.current = null;
      cleanupFns.forEach((f) => f());
    };
  }, [doSync]);

  return (
    <MainLayout
      onSync={doSync}
      isSyncing={isSyncing}
      scanState={scanState}
      hasLibraryLoaded={hasLibraryLoaded}
    >
      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/series" element={<SeriesIndex />} />
        <Route path="/series/:groupId" element={<SeriesDetail />} />
        <Route path="/movies" element={<MoviesPage />} />
        <Route path="/others" element={<OthersPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </MainLayout>
  );
}

export default App;
