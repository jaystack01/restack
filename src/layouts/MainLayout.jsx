import { motion, AnimatePresence } from "framer-motion";
import Sidebar from "./Sidebar";
import FirstScanOverlay from "../components/ui/FirstScanOverlay";

/**
 * Thin progress strip at the very top of the window.
 * Shows during sync when the library is already loaded (returning users).
 * The FirstScanOverlay handles the case where library hasn't loaded yet.
 */
function TopSyncBar({ isSyncing, scanState }) {
  const { current, total } = scanState;
  const hasTotal = total > 0;
  const pct = hasTotal ? Math.round((current / total) * 100) : 0;

  return (
    <AnimatePresence>
      {isSyncing && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="fixed top-0 left-0 right-0 z-40 h-0.5 bg-surface overflow-hidden"
        >
          {hasTotal ? (
            <motion.div
              className="h-full bg-accent"
              initial={{ width: "0%" }}
              animate={{ width: `${pct}%` }}
              transition={{ ease: "easeOut", duration: 0.2 }}
            />
          ) : (
            /* Indeterminate shimmer during directory walk */
            <motion.div
              className="h-full w-1/4 bg-accent"
              animate={{ x: ["-100%", "500%"] }}
              transition={{ duration: 1.4, repeat: Infinity, ease: "easeInOut" }}
            />
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function MainLayout({ children, onSync, isSyncing, scanState, hasLibraryLoaded }) {
  return (
    <div className="flex w-screen h-screen overflow-hidden bg-bg text-text-primary select-none">
      {/* App-wide sync progress strip (for returning user syncs) */}
      {hasLibraryLoaded && (
        <TopSyncBar isSyncing={isSyncing} scanState={scanState} />
      )}

      <Sidebar onSync={onSync} isSyncing={isSyncing} scanState={scanState} />

      <main className="flex-1 overflow-y-auto p-8 bg-gradient-to-b from-bg to-surface-card/20 scroll-smooth">
        <div className="max-w-7xl mx-auto space-y-6">
          {children}
        </div>
      </main>

      {/* Full-screen overlay — shown while library hasn't loaded yet (covers first scan + startup) */}
      <FirstScanOverlay
        scanState={scanState}
        isSyncing={isSyncing}
        visible={!hasLibraryLoaded}
      />
    </div>
  );
}

export default MainLayout;