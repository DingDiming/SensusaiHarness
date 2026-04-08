"use client";

import type { Contract } from "@/lib/types";

export function ContractViewer({ contracts }: { contracts: Contract[] }) {
  return (
    <div className="p-4 border rounded-xl">
      <h3 className="text-sm font-semibold mb-3">Sprint Contracts</h3>
      <div className="space-y-2">
        {contracts.map((c) => (
          <div key={c.contract_id || c.sprint} className="p-3 bg-accent/30 rounded-lg text-xs">
            <div className="flex items-center justify-between mb-1">
              <span className="font-medium">Sprint {c.sprint}</span>
              <span className={`px-1.5 py-0.5 rounded ${
                c.status === "accepted" ? "bg-green-500/20 text-green-400" : "bg-yellow-500/20 text-yellow-400"
              }`}>
                {c.status}
              </span>
            </div>
            <p className="text-muted-foreground whitespace-pre-wrap">{c.done_definition}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
