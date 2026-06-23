import { memo } from "react";
import { Play, Tv, Check } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cn } from "../../utils/cn";

function EpisodeGrid({
  episodes,
  selectedEpisodes,
  focusedEpisodeId,
  playingFilePath,
  onEpisodeClick,
  onEpisodeDoubleClick,
  bindEpisodeRef,
}) {
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 gap-5">
      {episodes.map((ep) => {
        const selected = selectedEpisodes.has(ep.filePath);
        const focused = focusedEpisodeId === ep.id;
        const playing = playingFilePath && ep.filePath === playingFilePath;
        const imgUrl = ep.thumbnailPath ? convertFileSrc(ep.thumbnailPath) : null;
        const progressPercent =
          ep.duration > 0 && ep.lastPosition > 0 && !ep.watched
            ? Math.min((ep.lastPosition / ep.duration) * 100, 100)
            : 0;

        const dragIds = selectedEpisodes.has(ep.filePath)
          ? episodes.filter((x) => selectedEpisodes.has(x.filePath)).map((x) => x.id).filter(Boolean)
          : [ep.id];

        return (
          <article
            key={ep.filePath}
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
              "group relative rounded-xl border bg-surface-card p-3 cursor-pointer transition-all duration-300 focus:outline-none",
              playing
                ? "border-accent ring-2 ring-accent/30 shadow-glow bg-accent/5"
                : selected
                  ? "border-accent/60 bg-accent/5"
                  : focused
                    ? "border-accent/40 ring-1 ring-accent/20"
                    : "border-border hover:border-border-hover hover:shadow-premium"
            )}
          >
            {playing && (
              <span className="absolute top-4 right-4 z-10 text-[9px] font-extrabold uppercase bg-accent text-white px-2 py-0.5 rounded tracking-wider shadow-sm select-none animate-pulse">
                Playing
              </span>
            )}
            
            <div className="relative aspect-video rounded-lg overflow-hidden bg-black/40 mb-3 select-none">
              {imgUrl ? (
                <img
                  src={imgUrl}
                  alt={ep.title || "Episode thumbnail"}
                  className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
                  draggable="false"
                />
              ) : (
                <div className="h-full w-full flex items-center justify-center text-zinc-600 bg-surface">
                  <Tv className="w-8 h-8" />
                </div>
              )}

              {/* Hover overlay with Lucide Play Icon */}
              <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity duration-300">
                <div className="w-9 h-9 rounded-full bg-accent text-white flex items-center justify-center shadow-lg transition-transform duration-300 transform scale-90 group-hover:scale-100">
                  <Play className="w-3.5 h-3.5 fill-white ml-0.5" />
                </div>
              </div>

              {/* Progress fill */}
              {!ep.watched && progressPercent > 0 && (
                <div className="absolute bottom-0 left-0 right-0 h-1 bg-black/40">
                  <div className="h-full bg-accent transition-all duration-300" style={{ width: `${progressPercent}%` }} />
                </div>
              )}
            </div>

            <h3 className="text-xs font-semibold text-white line-clamp-1 group-hover:text-accent transition-colors duration-200" title={ep.fileName}>
              {ep.fileName}
            </h3>
            
            <div className="mt-2 flex items-center justify-between">
              <span className="text-[10px] text-zinc-500 font-bold tracking-wider uppercase">
                Episode {ep.episode}
              </span>
              {ep.watched && (
                <span className="text-accent flex items-center gap-0.5 text-[10px] font-bold">
                  <Check className="w-3 h-3 stroke-[3.5]" />
                  Watched
                </span>
              )}
            </div>
          </article>
        );
      })}
    </div>
  );
}

export default memo(EpisodeGrid);
