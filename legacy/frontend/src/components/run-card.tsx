"use client";

import type { Run } from "@/lib/types";

const STATE_COLORS: Record<string, string> = {
  queued: "bg-gray-500",
  planning: "bg-blue-500",
  contracting: "bg-indigo-500",
  building: "bg-yellow-500",
  qa: "bg-purple-500",
  repair: "bg-orange-500",
  checkpointing: "bg-cyan-500",
  awaiting_approval: "bg-amber-500 animate-pulse",
  paused: "bg-gray-400",
  interrupted: "bg-red-400",
  completed: "bg-green-500",
  failed: "bg-red-500",
  cancelled: "bg-gray-500",
};

export function RunCard({ run, onClick }: { run: Run; onClick: () => void }) {
  const progress = run.planned_sprints
    ? Math.round((run.current_sprint / run.planned_sprints) * 100)
    : 0;

  return (
    <div
      onClick={onClick}
      className="p-4 border rounded-xl cursor-pointer hover:border-primary/50 transition-colors space-y-3"
    >
      {/* Header */}
      <div className="flex items-start justify-between">
        <p className="text-sm font-medium line-clamp-2 flex-1">{run.prompt}</p>
        <span
          className={`ml-2 px-2 py-0.5 text-xs font-medium text-white rounded-full whitespace-nowrap ${STATE_COLORS[run.state] || "bg-gray-500"}`}
        >
          {run.state.replace(/_/g, " ")}
        </span>
      </div>

      {/* Sprint Progress */}
      <div>
        <div className="flex justify-between text-xs text-muted-foreground mb-1">
          <span>Sprint {run.current_sprint}/{run.planned_sprints || "?"}</span>
          <span>{progress}%</span>
        </div>
        <div className="h-2 bg-secondary rounded-full overflow-hidden">
          <div
            className="h-full bg-primary rounded-full transition-all duration-500"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>

      {/* Roles */}
      {run.roles && run.roles.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {run.roles.map((r) => (
            <span key={r.role_name} className="px-1.5 py-0.5 text-[10px] bg-accent rounded">
              {r.role_name}: {r.model_id}
            </span>
          ))}
        </div>
      )}

      {/* Budget */}
      {run.budget && (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span>⚡ {Math.round(run.budget.tokens_used / 1000)}K tokens</span>
          <span>·</span>
          <span>⏱ {Math.round(run.budget.wall_clock_seconds / 60)}m</span>
        </div>
      )}
    </div>
  );
}
