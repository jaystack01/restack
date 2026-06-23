import { cn } from "../../utils/cn";

export default function Input({ className = "", ...props }) {
  return (
    <input
      className={cn(
        "w-full rounded-lg bg-surface-card border border-border px-3.5 py-2 text-sm text-white placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-accent/30 focus:border-accent hover:border-border-hover transition-all duration-200 shadow-sm",
        className
      )}
      {...props}
    />
  );
}
