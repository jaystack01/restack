import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { useState, useEffect } from "react";
import { Play, Check, Film, FileVideo } from "lucide-react";
import { cn } from "../../utils/cn";
import { getPlayingFilePath, subscribePlaying } from "../../store/libraryStore";

export default function ShowCard({
  episodeNumber,
  title,
  fileName,
  filePath,
  lastPosition,
  watched,
  duration,
  thumbnailPath,
}) {
  const [isOpening, setIsOpening] = useState(false);
  const [isPlaying, setIsPlaying] = useState(getPlayingFilePath() === filePath);

  useEffect(() => {
    const unsub = subscribePlaying((currentPlaying) => {
      setIsPlaying(currentPlaying === filePath);
    });
    setIsPlaying(getPlayingFilePath() === filePath);
    return unsub;
  }, [filePath]);

  function playEpisode() {
    if (isOpening) return;
    setIsOpening(true);
    invoke("play_episode", { filePath, startSeconds: null })
      .catch((err) => console.error("Failed to play:", err))
      .finally(() => setIsOpening(false));
  }

  const progressPercent =
    duration > 0 && lastPosition > 0 && !watched
      ? Math.min((lastPosition / duration) * 100, 100)
      : 0;

  const imgUrl = thumbnailPath ? convertFileSrc(thumbnailPath) : null;

  return (
    <article
      onClick={playEpisode}
      className={cn(
        "group relative flex flex-col rounded-xl bg-surface-card border p-3.5 cursor-pointer transition-all duration-300 select-none",
        isPlaying
          ? "border-accent ring-2 ring-accent/30 shadow-glow bg-accent/5"
          : "border-border hover:border-border-hover hover:shadow-premium"
      )}
    >
      {/* 16:9 Thumbnail Container */}
      <div className="relative w-full aspect-video rounded-lg overflow-hidden bg-black/40 mb-3 shadow-sm">
        {imgUrl ? (
          <img 
            src={imgUrl} 
            alt="Thumbnail" 
            className="w-full h-full object-cover transition-transform duration-500 group-hover:scale-105"
            draggable="false"
          />
        ) : (
          <div className="w-full h-full flex flex-col items-center justify-center text-zinc-600 bg-surface">
            <FileVideo className="w-8 h-8 mb-1" />
            <span className="text-[10px] font-semibold">No Preview</span>
          </div>
        )}

        {/* Hover play button overlay */}
        <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity duration-300">
          <div className="w-10 h-10 rounded-full bg-accent text-white flex items-center justify-center shadow-lg transition-transform duration-300 transform scale-90 group-hover:scale-100">
            <Play className="w-4 h-4 fill-white ml-0.5" />
          </div>
        </div>

        {/* Progress tracker */}
        {!watched && progressPercent > 0 && (
          <div className="absolute bottom-0 left-0 right-0 h-1 bg-black/40">
            <div
              className="h-full bg-accent transition-all duration-300"
              style={{ width: `${progressPercent}%` }}
            />
          </div>
        )}
      </div>

      {/* Floating statuses */}
      <div className="absolute top-5 right-5 flex gap-1.5">
        {watched && !isPlaying && (
          <span className="text-[9px] uppercase font-bold tracking-wider bg-black/75 backdrop-blur text-white px-2 py-0.5 rounded border border-white/5 shadow-sm">
            Watched
          </span>
        )}
        {isPlaying && (
          <span className="text-[9px] uppercase font-bold tracking-wider bg-accent text-white px-2 py-0.5 rounded shadow-sm animate-pulse">
            Playing
          </span>
        )}
      </div>

      {/* Metadata */}
      <div className="flex flex-col gap-1">
        <h4 className="text-xs font-bold text-white line-clamp-1 group-hover:text-accent transition-colors duration-200">
          E{String(episodeNumber).padStart(2, "0")}{title ? ` · ${title}` : ""}
        </h4>
        <p className="text-[10px] text-zinc-500 font-semibold truncate" title={fileName}>
          {fileName}
        </p>
      </div>

      {isOpening && (
        <div className="absolute inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center rounded-xl z-10 select-none">
          <p className="text-xs font-bold text-white animate-pulse">Initializing mpv...</p>
        </div>
      )}
    </article>
  );
}
