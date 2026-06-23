import { useState, useRef, useEffect } from "react";

/**
 * "⋯" button that opens an inline dropdown with Rename + Merge actions.
 * onRename() and onMerge() are called when the user picks an option.
 */
export default function GroupActionsMenu({ onRename, onMerge }) {
  const [open, setOpen] = useState(false);
  const ref = useRef(null);

  // Close on outside click
  useEffect(() => {
    function handler(e) {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen((v) => !v)}
        className="
          flex items-center justify-center w-8 h-8 rounded-full
          text-text-muted hover:text-white hover:bg-white/10
          transition-all duration-150 focus:outline-none
        "
        title="Group actions"
        aria-label="Group actions"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-5 h-5">
          <path fillRule="evenodd" d="M4.5 12a1.5 1.5 0 113 0 1.5 1.5 0 01-3 0zm6 0a1.5 1.5 0 113 0 1.5 1.5 0 01-3 0zm6 0a1.5 1.5 0 113 0 1.5 1.5 0 01-3 0z" clipRule="evenodd" />
        </svg>
      </button>

      {open && (
        <div className="
          absolute left-0 top-9 z-50 min-w-[160px]
          bg-surface border border-white/10 rounded-lg shadow-2xl
          py-1 overflow-hidden animate-in fade-in slide-in-from-top-2 duration-150
        ">
          <button
            onClick={() => { setOpen(false); onRename(); }}
            className="w-full text-left px-4 py-2.5 text-sm text-text-primary hover:bg-white/5 flex items-center gap-3 transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} className="w-4 h-4 text-accent">
              <path strokeLinecap="round" strokeLinejoin="round" d="M16.862 4.487a2.032 2.032 0 112.851 2.851L6.5 20.551H3v-3.5L16.862 4.487z" />
            </svg>
            Rename
          </button>
          <button
            onClick={() => { setOpen(false); onMerge(); }}
            className="w-full text-left px-4 py-2.5 text-sm text-text-primary hover:bg-white/5 flex items-center gap-3 transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} className="w-4 h-4 text-accent">
              <path strokeLinecap="round" strokeLinejoin="round" d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
            </svg>
            Merge into…
          </button>
        </div>
      )}
    </div>
  );
}
