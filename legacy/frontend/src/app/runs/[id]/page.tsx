"use client";

import { useEffect, useState, useCallback } from "react";
import { useParams, useRouter } from "next/navigation";
import { api, createSSE, getErrorMessage, getToken } from "@/lib/api";
import { SprintTimeline } from "@/components/run-detail/sprint-timeline";
import { EventStream } from "@/components/run-detail/event-stream";
import { RolePanel } from "@/components/run-detail/role-panel";
import { BudgetGauge } from "@/components/run-detail/budget-gauge";
import { ControlBar } from "@/components/run-detail/control-bar";
import { QAReportViewer } from "@/components/run-detail/qa-report-viewer";
import { ContractViewer } from "@/components/run-detail/contract-viewer";
import type { RunDetail, RunEvent } from "@/lib/types";
import { isRecord } from "@/lib/types";

export default function RunDetailPage() {
  const params = useParams();
  const router = useRouter();
  const runId = params.id as string;
  const [run, setRun] = useState<RunDetail | null>(null);
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [activeRole, setActiveRole] = useState<string | null>(null);
  const [error, setError] = useState("");

  const loadRun = useCallback(async () => {
    try {
      const data = await api.get<RunDetail>(`/runs/${runId}`);
      setRun(data);
      if (data.events) setEvents([...data.events].reverse());
    } catch (err: unknown) {
      setError(getErrorMessage(err, "Failed to load run"));
    }
  }, [runId]);

  useEffect(() => {
    if (!getToken()) {
      router.replace("/login");
      return;
    }
    const initialize = async () => {
      await loadRun();
    };
    void initialize();
  }, [loadRun, router]);

  // SSE real-time updates
  useEffect(() => {
    if (!runId || !getToken()) return;

    const source = createSSE(runId, (type, data) => {
      const event: RunEvent = {
        event_type: type,
        data_json: JSON.stringify(data),
        created_at: new Date().toISOString(),
      };

      setEvents((prev) => [event, ...prev]);

      const payload = isRecord(data) ? data : null;
      const nextState = typeof payload?.state === "string" ? payload.state : null;
      const nextRole = typeof payload?.role === "string" ? payload.role : null;

      if (type === "state_change" && nextState) {
        setRun((prev) => (prev ? { ...prev, state: nextState } : prev));
      }
      if (type === "role_switch" && nextRole) {
        setActiveRole(nextRole);
      }
      if (type === "progress" || type === "budget" || type === "checkpoint" || type === "done") {
        void loadRun();
      }
    });

    return () => source.close();
  }, [runId, loadRun]);

  if (error) {
    return (
      <div className="flex items-center justify-center h-screen text-destructive">
        {error}
      </div>
    );
  }

  if (!run) {
    return (
      <div className="flex items-center justify-center h-screen">
        <div className="animate-pulse text-muted-foreground">Loading run...</div>
      </div>
    );
  }
  return (
    <div className="min-h-screen">
      {/* Header */}
      <header className="border-b px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <button onClick={() => router.push("/dashboard")} className="text-muted-foreground hover:text-foreground">
            ← Back
          </button>
          <h1 className="text-lg font-bold">Run Detail</h1>
        </div>
        <ControlBar
          runId={runId}
          state={run.state}
          activeGate={run.active_gate}
          onAction={loadRun}
        />
      </header>

      <div className="max-w-7xl mx-auto p-6 space-y-6">
        {/* Summary Row */}
        <div className="grid gap-4 md:grid-cols-4">
          <div className="p-4 border rounded-xl">
            <div className="text-xs text-muted-foreground mb-1">Status</div>
            <div className="text-lg font-bold capitalize">{run.state.replace(/_/g, " ")}</div>
          </div>
          <div className="p-4 border rounded-xl">
            <div className="text-xs text-muted-foreground mb-1">Sprint</div>
            <div className="text-lg font-bold">{run.current_sprint} / {run.planned_sprints || "?"}</div>
          </div>
          <div className="p-4 border rounded-xl">
            <div className="text-xs text-muted-foreground mb-1">Active Role</div>
            <div className="text-lg font-bold capitalize">{activeRole || run.state}</div>
          </div>
          <BudgetGauge budget={run.budget} />
        </div>

        {/* Prompt */}
        <div className="p-4 border rounded-xl">
          <div className="text-xs text-muted-foreground mb-1">Prompt</div>
          <p className="text-sm">{run.prompt}</p>
        </div>

        {/* Sprint Timeline */}
        <SprintTimeline
          progress={run.progress || []}
          currentSprint={run.current_sprint}
          plannedSprints={run.planned_sprints}
        />

        {/* Two-column layout: Roles + Events */}
        <div className="grid gap-6 lg:grid-cols-3">
          {/* Left: Role panel + Contracts + QA */}
          <div className="space-y-6">
            <RolePanel roles={run.roles || []} activeRole={activeRole} />

            {run.contracts && run.contracts.length > 0 && (
              <ContractViewer contracts={run.contracts} />
            )}

            {run.qa_reports && run.qa_reports.length > 0 && (
              <QAReportViewer reports={run.qa_reports} />
            )}
          </div>

          {/* Right: Event stream */}
          <div className="lg:col-span-2">
            <EventStream events={events} />
          </div>
        </div>
      </div>
    </div>
  );
}
