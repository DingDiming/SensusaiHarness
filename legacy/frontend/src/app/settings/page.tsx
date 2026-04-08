"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { api, getToken } from "@/lib/api";
import type { RoleConfig } from "@/lib/types";

export default function SettingsPage() {
  const router = useRouter();
  const [roles, setRoles] = useState<RoleConfig[]>([]);

  const loadRoles = useCallback(async () => {
    const data = await api.get<RoleConfig[]>("/roles");
    setRoles(data);
  }, []);

  useEffect(() => {
    if (!getToken()) {
      router.replace("/login");
      return;
    }
    const initialize = async () => {
      await loadRoles();
    };
    void initialize();
  }, [loadRoles, router]);

  return (
    <div className="min-h-screen">
      <header className="border-b px-6 py-3 flex items-center gap-3">
        <button onClick={() => router.push("/dashboard")} className="text-muted-foreground hover:text-foreground">
          ← Back
        </button>
        <h1 className="text-lg font-bold">Settings</h1>
      </header>

      <div className="max-w-4xl mx-auto p-6 space-y-8">
        {/* Role Configurations */}
        <section>
          <h2 className="text-lg font-semibold mb-4">Role Configurations</h2>
          <p className="text-sm text-muted-foreground mb-4">
            Configure which AI model each role uses. Roles are automatically assigned when creating runs.
          </p>

          <div className="space-y-3">
            {roles.map((role) => (
              <div key={role.config_id} className="p-4 border rounded-xl">
                <div className="flex items-center justify-between mb-2">
                  <div>
                    <span className="font-medium capitalize">{role.role_name}</span>
                    {role.is_default && (
                      <span className="ml-2 px-1.5 py-0.5 text-[10px] bg-primary/10 text-primary rounded">
                        default
                      </span>
                    )}
                  </div>
                  <span className="text-sm text-muted-foreground">{role.model_id}</span>
                </div>

                <div className="grid grid-cols-3 gap-4 text-xs text-muted-foreground">
                  <div>
                    <span className="block font-medium text-foreground mb-0.5">Temperature</span>
                    {role.temperature}
                  </div>
                  <div>
                    <span className="block font-medium text-foreground mb-0.5">Max Tokens</span>
                    {role.max_tokens}
                  </div>
                  <div>
                    <span className="block font-medium text-foreground mb-0.5">Tools</span>
                    {role.tool_permissions.join(", ") || "none"}
                  </div>
                </div>

                {role.system_prompt && (
                  <details className="mt-2">
                    <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
                      System Prompt
                    </summary>
                    <pre className="mt-1 text-xs text-muted-foreground bg-accent/50 p-2 rounded whitespace-pre-wrap">
                      {role.system_prompt}
                    </pre>
                  </details>
                )}
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
