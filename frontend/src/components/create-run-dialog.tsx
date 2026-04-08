"use client";

import { useState, useEffect } from "react";
import { api, getErrorMessage } from "@/lib/api";
import type { RoleConfig, RoleSuggestion } from "@/lib/types";

export function CreateRunDialog({
  threadId,
  onClose,
  onCreated,
}: {
  threadId: string;
  onClose: () => void;
  onCreated: () => void;
}) {
  const [prompt, setPrompt] = useState("");
  const [maxSprints, setMaxSprints] = useState(6);
  const [suggestions, setSuggestions] = useState<RoleSuggestion[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  // Fetch role suggestions when prompt changes (debounced)
  useEffect(() => {
    if (prompt.length < 10) {
      setSuggestions([]);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const roles = await api.get<RoleConfig[]>("/roles");
        setSuggestions(
          roles
            .filter((r) => r.is_default)
            .map((r) => ({
              role_name: r.role_name,
              suggested_config_id: r.config_id,
              suggested_model: r.model_id,
              reason: `Default ${r.role_name}`,
            }))
        );
      } catch {
        // Ignore suggestion errors
      }
    }, 500);
    return () => clearTimeout(timer);
  }, [prompt]);

  const handleCreate = async () => {
    if (!prompt.trim()) return;
    setLoading(true);
    setError("");
    try {
      await api.post(`/threads/${threadId}/runs`, {
        prompt: prompt.trim(),
        max_sprints: maxSprints,
      });
      onCreated();
    } catch (err: unknown) {
      setError(getErrorMessage(err, "Failed to create run"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="w-full max-w-lg bg-card border rounded-xl p-6 space-y-4 mx-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">Start New Run</h2>
          <button onClick={onClose} className="text-muted-foreground hover:text-foreground">✕</button>
        </div>

        {/* Prompt */}
        <div>
          <label className="block text-sm font-medium mb-1">What do you want to build?</label>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            rows={3}
            className="w-full px-3 py-2 border rounded-md bg-background text-foreground resize-none focus:outline-none focus:ring-2 focus:ring-primary"
            placeholder="Describe your project in 1-4 sentences..."
            autoFocus
          />
        </div>

        {/* Max Sprints */}
        <div>
          <label className="block text-sm font-medium mb-1">Max Sprints</label>
          <input
            type="number"
            value={maxSprints}
            onChange={(e) => setMaxSprints(parseInt(e.target.value) || 6)}
            min={1}
            max={20}
            className="w-20 px-3 py-2 border rounded-md bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary"
          />
        </div>

        {/* Role Suggestions */}
        {suggestions.length > 0 && (
          <div>
            <label className="block text-sm font-medium mb-2">Assigned Roles</label>
            <div className="space-y-1">
              {suggestions.map((s) => (
                <div key={s.role_name} className="flex items-center justify-between p-2 bg-accent/50 rounded-md text-sm">
                  <span className="font-medium capitalize">{s.role_name}</span>
                  <span className="text-muted-foreground">{s.suggested_model}</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {error && <p className="text-sm text-destructive">{error}</p>}

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 text-sm border rounded-md hover:bg-accent">
            Cancel
          </button>
          <button
            onClick={handleCreate}
            disabled={loading || !prompt.trim()}
            className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded-md font-medium hover:opacity-90 disabled:opacity-50"
          >
            {loading ? "Starting..." : "Start Run"}
          </button>
        </div>
      </div>
    </div>
  );
}
