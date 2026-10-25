import type { DeviceInfo, DraftIR } from "./types";

const TOKEN_KEY = "sky.token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string | null): void {
  if (token) localStorage.setItem(TOKEN_KEY, token);
  else localStorage.removeItem(TOKEN_KEY);
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    public detail?: unknown,
  ) {
    super(`${status} ${code}`);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers = new Headers(init.headers);
  if (token) headers.set("authorization", `Bearer ${token}`);
  if (init.body && !(init.body instanceof Blob) && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }

  const res = await fetch(path, { ...init, headers });
  if (res.status === 204) return undefined as T;

  const text = await res.text();
  const body = text ? safeJson(text) : null;
  if (!res.ok) {
    throw new ApiError(res.status, (body as any)?.error ?? "request_failed", (body as any)?.detail);
  }
  return body as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return { raw: text };
  }
}

export const api = {
  login: (password: string) =>
    request<{ token: string }>("/v1/auth/login", { method: "POST", body: JSON.stringify({ password }) }),

  devices: () => request<{ devices: DeviceInfo[] }>("/v1/devices"),

  createDevice: (name: string, platform: string, caps: unknown) =>
    request<{ deviceId: string; deviceToken: string }>("/v1/devices/enroll", {
      method: "POST",
      body: JSON.stringify({ name, platform, caps }),
    }),

  scripts: () =>
    request<{ scripts: Array<{ id: string; name: string; currentVersion: number; hasDraft: boolean }> }>("/v1/scripts"),

  createScript: (name: string, game?: string) =>
    request<{ script: { id: string; name: string } }>("/v1/scripts", {
      method: "POST",
      body: JSON.stringify({ name, game }),
    }),

  script: (id: string) =>
    request<{ script: { id: string; name: string; currentVersion: number; draft: DraftIR | null } }>(
      `/v1/scripts/${id}`,
    ),

  saveDraft: (id: string, draft: DraftIR) =>
    request<{ ok: true }>(`/v1/scripts/${id}/draft`, { method: "PATCH", body: JSON.stringify({ draft }) }),

  publish: (id: string, ir: DraftIR, note?: string) =>
    request<{ ok: true; version: number }>(`/v1/scripts/${id}/publish`, {
      method: "POST",
      body: JSON.stringify({ ir, note }),
    }),

  putAsset: async (sha: string, blob: Blob, meta: { w: number; h: number; scale: number }) => {
    const query = new URLSearchParams({ w: String(Math.round(meta.w)), h: String(Math.round(meta.h)), scale: String(meta.scale) });
    return request<{ ok: true; url: string }>(`/v1/assets/${sha}?${query}`, {
      method: "PUT",
      body: blob,
      headers: { "content-type": "image/png" },
    });
  },

  startRun: (deviceId: string, scriptId: string, version?: number) =>
    request<{ runId: string }>("/v1/runs", {
      method: "POST",
      body: JSON.stringify({ deviceId, scriptId, version }),
    }),

  stopRun: (runId: string) => request<{ ok: true }>(`/v1/runs/${runId}/stop`, { method: "POST" }),

  runs: (deviceId?: string) =>
    request<{ runs: Array<Record<string, unknown>> }>(`/v1/runs${deviceId ? `?deviceId=${deviceId}` : ""}`),

  run: (id: string) => request<{ run: Record<string, unknown>; scores: Array<Record<string, unknown>> }>(`/v1/runs/${id}`),
};
