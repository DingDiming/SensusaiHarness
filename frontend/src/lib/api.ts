const API_BASE = process.env.NEXT_PUBLIC_API_BASE || "/api/app";
const CORE_BASE = process.env.NEXT_PUBLIC_CORE_BASE || "/api/core";

export function getToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("token");
}

export function setToken(token: string) {
  localStorage.setItem("token", token);
}

export function clearToken() {
  localStorage.removeItem("token");
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const token = getToken();
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(`${API_BASE}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (res.status === 401) {
    clearToken();
    if (typeof window !== "undefined") window.location.href = "/login";
    throw new ApiError(401, "Unauthorized");
  }

  if (!res.ok) {
    const data = await res.json().catch(() => ({ detail: "Request failed" }));
    throw new ApiError(res.status, data.detail || `Error ${res.status}`);
  }

  return res.json();
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body),
  delete: <T>(path: string) => request<T>("DELETE", path),
};

export function getErrorMessage(error: unknown, fallback = "Request failed"): string {
  return error instanceof Error ? error.message : fallback;
}

/**
 * SSE client that passes token via query parameter (EventSource compatible).
 */
export function createSSE(runId: string, onEvent: (type: string, data: unknown) => void): EventSource {
  const token = getToken();
  const url = `${CORE_BASE}/runs/${runId}/stream?token=${encodeURIComponent(token || "")}`;
  const source = new EventSource(url);

  const eventTypes = [
    "state_change", "message", "contract", "tool_call", "tool_result",
    "qa_report", "checkpoint", "approval", "budget", "artifact",
    "progress", "role_switch", "error", "done",
  ];

  for (const type of eventTypes) {
    source.addEventListener(type, (e: MessageEvent) => {
      try {
        onEvent(type, JSON.parse(e.data));
      } catch {
        onEvent(type, e.data);
      }
    });
  }

  source.onerror = () => {
    // Auto-reconnect is handled by EventSource
  };

  return source;
}
