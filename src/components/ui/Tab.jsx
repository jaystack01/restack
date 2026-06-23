import { cn } from "../../utils/cn";

export default function Tab({ active, className = "", ...props }) {
  return (
    <button
      className={cn(
        "relative min-w-[140px] px-4 py-2.5 rounded-lg text-sm font-semibold transition-all duration-200 focus:outline-none focus:ring-2 focus:ring-accent/30 cursor-pointer text-center",
        active
          ? "bg-accent/10 border border-accent/30 text-white shadow-sm font-bold scale-[1.01]"
          : "bg-surface-card hover:bg-surface-hover text-zinc-400 hover:text-zinc-100 border border-border hover:border-border-hover shadow-sm",
        className
      )}
      {...props}
    />
  );
}
