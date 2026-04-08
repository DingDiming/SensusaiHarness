"use client";

import type { ProgressSnapshot } from "@/lib/types";

const PHASE_ICONS: Record<string, string> = {
  planning: "📋",
  contracting: "📝",
  building: "🔨",
  qa: "🔍",
  repair: "🔧",
  checkpointing: "💾",
};

const PHASE_COLORS: Record<string, string> = {
  success: "border-green-500 bg-green-500/10",
  fail: "border-red-500 bg-red-500/10",
  active: "border-blue-500 bg-blue-500/10 animate-pulse",
  pending: "border-border bg-muted/30",
};

export function SprintTimeline({
  progress,
  currentSprint,
  plannedSprints,
}: {
  progress: ProgressSnapshot[];
  currentSprint: number;
  plannedSprints: number | null;
}) {
  const totalSprints = plannedSprints || 6;

  // Group progress by sprint
  const sprintGroups: Record<number, ProgressSnapshot[]> = {};
  for (const p of progress) {
    if (!sprintGroups[p.sprint]) sprintGroups[p.sprint] = [];
    sprintGroups[p.sprint].push(p);
  }

  return (
    <div className="p-4 border rounded-xl">
      <h3 className="text-sm font-semibold mb-4">Sprint Timeline</h3>

      <div className="space-y-3">
        {Array.from({ length: totalSprints }, (_, i) => i + 1).map((sprint) => {
          const phases = sprintGroups[sprint] || [];
          const isActive = sprint === currentSprint;
          const isDone = phases.some((p) => p.phase === "checkpointing" && p.outcome === "success");
          const isFuture = sprint > currentSprint;

          return (
            <div key={sprint} className="flex items-start gap-3">
              {/* Sprint number */}
              <div
                className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold shrink-0 ${
                  isDone
                    ? "bg-green-500 text-white"
                    : isActive
                    ? "bg-blue-500 text-white animate-pulse"
                    : "bg-muted text-muted-foreground"
                }`}
              >
                {sprint}
              </div>

              {/* Phase chips */}
              <div className="flex-1">
                {isFuture ? (
                  <div className="text-sm text-muted-foreground py-1">Pending</div>
                ) : (
                  <div className="flex flex-wrap gap-1.5">
                    {["planning", "contracting", "building", "qa", "repair", "checkpointing"].map((phase) => {
                      const snap = phases.find((p) => p.phase === phase);
                      if (!snap && !isActive) return null;

                      const isPhaseActive = isActive && !snap?.completed_at && snap;
                      const colorClass = snap?.completed_at
                        ? snap.outcome === "success" || snap.outcome === "pass"
                          ? PHASE_COLORS.success
                          : snap.outcome === "fail"
                          ? PHASE_COLORS.fail
                          : PHASE_COLORS.success
                        : isPhaseActive
                        ? PHASE_COLORS.active
                        : PHASE_COLORS.pending;

                      if (!snap) return null;

                      return (
                        <span
                          key={phase}
                          className={`inline-flex items-center gap-1 px-2 py-0.5 text-xs border rounded-md ${colorClass}`}
                        >
                          <span>{PHASE_ICONS[phase] || "•"}</span>
                          <span className="capitalize">{phase}</span>
                          {snap?.duration_seconds != null && (
                            <span className="text-muted-foreground">
                              {snap.duration_seconds < 60
                                ? `${snap.duration_seconds}s`
                                : `${Math.round(snap.duration_seconds / 60)}m`}
                            </span>
                          )}
                        </span>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
