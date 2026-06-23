import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { Tv, Film, Folder } from "lucide-react";
import { motion } from "framer-motion";
import { getSeries, getMovies, getOthers, subscribe } from "../store/libraryStore";
import ContinueWatchingRow from "../components/library/ContinueWatchingRow";

export default function Dashboard() {
  const navigate = useNavigate();
  const [continueItem, setContinueItem] = useState(null);
  const [library, setLibrary] = useState({
    series: getSeries(),
    movies: getMovies(),
    others: getOthers(),
  });

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

  useEffect(() => {
    invoke("get_continue_watching_single_cmd")
      .then((item) => setContinueItem(item ?? null))
      .catch(() => setContinueItem(null));
  }, []);

  const totalEpisodes = library.series.reduce(
    (acc, show) =>
      acc + show.seasons.reduce((a, s) => a + s.episodes.length, 0),
    0,
  );

  const containerVariants = {
    hidden: { opacity: 0 },
    show: {
      opacity: 1,
      transition: {
        staggerChildren: 0.08,
      },
    },
  };

  const itemVariants = {
    hidden: { opacity: 0, y: 15 },
    show: { opacity: 1, y: 0, transition: { type: "spring", stiffness: 100 } },
  };

  return (
    <motion.div
      variants={containerVariants}
      initial="hidden"
      animate="show"
      className="space-y-8 select-none"
    >
      <div>
        <h1 className="text-3xl font-display font-extrabold tracking-tight text-white">
          Dashboard
        </h1>
        <p className="text-sm text-zinc-400 mt-1">
          Track watch progress and manage your local media intelligence layers.
        </p>
      </div>

      {/* Continue Watching Section */}
      <motion.div variants={itemVariants}>
        <ContinueWatchingRow
          item={continueItem}
          onResume={(ep) =>
            invoke("play_episode", {
              filePath: ep.filePath,
              startSeconds: null,
            })
          }
        />
      </motion.div>

      {/* Metrics Navigation Cards Grid */}
      <motion.div
        variants={itemVariants}
        className="grid grid-cols-1 md:grid-cols-3 gap-6 mt-4"
      >
        <button
          type="button"
          className="group relative flex flex-col items-start p-6 bg-surface-card hover:bg-surface-hover border border-border hover:border-border-hover rounded-2xl transition-all duration-300 shadow-premium cursor-pointer text-left focus:outline-none focus:ring-2 focus:ring-accent/35"
          onClick={() => navigate("/series")}
        >
          <div className="p-3.5 rounded-xl bg-accent/10 text-accent group-hover:bg-accent group-hover:text-white transition-all duration-300">
            <Tv className="w-5 h-5" />
          </div>
          <span className="text-3xl font-display font-extrabold tracking-tight text-white mt-4">
            {library.series.length}
          </span>
          <span className="text-sm font-semibold text-zinc-300 mt-1">Series</span>
          <span className="text-xs text-zinc-500 mt-0.5">{totalEpisodes} episodes indexed</span>
          <div className="absolute top-6 right-6 opacity-0 group-hover:opacity-100 text-accent text-xs font-semibold tracking-wider transition-opacity duration-300 uppercase">
            Browse →
          </div>
        </button>

        <button
          type="button"
          className="group relative flex flex-col items-start p-6 bg-surface-card hover:bg-surface-hover border border-border hover:border-border-hover rounded-2xl transition-all duration-300 shadow-premium cursor-pointer text-left focus:outline-none focus:ring-2 focus:ring-accent/35"
          onClick={() => navigate("/movies")}
        >
          <div className="p-3.5 rounded-xl bg-accent/10 text-accent group-hover:bg-accent group-hover:text-white transition-all duration-300">
            <Film className="w-5 h-5" />
          </div>
          <span className="text-3xl font-display font-extrabold tracking-tight text-white mt-4">
            {library.movies.length}
          </span>
          <span className="text-sm font-semibold text-zinc-300 mt-1">Movies</span>
          <span className="text-xs text-zinc-500 mt-0.5">Standalone film catalogs</span>
          <div className="absolute top-6 right-6 opacity-0 group-hover:opacity-100 text-accent text-xs font-semibold tracking-wider transition-opacity duration-300 uppercase">
            Browse →
          </div>
        </button>

        <button
          type="button"
          className="group relative flex flex-col items-start p-6 bg-surface-card hover:bg-surface-hover border border-border hover:border-border-hover rounded-2xl transition-all duration-300 shadow-premium cursor-pointer text-left focus:outline-none focus:ring-2 focus:ring-accent/35"
          onClick={() => navigate("/others")}
        >
          <div className="p-3.5 rounded-xl bg-accent/10 text-accent group-hover:bg-accent group-hover:text-white transition-all duration-300">
            <Folder className="w-5 h-5" />
          </div>
          <span className="text-3xl font-display font-extrabold tracking-tight text-white mt-4">
            {library.others.length}
          </span>
          <span className="text-sm font-semibold text-zinc-300 mt-1">Others</span>
          <span className="text-xs text-zinc-500 mt-0.5">Unclassified media lists</span>
          <div className="absolute top-6 right-6 opacity-0 group-hover:opacity-100 text-accent text-xs font-semibold tracking-wider transition-opacity duration-300 uppercase">
            Browse →
          </div>
        </button>
      </motion.div>
    </motion.div>
  );
}
