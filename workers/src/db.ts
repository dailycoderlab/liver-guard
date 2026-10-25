import type { Env } from "./env";
import { OWNER } from "./env";
import { sha256Hex } from "./auth";

export const now = () => Date.now();
export const uuid = () => crypto.randomUUID();

/** Assets are content addressed; the R2 key mirrors the hash. */
export function assetKey(sha: string): string {
  return `u/${OWNER}/asset/${sha}`;
}

export function runLogKey(runId: string): string {
  return `u/${OWNER}/run/${runId}/log.jsonl`;
}

export function shotKey(runId: string, key: string): string {
  return `u/${OWNER}/run/${runId}/shot/${key}`;
}

export type DeviceRow = {
  id: string;
  name: string;
  platform: string;
  screen_w: number | null;
  screen_h: number | null;
  scale: number | null;
  caps_json: string | null;
  status: string;
  last_seen_at: number | null;
  created_at: number;
};

export async function deviceByToken(env: Env, token: string | null): Promise<DeviceRow | null> {
  if (!token) return null;
  const hash = await sha256Hex(token);
  return env.DB.prepare(
    "SELECT id, name, platform, screen_w, screen_h, scale, caps_json, status, last_seen_at, created_at FROM devices WHERE token_hash = ?",
  )
    .bind(hash)
    .first<DeviceRow>();
}

export async function hubState(env: Env): Promise<Map<string, { online: boolean; bounds?: unknown; caps?: unknown }>> {
  try {
    const stub = env.HUB.get(env.HUB.idFromName(OWNER));
    const res = await stub.fetch("https://hub/state");
    const data = (await res.json()) as {
      devices: Array<{ deviceId: string; online: boolean; bounds?: unknown; caps?: unknown }>;
    };
    return new Map(data.devices.map((d) => [d.deviceId, { online: d.online, bounds: d.bounds, caps: d.caps }]));
  } catch {
    return new Map();
  }
}

export function hub(env: Env) {
  return env.HUB.get(env.HUB.idFromName(OWNER));
}

export async function sendToDevice(env: Env, deviceId: string, message: unknown): Promise<boolean> {
  const res = await hub(env).fetch("https://hub/command", {
    method: "POST",
    body: JSON.stringify({ deviceId, message }),
    headers: { "content-type": "application/json" },
  });
  const data = (await res.json()) as { sent: boolean };
  return data.sent;
}
