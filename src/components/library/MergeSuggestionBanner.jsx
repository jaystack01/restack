import { useState } from "react";
import { AlertTriangle, GitMerge, X } from "lucide-react";
import MergeGroupModal from "./MergeGroupModal";
import Button from "../ui/Button";

export default function MergeSuggestionBanner({ suggestions, onMerged }) {
  // Track dismissed suggestion pairs
  const [dismissed, setDismissed] = useState(new Set());
  const [mergeTarget, setMergeTarget] = useState(null);

  const visible = suggestions.filter(
    (s) => !dismissed.has(`${s.groupIdA}-${s.groupIdB}`)
  );

  if (visible.length === 0) return null;

  function dismiss(s) {
    setDismissed((prev) => new Set([...prev, `${s.groupIdA}-${s.groupIdB}`]));
  }

  return (
    <>
      <div className="mb-6 rounded-xl border border-amber-500/20 bg-amber-500/5 p-4 space-y-3 shadow-md animate-in select-none">
        <div className="flex items-center gap-2 text-amber-400">
          <AlertTriangle className="w-4 h-4 shrink-0" />
          <span className="text-xs font-bold uppercase tracking-wider">Possible duplicate series groups</span>
        </div>

        <div className="flex flex-col gap-2.5">
          {visible.map((s) => (
            <div
              key={`${s.groupIdA}-${s.groupIdB}`}
              className="flex flex-col sm:flex-row sm:items-center sm:justify-between py-2.5 border-t border-amber-500/10 gap-3"
            >
              <div className="flex items-center gap-2 flex-wrap min-w-0 flex-1 text-sm text-zinc-300">
                <span className="font-semibold text-white bg-zinc-900 border border-border px-2.5 py-0.5 rounded-full text-xs">
                  {s.displayNameA}
                  <span className="text-zinc-500 ml-1.5">({s.episodeCountA} ep)</span>
                </span>
                <span className="text-zinc-500 text-xs">and</span>
                <span className="font-semibold text-white bg-zinc-900 border border-border px-2.5 py-0.5 rounded-full text-xs">
                  {s.displayNameB}
                  <span className="text-zinc-500 ml-1.5">({s.episodeCountB} ep)</span>
                </span>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <Button
                  onClick={() =>
                    setMergeTarget({ sourceGroupId: s.groupIdA, sourceDisplayName: s.displayNameA })
                  }
                  size="sm"
                  variant="primary"
                  className="bg-amber-500 hover:bg-amber-600 text-white border-amber-600/35 hover:scale-[1.01] flex items-center gap-1.5"
                >
                  <GitMerge className="w-3.5 h-3.5" />
                  <span>Merge</span>
                </Button>
                <Button
                  onClick={() => dismiss(s)}
                  size="sm"
                  variant="ghost"
                  className="text-zinc-400 hover:text-zinc-200"
                >
                  Dismiss
                </Button>
              </div>
            </div>
          ))}
        </div>
      </div>

      {mergeTarget && (
        <MergeGroupModal
          sourceGroupId={mergeTarget.sourceGroupId}
          sourceDisplayName={mergeTarget.sourceDisplayName}
          onClose={() => setMergeTarget(null)}
          onSuccess={() => {
            setMergeTarget(null);
            onMerged();
          }}
        />
      )}
    </>
  );
}
