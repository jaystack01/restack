import { memo } from "react";
import { Play, Check, FileVideo } from "lucide-react";
import { cn } from "../../utils/cn";

function formatTime(seconds) {
  if (!seconds || seconds < 0) return "0:00";
  const s = Math.floor(seconds % 60);
  const m = Math.floor((seconds / 60) % 60);
  const h = Math.floor(seconds / 3600);
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function EpisodeList({
  episodes,
  selectedEpisodes,
  focusedEpisodeId,
  playingFilePath,
  onEpisodeClick,
  onEpisodeDoubleClick,
  bindEpisodeRef,
}) {
  return (
    <div className="flex flex-col gap-2">
      {episodes.map((ep) => {
        const selected = selectedEpisodes.has(ep.filePath);
        const focused = focusedEpisodeId === ep.id;
        const playing = playingFilePath && ep.filePath === playingFilePath;
        const progressPercent =
          ep.duration > 0 && ep.lastPosition > 0 && !ep.watched
            ? Math.min((ep.lastPosition / ep.duration) * 100, 100)
            : 0;

        const dragIds = selectedEpisodes.has(ep.filePath)
          ? episodes.filter((x) => selectedEpisodes.has(x.filePath)).map((x) => x.id).filter(Boolean)
          : [ep.id];

        return (
          <button
            key={ep.filePath}
            type="button"
            ref={bindEpisodeRef(ep.id)}
            tabIndex={focused ? 0 : -1}
            onClick={(e) => onEpisodeClick(e, ep)}
            onDoubleClick={(e) => onEpisodeDoubleClick(e, ep)}
            draggable="true"
            onDragStart={(e) => {
              e.dataTransfer.setData(
                "text/plain",
                JSON.stringify({ mediaIds: dragIds })
              );
            }}
            className={cn(
              "relative w-full text-left p-3.5 rounded-xl border transition-all duration-200 cursor-pointer focus:outline-none flex flex-col gap-2.5",
              playing
                ? "border-accent bg-accent/5 shadow-glow"
                : selected
                  ? "border-accent/60 bg-accent/5"
                  : focused
                    ? "border-accent/40 ring-1 ring-accent/20"
                    : "border-border bg-surface-card hover:bg-surface-hover hover:border-border-hover hover:shadow-premium"
            )}
          >
            {playing && (
              <span className="absolute top-3.5 right-3.5 text-[9px] font-extrabold uppercase bg-accent text-white px-2 py-0.5 rounded tracking-wider shadow-sm select-none animate-pulse">
                Playing
              </span>
            )}

            <div className="flex items-center justify-between gap-4 w-full pr-12">
              <div className="flex items-center gap-2.5 min-w-0 flex-1">
                <div className={cn(
                  "p-1.5 rounded transition-colors duration-200",
                  playing ? "bg-accent text-white" : "bg-zinc-800 text-zinc-500 group-hover:bg-accent group-hover:text-white"
                )}>
                  <Play className="w-3.5 h-3.5 fill-current" />
                </div>
                <span className="text-sm font-semibold text-white truncate group-hover:text-accent transition-colors duration-200" title={ep.fileName}>
                  {ep.fileName}
                </span>
              </div>
              <span className="text-xs text-zinc-500 font-semibold shrink-0">
                {formatTime(ep.lastPosition)} / {formatTime(ep.duration)}
              </span>
            </div>

            <div className="flex items-center gap-3 w-full">
              <span className="text-[10px] text-zinc-500 font-bold tracking-wider uppercase shrink-0">
                Episode {ep.episode}
              </span>

              {ep.watched ? (
                <span className="text-accent flex items-center gap-0.5 text-[10px] font-bold shrink-0">
                  <Check className="w-3 h-3 stroke-[3.5]" />
                  Watched
                </span>
              ) : (
                progressPercent > 0 && (
                  <div className="flex-1 h-1 rounded-full bg-zinc-800 overflow-hidden relative shadow-inner">
                    <div className="h-full bg-accent transition-all duration-300" style={{ width: `${progressPercent}%` }} />
                  </div>
                )
              )}
            </div>
          </button>
        );
      })}
    </div>
  );
}

export default memo(EpisodeList);
