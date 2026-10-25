import { Hono } from "hono";
import { z } from "zod";
import { isResponse, requireConsole } from "../auth";
import { deviceByToken, now, uuid } from "../db";
import type { Env } from "../env";
import { OWNER } from "../env";
import { IR, collectAssets, validateIR } from "../ir";

export const scriptRoutes = new Hono<{ Bindings: Env }>();

scriptRoutes.get("/scripts", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;

  const rows = await c.env.DB.prepare(
    "SELECT id, name, game, current_version, draft_json IS NOT NULL AS has_draft, created_at, updated_at FROM scripts WHERE user_id = ? ORDER BY updated_at DESC",
  )
    .bind(OWNER)
    .all();

  const scripts = (rows.results ?? []).map((row: any) => ({
    id: row.id,
    name: row.name,
    game: row.game,
    currentVersion: row.current_version,
    hasDraft: !!row.has_draft,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  }));
  return c.json({ scripts });
});

scriptRoutes.post("/scripts", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const body = z
    .object({ name: z.string().min(1).max(96), game: z.string().max(64).optional() })
    .safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  const id = uuid();
  const ts = now();
  await c.env.DB.prepare(
    "INSERT INTO scripts (id, user_id, name, game, current_version, draft_json, created_at, updated_at) VALUES (?, ?, ?, ?, 0, NULL, ?, ?)",
  )
    .bind(id, OWNER, body.data.name, body.data.game ?? null, ts, ts)
    .run();

  return c.json({ script: { id, name: body.data.name, game: body.data.game ?? null, currentVersion: 0 } }, 201);
});

scriptRoutes.get("/scripts/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const row = await c.env.DB.prepare("SELECT * FROM scripts WHERE id = ? AND user_id = ?")
    .bind(c.req.param("id"), OWNER)
    .first<any>();
  if (!row) return c.json({ error: "not_found" }, 404);
  return c.json({
    script: {
      id: row.id,
      name: row.name,
      game: row.game,
      currentVersion: row.current_version,
      draft: row.draft_json ? JSON.parse(row.draft_json) : null,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
    },
  });
});

scriptRoutes.patch("/scripts/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const body = z
    .object({ name: z.string().min(1).max(96).optional(), game: z.string().max(64).optional() })
    .safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  const sets: string[] = [];
  const binds: unknown[] = [];
  if (body.data.name !== undefined) {
    sets.push("name = ?");
    binds.push(body.data.name);
  }
  if (body.data.game !== undefined) {
    sets.push("game = ?");
    binds.push(body.data.game);
  }
  if (sets.length === 0) return c.json({ ok: true });
  sets.push("updated_at = ?");
  binds.push(now(), c.req.param("id"), OWNER);
  await c.env.DB.prepare(`UPDATE scripts SET ${sets.join(", ")} WHERE id = ? AND user_id = ?`)
    .bind(...binds)
    .run();
  return c.json({ ok: true });
});

const DraftBody = z.object({ draft: z.unknown() });

scriptRoutes.patch("/scripts/:id/draft", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const body = DraftBody.safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  // Drafts are intentionally not schema-validated: they are work in progress and
  // the editor saves them every few steps. Publishing is what validates.
  await c.env.DB.prepare("UPDATE scripts SET draft_json = ?, updated_at = ? WHERE id = ? AND user_id = ?")
    .bind(JSON.stringify(body.data.draft), now(), c.req.param("id"), OWNER)
    .run();
  return c.json({ ok: true });
});

const PublishBody = z.object({ ir: z.unknown(), note: z.string().max(200).optional() });

