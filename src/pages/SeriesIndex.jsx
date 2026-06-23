import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Search, Tv } from "lucide-react";
import { motion } from "framer-motion";
import { getSeries, subscribe } from "../store/libraryStore";
import { convertFileSrc } from "@tauri-apps/api/core";
import Input from "../components/ui/Input";

export default function SeriesIndex() {
  const navigate = useNavigate();
  const [series, setSeries] = useState(getSeries());
  const [search, setSearch] = useState("");

  useEffect(() => {
    const unsub = subscribe(() => setSeries(getSeries()));
    return unsub;
  }, []);

  const filtered = search.trim()
    ? series.filter((s) =>
        s.displayName.toLowerCase().includes(search.trim().toLowerCase()),
      )
    : series;

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
      {/* Header and Search Box */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <div>
          <h1 className="text-3xl font-display font-extrabold tracking-tight text-white">
            Series
          </h1>
          <p className="text-sm text-zinc-400 mt-1">
            Browse and organize your television show groups.
          </p>
        </div>

        <div className="relative w-full sm:w-72">
          <Search className="absolute left-3 top-2.5 h-4.5 w-4.5 text-zinc-500" />
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search series..."
            className="pl-10"
          />
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center rounded-2xl bg-surface border border-border shadow-sm">
          <div className="p-4 rounded-full bg-zinc-900 border border-border text-zinc-500 mb-4 animate-pulse-glow">
            <Tv className="w-8 h-8" />
          </div>
          <h3 className="text-lg font-bold text-white">No series found</h3>
          <p className="text-sm text-zinc-500 mt-1 max-w-sm">
            {search
              ? "No groups match your current query. Try adjusting your keywords."
              : "Sync your library from the sidebar button to discover local folders."}
          </p>
        </div>
      ) : (
        <motion.div
          variants={containerVariants}
          initial="hidden"
          animate="show"
          className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-6"
        >
          {filtered.map((show) => {
            const epCount = show.seasons.reduce(
              (acc, s) => acc + s.episodes.length,
              0,
            );
            const firstEp = show.seasons[0]?.episodes[0];
            const thumbUrl = firstEp?.thumbnailPath
              ? convertFileSrc(firstEp.thumbnailPath)
              : null;
            const watchedCount = show.seasons.reduce(
              (acc, s) =>
                acc + s.episodes.filter((ep) => ep.watched).length,
              0,
            );

            const watchPercent = epCount > 0 ? (watchedCount / epCount) * 100 : 0;

            return (
              <motion.button
                variants={itemVariants}
                whileHover={{
                  y: -4,
                  borderColor: "var(--color-border-hover)",
                  boxShadow: "var(--shadow-premium)",
                }}
                transition={{ type: "tween", ease: "easeOut", duration: 0.2 }}
                key={show.groupId}
                type="button"
                className="group flex flex-col bg-surface-card border border-border rounded-xl overflow-hidden cursor-pointer shadow-sm text-left focus:outline-none focus:ring-2 focus:ring-accent/35"
                onClick={() => navigate(`/series/${show.groupId}`)}
              >
                <div className="relative aspect-video w-full bg-black/40 overflow-hidden">
                  {thumbUrl ? (
                    <img
                      src={thumbUrl}
                      alt={show.displayName}
                      className="w-full h-full object-cover transition-transform duration-500 group-hover:scale-105"
                      draggable="false"
                    />
                  ) : (
                    <div className="w-full h-full flex items-center justify-center text-zinc-600 bg-surface">
                      <Tv className="w-8 h-8" />
                    </div>
                  )}
                  {watchedCount > 0 && (
                    <div className="absolute bottom-0 left-0 right-0 h-1.5 bg-black/40">
                      <div
                        className="h-full bg-accent transition-all duration-300"
                        style={{ width: `${watchPercent}%` }}
                      />
                    </div>
                  )}
                </div>

                <div className="p-4 flex-1 flex flex-col justify-between">
                  <div>
                    <h2 className="text-sm font-semibold text-white truncate group-hover:text-accent transition-colors duration-200">
                      {show.displayName}
                    </h2>
                    <p className="text-xs text-zinc-400 mt-1">
                      {show.seasons.length} Season{show.seasons.length !== 1 ? "s" : ""} · {epCount} Episode{epCount !== 1 ? "s" : ""}
                    </p>
                  </div>
                  {watchedCount === epCount && (
                    <span className="inline-block mt-3 text-[10px] font-bold text-accent tracking-wider uppercase">
                      ✓ Completed
                    </span>
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
