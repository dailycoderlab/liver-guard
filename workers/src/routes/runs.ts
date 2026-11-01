import { Hono } from "hono";
import { z } from "zod";
import { readLimitedBody, runLogKey, shotKey, uuid } from "../db";
import type { AppEnv } from "../index";

export const runRoutes = new Hono<AppEnv>();
const Run = z.object({
  scriptId: z.string().uuid(), version: z.number().int().positive(), deviceId: z.string().min(1).max(128).optional(),
  state: z.enum(["completed", "failed", "cancelled"]), startedAt: z.number().int().nonnegative(), endedAt: z.number().int().nonnegative(),
  stepCount: z.number().int().nonnegative().default(0), errorCode: z.string().max(128).nullable().optional(),
  errorMsg: z.string().max(2000).nullable().optional(), aggregate: z.unknown().optional(),
}).refine((r) => r.endedAt >= r.startedAt);
const owned = (c: any) => c.env.DB.prepare("SELECT * FROM runs WHERE id = ? AND user_id = ?").bind(c.req.param("id"), c.get("userId")).first();
runRoutes.post("/runs", async (c) => {
  const body = Run.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const r = body.data, user = c.get("userId"), id = uuid();
  const result = await c.env.DB.prepare("INSERT INTO runs (id, user_id, device_id, script_id, version, state, started_at, ended_at, step_count, error_code, error_msg, aggregate_json) SELECT ?, ?, ?, s.id, v.version, ?, ?, ?, ?, ?, ?, ? FROM scripts s JOIN script_versions v ON v.script_id = s.id WHERE s.id = ? AND s.user_id = ? AND v.version = ?")
    .bind(id, user, r.deviceId ?? "local", r.state, r.startedAt, r.endedAt, r.stepCount, r.errorCode ?? null, r.errorMsg ?? null, r.aggregate === undefined ? null : JSON.stringify(r.aggregate), r.scriptId, user, r.version).run();
  if (!result.meta.changes) return c.json({ error: "script_version_not_found" }, 404);
  return c.json({ runId: id }, 201);
});
runRoutes.get("/runs", async (c) => {
  const limit = Number(c.req.query("limit") ?? 50);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 200) return c.json({ error: "bad_request" }, 400);
  const rows = await c.env.DB.prepare("SELECT * FROM runs WHERE user_id = ? ORDER BY started_at DESC LIMIT ?").bind(c.get("userId"), limit).all();
  return c.json({ runs: rows.results });
});
runRoutes.get("/runs/:id", async (c) => {
  const run = await owned(c);
  if (!run) return c.json({ error: "not_found" }, 404);
  const scores = await c.env.DB.prepare("SELECT * FROM scores WHERE run_id = ? ORDER BY min_score ASC").bind(c.req.param("id")).all();
  return c.json({ run, scores: scores.results });
});
runRoutes.put("/runs/:id/logs", async (c) => {
  if (!await owned(c)) return c.json({ error: "not_found" }, 404);
  const max = 8 * 1024 * 1024;
  const data = await readLimitedBody(c.req.raw, max);
  if (!data) return c.json({ error: "too_large" }, 413);
  await c.env.BUCKET.put(runLogKey(c.get("userId"), c.req.param("id")), data, { httpMetadata: { contentType: "application/x-ndjson" } });
  return c.json({ ok: true });
});
runRoutes.get("/runs/:id/logs", async (c) => {
  if (!await owned(c)) return c.json({ error: "not_found" }, 404);
  const object = await c.env.BUCKET.get(runLogKey(c.get("userId"), c.req.param("id")));
  if (!object) return c.json({ error: "not_found" }, 404);
  return new Response(object.body, { headers: { "content-type": "application/x-ndjson; charset=utf-8", "cache-control": "private, no-store", "x-content-type-options": "nosniff" } });
});
runRoutes.get("/runs/:id/events", async (c) => {
  if (!await owned(c)) return c.json({ error: "not_found" }, 404);
  const rows = await c.env.DB.prepare("SELECT * FROM run_logs WHERE run_id = ? ORDER BY ts ASC LIMIT 500").bind(c.req.param("id")).all();
  return c.json({ events: rows.results });
});
runRoutes.put("/runs/:id/shots/:key", async (c) => {
  if (!await owned(c)) return c.json({ error: "not_found" }, 404);
  const key = c.req.param("key");
  if (!/^[a-zA-Z0-9_.-]{1,128}$/.test(key) || key === "." || key === "..") return c.json({ error: "bad_key" }, 400);
  const max = 3 * 1024 * 1024;
  const data = await readLimitedBody(c.req.raw, max);
  if (!data) return c.json({ error: "too_large" }, 413);
  if (!data.byteLength) return c.json({ error: "empty_body" }, 400);
  await c.env.BUCKET.put(shotKey(c.get("userId"), c.req.param("id"), key), data);
  return c.json({ ok: true });
});
runRoutes.get("/runs/:id/shots/:key", async (c) => {
  if (!await owned(c)) return c.json({ error: "not_found" }, 404);
  const key = c.req.param("key");
  if (!/^[a-zA-Z0-9_.-]{1,128}$/.test(key) || key === "." || key === "..") return c.json({ error: "not_found" }, 404);
  const object = await c.env.BUCKET.get(shotKey(c.get("userId"), c.req.param("id"), key));
  if (!object) return c.json({ error: "not_found" }, 404);
  return new Response(object.body, { headers: { "content-type": "application/octet-stream", "cache-control": "private, no-store", "x-content-type-options": "nosniff" } });
});
