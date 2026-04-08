"use client";

import type { BlockingIssue, QAReport, ScoreMap } from "@/lib/types";
import { isRecord } from "@/lib/types";

export function QAReportViewer({ reports }: { reports: QAReport[] }) {
  const parseJson = (json: string): unknown => {
    try {
      return JSON.parse(json);
    } catch {
      return json;
    }
  };

  return (
    <div className="p-4 border rounded-xl">
      <h3 className="text-sm font-semibold mb-3">QA Reports</h3>
      <div className="space-y-3">
        {reports.map((r) => {
          const parsedScores = typeof r.scores_json === "string" ? parseJson(r.scores_json) : r.scores_json;
          const scores: ScoreMap | null = isRecord(parsedScores)
            ? Object.fromEntries(
                Object.entries(parsedScores)
                  .filter(([, value]) => typeof value === "number")
                  .map(([key, value]) => [key, value as number])
              )
            : null;
          const parsedIssues = typeof r.blocking_issues_json === "string"
            ? parseJson(r.blocking_issues_json)
            : r.blocking_issues_json;
          const issues = Array.isArray(parsedIssues) ? parsedIssues as BlockingIssue[] : [];

          return (
            <div key={r.report_id || r.sprint} className="p-3 bg-accent/30 rounded-lg">
              <div className="flex items-center justify-between mb-2">
                <span className="text-xs font-medium">Sprint {r.sprint}</span>
                <span
                  className={`px-1.5 py-0.5 text-[10px] font-bold rounded ${
                    r.result === "pass"
                      ? "bg-green-500/20 text-green-400"
                      : "bg-red-500/20 text-red-400"
                  }`}
                >
                  {r.result?.toUpperCase()}
                </span>
              </div>

              {/* Score bars */}
              {scores && (
                <div className="space-y-1 mb-2">
                  {Object.entries(scores).map(([key, val]) => (
                    <div key={key} className="flex items-center gap-2 text-xs">
                      <span className="w-24 text-muted-foreground capitalize">{key.replace(/_/g, " ")}</span>
                      <div className="flex-1 h-1.5 bg-secondary rounded-full overflow-hidden">
                        <div
                          className={`h-full rounded-full ${
                            val >= 0.7 ? "bg-green-500" : val >= 0.4 ? "bg-yellow-500" : "bg-red-500"
                          }`}
                          style={{ width: `${(val || 0) * 100}%` }}
                        />
                      </div>
                      <span className="w-8 text-right tabular-nums">{Math.round((val || 0) * 100)}%</span>
                    </div>
                  ))}
                </div>
              )}

              {/* Blocking issues */}
              {Array.isArray(issues) && issues.length > 0 && (
                <div className="text-xs">
                  <div className="text-destructive font-medium mb-0.5">Blocking Issues:</div>
                  <ul className="list-disc list-inside text-muted-foreground">
                    {issues.map((issue, i) => (
                      <li key={i}>{typeof issue === "string" ? issue : issue.title || JSON.stringify(issue)}</li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
