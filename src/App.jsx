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

  const doSync = useCallback(async () => {
    if (isSyncingRef.current) return;
    isSyncingRef.current = true;

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
    setTimeout(() => setScanState(IDLE_SCAN_STATE), 2000);
  }, []);

  useEffect(() => {
    const cleanupFns = [];

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
      cleanupFns.push(unlistenStart, unlistenProgress);

      // Now safe to scan — listeners are live
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
      ]).then((fns) => cleanupFns.push(...fns));
    };

    init();

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
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
