import { useState } from "react";
import { Edit2, AlertCircle } from "lucide-react";
import { renameGroup } from "../../api";
import Modal from "../ui/Modal";
import Button from "../ui/Button";
import Input from "../ui/Input";

/**
 * Modal for renaming a series. Pre-fills the current display name and is
 * rendered only while open.
 * onClose()    — cancel, no changes
 * onSuccess()  — rename worked, parent should refresh the library
 */
export default function RenameGroupModal({
  groupId,
  currentName,
  onClose,
  onSuccess,
}) {
  const [value, setValue] = useState(currentName);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function handleSubmit(e) {
    e.preventDefault();
    const trimmed = value.trim();
    if (!trimmed || loading) return;
    if (trimmed === currentName) {
      onClose();
      return;
    }

    setLoading(true);
    setError("");
    try {
      await renameGroup(groupId, trimmed);
      onSuccess();
    } catch (err) {
      setError(typeof err === "string" ? err : "Failed to rename series.");
      setLoading(false);
    }
  }

  return (
    <Modal
      isOpen
      onClose={loading ? undefined : onClose}
      className="max-w-md"
      labelledBy="rename-title"
    >
      <form onSubmit={handleSubmit}>
        <div className="flex items-center gap-3 text-accent mb-2 select-none">
          <Edit2 className="w-5 h-5 stroke-[2]" />
          <h2
            id="rename-title"
            className="text-lg font-bold text-text-primary leading-none"
          >
            Rename Series
          </h2>
        </div>
        <p className="text-xs text-text-secondary leading-relaxed mb-5 select-none">
          Only the display name changes. Episodes stay in this series, and
          rescans keep the new name.
        </p>

        <Input
          data-autofocus
          type="text"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onFocus={(e) => e.target.select()}
          placeholder="Series name"
          aria-label="Series name"
        />

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
            type="submit"
            disabled={loading || !value.trim()}
            variant="primary"
          >
            {loading ? "Saving…" : "Rename"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