scriptRoutes.post("/scripts/:id/publish", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;

  const scriptId = c.req.param("id");
  const script = await c.env.DB.prepare("SELECT * FROM scripts WHERE id = ? AND user_id = ?")
    .bind(scriptId, OWNER)
    .first<any>();
  if (!script) return c.json({ error: "not_found" }, 404);

  const body = PublishBody.safeParse(await c.req.json().catch(() => ({})));
  if (!body.success) return c.json({ error: "bad_request" }, 400);

  const parsed = IR.safeParse(body.data.ir);
  if (!parsed.success) {
    return c.json({ error: "invalid_ir", detail: parsed.error.issues.slice(0, 20) }, 400);
  }

  const ir = parsed.data;
  const structural = validateIR(ir);
  if (structural.length > 0) return c.json({ error: "invalid_ir", detail: structural }, 400);

  const wanted = collectAssets(ir);
  const missing: string[] = [];
  for (const sha of wanted) {
    const row = await c.env.DB.prepare("SELECT sha256 FROM assets WHERE sha256 = ? AND user_id = ?")
      .bind(sha, OWNER)
      .first();
    if (!row) missing.push(sha);
  }
  if (missing.length > 0) return c.json({ error: "missing_assets", detail: missing }, 400);

  const version = (script.current_version ?? 0) + 1;
  const ts = now();
  await c.env.DB.batch([
    c.env.DB.prepare(
      "INSERT INTO script_versions (id, script_id, version, ir_json, note, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    ).bind(uuid(), scriptId, version, JSON.stringify(ir), body.data.note ?? null, ts),
    c.env.DB.prepare("UPDATE scripts SET current_version = ?, updated_at = ? WHERE id = ? AND user_id = ?").bind(
      version,
      ts,
      scriptId,
      OWNER,
    ),
  ]);

  return c.json({ ok: true, version });
});

scriptRoutes.get("/scripts/:id/versions", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const rows = await c.env.DB.prepare(
    "SELECT version, note, created_at FROM script_versions WHERE script_id = ? ORDER BY version DESC",
  )
    .bind(c.req.param("id"))
    .all();
  return c.json({ versions: rows.results ?? [] });
});

scriptRoutes.get("/scripts/:id/versions/:version", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const row = await c.env.DB.prepare("SELECT * FROM script_versions WHERE script_id = ? AND version = ?")
    .bind(c.req.param("id"), Number(c.req.param("version")))
    .first<any>();
  if (!row) return c.json({ error: "not_found" }, 404);
  return c.json({ version: { version: row.version, note: row.note, createdAt: row.created_at, ir: JSON.parse(row.ir_json) } });
});

/**
 * Everything a device needs to execute one version: the IR plus asset metadata.
 * Assets are content addressed and served from `/v1/assets/:sha`.
 */
scriptRoutes.get("/scripts/:id/bundle", async (c) => {
  const scriptId = c.req.param("id");
  const consoleAuth = await requireConsole(c);
  let allowed = !isResponse(consoleAuth);
  if (!allowed) {
    const device = await deviceByToken(c.env, c.req.header("authorization")?.replace(/^Bearer\s+/i, "") ?? null);
    allowed = !!device;
  }
  if (!allowed) return c.json({ error: "unauthorized" }, 401);

  const script = await c.env.DB.prepare("SELECT * FROM scripts WHERE id = ? AND user_id = ?")
    .bind(scriptId, OWNER)
    .first<any>();
  if (!script) return c.json({ error: "not_found" }, 404);

  const version = Number(c.req.query("v") ?? script.current_version ?? 0);
  if (!version) return c.json({ error: "no_published_version" }, 404);

  const row = await c.env.DB.prepare("SELECT * FROM script_versions WHERE script_id = ? AND version = ?")
    .bind(scriptId, version)
    .first<any>();
  if (!row) return c.json({ error: "not_found" }, 404);

  const ir = JSON.parse(row.ir_json);
  const assets: Record<string, { url: string; w: number; h: number; scale: number }> = {};
  for (const sha of collectAssets(ir)) {
    const asset = await c.env.DB.prepare("SELECT sha256, w, h, scale FROM assets WHERE sha256 = ?").bind(sha).first<any>();
    if (asset) {
      assets[sha] = { url: `/v1/assets/${sha}`, w: asset.w, h: asset.h, scale: asset.scale };
    }
  }

  return c.json({ scriptId, version, name: script.name, ir, assets });
});

scriptRoutes.delete("/scripts/:id", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const scriptId = c.req.param("id");
  await c.env.DB.batch([
    c.env.DB.prepare("DELETE FROM script_versions WHERE script_id = ?").bind(scriptId),
    c.env.DB.prepare("DELETE FROM scripts WHERE id = ? AND user_id = ?").bind(scriptId, OWNER),
  ]);
  return c.json({ ok: true });
});
