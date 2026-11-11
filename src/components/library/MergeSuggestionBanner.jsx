import { useState } from "react";
import { GitMerge, Info } from "lucide-react";
import MergeGroupModal from "./MergeGroupModal";
import Button from "../ui/Button";

const pairKey = (s) => `${s.groupIdA}-${s.groupIdB}`;

/**
 * Lists near-duplicate series names (from getMergeSuggestions). Merge opens
 * the merge dialog with the smaller group as the source and the larger one
 * preselected as the target. Dismissals last until the page unmounts.
 * onMerged() — a merge worked, parent should refresh the library
 */
export default function MergeSuggestionBanner({ suggestions, onMerged }) {
  const [dismissed, setDismissed] = useState(() => new Set());
  const [mergeTarget, setMergeTarget] = useState(null);

  const visible = suggestions.filter((s) => !dismissed.has(pairKey(s)));

  function dismiss(s) {
    setDismissed((prev) => new Set([...prev, pairKey(s)]));
  }

  function openMerge(s) {
    // Fold the smaller group into the larger one by default
    const aIntoB = s.episodeCountA <= s.episodeCountB;
    setMergeTarget({
      sourceGroupId: aIntoB ? s.groupIdA : s.groupIdB,
      sourceDisplayName: aIntoB ? s.displayNameA : s.displayNameB,
      suggestedTargetId: aIntoB ? s.groupIdB : s.groupIdA,
    });
  }

  return (
    <>
      {visible.length > 0 && (
        <div
          className="rounded-xl border border-accent/20 bg-accent/5 p-4 space-y-3 shadow-premium select-none"
          role="region"
          aria-label="Possible duplicate series"
        >
          <div className="flex items-center gap-2 text-accent">
            <Info className="w-4 h-4 shrink-0" />
            <span className="text-xs font-bold uppercase tracking-wider">
              Possible duplicate series
            </span>
          </div>

          <ul className="flex flex-col">
            {visible.map((s) => (
              <li
                key={pairKey(s)}
                className="flex flex-col sm:flex-row sm:items-center sm:justify-between py-2.5 border-t border-accent/10 gap-3"
              >
                <div className="flex items-center gap-2 flex-wrap min-w-0 flex-1 text-sm">
                  <span className="font-semibold text-text-primary bg-surface-card border border-border px-2.5 py-0.5 rounded-full text-xs">
                    {s.displayNameA}
                    <span className="text-text-muted ml-1.5">
                      ({s.episodeCountA} ep)
                    </span>
                  </span>
                  <span className="text-text-muted text-xs">and</span>
                  <span className="font-semibold text-text-primary bg-surface-card border border-border px-2.5 py-0.5 rounded-full text-xs">
                    {s.displayNameB}
                    <span className="text-text-muted ml-1.5">
                      ({s.episodeCountB} ep)
                    </span>
                  </span>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <Button
                    onClick={() => openMerge(s)}
                    size="sm"
                    variant="primary"
                  >
                    <GitMerge className="w-3.5 h-3.5" />
                    <span>Merge…</span>
                  </Button>
                  <Button onClick={() => dismiss(s)} size="sm" variant="ghost">
                    Dismiss
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}

      {mergeTarget && (
        <MergeGroupModal
          sourceGroupId={mergeTarget.sourceGroupId}
          sourceDisplayName={mergeTarget.sourceDisplayName}
          suggestedTargetId={mergeTarget.suggestedTargetId}
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
