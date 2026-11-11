import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { AlertCircle, Info, X } from "lucide-react";
import { cn } from "../../utils/cn";
import { dismissToast, getToasts, subscribe } from "../../store/toastStore";

const VARIANTS = {
  info: {
    icon: Info,
    className: "border-border-card",
    iconClassName: "text-accent",
  },
  error: {
    icon: AlertCircle,
    className: "border-red-500/30",
    iconClassName: "text-red-400",
  },
};

/** App-wide toast stack. Render once (MainLayout); feed it via toastStore. */
function Toaster() {
  const [toasts, setToasts] = useState(getToasts);

  useEffect(() => subscribe(setToasts), []);

  return (
    <div
      aria-live="polite"
      className="fixed bottom-4 right-4 z-50 flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2 pointer-events-none"
    >
      <AnimatePresence initial={false}>
        {toasts.map((toast) => {
          const variant = VARIANTS[toast.variant] ?? VARIANTS.info;
          const Icon = variant.icon;
          return (
            <motion.div
              key={toast.id}
              layout
              initial={{ opacity: 0, y: 12, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, x: 24 }}
              transition={{ duration: 0.18 }}
              role={toast.variant === "error" ? "alert" : "status"}
              className={cn(
                "pointer-events-auto flex items-start gap-3 rounded-xl border bg-surface-card p-3 shadow-premium",
                variant.className,
              )}
            >
              <Icon
                className={cn("mt-0.5 h-4 w-4 shrink-0", variant.iconClassName)}
                aria-hidden="true"
              />
              <p className="flex-1 text-sm text-text-primary break-words select-text">
                {toast.message}
              </p>
              <button
                type="button"
                onClick={() => dismissToast(toast.id)}
                className="shrink-0 rounded-md p-0.5 text-text-muted hover:bg-surface-hover hover:text-text-primary focus:outline-none focus:ring-2 focus:ring-accent/40"
                aria-label="Dismiss notification"
              >
                <X className="h-4 w-4" aria-hidden="true" />
              </button>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
}

export default Toaster;
