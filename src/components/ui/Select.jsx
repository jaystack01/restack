import { cn } from "../../utils/cn";

export default function Select({ className = "", children, ...props }) {
  return (
    <div className="relative w-auto max-w-[200px]">
      <select
        className={cn(
          "w-full appearance-none bg-surface-card hover:bg-surface-hover border border-border hover:border-border-hover rounded-lg px-3.5 pr-10 py-2 text-sm text-white transition-all duration-200 ease-out focus:outline-none focus:ring-2 focus:ring-accent/30 focus:border-accent shadow-sm cursor-pointer",
          className
        )}
        {...props}
      >
        {children}
      </select>

      {/* SVG arrow indicator */}
      <div className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-zinc-400">
        <svg
          className="w-4 h-4"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          viewBox="0 0 24 24"
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </div>
    </div>
  );
}
