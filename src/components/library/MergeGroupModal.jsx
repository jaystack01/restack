import { useState, useEffect } from "react";
import { getAllGroups, mergeGroups } from "../../api";
import { GitMerge, AlertCircle, Search } from "lucide-react";
import Modal from "../ui/Modal";
import Button from "../ui/Button";
import Input from "../ui/Input";
import { cn } from "../../utils/cn";

/**
 * Pick a target series and merge the source group into it. Rendered only
 * while open; Cancel gets initial focus because a merge can't be undone.
 * onClose()               — cancel, no changes
 * onSuccess(targetGroupId) — merge worked; the source group no longer exists
 */
export default function MergeGroupModal({
  sourceGroupId,
  sourceDisplayName,
  suggestedTargetId = null,
  onClose,
  onSuccess,
}) {
  const [groups, setGroups] = useState([]);
  const [selectedId, setSelectedId] = useState(suggestedTargetId);
  const [filter, setFilter] = useState("");
  const [loading, setLoading] = useState(false);
  const [fetching, setFetching] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let cancelled = false;
    getAllGroups()
      .then((all) => {
        if (!cancelled)
          setGroups(all.filter((g) => g.groupId !== sourceGroupId));
      })
      .catch(() => {
        if (!cancelled) setError("Failed to load series.");
      })
      .finally(() => {
        if (!cancelled) setFetching(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sourceGroupId]);

  const q = filter.trim().toLowerCase();
  const visible = q
    ? groups.filter(
        (g) =>
          g.groupId === selectedId || g.displayName.toLowerCase().includes(q),
      )
    : groups;
  const selected = groups.find((g) => g.groupId === selectedId);

  async function handleConfirm() {
    if (!selectedId || loading) return;
    setLoading(true);
    setError("");
    try {
      await mergeGroups(sourceGroupId, selectedId);
      onSuccess(selectedId);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to merge series.");
      setLoading(false);
    }
  }

  return (
    <Modal
      isOpen
      onClose={loading ? undefined : onClose}
      className="max-w-md"
      labelledBy="merge-title"
    >
      <div className="flex items-center gap-3 text-accent mb-2 select-none">
        <GitMerge className="w-6 h-6 stroke-[2]" />
        <h2
          id="merge-title"
          className="text-lg font-bold text-text-primary leading-none"
        >
          Merge Series
        </h2>
      </div>

      <p className="text-xs text-text-secondary leading-relaxed mb-4 select-none">
        All episodes of{" "}
        <span className="text-text-primary font-semibold">
          “{sourceDisplayName}”
        </span>{" "}
        move into the series you pick, and “{sourceDisplayName}” is removed.
        Later scans keep the files in the merged series. This can't be undone.
      </p>

      {!fetching && groups.length > 6 && (
        <div className="relative mb-3">
          <Search className="absolute left-3 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter series..."
            className="pl-9 py-1.5"
            aria-label="Filter series"
          />
        </div>
      )}

      {fetching ? (
        <div className="py-8 text-center text-text-muted text-xs font-semibold animate-pulse select-none">
          Loading series…
        </div>
      ) : groups.length === 0 ? (
        <div className="py-4 text-center text-text-muted text-xs font-semibold select-none">
          There are no other series to merge into.
        </div>
      ) : (
        <ul
          className="space-y-1.5 max-h-52 overflow-y-auto pr-1 select-none"
          aria-label="Target series"
        >
          {visible.map((g) => {
            const active = selectedId === g.groupId;
            return (
              <li key={g.groupId}>
                <button
                  type="button"
                  aria-pressed={active}
                  onClick={() => setSelectedId(g.groupId)}
                  className={cn(
                    "w-full text-left px-4 py-3 rounded-xl text-xs flex items-center justify-between gap-4 border transition-colors duration-200 cursor-pointer focus:outline-none focus:ring-2 focus:ring-accent/30",
                    active
                      ? "bg-accent/10 border-accent/30 text-text-primary font-semibold"
                      : "border-transparent text-text-secondary hover:bg-surface-hover hover:text-text-primary",
                  )}
                >
                  <span className="truncate flex-1">{g.displayName}</span>
                  <span className="text-text-muted font-bold shrink-0">
                    {g.episodeCount} ep
                  </span>
                </button>
              </li>
            );
          })}
          {visible.length === 0 && (
            <li className="py-4 text-center text-text-muted text-xs font-semibold">
              No series match the filter.
            </li>
          )}
        </ul>
      )}

      {error && (
        <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-red-200 text-xs mt-3 select-none">
          <AlertCircle className="w-4 h-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex gap-2.5 mt-6 justify-end">
        <Button
          onClick={onClose}
          disabled={loading}
          variant="secondary"
          data-autofocus
        >
          Cancel
        </Button>
        <Button
          onClick={handleConfirm}
          disabled={loading || !selected}
          variant="primary"
          className="min-w-0"
        >
          <GitMerge className="w-4 h-4 shrink-0" />
          <span className="truncate">
            {loading
              ? "Merging…"
              : selected
                ? `Merge into “${selected.displayName}”`
                : "Merge"}
          </span>
        </Button>
      </div>
    </Modal>
  );
}
