import { Hono } from "hono";
import { z } from "zod";
import { now, uuid } from "../db";
import type { AppEnv } from "../index";
import { IR, collectAssets, validateIR } from "../ir";

export const scriptRoutes = new Hono<AppEnv>();
const details = (row: any) => ({ id: row.id, name: row.name, game: row.game, currentVersion: row.current_version, draft: row.draft_json === null ? null : JSON.parse(row.draft_json), draftRevision: row.draft_revision, createdAt: row.created_at, updatedAt: row.updated_at });

scriptRoutes.get("/scripts", async (c) => {
  const rows = await c.env.DB.prepare("SELECT id, name, game, current_version, draft_json IS NOT NULL AS has_draft, draft_revision, created_at, updated_at FROM scripts WHERE user_id = ? ORDER BY updated_at DESC").bind(c.get("userId")).all<any>();
  return c.json({ scripts: rows.results.map((r) => ({ id: r.id, name: r.name, game: r.game, currentVersion: r.current_version, hasDraft: !!r.has_draft, draftRevision: r.draft_revision, createdAt: r.created_at, updatedAt: r.updated_at })) });
});
scriptRoutes.post("/scripts", async (c) => {
  const body = z.object({ name: z.string().min(1).max(96), game: z.string().max(64).optional() }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const id = uuid(), ts = now();
  await c.env.DB.prepare("INSERT INTO scripts (id, user_id, name, game, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)").bind(id, c.get("userId"), body.data.name, body.data.game ?? null, ts, ts).run();
  return c.json({ script: { id, name: body.data.name, game: body.data.game ?? null, currentVersion: 0, draftRevision: 0 } }, 201);
});
scriptRoutes.get("/scripts/:id", async (c) => {
  const row = await c.env.DB.prepare("SELECT * FROM scripts WHERE id = ? AND user_id = ?").bind(c.req.param("id"), c.get("userId")).first();
  return row ? c.json({ script: details(row) }) : c.json({ error: "not_found" }, 404);
});
scriptRoutes.patch("/scripts/:id", async (c) => {
  const body = z.object({ name: z.string().min(1).max(96).optional(), game: z.string().max(64).nullable().optional() }).strict().refine((x) => Object.keys(x).length > 0).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const sets: string[] = [], args: unknown[] = [];
  if (body.data.name !== undefined) { sets.push("name = ?"); args.push(body.data.name); }
  if (body.data.game !== undefined) { sets.push("game = ?"); args.push(body.data.game); }
  const result = await c.env.DB.prepare(`UPDATE scripts SET ${sets.join(", ")}, updated_at = ? WHERE id = ? AND user_id = ?`).bind(...args, now(), c.req.param("id"), c.get("userId")).run();
  return result.meta.changes ? c.json({ ok: true }) : c.json({ error: "not_found" }, 404);
});
scriptRoutes.patch("/scripts/:id/draft", async (c) => {
  const body = z.object({ draft: z.unknown().refine((v) => v !== undefined), revision: z.number().int().nonnegative() }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const id = c.req.param("id"), user = c.get("userId");
  const result = await c.env.DB.prepare("UPDATE scripts SET draft_json = ?, draft_revision = draft_revision + 1, updated_at = ? WHERE id = ? AND user_id = ? AND draft_revision = ?").bind(JSON.stringify(body.data.draft), now(), id, user, body.data.revision).run();
  if (result.meta.changes) return c.json({ ok: true, revision: body.data.revision + 1 });
  const current = await c.env.DB.prepare("SELECT draft_revision FROM scripts WHERE id = ? AND user_id = ?").bind(id, user).first<any>();
  return current ? c.json({ error: "revision_conflict", revision: current.draft_revision }, 409) : c.json({ error: "not_found" }, 404);
});
scriptRoutes.post("/scripts/:id/publish", async (c) => {
  const id = c.req.param("id"), user = c.get("userId");
  const exists = await c.env.DB.prepare("SELECT id FROM scripts WHERE id = ? AND user_id = ?").bind(id, user).first();
  if (!exists) return c.json({ error: "not_found" }, 404);
  const body = z.object({ ir: z.unknown(), note: z.string().max(200).optional() }).safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const parsed = IR.safeParse(body.data.ir);
  if (!parsed.success) return c.json({ error: "invalid_ir", detail: parsed.error.issues.slice(0, 20) }, 400);
  const errors = validateIR(parsed.data);
  if (errors.length) return c.json({ error: "invalid_ir", detail: errors }, 400);
  const missing: string[] = [];
  for (const ref of collectAssets(parsed.data)) {
    const sha = ref.replace(/^sha256:/, "");
    if (!/^[0-9a-f]{64}$/.test(sha) || !await c.env.DB.prepare("SELECT 1 FROM user_assets WHERE user_id = ? AND sha256 = ?").bind(user, sha).first()) missing.push(ref);
  }
  if (missing.length) return c.json({ error: "missing_assets", detail: missing }, 400);
  const ts = now();
  const result = await c.env.DB.batch([
    c.env.DB.prepare("UPDATE scripts SET current_version = current_version + 1, updated_at = ? WHERE id = ? AND user_id = ?").bind(ts, id, user),
    c.env.DB.prepare("INSERT INTO script_versions (id, script_id, version, ir_json, note, created_at) SELECT ?, id, current_version, ?, ?, ? FROM scripts WHERE id = ? AND user_id = ? RETURNING version").bind(uuid(), JSON.stringify(parsed.data), body.data.note ?? null, ts, id, user),
  ]);
  if (!result[1].results.length) return c.json({ error: "not_found" }, 404);
  return c.json({ ok: true, version: (result[1].results[0] as any).version });
});
scriptRoutes.get("/scripts/:id/versions", async (c) => {
  const id = c.req.param("id"), user = c.get("userId");
  if (!await c.env.DB.prepare("SELECT 1 FROM scripts WHERE id = ? AND user_id = ?").bind(id, user).first()) return c.json({ error: "not_found" }, 404);
  const rows = await c.env.DB.prepare("SELECT version, note, created_at FROM script_versions WHERE script_id = ? ORDER BY version DESC").bind(id).all();
  return c.json({ versions: rows.results });
});
scriptRoutes.get("/scripts/:id/versions/:version", async (c) => {
  const row = await c.env.DB.prepare("SELECT v.* FROM script_versions v JOIN scripts s ON s.id = v.script_id WHERE v.script_id = ? AND v.version = ? AND s.user_id = ?").bind(c.req.param("id"), Number(c.req.param("version")), c.get("userId")).first<any>();
  return row ? c.json({ version: { version: row.version, note: row.note, createdAt: row.created_at, ir: JSON.parse(row.ir_json) } }) : c.json({ error: "not_found" }, 404);
});
scriptRoutes.get("/scripts/:id/bundle", async (c) => {
  const user = c.get("userId"), id = c.req.param("id");
  const script = await c.env.DB.prepare("SELECT * FROM scripts WHERE id = ? AND user_id = ?").bind(id, user).first<any>();
  if (!script) return c.json({ error: "not_found" }, 404);
  const version = Number(c.req.query("v") ?? script.current_version);
  if (!Number.isSafeInteger(version) || version <= 0) return c.json({ error: "no_published_version" }, 404);
  const row = await c.env.DB.prepare("SELECT ir_json FROM script_versions WHERE script_id = ? AND version = ?").bind(id, version).first<any>();
  if (!row) return c.json({ error: "not_found" }, 404);
  const ir = JSON.parse(row.ir_json), assets: Record<string, unknown> = {};
  for (const ref of collectAssets(ir)) {
    const sha = ref.replace(/^sha256:/, "");
    const asset = await c.env.DB.prepare("SELECT w, h, scale FROM user_assets WHERE user_id = ? AND sha256 = ?").bind(user, sha).first<any>();
    if (asset) assets[ref] = { url: `/v1/assets/${sha}`, w: asset.w, h: asset.h, scale: asset.scale };
  }
  return c.json({ scriptId: id, version, name: script.name, ir, assets });
});
scriptRoutes.delete("/scripts/:id", async (c) => {
  const id = c.req.param("id"), user = c.get("userId");
  const results = await c.env.DB.batch([
    c.env.DB.prepare("DELETE FROM script_versions WHERE script_id IN (SELECT id FROM scripts WHERE id = ? AND user_id = ? AND NOT EXISTS (SELECT 1 FROM runs WHERE runs.script_id = scripts.id))").bind(id, user),
    c.env.DB.prepare("DELETE FROM scripts WHERE id = ? AND user_id = ? AND NOT EXISTS (SELECT 1 FROM runs WHERE runs.script_id = scripts.id)").bind(id, user),
  ]);
  if (results[1].meta.changes) return c.json({ ok: true });
  const script = await c.env.DB.prepare("SELECT id FROM scripts WHERE id = ? AND user_id = ?").bind(id, user).first();
  return script ? c.json({ error: "script_has_runs" }, 409) : c.json({ error: "not_found" }, 404);
});
