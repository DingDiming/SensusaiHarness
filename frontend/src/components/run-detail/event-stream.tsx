"use client";

import type { RunEvent } from "@/lib/types";
import { isRecord } from "@/lib/types";

const TYPE_COLORS: Record<string, string> = {
  state_change: "text-blue-400",
  message: "text-foreground",
  contract: "text-indigo-400",
  tool_call: "text-yellow-400",
  tool_result: "text-yellow-300",
  qa_report: "text-purple-400",
  checkpoint: "text-cyan-400",
  approval: "text-amber-400",
  budget: "text-muted-foreground",
  artifact: "text-green-400",
  progress: "text-muted-foreground",
  role_switch: "text-pink-400",
  error: "text-red-400",
  done: "text-green-500",
};

export function EventStream({ events }: { events: RunEvent[] }) {
  const parseData = (json: string): unknown => {
    try {
      return JSON.parse(json);
    } catch {
      return json;
    }
  };

  const formatTime = (iso: string) => {
    try {
      return new Date(iso).toLocaleTimeString();
    } catch {
      return iso;
    }
  };

  const renderEventContent = (type: string, data: unknown) => {
    const payload = isRecord(data) ? data : null;

    switch (type) {
      case "state_change":
        return `${typeof payload?.from === "string" ? payload.from : "?"} → ${typeof payload?.state === "string" ? payload.state : "?"}`;
      case "role_switch":
        return `🎭 ${typeof payload?.role === "string" ? payload.role : "unknown"} (${typeof payload?.phase === "string" ? payload.phase : "unknown"})`;
      case "message":
        return typeof payload?.content === "string"
          ? payload.content.substring(0, 200)
          : JSON.stringify(data);
      case "contract":
        return `Sprint ${typeof payload?.sprint === "number" ? payload.sprint : "?"} contract: ${typeof payload?.status === "string" ? payload.status : "unknown"}`;
      case "qa_report":
        return `Sprint ${typeof payload?.sprint === "number" ? payload.sprint : "?"} QA: ${typeof payload?.result === "string" ? payload.result : "unknown"}`;
      case "checkpoint":
        return `Sprint ${typeof payload?.sprint === "number" ? payload.sprint : "?"} checkpoint saved`;
      case "error":
        return typeof payload?.message === "string" ? payload.message : JSON.stringify(data);
      case "done":
        return `Run ${typeof payload?.result === "string" ? payload.result : "unknown"}`;
      default:
        return JSON.stringify(data).substring(0, 200);
    }
  };

  return (
    <div className="p-4 border rounded-xl">
      <h3 className="text-sm font-semibold mb-3">Event Stream</h3>
      <div className="space-y-1 max-h-[600px] overflow-y-auto">
        {events.length === 0 ? (
          <p className="text-sm text-muted-foreground">Waiting for events...</p>
        ) : (
          events.map((event, i) => {
            const data = parseData(event.data_json);
            return (
              <div key={i} className="flex gap-2 text-xs py-1 border-b border-border/30">
                <span className="text-muted-foreground shrink-0 w-16 tabular-nums">
                  {formatTime(event.created_at)}
                </span>
                <span className={`shrink-0 w-24 font-mono ${TYPE_COLORS[event.event_type] || "text-foreground"}`}>
                  {event.event_type}
                </span>
                <span className="text-foreground/80 break-all">
                  {renderEventContent(event.event_type, data)}
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
