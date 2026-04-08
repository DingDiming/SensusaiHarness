"use client";

import type { RoleAssignment } from "@/lib/types";

const ROLE_ICONS: Record<string, string> = {
  planner: "📋",
  generator: "⚡",
  evaluator: "🔍",
  researcher: "🔬",
  designer: "🎨",
};

export function RolePanel({
  roles,
  activeRole,
}: {
  roles: RoleAssignment[];
  activeRole: string | null;
}) {
  return (
    <div className="p-4 border rounded-xl">
      <h3 className="text-sm font-semibold mb-3">Assigned Roles</h3>
      <div className="space-y-2">
        {roles.length === 0 ? (
          <p className="text-sm text-muted-foreground">No roles assigned</p>
        ) : (
          roles.map((role) => (
            <div
              key={role.role_name}
              className={`p-3 rounded-lg border ${
                activeRole === role.role_name
                  ? "border-blue-500 bg-blue-500/10"
                  : "border-border"
              }`}
            >
              <div className="flex items-center gap-2">
                <span>{ROLE_ICONS[role.role_name] || "🤖"}</span>
                <span className="font-medium capitalize text-sm">{role.role_name}</span>
                {activeRole === role.role_name && (
                  <span className="ml-auto text-[10px] bg-blue-500 text-white px-1.5 py-0.5 rounded animate-pulse">
                    ACTIVE
                  </span>
                )}
              </div>
              <div className="text-xs text-muted-foreground mt-1">{role.model_id}</div>
              {role.assigned_reason && (
                <div className="text-xs text-muted-foreground/70 mt-0.5">{role.assigned_reason}</div>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
