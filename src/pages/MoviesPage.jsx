import { useEffect, useState } from "react";
import { Search, Film, Play, Check } from "lucide-react";
import { motion } from "framer-motion";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { getMovies, getPlayingFilePath, subscribe, subscribePlaying } from "../store/libraryStore";
import Input from "../components/ui/Input";
import { cn } from "../utils/cn";

export default function MoviesPage() {
  const [movies, setMovies] = useState(getMovies());
  const [search, setSearch] = useState("");
  const [playingFilePath, setPlayingFilePath] = useState(() => getPlayingFilePath() ?? null);

  useEffect(() => {
    const unsub = subscribe(() => setMovies(getMovies()));
    return unsub;
  }, []);

  useEffect(() => {
    const unsub = subscribePlaying((path) => setPlayingFilePath(path ?? null));
    return unsub;
  }, []);

  const filtered = search.trim()
    ? movies.filter((m) => m.title.toLowerCase().includes(search.trim().toLowerCase()))
    : movies;

  async function playMovie(movie) {
    try {
      await invoke("play_episode", { filePath: movie.filePath, startSeconds: null });
    } catch {
      // silent
    }
  }

  const containerVariants = {
    hidden: { opacity: 0 },
    show: {
      opacity: 1,
      transition: {
        staggerChildren: 0.04,
      },
    },
  };

  const itemVariants = {
    hidden: { opacity: 0, y: 12 },
    show: {
      opacity: 1,
      y: 0,
      transition: {
        type: "spring",
        stiffness: 120,
        damping: 16,
      },
    },
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6 select-none"
    >
      {/* Page Header and Search */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-display font-extrabold tracking-tight text-white">
            Movies
          </h1>
          <p className="text-sm text-zinc-400 mt-1">
            Browse and stream your standalone film collections.
          </p>
        </div>

        <div className="relative w-full sm:w-72">
          <Search className="absolute left-3 top-2.5 h-4.5 w-4.5 text-zinc-500" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search movies..."
            className="pl-10"
          />
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center rounded-2xl bg-surface border border-border shadow-sm">
          <div className="p-4 rounded-full bg-zinc-900 border border-border text-zinc-500 mb-4 animate-pulse-glow">
            <Film className="w-8 h-8" />
          </div>
          <h3 className="text-lg font-bold text-white">No movies found</h3>
          <p className="text-sm text-zinc-500 mt-1 max-w-sm">
            {search
              ? "No movie files match your current search terms. Try keywords."
              : "Standalone movie files will be automatically indexed based on filenames."}
          </p>
        </div>
      ) : (
        <motion.div
          variants={containerVariants}
          initial="hidden"
          animate="show"
          className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6"
        >
          {filtered.map((movie) => {
            const imgUrl = movie.thumbnailPath ? convertFileSrc(movie.thumbnailPath) : null;
            const playing = playingFilePath === movie.filePath;
            const progressPercent =
              movie.duration > 0 && movie.lastPosition > 0 && !movie.watched
                ? Math.min((movie.lastPosition / movie.duration) * 100, 100)
                : 0;

            return (
              <motion.button
                variants={itemVariants}
                draggable="true"
                onDragStart={(e) => {
                  e.dataTransfer.setData(
                    "text/plain",
                    JSON.stringify({ mediaIds: [movie.id] })
                  );
                }}
                whileHover={{
                  y: -4,
                  borderColor: playing ? "var(--color-accent)" : "var(--color-border-hover)",
                  boxShadow: "var(--shadow-premium)",
                }}
                transition={{ type: "tween", ease: "easeOut", duration: 0.2 }}
                key={movie.id}
                type="button"
                className={cn(
                  "group flex flex-col bg-surface-card border rounded-xl overflow-hidden cursor-pointer shadow-sm text-left focus:outline-none focus:ring-2 focus:ring-accent/35",
                  playing ? "border-accent shadow-glow" : "border-border"
                )}
                onDoubleClick={() => playMovie(movie)}
              >
                <div className="relative aspect-video w-full bg-black/40 overflow-hidden">
                  {imgUrl ? (
                    <img
                      src={imgUrl}
                      alt={movie.title}
                      className="w-full h-full object-cover transition-transform duration-500 group-hover:scale-105"
                      draggable="false"
                    />
                  ) : (
                    <div className="w-full h-full flex items-center justify-center text-zinc-600 bg-surface">
                      <Film className="w-8 h-8" />
                    </div>
                  )}

                  {playing && (
                    <span className="absolute top-3 right-3 text-[9px] font-extrabold uppercase bg-accent text-white px-2 py-0.5 rounded tracking-wider shadow-sm select-none animate-pulse">
                      Playing
                    </span>
                  )}

                  {/* Glass Play Overlay */}
                  <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity duration-300">
                    <div className="w-10 h-10 rounded-full bg-accent text-white flex items-center justify-center shadow-lg transition-transform duration-300 transform scale-90 group-hover:scale-100">
                      <Play className="w-4 h-4 fill-white ml-0.5" />
                    </div>
                  </div>

                  {/* Progress tracker */}
                  {!movie.watched && progressPercent > 0 && (
                    <div className="absolute bottom-0 left-0 right-0 h-1 bg-black/40">
                      <div className="h-full bg-accent transition-all duration-300" style={{ width: `${progressPercent}%` }} />
                    </div>
                  )}
                </div>

                <div className="p-4 flex-1 flex flex-col justify-between gap-1">
                  <div>
                    <h2 className="text-sm font-semibold text-white truncate group-hover:text-accent transition-colors duration-200">
                      {movie.title}
                    </h2>
                    <p className="text-xs text-zinc-400 mt-1 flex items-center gap-2">
                      {movie.year && <span>{movie.year}</span>}
                      {movie.watched && (
                        <span className="text-accent font-bold flex items-center gap-0.5">
                          <Check className="w-3.5 h-3.5 stroke-[3]" />
                          Watched
                        </span>
                      )}
                    </p>
                  </div>
                </div>
              </motion.button>
            );
          })}
        </motion.div>
      )}
    </motion.div>
  );
}
