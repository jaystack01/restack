import { motion, AnimatePresence } from "framer-motion";
import { Film } from "lucide-react";

/**
 * Full-screen overlay shown whenever the library hasn't loaded yet.
 * This covers three states:
 *   1. App just opened, scan listeners registering (~30-50ms) — isSyncing=false, total=0
 *   2. Scan running, discovering files — isSyncing=true, total=0
 *   3. Scan running, indexing files — isSyncing=true, total>0
 * Dismissed automatically when the scan completes (hasLibraryLoaded → true).
 */
export default function FirstScanOverlay({ scanState, isSyncing, visible }) {
  const { current, total, currentFile } = scanState;
  const hasTotal = total > 0;
  const pct = hasTotal ? Math.round((current / total) * 100) : 0;

  // While we're waiting for listeners to register (isSyncing not yet true),
  // show a "preparing" message instead of a stuck bar.
  const isPreparing = !isSyncing;

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: { duration: 0.4 } }}
          transition={{ duration: 0.2 }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-bg"
        >
          <motion.div
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.05, duration: 0.35 }}
            className="flex flex-col items-center gap-6 max-w-sm w-full px-8"
          >
            {/* Icon */}
            <div className="p-5 rounded-2xl bg-accent/10 border border-accent/20">
              <Film className="w-10 h-10 text-accent" />
            </div>

            {/* Title + subtitle */}
            <div className="text-center">
              <h2 className="text-2xl font-display font-extrabold text-white tracking-tight">
                {isPreparing ? "Starting up…" : hasTotal ? "Indexing your library" : "Scanning your library"}
              </h2>
              <p className="text-sm text-zinc-400 mt-1.5">
                {isPreparing
                  ? "Preparing the scanner…"
                  : hasTotal
                  ? `Processing ${total} files`
                  : "Discovering media files…"}
              </p>
            </div>

            {/* Progress bar */}
            <div className="w-full space-y-2.5">
              <div className="w-full h-1.5 rounded-full bg-surface-card overflow-hidden">
                {hasTotal ? (
                  /* Determinate — fills as files are processed */
                  <motion.div
                    className="h-full rounded-full bg-accent"
                    initial={{ width: "0%" }}
                    animate={{ width: `${pct}%` }}
                    transition={{ ease: "easeOut", duration: 0.2 }}
                  />
                ) : (
                  /* Indeterminate shimmer — plays during startup + discovery */
                  <motion.div
                    className="h-full w-1/3 rounded-full bg-accent/70"
                    animate={{ x: ["0%", "200%", "0%"] }}
                    transition={{
                      duration: 1.6,
                      repeat: Infinity,
                      ease: "easeInOut",
                    }}
                  />
                )}
              </div>

              {/* Counter + filename row — only when we have real data */}
              {hasTotal ? (
                <div className="flex items-center justify-between text-xs text-zinc-500">
                  <span className="truncate max-w-[200px] leading-tight">
                    {currentFile || "Processing…"}
                  </span>
                  <span className="shrink-0 ml-2 tabular-nums font-medium">
                    {current} / {total}
                  </span>
                </div>
              ) : (
                <div className="h-4" /> /* spacer to keep layout stable */
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
