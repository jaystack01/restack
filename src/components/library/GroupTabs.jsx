import { memo, useRef, useEffect } from "react";
import Tab from "../ui/Tab";

function GroupTabs({
  groups,
  selectedGroupId,
  onSelectGroup,
  libraryRefreshing = false,
}) {
  const containerRef = useRef(null);

  // Auto-scroll active tab into view
  useEffect(() => {
    const activeEl = containerRef.current?.querySelector(
      "[data-active='true']",
    );
    activeEl?.scrollIntoView({
      behavior: "smooth",
      inline: "center",
      block: "nearest",
    });
  }, [selectedGroupId]);

  return (
    <section className="mb-6">
      <div className="relative">
        {/* fade edges for scroll hint */}
        {/* <div className="pointer-events-none absolute left-0 top-0 h-full w-8 bg-gradient-to-r from-[#121212] to-transparent z-10" />
        <div className="pointer-events-none absolute right-0 top-0 h-full w-8 bg-gradient-to-l from-[#121212] to-transparent z-10" /> */}

        <div
          ref={containerRef}
          className="
            flex gap-2 overflow-x-auto py-2 px-1
            scroll-smooth
            scrollbar-none
          "
        >
          {groups.map((group) => {
            const active = group.groupId === selectedGroupId;

            return (
              <Tab
                key={group.groupId}
                type="button"
                onClick={() => onSelectGroup(group.groupId)}
                active={active}
                data-active={active}
                className="shrink-0 min-w-[160px]"
              >
                <p className="text-sm font-semibold truncate">
                  {group.displayName}
                </p>

                <p
                  className={`
                    text-xs mt-1
                    ${active ? "text-[#E0B0FF]" : "text-white/50"}
                  `}
                >
                  {group.episodeCount} Episodes
                </p>
              </Tab>
            );
          })}
        </div>
      </div>

      {libraryRefreshing && (
        <div className="mt-2 text-xs text-white/50 flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-[#E0B0FF] animate-pulse" />
          Refreshing library…
        </div>
      )}
    </section>
  );
}

export default memo(GroupTabs);
