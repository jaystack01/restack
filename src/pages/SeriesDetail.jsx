import React, { useEffect, useMemo, useRef, useState } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Edit2, Search, LayoutGrid, List, SlidersHorizontal, ChevronRight, Info, Undo2, AlertCircle } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import EpisodeGrid from "../components/library/EpisodeGrid";
import EpisodeList from "../components/library/EpisodeList";
import MultiSelectBar from "../components/library/MultiSelectBar";
import Button from "../components/ui/Button";
import Input from "../components/ui/Input";
import Select from "../components/ui/Select";
import Checkbox from "../components/ui/Checkbox";
import Modal from "../components/ui/Modal";
import {
  getSeries,
  getPlayingFilePath,
  setLibrary,
  subscribe,
  subscribePlaying,
} from "../store/libraryStore";

const EPISODE_CLICK_DELAY_MS = 280;
const getFileName = (path) => path.split(/[\\/]/).pop() || path;
const seasonKey = (groupId, seasonNumber) => `${groupId}:${seasonNumber}`;

export default function SeriesDetail() {
  const { groupId } = useParams();
  const navigate = useNavigate();
  const numericGroupId = Number(groupId);

  const [shows, setShows] = useState(() => getSeries());
  const [error, setError] = useState("");
  const episodeClickTimerRef = useRef(null);

  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState("grid");
  const [sortMode, setSortMode] = useState("episode");
  const [selectedEpisodes, setSelectedEpisodes] = useState(new Set());
  const [collapsedSeasons, setCollapsedSeasons] = useState({});
  const [recentlyDeleted, setRecentlyDeleted] = useState(null);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [pendingDeletePaths, setPendingDeletePaths] = useState([]);
  const [renamingGroupId, setRenamingGroupId] = useState(null);
  const [renameValue, setRenameValue] = useState("");
  const [focusedEpisodeId, setFocusedEpisodeId] = useState(null);
  const [lastSelectedEpisodeId, setLastSelectedEpisodeId] = useState(null);
  const [playingFilePath, setPlayingFilePathLocal] = useState(
    () => getPlayingFilePath() ?? null,
  );
  const episodeRefs = useRef(new Map());

  useEffect(() => {
    const unsub = subscribe((data) => setShows(data?.series || []));
    return unsub;
  }, []);

  useEffect(() => {
    const unsub = subscribePlaying((path) =>
      setPlayingFilePathLocal(path ?? null),
    );
    return unsub;
  }, []);

  useEffect(() => {
    return () => {
      if (episodeClickTimerRef.current) {
        clearTimeout(episodeClickTimerRef.current);
      }
    };
  }, []);

  const selectedGroupData = useMemo(() => {
    const group = shows.find((s) => s.groupId === numericGroupId);
    if (!group) return null;
    const seasons = group.seasons
      .map((season) => ({
        season: season.season,
        episodes: season.episodes.map((ep) => ({
          ...ep,
          season: season.season,
          groupId: group.groupId,
          displayName: group.displayName,
          fileName: getFileName(ep.filePath),
        })),
      }))
      .sort((a, b) => a.season - b.season);
    return {
      ...group,
      seasonCount: seasons.length,
      seasons,
      episodeCount: seasons.reduce(
        (acc, season) => acc + season.episodes.length,
        0,
      ),
      episodes: seasons.flatMap((season) => season.episodes),
    };
  }, [shows, numericGroupId]);

  // Redirect if group not found
  useEffect(() => {
    if (shows.length > 0 && !selectedGroupData) {
      navigate("/series", { replace: true });
    }
  }, [shows, selectedGroupData, navigate]);

  const groups = useMemo(() => {
    return shows.map((show) => ({
      ...show,
      episodeCount: show.seasons.reduce(
        (acc, s) => acc + s.episodes.length,
        0,
      ),
    }));
  }, [shows]);

  const visibleSeasonGroups = useMemo(() => {
    if (!selectedGroupData) return [];
    const q = search.trim().toLowerCase();
    return selectedGroupData.seasons
      .map((season) => {
        let episodes = season.episodes;
        if (q) {
          episodes = episodes.filter((ep) =>
            ep.fileName.toLowerCase().includes(q),
          );
        }
        episodes = [...episodes].sort((a, b) => {
          if (sortMode === "recent") return b.lastPosition - a.lastPosition;
          if (sortMode === "episode") return a.episode - b.episode;
          const aBoost = !a.watched && a.lastPosition > 0 ? 1 : 0;
          const bBoost = !b.watched && b.lastPosition > 0 ? 1 : 0;
          if (aBoost !== bBoost) return bBoost - aBoost;
          return a.episode - b.episode;
        });
        return { season: season.season, episodes };
      })
      .filter((season) => season.episodes.length > 0);
  }, [selectedGroupData, search, sortMode]);

  const visibleEpisodePaths = useMemo(() => {
    if (!selectedGroupData) return [];
    return visibleSeasonGroups
      .filter(
        (season) =>
          !collapsedSeasons?.[selectedGroupData.groupId]?.[season.season],
      )
      .flatMap((season) => season.episodes.map((ep) => ep.filePath));
  }, [visibleSeasonGroups, selectedGroupData, collapsedSeasons]);

  const visibleEpisodesOrdered = useMemo(() => {
    if (!selectedGroupData) return [];
    return visibleSeasonGroups
      .filter(
        (season) =>
          !collapsedSeasons?.[selectedGroupData.groupId]?.[season.season],
      )
      .flatMap((season) => season.episodes);
  }, [visibleSeasonGroups, selectedGroupData, collapsedSeasons]);

  const allInViewSelected = useMemo(() => {
    if (!visibleEpisodePaths.length) return false;
    return visibleEpisodePaths.every((fp) => selectedEpisodes.has(fp));
  }, [visibleEpisodePaths, selectedEpisodes]);

  useEffect(() => {
    if (!visibleEpisodesOrdered.length) {
      setFocusedEpisodeId(null);
      return;
    }
    if (
      !focusedEpisodeId ||
      !visibleEpisodesOrdered.some((ep) => ep.id === focusedEpisodeId)
    ) {
      setFocusedEpisodeId(visibleEpisodesOrdered[0].id);
    }
  }, [visibleEpisodesOrdered, focusedEpisodeId]);

  useEffect(() => {
    if (!focusedEpisodeId) return;
    const node = episodeRefs.current.get(focusedEpisodeId);
    if (node) node.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [focusedEpisodeId]);

  function bindEpisodeRef(id) {
    return (node) => {
      if (node) episodeRefs.current.set(id, node);
      else episodeRefs.current.delete(id);
    };
  }

  function getGridColumns() {
    const w = window.innerWidth;
    if (w >= 1536) return 5;
    if (w >= 1280) return 4;
    if (w >= 1024) return 3;
    if (w >= 640) return 2;
    return 1;
  }

  function selectRange(toEpisodeId) {
    if (!lastSelectedEpisodeId) return;
    const fromIndex = visibleEpisodesOrdered.findIndex(
      (ep) => ep.id === lastSelectedEpisodeId,
    );
    const toIndex = visibleEpisodesOrdered.findIndex(
      (ep) => ep.id === toEpisodeId,
    );
    if (fromIndex < 0 || toIndex < 0) return;
    const [start, end] =
      fromIndex < toIndex ? [fromIndex, toIndex] : [toIndex, fromIndex];
    setSelectedEpisodes((prev) => {
      const next = new Set(prev);
      for (let i = start; i <= end; i += 1)
        next.add(visibleEpisodesOrdered[i].filePath);
      return next;
    });
  }

  function applyEpisodeSelection(event, ep) {
    const multi = event.ctrlKey || event.metaKey;
    const range = event.shiftKey;
    if (range) {
      selectRange(ep.id);
      setLastSelectedEpisodeId(ep.id);
      return;
    }
    if (multi) {
      setSelectedEpisodes((prev) => {
        const next = new Set(prev);
        if (next.has(ep.filePath)) next.delete(ep.filePath);
        else next.add(ep.filePath);
        return next;
      });
      setLastSelectedEpisodeId(ep.id);
      return;
    }
    setSelectedEpisodes(new Set([ep.filePath]));
    setLastSelectedEpisodeId(ep.id);
  }

  function onEpisodeClick(event, ep) {
    setFocusedEpisodeId(ep.id);
    if (episodeClickTimerRef.current) {
      clearTimeout(episodeClickTimerRef.current);
      episodeClickTimerRef.current = null;
    }
    episodeClickTimerRef.current = setTimeout(() => {
      episodeClickTimerRef.current = null;
      applyEpisodeSelection(event, ep);
    }, EPISODE_CLICK_DELAY_MS);
  }

  async function onEpisodeDoubleClick(event, ep) {
    if (episodeClickTimerRef.current) {
      clearTimeout(episodeClickTimerRef.current);
      episodeClickTimerRef.current = null;
    }
    event.preventDefault();
    try {
      await invoke("play_episode", { filePath: ep.filePath, startSeconds: null });
    } catch {
      setError("Failed to start playback.");
    }
  }

  function toggleSelectAllVisible() {
    setSelectedEpisodes((prev) => {
      const next = new Set(prev);
      if (allInViewSelected) {
        visibleEpisodePaths.forEach((fp) => next.delete(fp));
      } else {
        visibleEpisodePaths.forEach((fp) => next.add(fp));
      }
      return next;
    });
  }

  function toggleSeasonCollapse(seasonNumber) {
    if (!selectedGroupData) return;
    setCollapsedSeasons((prev) => {
      const prevGroup = prev[selectedGroupData.groupId] || {};
      return {
        ...prev,
        [selectedGroupData.groupId]: {
          ...prevGroup,
          [seasonNumber]: !prevGroup[seasonNumber],
        },
      };
    });
  }

  async function fetchLibrary(scan = false) {
    try {
      const data = await invoke(
        scan ? "scan_user_storage" : "get_library_grouped",
      );
      setLibrary(data);
    } catch (err) {
      setError(typeof err === "string" ? err : "Something went wrong");
    }
  }

  async function updateEpisodesWatchState(episodes, watched) {
    if (!episodes.length) return;
    try {
      await Promise.all(
        episodes.map((ep) =>
          invoke("update_watch_progress", {
            payload: {
              filePath: ep.filePath,
              lastPosition: watched ? ep.duration || ep.lastPosition || 0 : 0,
              watched,
            },
          }),
        ),
      );
      await fetchLibrary(false);
    } catch {
      setError("Failed to update watch state.");
    }
  }

  async function markSelectedWatched() {
    const selected =
      selectedGroupData?.episodes.filter((ep) =>
        selectedEpisodes.has(ep.filePath),
      ) || [];
    await updateEpisodesWatchState(selected, true);
    setSelectedEpisodes(new Set());
  }

  async function markSelectedUnwatched() {
    const selected =
      selectedGroupData?.episodes.filter((ep) =>
        selectedEpisodes.has(ep.filePath),
      ) || [];
    await updateEpisodesWatchState(selected, false);
    setSelectedEpisodes(new Set());
  }

  async function moveSelectedEpisodes(targetGroupId) {
    const selected =
      selectedGroupData?.episodes.filter((ep) =>
        selectedEpisodes.has(ep.filePath),
      ) || [];
    const mediaIds = selected.map((ep) => ep.id).filter(Boolean);
    if (!mediaIds.length) return;
    try {
      await invoke("move_episodes_cmd", { mediaIds, targetGroupId });
      await fetchLibrary(false);
      setSelectedEpisodes(new Set());
    } catch {
      setError("Failed to move selected episodes.");
    }
  }

  async function deleteSelectedEpisodes() {
    const filePaths = Array.from(selectedEpisodes);
    if (!filePaths.length) return;
    setPendingDeletePaths(filePaths);
    setDeleteConfirmOpen(true);
  }

  async function confirmDelete() {
    setDeleteConfirmOpen(false);
    const filePaths = pendingDeletePaths;
    setPendingDeletePaths([]);
    try {
      await invoke("delete_episodes_cmd", { filePaths });
      await fetchLibrary(false);
      setSelectedEpisodes(new Set());
      const timeoutId = setTimeout(() => setRecentlyDeleted(null), 10000);
      setRecentlyDeleted({ filePaths, timeoutId });
    } catch {
      setError("Failed to delete episodes.");
    }
  }

  async function undoDelete() {
    if (!recentlyDeleted) return;
    try {
      await invoke("restore_episodes_cmd", {
        filePaths: recentlyDeleted.filePaths,
      });
      await fetchLibrary(false);
      clearTimeout(recentlyDeleted.timeoutId);
      setRecentlyDeleted(null);
    } catch {
      setError("Failed to restore episodes.");
    }
  }

  async function submitInlineRename() {
    if (!selectedGroupData) return;
    const newName = renameValue.trim();
    if (!newName || newName === selectedGroupData.displayName)
      return setRenamingGroupId(null);
    try {
      await invoke("rename_group_cmd", {
        groupId: selectedGroupData.groupId,
        newName,
      });
      setRenamingGroupId(null);
      await fetchLibrary(false);
    } catch {
      setError("Failed to rename group.");
    }
  }

  // Keyboard navigation
  useEffect(() => {
    function onKeyDown(e) {
      const key = e.key.toLowerCase();
      const activeTag = document.activeElement?.tagName?.toLowerCase();
      if (activeTag === "input" || activeTag === "select" || activeTag === "textarea") return;

      if ((e.ctrlKey || e.metaKey) && key === "a") {
        e.preventDefault();
        setSelectedEpisodes(new Set(visibleEpisodePaths));
        return;
      }
      if (key === "escape") {
        e.preventDefault();
        setSelectedEpisodes(new Set());
        return;
      }
      if (!visibleEpisodesOrdered.length) return;
      if (["arrowleft", "arrowright", "arrowup", "arrowdown"].includes(key)) {
        e.preventDefault();
        const cols = viewMode === "grid" ? getGridColumns() : 1;
        const currentIndex = Math.max(
          0,
          visibleEpisodesOrdered.findIndex((ep) => ep.id === focusedEpisodeId),
        );
        let nextIndex = currentIndex;
        if (key === "arrowleft") nextIndex = Math.max(0, currentIndex - 1);
        if (key === "arrowright")
          nextIndex = Math.min(visibleEpisodesOrdered.length - 1, currentIndex + 1);
        if (key === "arrowup") nextIndex = Math.max(0, currentIndex - cols);
        if (key === "arrowdown")
          nextIndex = Math.min(visibleEpisodesOrdered.length - 1, currentIndex + cols);
        const target = visibleEpisodesOrdered[nextIndex];
        if (target) setFocusedEpisodeId(target.id);
        return;
      }
      if (key === "enter" && focusedEpisodeId) {
        e.preventDefault();
        const target = visibleEpisodesOrdered.find((ep) => ep.id === focusedEpisodeId);
        if (target) onEpisodeDoubleClick(e, target);
        return;
      }
      if (key === "w" && selectedEpisodes.size > 0) { e.preventDefault(); markSelectedWatched(); }
      if (key === "u" && selectedEpisodes.size > 0) { e.preventDefault(); markSelectedUnwatched(); }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visibleEpisodePaths, visibleEpisodesOrdered, focusedEpisodeId, selectedEpisodes, viewMode]);

  if (!selectedGroupData) return null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6 select-none"
    >
      {/* Back Link navigation */}
      <Link
        to="/series"
        className="inline-flex items-center gap-1.5 text-xs font-semibold text-zinc-400 hover:text-zinc-100 transition-colors duration-150 group"
      >
        <ArrowLeft className="w-3.5 h-3.5 group-hover:-translate-x-0.5 transition-transform" />
        <span>Back to Series</span>
      </Link>

      {error && (
        <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-red-200 text-sm">
          <AlertCircle className="w-4 h-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {recentlyDeleted && (
        <div className="flex items-center justify-between gap-4 p-4 rounded-xl bg-accent/5 border border-accent/20 text-white text-sm animate-in shadow-premium shadow-accent/5">
          <div className="flex items-center gap-2">
            <Info className="w-4 h-4 text-accent shrink-0" />
            <span>{recentlyDeleted.filePaths.length} episode(s) moved to Trash.</span>
          </div>
          <Button onClick={undoDelete} variant="primary" size="sm" className="flex items-center gap-1">
            <Undo2 className="w-3.5 h-3.5" />
            <span>Undo</span>
          </Button>
        </div>
      )}

      <MultiSelectBar
        selectedCount={selectedEpisodes.size}
        allInViewSelected={allInViewSelected}
        groups={groups}
        selectedGroupId={numericGroupId}
        onToggleSelectAll={toggleSelectAllVisible}
        onMove={moveSelectedEpisodes}
        onMarkWatched={markSelectedWatched}
        onMarkUnwatched={markSelectedUnwatched}
        onDelete={deleteSelectedEpisodes}
        onClear={() => setSelectedEpisodes(new Set())}
      />

      <section className="space-y-6">
        {/* Detail Header controls */}
        <div className="flex flex-col md:flex-row md:items-start md:justify-between gap-6">
          <div className="flex-1 min-w-0">
            {renamingGroupId === selectedGroupData.groupId ? (
              <input
                autoFocus
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                onBlur={submitInlineRename}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submitInlineRename();
                  if (e.key === "Escape") setRenamingGroupId(null);
                }}
                className="w-full max-w-xl text-3xl font-display font-extrabold text-white bg-surface-card border border-border focus:border-accent rounded-lg px-3 py-1.5 outline-none transition-colors duration-200 shadow-inner"
              />
            ) : (
              <h2
                className="group inline-flex items-center gap-3 text-3xl font-display font-extrabold tracking-tight text-white cursor-pointer hover:text-accent transition-colors duration-200"
                onClick={() => {
                  setRenamingGroupId(selectedGroupData.groupId);
                  setRenameValue(selectedGroupData.displayName);
                }}
                title="Click to rename series display name"
              >
                <span>{selectedGroupData.displayName}</span>
                <Edit2 className="w-4 h-4 text-zinc-500 opacity-0 group-hover:opacity-100 transition-opacity" />
              </h2>
            )}
            <p className="text-sm text-zinc-400 mt-1">
              {selectedGroupData.episodeCount} Episodes · {selectedGroupData.seasonCount} Season{selectedGroupData.seasonCount !== 1 ? "s" : ""}
            </p>
          </div>

          {/* List and Layout mode selectors */}
          <div className="flex flex-wrap items-center gap-3 md:shrink-0 justify-start md:justify-end">
            <div className="relative w-44 sm:w-48">
              <Search className="absolute left-3 top-2.5 h-4 w-4 text-zinc-500" />
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search..."
                className="pl-9 py-1.5"
              />
            </div>

            <div className="inline-flex rounded-lg border border-border bg-surface-card p-0.5 shadow-sm">
              <button
                type="button"
                onClick={() => setViewMode("grid")}
                className={`p-1.5 rounded-md transition-colors focus:outline-none ${
                  viewMode === "grid" ? "bg-accent text-white" : "text-zinc-400 hover:text-zinc-100"
                }`}
                title="Grid layout view"
              >
                <LayoutGrid className="w-4 h-4" />
              </button>
              <button
                type="button"
                onClick={() => setViewMode("list")}
                className={`p-1.5 rounded-md transition-colors focus:outline-none ${
                  viewMode === "list" ? "bg-accent text-white" : "text-zinc-400 hover:text-zinc-100"
                }`}
                title="List layout view"
              >
                <List className="w-4 h-4" />
              </button>
            </div>

            <div className="flex items-center gap-1.5 text-zinc-400 text-xs font-semibold bg-surface-card border border-border rounded-lg px-2.5 py-1.5 shadow-sm">
              <SlidersHorizontal className="w-3.5 h-3.5" />
              <Select
                value={sortMode}
                onChange={(e) => setSortMode(e.target.value)}
                className="bg-transparent border-0 hover:bg-transparent px-0 py-0 text-xs max-w-[90px] focus:ring-0 shadow-none text-zinc-300"
              >
                <option value="episode">Order</option>
                <option value="recent">Recent</option>
              </Select>
            </div>
          </div>
        </div>

        {/* Dynamic collapsible seasons container */}
        {visibleSeasonGroups.length > 0 ? (
          <div className="space-y-6">
            {visibleSeasonGroups.map((season) => {
              const key = seasonKey(selectedGroupData.groupId, season.season);
              const collapsed = Boolean(
                collapsedSeasons?.[selectedGroupData.groupId]?.[season.season],
              );
              const seasonAll = season.episodes.every((ep) =>
                selectedEpisodes.has(ep.filePath),
              );
              const seasonSome = season.episodes.some((ep) =>
                selectedEpisodes.has(ep.filePath),
              );
              return (
                <section key={key} className="space-y-4">
                  {/* Collapsible header wrapper */}
                  <div className="flex items-center justify-between p-4 rounded-xl bg-surface-card border border-border hover:bg-surface-hover hover:border-border-hover transition-colors duration-250">
                    <div className="flex items-center gap-3.5">
                      <Checkbox
                        checked={seasonAll}
                        indeterminate={seasonSome && !seasonAll}
                        onChange={(e) =>
                          setSelectedEpisodes((prev) => {
                            const next = new Set(prev);
                            if (e.target.checked)
                              season.episodes.forEach((ep) => next.add(ep.filePath));
                            else
                              season.episodes.forEach((ep) => next.delete(ep.filePath));
                            return next;
                          })
                        }
                      />
                      <div className="text-left select-none">
                        <h3 className="text-base font-bold text-white leading-none">
                          Season {season.season}
                        </h3>
                        <span className="text-[10px] text-zinc-500 font-bold uppercase tracking-wider mt-1 block">
                          {season.episodes.length} Episodes
                        </span>
                      </div>
                    </div>

                    <Button
                      type="button"
                      onClick={() => toggleSeasonCollapse(season.season)}
                      variant="ghost"
                      size="sm"
                      className="flex items-center gap-1"
                    >
                      <ChevronRight
                        className={`w-4 h-4 text-zinc-400 transition-transform duration-200 ${
                          !collapsed && "rotate-90"
                        }`}
                      />
                      <span>{collapsed ? "Expand" : "Collapse"}</span>
                    </Button>
                  </div>

                  {/* Smooth animated expander */}
                  <AnimatePresence initial={false}>
                    {!collapsed && (
                      <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.2, ease: "easeInOut" }}
                        className="overflow-hidden"
                      >
                        {viewMode === "grid" ? (
                          <EpisodeGrid
                            episodes={season.episodes}
                            selectedEpisodes={selectedEpisodes}
                            focusedEpisodeId={focusedEpisodeId}
                            playingFilePath={playingFilePath}
                            onEpisodeClick={onEpisodeClick}
                            onEpisodeDoubleClick={onEpisodeDoubleClick}
                            bindEpisodeRef={bindEpisodeRef}
                          />
                        ) : (
                          <EpisodeList
                            episodes={season.episodes}
                            selectedEpisodes={selectedEpisodes}
                            focusedEpisodeId={focusedEpisodeId}
                            playingFilePath={playingFilePath}
                            onEpisodeClick={onEpisodeClick}
                            onEpisodeDoubleClick={onEpisodeDoubleClick}
                            bindEpisodeRef={bindEpisodeRef}
                          />
                        )}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </section>
              );
            })}
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center p-12 text-center rounded-2xl bg-surface border border-border shadow-sm">
            <h3 className="text-lg font-bold text-white">No episodes found</h3>
            <p className="text-sm text-zinc-500 mt-1 max-w-sm">
              Adjust search keywords or sync library folders to locate files.
            </p>
          </div>
        )}
      </section>

      {/* Delete Confirmation Modal */}
      <Modal
        isOpen={deleteConfirmOpen}
        onClose={() => setDeleteConfirmOpen(false)}
        className="max-w-md"
      >
        <div className="flex items-center gap-3 text-red-400 mb-4 select-none">
          <AlertCircle className="w-6 h-6 stroke-[2]" />
          <h3 className="text-lg font-bold text-white">Delete Episodes</h3>
        </div>
        <p className="text-zinc-400 text-sm mb-6 leading-relaxed select-none">
          Are you sure you want to delete <span className="text-white font-semibold">{pendingDeletePaths.length}</span> selected episode(s)?
          They will be relocated to your operating system's Trash container.
        </p>
        <div className="flex gap-2.5 justify-end">
          <Button onClick={() => setDeleteConfirmOpen(false)} variant="secondary">
            Cancel
          </Button>
          <Button onClick={confirmDelete} variant="danger">
            Move to Trash
          </Button>
        </div>
      </Modal>
    </motion.div>
  );
}
