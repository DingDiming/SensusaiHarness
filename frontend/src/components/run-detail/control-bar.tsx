"use client";

import { useState } from "react";
import { api, getErrorMessage } from "@/lib/api";
import type { ApprovalGate } from "@/lib/types";

export function ControlBar({
  runId,
  state,
  activeGate,
  onAction,
}: {
  runId: string;
  state: string;
  activeGate: ApprovalGate | null;
  onAction: () => void;
}) {
  const [loading, setLoading] = useState("");

  const doAction = async (action: string, body?: Record<string, string>) => {
    setLoading(action);
    try {
      await api.post(`/runs/${runId}/${action}`, body);
      onAction();
    } catch (err: unknown) {
      alert(getErrorMessage(err, "Action failed"));
    } finally {
      setLoading("");
    }
  };

  const isTerminal = ["completed", "failed", "cancelled"].includes(state);
  const canPause = !isTerminal && state !== "paused" && state !== "awaiting_approval";
  const canResume = state === "paused" || state === "interrupted";
  const canCancel = !isTerminal;

  return (
    <div className="flex items-center gap-2">
      {/* Approval controls */}
      {activeGate && state === "awaiting_approval" && (
        <div className="flex items-center gap-2 mr-4 px-3 py-1 bg-amber-500/10 border border-amber-500/30 rounded-lg">
          <span className="text-sm text-amber-400">⚠️ {activeGate.title || "Approval needed"}</span>
          <button
            onClick={() => doAction("approve", { gate_id: activeGate.gate_id })}
            disabled={loading === "approve"}
            className="px-3 py-1 text-xs bg-green-600 text-white rounded hover:bg-green-700 disabled:opacity-50"
          >
            {loading === "approve" ? "..." : "Approve"}
          </button>
          <button
            onClick={() => doAction("reject", { gate_id: activeGate.gate_id })}
            disabled={loading === "reject"}
            className="px-3 py-1 text-xs bg-red-600 text-white rounded hover:bg-red-700 disabled:opacity-50"
          >
            {loading === "reject" ? "..." : "Reject"}
          </button>
        </div>
      )}

      {canPause && (
        <button
          onClick={() => doAction("pause")}
          disabled={!!loading}
          className="px-3 py-1 text-sm border rounded-md hover:bg-accent disabled:opacity-50"
        >
          ⏸ Pause
        </button>
      )}
      {canResume && (
        <button
          onClick={() => doAction("resume")}
          disabled={!!loading}
          className="px-3 py-1 text-sm bg-primary text-primary-foreground rounded-md hover:opacity-90 disabled:opacity-50"
        >
          ▶ Resume
        </button>
      )}
      {canCancel && (
        <button
          onClick={() => doAction("cancel")}
          disabled={!!loading}
          className="px-3 py-1 text-sm text-destructive border border-destructive/30 rounded-md hover:bg-destructive/10 disabled:opacity-50"
        >
          ✕ Cancel
        </button>
      )}
    </div>
  );
}
