import { cn } from "../../utils/cn";

export default function Button({
  variant = "secondary",
  size = "md",
  className = "",
  type = "button",
  ...props
}) {
  const base = cn(
    "inline-flex items-center justify-center gap-2 rounded-lg text-sm font-semibold transition-all duration-200 ease-out focus:outline-none focus:ring-2 disabled:opacity-50 disabled:pointer-events-none active:scale-[0.98]"
  );

  const sizes = {
    sm: "px-3 py-1.5 text-xs rounded-md",
    md: "px-4 py-2 text-sm",
    lg: "px-5 py-2.5 text-base rounded-xl",
  };

  const variants = {
    primary: "bg-accent text-white hover:bg-accent-hover focus:ring-accent/30 shadow-md shadow-accent/15 border border-accent/20",
    secondary: "bg-surface-card text-zinc-100 hover:bg-surface-hover border border-border hover:border-border-hover focus:ring-zinc-700/40 shadow-sm",
    danger: "bg-red-500/10 text-red-300 border border-red-500/20 hover:bg-red-500/20 hover:border-red-500/30 focus:ring-red-500/30 shadow-sm",
    ghost: "bg-transparent text-zinc-400 hover:text-zinc-100 hover:bg-surface-hover focus:ring-zinc-700/20",
  };

  return (
    <button
      type={type}
      className={cn(base, sizes[size], variants[variant], className)}
      {...props}
    />
  );
}
