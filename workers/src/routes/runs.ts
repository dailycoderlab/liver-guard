import { Hono } from "hono";
import { z } from "zod";
import { isResponse, requireConsole } from "../auth";
import { deviceByToken, now, runLogKey, sendToDevice, shotKey, uuid } from "../db";
import type { Env } from "../env";
import { OWNER } from "../env";

export const runRoutes = new Hono<{ Bindings: Env }>();

const StartRun = z.object({
  deviceId: z.string().min(1),
  scriptId: z.string().min(1),
  version: z.number().int().positive().optional(),
  params: z.record(z.unknown()).optional(),
});

runRoutes.post("/runs", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;

  const body = StartRun.safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  const script = await c.env.DB.prepare("SELECT id, current_version FROM scripts WHERE id = ? AND user_id = ?")
    .bind(body.data.scriptId, OWNER)
    .first<any>();
  if (!script) return c.json({ error: "script_not_found" }, 404);
  const version = body.data.version ?? script.current_version;
  if (!version) return c.json({ error: "script_not_published" }, 400);

  const device = await c.env.DB.prepare("SELECT id FROM devices WHERE id = ? AND user_id = ?")
    .bind(body.data.deviceId, OWNER)
    .first();
  if (!device) return c.json({ error: "device_not_found" }, 404);

  const runId = uuid();
  await c.env.DB.prepare(
    "INSERT INTO runs (id, user_id, device_id, script_id, version, state, started_at) VALUES (?, ?, ?, ?, ?, 'starting', ?)",
  )
    .bind(runId, OWNER, body.data.deviceId, body.data.scriptId, version, now())
    .run();

  const sent = await sendToDevice(c.env, body.data.deviceId, {
    t: "run",
    runId,
    scriptId: body.data.scriptId,
    version,
    params: body.data.params ?? {},
  });

  if (!sent) {
    await c.env.DB.prepare("UPDATE runs SET state = 'failed', error_code = 'device_offline', ended_at = ? WHERE id = ?")
      .bind(now(), runId)
      .run();
    return c.json({ error: "device_offline", runId }, 409);
  }

  return c.json({ runId, state: "starting" }, 201);
});

runRoutes.post("/runs/:id/stop", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const run = await c.env.DB.prepare("SELECT id, device_id FROM runs WHERE id = ? AND user_id = ?")
    .bind(c.req.param("id"), OWNER)
    .first<any>();
  if (!run) return c.json({ error: "not_found" }, 404);
  const sent = await sendToDevice(c.env, run.device_id, { t: "stop", runId: run.id });
  return c.json({ ok: true, sent });
});

runRoutes.get("/runs", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const limit = Math.min(Number(c.req.query("limit") ?? 50), 200);
  const deviceId = c.req.query("deviceId");
  const rows = deviceId
    ? await c.env.DB.prepare(
        "SELECT * FROM runs WHERE user_id = ? AND device_id = ? ORDER BY started_at DESC LIMIT ?",
      )
        .bind(OWNER, deviceId, limit)
        .all()
    : await c.env.DB.prepare("SELECT * FROM runs WHERE user_id = ? ORDER BY started_at DESC LIMIT ?")
        .bind(OWNER, limit)
        .all();
  return c.json({ runs: rows.results ?? [] });
});

runRoutes.get("/runs/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const run = await c.env.DB.prepare("SELECT * FROM runs WHERE id = ? AND user_id = ?")
    .bind(c.req.param("id"), OWNER)
    .first<any>();
  if (!run) return c.json({ error: "not_found" }, 404);
  const scores = await c.env.DB.prepare("SELECT * FROM scores WHERE run_id = ? ORDER BY min_score ASC")
    .bind(run.id)
    .all();
  return c.json({ run, scores: scores.results ?? [] });
});

const LogLine = z.object({
  ts: z.number(),
  level: z.enum(["debug", "info", "warn", "error"]),
  event: z.string(),
  payload: z.unknown().optional(),
});

/**
 * Full-fidelity log upload. Details go to R2 as JSONL; D1 only keeps the run
 * summary, which is what keeps the free tier viable.
 */
runRoutes.post("/runs/:id/logs", async (c) => {
  const token = c.req.header("authorization")?.replace(/^Bearer\s+/i, "") ?? null;
  const device = await deviceByToken(c.env, token);
  if (!device) return c.json({ error: "unauthorized" }, 401);

  const runId = c.req.param("id");
  const raw = await c.req.text();
  if (raw.length > 8 * 1024 * 1024) return c.json({ error: "too_large" }, 413);

  const lines = raw
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    })
    .filter(Boolean)
    .slice(0, 20000);

  const parsed = lines
    .map((line) => LogLine.safeParse(line))
    .filter((r) => r.success)
    .map((r) => (r as any).data);

  const body = parsed.map((line) => JSON.stringify(line)).join("\n");

  const existing = await c.env.BUCKET.get(runLogKey(runId));
  const merged = existing ? `${await existing.text()}\n${body}` : body;
  await c.env.BUCKET.put(runLogKey(runId), merged, {
    httpMetadata: { contentType: "application/x-ndjson" },
  });

  // Keep warn/error in D1 so the console can show failures without reading R2.
  const interesting = parsed.filter((line) => line.level === "warn" || line.level === "error").slice(0, 200);
  if (interesting.length > 0) {
    await c.env.DB.batch(
      interesting.map((line) =>
        c.env.DB.prepare("INSERT INTO run_logs (id, run_id, ts, level, event, payload_json) VALUES (?, ?, ?, ?, ?, ?)")
          .bind(uuid(), runId, line.ts, line.level, line.event, line.payload ? JSON.stringify(line.payload) : null),
      ),
    ).catch(() => undefined);
  }

  return c.json({ ok: true, lines: parsed.length });
});

runRoutes.get("/runs/:id/logs", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const object = await c.env.BUCKET.get(runLogKey(c.req.param("id")));
  if (!object) return c.json({ error: "not_found" }, 404);
  return new Response(object.body, { headers: { "content-type": "application/x-ndjson; charset=utf-8" } });
});

runRoutes.get("/runs/:id/events", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const rows = await c.env.DB.prepare("SELECT * FROM run_logs WHERE run_id = ? ORDER BY ts ASC LIMIT 500")
    .bind(c.req.param("id"))
    .all();
  return c.json({ events: rows.results ?? [] });
});

runRoutes.put("/runs/:id/shots/:key", async (c) => {
  const token = c.req.header("authorization")?.replace(/^Bearer\s+/i, "") ?? null;
  const device = await deviceByToken(c.env, token);
  if (!device) return c.json({ error: "unauthorized" }, 401);

  const body = await c.req.arrayBuffer();
  if (body.byteLength === 0) return c.json({ error: "empty_body" }, 400);
  if (body.byteLength > 3 * 1024 * 1024) return c.json({ error: "too_large" }, 413);

  const contentType = c.req.header("content-type") ?? "image/jpeg";
  await c.env.BUCKET.put(shotKey(c.req.param("id"), c.req.param("key")), body, { httpMetadata: { contentType } });
  return c.json({ ok: true });
});
