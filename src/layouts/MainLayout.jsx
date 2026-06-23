import Sidebar from "./Sidebar";

function MainLayout({ children, onSync, isSyncing }) {
  return (
    <div className="flex w-screen h-screen overflow-hidden bg-bg text-text-primary select-none">
      <Sidebar onSync={onSync} isSyncing={isSyncing} />
      <main className="flex-1 overflow-y-auto p-8 bg-gradient-to-b from-bg to-surface-card/20 scroll-smooth">
        <div className="max-w-7xl mx-auto space-y-6">
          {children}
        </div>
      </main>
    </div>
  );
}

export default MainLayout;