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
  getLibrary,
} from "./store/libraryStore";

function App() {
  const [isSyncing, setIsSyncing] = useState(false);
  const isSyncingRef = useRef(false);

  const doSync = useCallback(async () => {
    if (isSyncingRef.current) return;
    isSyncingRef.current = true;
    setIsSyncing(true);
    try {
      const data = await invoke("scan_user_storage");
      setLibrary(data);
    } catch {
      // silent
    } finally {
      isSyncingRef.current = false;
      setIsSyncing(false);
    }
  }, []);

  useEffect(() => {
    // Initial scan
    doSync();

    const unlistenProgress = listen("video_progress_updated", (event) => {
      const { file_path, last_position, watched } = event.payload;
      updateEpisodeProgress(file_path, last_position, watched);
    });

    const unlistenStart = listen("video_playback_started", (event) => {
      setPlayingFilePath(event.payload);
    });

    const unlistenStop = listen("video_playback_stopped", (event) => {
      if (getPlayingFilePath() === event.payload) {
        setPlayingFilePath(null);
      }
    });

    const unlistenPlayNext = listen("play_next_episode", (event) => {
      const nextPath =
        typeof event.payload === "string" ? event.payload : null;
      if (!nextPath) return;
      invoke("play_episode", { filePath: nextPath, startSeconds: null }).catch(
        () => {},
      );
    });

    // Ctrl+R sync
    const handleKeyDown = (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R")) {
        e.preventDefault();
        doSync();
      }
    };
    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      Promise.all([
        unlistenProgress,
        unlistenStart,
        unlistenStop,
        unlistenPlayNext,
      ]).then((fs) => fs.forEach((f) => f()));
    };
  }, [doSync]);

  return (
    <MainLayout onSync={doSync} isSyncing={isSyncing}>
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
