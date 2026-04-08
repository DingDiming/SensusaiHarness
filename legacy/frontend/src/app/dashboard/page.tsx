"use client";

import { useEffect, useState, useCallback } from "react";
import { useRouter } from "next/navigation";
import { api, getToken, clearToken } from "@/lib/api";
import { RunCard } from "@/components/run-card";
import { CreateRunDialog } from "@/components/create-run-dialog";
import type { Run, Thread } from "@/lib/types";

export default function DashboardPage() {
  const router = useRouter();
  const [threads, setThreads] = useState<Thread[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [newTitle, setNewTitle] = useState("");
  const [showCreateRun, setShowCreateRun] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    try {
      const [t, r] = await Promise.all([
        api.get<Thread[]>("/threads"),
        api.get<Run[]>("/runs"),
      ]);
      setThreads(t);
      setRuns(r);
    } catch {
      // Error handled by api client (redirect on 401)
    }
  }, []);

  useEffect(() => {
    if (!getToken()) {
      router.replace("/login");
      return;
    }

    let active = true;
    const initialize = async () => {
      if (!active) return;
      await loadData();
    };

    void initialize();
    const interval = window.setInterval(() => {
      void loadData();
    }, 5000);

    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, [loadData, router]);

  const createThread = async () => {
    if (!newTitle.trim()) return;
    const thread = await api.post<Thread>("/threads", { title: newTitle.trim() });
    setNewTitle("");
    setThreads((prev) => [thread, ...prev]);
  };

  const handleRunCreated = () => {
    setShowCreateRun(null);
    void loadData();
  };

  return (
    <div className="min-h-screen">
      {/* Header */}
      <header className="border-b px-6 py-3 flex items-center justify-between">
        <h1 className="text-lg font-bold">SensusAI Harness</h1>
        <button
          onClick={() => { clearToken(); router.push("/login"); }}
          className="text-sm text-muted-foreground hover:text-foreground"
        >
          Sign out
        </button>
      </header>

      <div className="max-w-6xl mx-auto p-6 space-y-8">
        {/* Active Runs */}
        <section>
          <h2 className="text-lg font-semibold mb-4">Active Runs</h2>
          {runs.filter((r) => !["completed", "failed", "cancelled"].includes(r.state)).length === 0 ? (
            <p className="text-muted-foreground text-sm">No active runs</p>
          ) : (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              {runs
                .filter((r) => !["completed", "failed", "cancelled"].includes(r.state))
                .map((run) => (
                  <RunCard key={run.run_id} run={run} onClick={() => router.push(`/runs/${run.run_id}`)} />
                ))}
            </div>
          )}
        </section>

        {/* Threads */}
        <section>
          <h2 className="text-lg font-semibold mb-4">Threads</h2>

          <div className="flex gap-2 mb-4">
            <input
              type="text"
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && createThread()}
              placeholder="New thread title..."
              className="flex-1 px-3 py-2 border rounded-md bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary"
            />
            <button
              onClick={createThread}
              className="px-4 py-2 bg-primary text-primary-foreground rounded-md font-medium hover:opacity-90"
            >
              Create
            </button>
          </div>

          <div className="space-y-2">
            {threads.map((thread) => (
              <div
                key={thread.thread_id}
                className="flex items-center justify-between p-4 border rounded-lg hover:bg-accent/50 cursor-pointer"
              >
                <div
                  className="flex-1"
                  onClick={() => router.push(`/threads/${thread.thread_id}`)}
                >
                  <h3 className="font-medium">{thread.title}</h3>
                  <p className="text-xs text-muted-foreground">
                    {thread.default_mode} · {new Date(thread.created_at).toLocaleDateString()}
                  </p>
                </div>
                <button
                  onClick={() => setShowCreateRun(thread.thread_id)}
                  className="px-3 py-1 text-sm border rounded-md hover:bg-accent"
                >
                  New Run
                </button>
              </div>
            ))}
          </div>
        </section>

        {/* Recent Completed Runs */}
        <section>
          <h2 className="text-lg font-semibold mb-4">Completed Runs</h2>
          {runs.filter((r) => ["completed", "failed", "cancelled"].includes(r.state)).length === 0 ? (
            <p className="text-muted-foreground text-sm">No completed runs</p>
          ) : (
            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
              {runs
                .filter((r) => ["completed", "failed", "cancelled"].includes(r.state))
                .map((run) => (
                  <RunCard key={run.run_id} run={run} onClick={() => router.push(`/runs/${run.run_id}`)} />
                ))}
            </div>
          )}
        </section>
      </div>

      {showCreateRun && (
        <CreateRunDialog
          threadId={showCreateRun}
          onClose={() => setShowCreateRun(null)}
          onCreated={handleRunCreated}
        />
      )}
    </div>
  );
}
