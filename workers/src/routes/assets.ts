import { Hono } from "hono";
import { sha256Hex } from "../auth";
import { assetKey, now, readLimitedBody } from "../db";
import type { AppEnv } from "../index";

export const assetRoutes = new Hono<AppEnv>();
const MAX_BYTES = 512 * 1024;
assetRoutes.get("/assets", async (c) => {
  const rows = await c.env.DB.prepare("SELECT sha256, w, h, scale, kind, created_at FROM user_assets WHERE user_id = ? ORDER BY created_at DESC LIMIT 200").bind(c.get("userId")).all();
  return c.json({ assets: rows.results });
});
assetRoutes.put("/assets/:sha", async (c) => {
  const sha = c.req.param("sha"), user = c.get("userId");
  if (!/^[0-9a-f]{64}$/.test(sha)) return c.json({ error: "bad_sha" }, 400);
  const w = Number(c.req.query("w")), h = Number(c.req.query("h")), scale = Number(c.req.query("scale") ?? 1);
  const kind = c.req.query("kind") ?? "template";
  if (!Number.isSafeInteger(w) || w < 1 || w > 32768 || !Number.isSafeInteger(h) || h < 1 || h > 32768 || !Number.isFinite(scale) || scale <= 0 || scale > 100 || !/^[a-zA-Z0-9_-]{1,24}$/.test(kind)) return c.json({ error: "bad_metadata" }, 400);
  const data = await readLimitedBody(c.req.raw, MAX_BYTES);
  if (!data) return c.json({ error: "too_large" }, 413);
  if (!data.byteLength) return c.json({ error: "empty_body" }, 400);
  if (await sha256Hex(data) !== sha) return c.json({ error: "hash_mismatch" }, 400);
  const key = assetKey(user, sha);
  await c.env.BUCKET.put(key, data, { httpMetadata: { contentType: "application/octet-stream" } });
  await c.env.DB.prepare("INSERT INTO user_assets (user_id, sha256, r2_key, w, h, scale, kind, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(user_id, sha256) DO NOTHING").bind(user, sha, key, w, h, scale, kind, now()).run();
  return c.json({ ok: true, sha256: sha });
});
assetRoutes.get("/assets/:sha", async (c) => {
  const sha = c.req.param("sha");
  if (!/^[0-9a-f]{64}$/.test(sha)) return c.json({ error: "not_found" }, 404);
  const row = await c.env.DB.prepare("SELECT r2_key FROM user_assets WHERE user_id = ? AND sha256 = ?").bind(c.get("userId"), sha).first<{ r2_key: string }>();
  if (!row) return c.json({ error: "not_found" }, 404);
  const object = await c.env.BUCKET.get(row.r2_key);
  if (!object) return c.json({ error: "not_found" }, 404);
  return new Response(object.body, { headers: { "content-type": "application/octet-stream", "cache-control": "private, no-store", "x-content-type-options": "nosniff" } });
});
