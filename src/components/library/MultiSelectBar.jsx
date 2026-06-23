import { memo } from "react";
import { Check, EyeOff, Trash2, X, MoveHorizontal, FolderPlus } from "lucide-react";
import Button from "../ui/Button";
import Select from "../ui/Select";

function MultiSelectBar({
  selectedCount,
  allInViewSelected,
  groups,
  selectedGroupId,
  onToggleSelectAll,
  onMove,
  onMarkWatched,
  onMarkUnwatched,
  onDelete,
  onClear,
}) {
  if (selectedCount < 1) return null;

  return (
    <div
      className="
        sticky top-6 z-30 mb-6
        w-full
        rounded-xl border border-accent/20
        bg-surface/85 backdrop-blur-md shadow-premium shadow-accent/5
        px-5 py-3
        flex flex-wrap items-center justify-between gap-4
        animate-in
      "
    >
      {/* LEFT: selection count and toggle */}
      <div className="flex items-center gap-3 shrink-0">
        <div className="flex items-center justify-center h-6 px-2.5 rounded bg-accent text-white text-xs font-extrabold select-none tracking-wider">
          {selectedCount} Selected
        </div>

        <Button type="button" onClick={onToggleSelectAll} size="sm" variant="secondary">
          {allInViewSelected ? "Unselect All" : "Select All"}
        </Button>
      </div>

      {/* MIDDLE: primary bulk operations */}
      <div className="flex items-center flex-wrap gap-2.5 flex-1 min-w-0 justify-start">
        <div className="relative flex items-center gap-2">
          <Select
            defaultValue=""
            onChange={(e) => {
              const targetGroupId = Number(e.target.value);
              if (targetGroupId) onMove(targetGroupId);
              e.target.value = "";
            }}
            className="text-xs min-w-[140px]"
          >
            <option value="" disabled>
              Relocate episodes…
            </option>
            {groups
              .filter((g) => g.groupId !== selectedGroupId)
              .map((group) => (
                <option value={group.groupId} key={group.groupId}>
                  {group.displayName}
                </option>
              ))}
          </Select>
        </div>

        <Button
          type="button"
          variant="primary"
          onClick={onMarkWatched}
          size="sm"
          className="flex items-center gap-1.5"
        >
          <Check className="w-3.5 h-3.5 stroke-[3]" />
          <span>Watched</span>
        </Button>

        <Button
          type="button"
          onClick={onMarkUnwatched}
          size="sm"
          className="flex items-center gap-1.5"
        >
          <EyeOff className="w-3.5 h-3.5" />
          <span>Unwatched</span>
        </Button>
      </div>

      {/* RIGHT: destructive actions and clear */}
      <div className="flex items-center gap-2.5 shrink-0">
        <Button
          type="button"
          variant="danger"
          onClick={onDelete}
          size="sm"
          className="p-2"
          title="Move selected to Trash"
        >
          <Trash2 className="w-3.5 h-3.5" />
        </Button>

        <Button
          type="button"
          onClick={onClear}
          size="sm"
          className="p-2 text-zinc-400 hover:text-zinc-100"
          variant="ghost"
          title="Clear selections"
        >
          <X className="w-3.5 h-3.5" />
        </Button>
      </div>
    </div>
  );
}

export default memo(MultiSelectBar);
