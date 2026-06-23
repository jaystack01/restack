import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Modal for renaming a group. Pre-fills the current display name.
 * onClose()    — cancel, no changes
 * onSuccess()  — rename worked, parent should refresh library
 */
export default function RenameGroupModal({ groupId, currentName, onClose, onSuccess }) {
  const [value, setValue] = useState(currentName);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const inputRef = useRef(null);

  useEffect(() => {
    inputRef.current?.select();
  }, []);

  async function handleConfirm() {
    const trimmed = value.trim();
    if (!trimmed) return;
    if (trimmed === currentName) { onClose(); return; }

    setLoading(true);
    setError("");
    try {
      await invoke("rename_group_cmd", { groupId, newName: trimmed });
      onSuccess();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to rename group");
    } finally {
      setLoading(false);
    }
  }

  function handleKeyDown(e) {
    if (e.key === "Enter") handleConfirm();
    if (e.key === "Escape") onClose();
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-in fade-in duration-200">
      <div
        className="bg-surface border border-white/10 rounded-2xl shadow-2xl w-full max-w-md mx-4 p-6"
        role="dialog"
        aria-modal="true"
        aria-labelledby="rename-title"
      >
        <h2 id="rename-title" className="text-xl font-bold text-white mb-1">Rename Group</h2>
        <p className="text-sm text-text-muted mb-5">
          Only the display name changes. Episode assignments stay intact.
        </p>

        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="New group name"
          className="
            w-full bg-surface-2 border border-white/10 rounded-lg px-4 py-2.5
            text-white text-sm placeholder:text-text-muted
            focus:outline-none focus:ring-2 focus:ring-accent/60
            transition-all duration-150
          "
        />

        {error && (
          <p className="text-red-400 text-xs mt-2">{error}</p>
        )}

        <div className="flex gap-3 mt-6 justify-end">
          <button
            onClick={onClose}
            disabled={loading}
            className="px-5 py-2 rounded-lg text-sm text-text-muted hover:text-white hover:bg-white/5 transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleConfirm}
            disabled={loading || !value.trim()}
            className="
              px-5 py-2 rounded-lg text-sm font-semibold
              bg-accent text-black hover:bg-accent/80
              disabled:opacity-40 disabled:cursor-not-allowed
              transition-all duration-150
            "
          >
            {loading ? "Saving…" : "Rename"}
          </button>
        </div>
      </div>
    </div>
  );
}
