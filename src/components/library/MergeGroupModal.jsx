import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { GitMerge, X, AlertTriangle } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import Button from "../ui/Button";

export default function MergeGroupModal({ sourceGroupId, sourceDisplayName, onClose, onSuccess }) {
  const [groups, setGroups] = useState([]);
  const [selectedId, setSelectedId] = useState(null);
  const [loading, setLoading] = useState(false);
  const [fetching, setFetching] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke("get_all_groups_cmd")
      .then((all) => setGroups(all.filter((g) => g.groupId !== sourceGroupId)))
      .catch(() => setError("Failed to load groups"))
      .finally(() => setFetching(false));
  }, [sourceGroupId]);

  async function handleConfirm() {
    if (!selectedId) return;
    setLoading(true);
    setError("");
    try {
      await invoke("merge_groups_cmd", {
        sourceGroupId,
        targetGroupId: selectedId,
      });
      onSuccess();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to merge groups");
      setLoading(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        className="fixed inset-0 bg-black/60 backdrop-blur-md"
        onClick={onClose}
      />

      {/* Modal Card */}
      <motion.div
        initial={{ opacity: 0, scale: 0.95, y: 15 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        className="relative w-full max-w-md bg-surface border border-border p-6 rounded-2xl shadow-premium z-10 overflow-hidden select-none"
        role="dialog"
        aria-modal="true"
        aria-labelledby="merge-title"
      >
        <button
          onClick={onClose}
          className="absolute top-4 right-4 p-1.5 rounded-full text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover transition-colors duration-150 focus:outline-none"
          aria-label="Close modal"
        >
          <X className="w-4 h-4" />
        </button>

        <div className="flex items-center gap-3 text-accent mb-2">
          <GitMerge className="w-6 h-6 stroke-[2]" />
          <h2 id="merge-title" className="text-lg font-bold text-white leading-none">
            Merge Series Groups
          </h2>
        </div>

        <p className="text-xs text-zinc-400 leading-relaxed mb-5">
          All episodes from <span className="text-white font-semibold">"{sourceDisplayName}"</span> will be relocated into the selected target group. This action is irreversible.
        </p>

        {fetching ? (
          <div className="flex flex-col items-center justify-center py-8 text-zinc-500 text-xs font-semibold animate-pulse">
            Loading database groups…
          </div>
        ) : groups.length === 0 ? (
          <div className="text-zinc-500 text-xs font-semibold py-4 text-center">
            No other active series groups discovered.
          </div>
        ) : (
          <ul className="space-y-1.5 max-h-52 overflow-y-auto pr-1">
            {groups.map((g) => (
              <li key={g.groupId}>
                <button
                  onClick={() => setSelectedId(g.groupId)}
                  className={`
                    w-full text-left px-4 py-3 rounded-xl text-xs flex items-center justify-between gap-4 transition-all duration-200 cursor-pointer focus:outline-none
                    ${selectedId === g.groupId
                      ? "bg-accent/10 border border-accent/30 text-white font-semibold shadow-inner"
                      : "hover:bg-surface-hover text-zinc-300 hover:text-white border border-transparent"}
                  `}
                >
                  <span className="truncate flex-1">{g.displayName}</span>
                  <span className="text-zinc-500 font-bold shrink-0">{g.episodeCount} ep</span>
                </button>
              </li>
            ))}
          </ul>
        )}

        {error && (
          <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-red-200 text-xs mt-3 select-none">
            <AlertCircle className="w-4 h-4 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        <div className="flex gap-2.5 mt-6 justify-end">
          <Button onClick={onClose} disabled={loading} variant="secondary">
            Cancel
          </Button>
          <Button
            onClick={handleConfirm}
            disabled={loading || !selectedId}
            variant="primary"
            className="flex items-center gap-1.5"
          >
            <span>{loading ? "Merging…" : "Merge"}</span>
          </Button>
        </div>
      </motion.div>
    </div>
  );
}
