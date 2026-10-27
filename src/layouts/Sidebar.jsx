import { NavLink, useLocation } from "react-router-dom";
import { useEffect, useState } from "react";
import { Home, Tv, Film, Folder, RefreshCw, ChevronRight, CheckCircle2 } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "../utils/cn";
import { invoke } from "@tauri-apps/api/core";
import {
  getSeries,
  getMovies,
  getOthers,
  subscribe,
  setLibrary,
} from "../store/libraryStore";

function Sidebar({ onSync, isSyncing, scanState = {} }) {
  const location = useLocation();
  const [library, setLibrary] = useState({
    series: getSeries(),
    movies: getMovies(),
    others: getOthers(),
  });
  const [seriesOpen, setSeriesOpen] = useState(true);

  useEffect(() => {
    const unsub = subscribe(() => {
      setLibrary({
        series: getSeries(),
        movies: getMovies(),
        others: getOthers(),
      });
    });
    return unsub;
  }, []);

  const isSeriesDetail = location.pathname.startsWith("/series/");
  const [activeDragTarget, setActiveDragTarget] = useState(null); // 'movies' | 'others' | 'series-[id]'

  const handleDropReclassify = async (e, targetType, targetGroupId = null) => {
    e.preventDefault();
    setActiveDragTarget(null);
    try {
      const dataStr = e.dataTransfer.getData("text/plain");
      if (!dataStr) return;
      const data = JSON.parse(dataStr);
      const { mediaIds } = data;
      if (!mediaIds || !mediaIds.length) return;

      await invoke("reclassify_media_cmd", {
        mediaIds,
        targetType,
        targetGroupId,
      });

      // Refresh frontend store instantly
      const updated = await invoke("get_library_grouped");
      setLibrary(updated);
    } catch (err) {
      console.error("Reclassification failed:", err);
    }
  };

  return (
    <aside className="w-[260px] min-w-[260px] h-screen bg-surface border-r border-border flex flex-col overflow-hidden select-none">
      {/* Brand Header */}
      <div className="p-6 pb-4">
        <div className="flex items-center gap-3">
          <img src="/restack-icon-transparent.png" alt="Restack" className="w-8 h-8 object-cover" />
          <h1 className="text-xl font-display font-extrabold bg-gradient-to-r from-accent to-accent-hover bg-clip-text text-transparent tracking-tight">
            Restack
          </h1>
        </div>
      </div>

      {/* Navigation Links */}
      <nav className="flex-1 overflow-y-auto px-4 py-2 flex flex-col gap-1">
        {/* Dashboard */}
        <NavLink
          to="/"
          end
          className={({ isActive }) =>
            cn(
              "flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-semibold border border-transparent transition-all duration-200",
              isActive
                ? "bg-accent/10 text-white shadow-sm border-accent/20"
                : "text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover"
            )
          }
        >
          <Home className="w-4 h-4" />
          <span>Dashboard</span>
        </NavLink>

        {/* Series Section Container */}
        <div className="flex flex-col">
          <button
            type="button"
            className={cn(
              "w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-semibold transition-all duration-200",
              isSeriesDetail || location.pathname === "/series"
                ? "text-white"
                : "text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover"
            )}
            onClick={() => setSeriesOpen((v) => !v)}
          >
            <Tv className="w-4 h-4" />
            <span className="flex-1 text-left">Series</span>
            <span className="text-xs px-2 py-0.5 rounded-full bg-zinc-800 text-zinc-400 font-medium">
              {library.series.length}
            </span>
            <ChevronRight
              className={cn(
                "w-4 h-4 text-zinc-500 transition-transform duration-200",
                seriesOpen && "rotate-90"
              )}
            />
          </button>

          {/* Smooth Collapsible list */}
          <AnimatePresence initial={false}>
            {seriesOpen && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2, ease: "easeInOut" }}
                className="overflow-hidden flex flex-col gap-0.5 pl-3 mt-1"
              >
                <NavLink
                  to="/series"
                  end
                  className={({ isActive }) =>
                    cn(
                      "px-3 py-1.5 pl-7 rounded-md text-xs font-semibold border border-transparent transition-all duration-150",
                      isActive
                        ? "text-accent bg-accent/5 border-accent/10"
                        : "text-zinc-500 hover:text-zinc-200 hover:bg-surface-hover"
                    )
                  }
                >
                  All Series
                </NavLink>
                {library.series.map((show) => {
                  const epCount = show.seasons.reduce(
                    (acc, s) => acc + s.episodes.length,
                    0,
                  );
                  return (
                    <NavLink
                      key={show.groupId}
                      to={`/series/${show.groupId}`}
                      onDragOver={(e) => e.preventDefault()}
                      onDragEnter={(e) => {
                        e.preventDefault();
                        setActiveDragTarget(`series-${show.groupId}`);
                      }}
                      onDragLeave={() => setActiveDragTarget(null)}
                      onDrop={(e) => handleDropReclassify(e, "series", show.groupId)}
                      className={({ isActive }) =>
                        cn(
                          "flex items-center justify-between px-3 py-1.5 pl-7 rounded-md text-xs font-semibold border transition-all duration-150 truncate",
                          isActive
                            ? "text-accent bg-accent/5 border-accent/10 font-bold"
                            : "text-zinc-500 hover:text-zinc-200 hover:bg-surface-hover border-transparent",
                          activeDragTarget === `series-${show.groupId}` && "border-accent bg-accent/10 shadow-glow scale-[1.02] text-white border-solid"
                        )
                      }
                    >
                      <span className="truncate flex-1 text-left mr-2">
                        {show.displayName}
                      </span>
                      <span className="text-[10px] text-zinc-600 font-medium">
                        {epCount}
                      </span>
                    </NavLink>
                  );
                })}
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* Movies */}
        <NavLink
          to="/movies"
          onDragOver={(e) => e.preventDefault()}
          onDragEnter={(e) => {
            e.preventDefault();
            setActiveDragTarget("movies");
          }}
          onDragLeave={() => setActiveDragTarget(null)}
          onDrop={(e) => handleDropReclassify(e, "movie")}
          className={({ isActive }) =>
            cn(
              "flex items-center justify-between px-3 py-2 rounded-lg text-sm font-semibold border border-transparent transition-all duration-200",
              isActive
                ? "bg-accent/10 text-white shadow-sm border-accent/20"
                : "text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover border-transparent",
              activeDragTarget === "movies" && "border-accent bg-accent/10 shadow-glow scale-[1.02] text-white border-solid"
            )
          }
        >
          <div className="flex items-center gap-3">
            <Film className="w-4 h-4" />
            <span>Movies</span>
          </div>
          <span className="text-xs px-2 py-0.5 rounded-full bg-zinc-800 text-zinc-400 font-medium">
            {library.movies.length}
          </span>
        </NavLink>

        {/* Others */}
        <NavLink
          to="/others"
          onDragOver={(e) => e.preventDefault()}
          onDragEnter={(e) => {
            e.preventDefault();
            setActiveDragTarget("others");
          }}
          onDragLeave={() => setActiveDragTarget(null)}
          onDrop={(e) => handleDropReclassify(e, "other")}
          className={({ isActive }) =>
            cn(
              "flex items-center justify-between px-3 py-2 rounded-lg text-sm font-semibold border border-transparent transition-all duration-200",
              isActive
                ? "bg-accent/10 text-white shadow-sm border-accent/20"
                : "text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover border-transparent",
              activeDragTarget === "others" && "border-accent bg-accent/10 shadow-glow scale-[1.02] text-white border-solid"
            )
          }
        >
          <div className="flex items-center gap-3">
            <Folder className="w-4 h-4" />
            <span>Others</span>
          </div>
          <span className="text-xs px-2 py-0.5 rounded-full bg-zinc-800 text-zinc-400 font-medium">
            {library.others.length}
          </span>
        </NavLink>
      </nav>

      {/* Sync / Progress Footer Panel */}
      <div className="p-4 border-t border-border bg-surface-card/40">

        {/* ── Scanning state: live progress ── */}
        {isSyncing && (
          <div className="space-y-2.5">
            {/* Header row */}
            <div className="flex items-center gap-2">
              <RefreshCw className="w-3.5 h-3.5 text-accent animate-spin shrink-0" />
              <span className="text-xs font-semibold text-zinc-300">Scanning library…</span>
              {scanState.total > 0 && (
                <span className="ml-auto text-[10px] tabular-nums text-zinc-500 shrink-0">
                  {scanState.current} / {scanState.total}
                </span>
              )}
            </div>

            {/* Progress bar */}
            <div className="w-full h-1 rounded-full bg-surface overflow-hidden">
              {scanState.total > 0 ? (
                <div
                  className="h-full rounded-full bg-accent transition-all duration-200 ease-out"
                  style={{ width: `${Math.round((scanState.current / scanState.total) * 100)}%` }}
                />
              ) : (
                <div className="h-full w-1/3 rounded-full bg-accent/70 animate-sidebar-shimmer" />
              )}
            </div>

            {/* Current file */}
            <p className="text-[10px] text-zinc-600 truncate leading-tight">
              {scanState.currentFile || "Scanning directories…"}
            </p>
          </div>
        )}

        {/* ── Done state: brief success flash ── */}
        {!isSyncing && scanState.phase === "done" && (
          <div className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-progress/10 border border-progress/20">
            <CheckCircle2 className="w-3.5 h-3.5 text-progress shrink-0" />
            <span className="text-xs font-semibold text-zinc-300">Sync complete</span>
          </div>
        )}

        {/* ── Idle state: normal sync button ── */}
        {!isSyncing && scanState.phase !== "done" && (
          <button
            type="button"
            className="w-full flex items-center justify-center gap-2.5 px-3 py-2.5 rounded-lg bg-surface border border-border hover:bg-surface-hover hover:border-border-hover text-zinc-300 hover:text-white text-xs font-semibold transition-all duration-200 cursor-pointer active:scale-[0.98]"
            onClick={onSync}
          >
            <RefreshCw className="w-3.5 h-3.5 text-zinc-400" />
            <span>Sync Library</span>
          </button>
        )}

      </div>
    </aside>
  );
}

export default Sidebar;
