import { useEffect, useRef } from "react";
import { cn } from "../../utils/cn";

export default function Checkbox({
  checked,
  indeterminate = false,
  onChange,
  className = "",
}) {
  const ref = useRef(null);

  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);

  return (
    <span className={cn("relative inline-flex items-center justify-center select-none", className)}>
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        onChange={onChange}
        className="peer appearance-none h-5 w-5 rounded bg-surface border border-border checked:bg-accent checked:border-accent focus:outline-none focus:ring-2 focus:ring-accent/30 hover:border-border-hover transition-colors duration-200 cursor-pointer"
      />

      {/* Checkmark SVG */}
      <svg
        className="pointer-events-none absolute w-3 h-3 text-white opacity-0 peer-checked:opacity-100 transition-opacity duration-150"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="3.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <polyline points="20 6 9 17 4 12" />
      </svg>

      {/* Indeterminate State */}
      {indeterminate && !checked && (
        <div className="pointer-events-none absolute w-2.5 h-[2px] bg-accent rounded" />
      )}
    </span>
  );
}
