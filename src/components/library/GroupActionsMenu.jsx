import { useState, useRef, useEffect } from "react";
import { MoreHorizontal, Edit2, GitMerge } from "lucide-react";
import { cn } from "../../utils/cn";

/**
 * "⋯" button that opens a small menu with Rename + Merge actions.
 * onRename() and onMerge() are called when the user picks an option.
 * Keys pressed inside the menu stay here, so the page shortcuts (Enter to
 * play, arrows to move, Escape to clear the selection) don't fire.
 */
export default function GroupActionsMenu({ onRename, onMerge }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  const triggerRef = useRef(null);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    function handler(e) {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  // Move focus to the first item when the menu opens
  useEffect(() => {
    if (open) ref.current?.querySelector("[role=menuitem]")?.focus();
  }, [open]);

  function items() {
    return Array.from(ref.current?.querySelectorAll("[role=menuitem]") ?? []);
  }

  function onKeyDown(e) {
    e.stopPropagation();
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      triggerRef.current?.focus();
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const nodes = items();
      if (!nodes.length) return;
      const i = nodes.indexOf(document.activeElement);
      const step = e.key === "ArrowDown" ? 1 : -1;
      nodes[(i + step + nodes.length) % nodes.length].focus();
    }
  }

  function pick(action) {
    setOpen(false);
    // The dialog that opens next returns focus to the trigger when it closes
    triggerRef.current?.focus();
    action();
  }

  const itemClass =
    "w-full text-left px-4 py-2.5 text-sm text-text-primary hover:bg-surface-hover focus:bg-surface-hover focus:outline-none flex items-center gap-3 transition-colors cursor-pointer";

  return (
    <div className="relative" ref={ref} onKeyDown={onKeyDown}>
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex items-center justify-center w-9 h-9 rounded-full text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors duration-150 focus:outline-none focus:ring-2 focus:ring-accent/30",
          open && "bg-surface-hover text-text-primary",
        )}
        title="Series actions"
        aria-label="Series actions"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <MoreHorizontal className="w-5 h-5" />
      </button>

      {open && (
        <div
          role="menu"
          aria-label="Series actions"
          className="absolute left-0 top-10 z-40 min-w-[200px] bg-surface border border-border rounded-lg shadow-premium py-1 overflow-hidden"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => pick(onRename)}
            className={itemClass}
          >
            <Edit2 className="w-4 h-4 text-accent" />
            Rename…
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => pick(onMerge)}
            className={itemClass}
          >
            <GitMerge className="w-4 h-4 text-accent" />
            Merge into another series…
          </button>
        </div>
      )}
    </div>
  );
}
