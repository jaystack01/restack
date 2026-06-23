import { memo } from "react";
import { Play, FileVideo } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "../../utils/cn";

function formatTime(seconds) {
  if (!seconds || seconds < 0) return "0:00";
  const s = Math.floor(seconds % 60);
  const m = Math.floor((seconds / 60) % 60);
  const h = Math.floor(seconds / 3600);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function getFileName(path) {
  return path.split(/[\\/]/).pop() || path;
}

function ContinueWatchingRow({ item, onResume }) {
  if (!item) return null;

  const imgUrl = item.thumbnailPath ? convertFileSrc(item.thumbnailPath) : null;
  const progressPercent =
    item.duration > 0 ? Math.min((item.lastPosition / item.duration) * 100, 100) : 0;
  const heading = item.mode === "up_next" ? "Up Next" : "Continue Watching";
  const subTag =
    item.mode === "up_next"
      ? item.upNextType === "next_season"
        ? "Next Season"
        : "Next Episode"
      : null;

  return (
    <section className="space-y-4 select-none">
      <div className="flex items-center justify-between">
        <h2 className="text-xl md:text-2xl font-display font-extrabold tracking-tight text-white">
          {heading}
        </h2>
        {subTag && (
          <span className="text-[10px] font-extrabold uppercase bg-accent/15 text-accent border border-accent/25 px-2.5 py-0.5 rounded tracking-wider shadow-sm select-none">
            {subTag}
          </span>
        )}
      </div>

      <button
        key={item.filePath}
        type="button"
        onClick={() => onResume(item)}
        className="group relative flex flex-col md:flex-row items-stretch w-full max-w-2xl text-left rounded-2xl bg-surface-card border border-border hover:bg-surface-hover hover:border-border-hover transition-all duration-300 shadow-premium focus:outline-none focus:ring-2 focus:ring-accent/35 cursor-pointer overflow-hidden active:scale-[0.99]"
      >
        {/* Left/Top Thumbnail container */}
        <div className="relative aspect-video md:w-80 bg-black/40 overflow-hidden shrink-0">
          {imgUrl ? (
            <img
              src={imgUrl}
              alt="Episode thumbnail"
              className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
              draggable="false"
            />
          ) : (
            <div className="h-full w-full flex items-center justify-center text-zinc-600 bg-surface">
              <FileVideo className="w-10 h-10" />
            </div>
          )}

          {/* Interactive Play icon on Hover */}
          <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity duration-300">
            <div className="w-11 h-11 rounded-full bg-accent text-white flex items-center justify-center shadow-lg transition-transform duration-300 transform scale-90 group-hover:scale-100">
              <Play className="w-4 h-4 fill-white ml-0.5" />
            </div>
          </div>

          {/* Bottom inline progress bar */}
          {item.mode === "continue" && (
            <div className="absolute bottom-0 left-0 right-0 h-1 bg-black/40">
              <div className="h-full bg-accent transition-all duration-300" style={{ width: `${progressPercent}%` }} />
            </div>
          )}
        </div>

        {/* Right/Bottom Metadata container */}
        <div className="p-6 flex flex-col justify-between flex-1 min-w-0">
          <div className="space-y-1">
            <span className="text-[10px] font-extrabold tracking-wider uppercase text-accent">
              S{String(item.season).padStart(2, "0")}E{String(item.episode).padStart(2, "0")}
            </span>
            <h3 className="text-base font-semibold text-white truncate leading-snug group-hover:text-accent transition-colors duration-200">
              {getFileName(item.filePath)}
            </h3>
            <p className="text-xs text-zinc-400 truncate font-medium">
              {item.displayName}
            </p>
          </div>

          <div className="mt-4 flex items-center justify-between text-xs text-zinc-500 font-semibold select-none border-t border-border/50 pt-3">
            <span>
              {item.mode === "continue"
                ? `Resume at ${formatTime(item.lastPosition)}`
                : "Ready to stream"}
            </span>
            <span className="text-[10px] font-bold text-zinc-400 bg-zinc-800 px-2 py-0.5 rounded">
              {formatTime(item.duration)}
            </span>
          </div>
        </div>
      </button>
    </section>
  );
}

export default memo(ContinueWatchingRow);
