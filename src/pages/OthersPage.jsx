import { useEffect, useState } from "react";
import { Search, FileVideo, Play, Check, FolderOpen } from "lucide-react";
import { motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { getOthers, getPlayingFilePath, subscribe, subscribePlaying } from "../store/libraryStore";
import Input from "../components/ui/Input";

function formatDuration(seconds) {
  if (!seconds || seconds < 0) return "—";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}h ${m % 60}m`;
  return `${m}m ${s}s`;
}

export default function OthersPage() {
  const [others, setOthers] = useState(getOthers());
  const [search, setSearch] = useState("");
  const [playingFilePath, setPlayingFilePath] = useState(() => getPlayingFilePath() ?? null);

  useEffect(() => {
    const unsub = subscribe(() => setOthers(getOthers()));
    return unsub;
  }, []);

  useEffect(() => {
    const unsub = subscribePlaying((path) => setPlayingFilePath(path ?? null));
    return unsub;
  }, []);

  const filtered = search.trim()
    ? others.filter((o) => o.fileName.toLowerCase().includes(search.trim().toLowerCase()))
    : others;

  async function playFile(item) {
    try {
      await invoke("play_episode", { filePath: item.filePath, startSeconds: null });
    } catch {
      // silent
    }
  }

  const listVariants = {
    hidden: { opacity: 0 },
    show: {
      opacity: 1,
      transition: {
        staggerChildren: 0.03,
      },
    },
  };

  const itemVariants = {
    hidden: { opacity: 0, y: 10 },
    show: { opacity: 1, y: 0, transition: { type: "spring", stiffness: 100 } },
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6 select-none"
    >
      {/* Header Panel */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-display font-extrabold tracking-tight text-white">
            Others
          </h1>
          <p className="text-sm text-zinc-400 mt-1">
            Videos that couldn't be parsed into a specific series or movie pattern.
          </p>
        </div>

        <div className="relative w-full sm:w-72">
          <Search className="absolute left-3 top-2.5 h-4.5 w-4.5 text-zinc-500" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search files..."
            className="pl-10"
          />
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center rounded-2xl bg-surface border border-border shadow-sm">
          <div className="p-4 rounded-full bg-zinc-900 border border-border text-zinc-500 mb-4 animate-pulse-glow">
            <FolderOpen className="w-8 h-8" />
          </div>
          <h3 className="text-lg font-bold text-white">All files organized</h3>
          <p className="text-sm text-zinc-500 mt-1 max-w-sm">
            {search
              ? "No unidentified files match your search keywords."
              : "Excellent! All of your local media files have been successfully classified as series or movies."}
          </p>
        </div>
      ) : (
        <motion.div
          variants={listVariants}
          initial="hidden"
          animate="show"
          className="flex flex-col gap-2.5"
        >
          {filtered.map((item) => {
            const playing = playingFilePath === item.filePath;
            const progressPercent =
              item.duration > 0 && item.lastPosition > 0 && !item.watched
                ? Math.min((item.lastPosition / item.duration) * 100, 100)
                : 0;

            return (
              <motion.button
                variants={itemVariants}
                draggable="true"
                onDragStart={(e) => {
                  e.dataTransfer.setData(
                    "text/plain",
                    JSON.stringify({ mediaIds: [item.id] })
                  );
                }}
                key={item.id}
                type="button"
                className={`group flex flex-col sm:flex-row sm:items-center sm:justify-between p-4 rounded-xl border transition-all duration-200 cursor-pointer shadow-premium gap-4 text-left focus:outline-none focus:ring-2 focus:ring-accent/35 ${
                  playing
                    ? "bg-accent/5 border-accent shadow-glow"
                    : "bg-surface-card border-border hover:bg-surface-hover hover:border-border-hover"
                }`}
                onDoubleClick={() => playFile(item)}
              >
                <div className="flex items-center gap-3.5 min-w-0 flex-1">
                  <div className={`p-2.5 rounded-lg transition-colors duration-200 ${
                    playing ? "bg-accent text-white" : "bg-zinc-800 text-zinc-400 group-hover:bg-accent group-hover:text-white"
                  }`}>
                    <FileVideo className="w-4 h-4" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-semibold text-white truncate group-hover:text-accent transition-colors duration-200">
                      {item.fileName}
                    </p>
                    <p className="text-xs text-zinc-500 truncate mt-0.5 font-medium">
                      {item.filePath}
                    </p>
                  </div>
                </div>

                <div className="flex items-center gap-4 sm:shrink-0 justify-end">
                  <span className="text-xs text-zinc-400 font-semibold">
                    {formatDuration(item.duration)}
                  </span>
                  
                  {item.watched && (
                    <span className="text-accent flex items-center gap-0.5 text-xs font-bold">
                      <Check className="w-3.5 h-3.5 stroke-[3]" />
                    </span>
                  )}

                  {playing && (
                    <span className="text-[10px] font-extrabold uppercase bg-accent text-white px-2 py-0.5 rounded tracking-wider shadow-sm select-none animate-pulse">
                      Playing
                    </span>
                  )}

                  {!item.watched && progressPercent > 0 && (
                    <div className="w-16 h-1.5 rounded-full bg-zinc-800 overflow-hidden relative shadow-inner">
                      <div className="h-full bg-accent transition-all duration-300" style={{ width: `${progressPercent}%` }} />
                    </div>
                  )}
                </div>
              </motion.button>
            );
          })}
        </motion.div>
      )}
    </motion.div>
  );
}
