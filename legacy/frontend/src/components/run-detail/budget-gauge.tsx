"use client";

import type { RunBudget } from "@/lib/types";

export function BudgetGauge({ budget }: { budget: RunBudget | null }) {
  if (!budget) return null;

  const tokenPercent = budget.tokens_limit
    ? Math.min(100, Math.round((budget.tokens_used / budget.tokens_limit) * 100))
    : 0;

  const timePercent = budget.wall_clock_limit
    ? Math.min(100, Math.round((budget.wall_clock_seconds / budget.wall_clock_limit) * 100))
    : 0;

  const getColor = (pct: number) =>
    pct > 80 ? "bg-red-500" : pct > 50 ? "bg-yellow-500" : "bg-green-500";

  return (
    <div className="p-4 border rounded-xl">
      <div className="text-xs text-muted-foreground mb-2">Budget</div>
      <div className="space-y-2">
        <div>
          <div className="flex justify-between text-xs mb-0.5">
            <span>Tokens</span>
            <span>{Math.round(budget.tokens_used / 1000)}K / {Math.round((budget.tokens_limit || 0) / 1000)}K</span>
          </div>
          <div className="h-1.5 bg-secondary rounded-full overflow-hidden">
            <div className={`h-full rounded-full ${getColor(tokenPercent)}`} style={{ width: `${tokenPercent}%` }} />
          </div>
        </div>
        <div>
          <div className="flex justify-between text-xs mb-0.5">
            <span>Time</span>
            <span>{Math.round(budget.wall_clock_seconds / 60)}m / {Math.round((budget.wall_clock_limit || 0) / 60)}m</span>
          </div>
          <div className="h-1.5 bg-secondary rounded-full overflow-hidden">
            <div className={`h-full rounded-full ${getColor(timePercent)}`} style={{ width: `${timePercent}%` }} />
          </div>
        </div>
        <div className="text-xs text-muted-foreground">
          Repairs: {budget.repair_count || 0} / {budget.max_repairs || 3}
        </div>
      </div>
    </div>
  );
}
