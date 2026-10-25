import { Hono } from "hono";
import { z } from "zod";
import { checkPassword, signConsoleToken, newDeviceToken, sha256Hex, requireConsole, isResponse } from "../auth";
import { deviceByToken, hubState, now, uuid } from "../db";
import type { Env } from "../env";
import { OWNER } from "../env";

export const authRoutes = new Hono<{ Bindings: Env }>();

const LoginBody = z.object({ password: z.string().min(1) });

authRoutes.post("/auth/login", async (c) => {
  const body = LoginBody.safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  if (!(await checkPassword(c.env, body.data.password))) {
    // Small delay so a wrong password is not a free oracle at full speed.
    await new Promise((r) => setTimeout(r, 250));
    return c.json({ error: "invalid_credentials" }, 401);
  }

  const token = await signConsoleToken(c.env);
  return c.json({ token, role: "console", user: OWNER });
});

authRoutes.get("/auth/me", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  return c.json({ user: payload.sub, role: payload.role });
});

const EnrollBody = z.object({
  name: z.string().min(1).max(64),
  platform: z.string().min(1).max(32),
  caps: z.record(z.unknown()).optional(),
  screen: z.object({ w: z.number(), h: z.number(), scale: z.number() }).optional(),
});

authRoutes.post("/devices/enroll", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;

  const body = EnrollBody.safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request", detail: body.error.flatten() }, 400);

  const id = uuid();
  const token = newDeviceToken();
  const tokenHash = await sha256Hex(token);
  const ts = now();

  await c.env.DB.prepare(
    `INSERT INTO devices (id, user_id, name, platform, screen_w, screen_h, scale, caps_json, token_hash, status, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'offline', ?)`,
  )
    .bind(
      id,
      OWNER,
      body.data.name,
      body.data.platform,
      body.data.screen?.w ?? null,
      body.data.screen?.h ?? null,
      body.data.screen?.scale ?? null,
      body.data.caps ? JSON.stringify(body.data.caps) : null,
      tokenHash,
      ts,
    )
    .run();

  // The token is returned exactly once; only its hash is stored.
  return c.json({ deviceId: id, deviceToken: token });
});

authRoutes.get("/devices", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;

  const rows = await c.env.DB.prepare(
    "SELECT id, name, platform, screen_w, screen_h, scale, caps_json, status, last_seen_at, created_at FROM devices WHERE user_id = ? ORDER BY created_at DESC",
  )
    .bind(OWNER)
    .all();

  const live = await hubState(c.env);
  const devices = (rows.results ?? []).map((row: any) => {
    const state = live.get(row.id);
    return {
      id: row.id,
      name: row.name,
      platform: row.platform,
      screen: row.screen_w ? { w: row.screen_w, h: row.screen_h, scale: row.scale } : null,
      caps: row.caps_json ? JSON.parse(row.caps_json) : null,
      online: state?.online ?? false,
      bounds: state?.bounds ?? null,
      lastSeenAt: state ? Date.now() : row.last_seen_at,
      createdAt: row.created_at,
    };
  });

  return c.json({ devices });
});

authRoutes.get("/devices/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const row = await c.env.DB.prepare("SELECT * FROM devices WHERE id = ? AND user_id = ?")
    .bind(c.req.param("id"), OWNER)
    .first();
  if (!row) return c.json({ error: "not_found" }, 404);
  const { token_hash, ...safe } = row as any;
  return c.json({ device: safe });
});

authRoutes.patch("/devices/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const body = z.object({ name: z.string().min(1).max(64) }).safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  await c.env.DB.prepare("UPDATE devices SET name = ? WHERE id = ? AND user_id = ?")
    .bind(body.data.name, c.req.param("id"), OWNER)
    .run();
  return c.json({ ok: true });
});

authRoutes.delete("/devices/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  await c.env.DB.prepare("DELETE FROM devices WHERE id = ? AND user_id = ?").bind(c.req.param("id"), OWNER).run();
  return c.json({ ok: true });
});

/** Device-facing identity check, useful for the CLI to validate its token. */
authRoutes.get("/device/me", async (c) => {
  const token = c.req.header("authorization")?.replace(/^Bearer\s+/i, "") ?? null;
  const device = await deviceByToken(c.env, token);
  if (!device) return c.json({ error: "invalid_device_token" }, 401);
  return c.json({ device });
});
