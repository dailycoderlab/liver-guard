import { Hono } from "hono";
import { isResponse, requireConsole, sha256Hex } from "../auth";
import { assetKey, deviceByToken, now } from "../db";
import type { Env } from "../env";
import { OWNER } from "../env";

export const assetRoutes = new Hono<{ Bindings: Env }>();

const MAX_ASSET_BYTES = 512 * 1024;

/**
 * Template upload. The hash in the path is verified against the body, so a
 * mislabelled asset can never poison the content addressed store.
 */
assetRoutes.put("/assets/:sha", async (c) => {
  const consoleAuth = await requireConsole(c);
  let allowed = !isResponse(consoleAuth);
  if (!allowed) {
    const device = await deviceByToken(c.env, c.req.header("authorization")?.replace(/^Bearer\s+/i, "") ?? null);
    allowed = !!device;
  }
  if (!allowed) return c.json({ error: "unauthorized" }, 401);

  const sha = c.req.param("sha");
  if (!/^[0-9a-f]{64}$/.test(sha)) return c.json({ error: "bad_sha" }, 400);

  const body = await c.req.arrayBuffer();
  if (body.byteLength === 0) return c.json({ error: "empty_body" }, 400);
  if (body.byteLength > MAX_ASSET_BYTES) return c.json({ error: "too_large", max: MAX_ASSET_BYTES }, 413);

  const actual = await sha256Hex(body);
  if (actual !== sha) return c.json({ error: "hash_mismatch", actual }, 400);

  const w = Number(c.req.query("w") ?? 0);
  const h = Number(c.req.query("h") ?? 0);
  const scale = Number(c.req.query("scale") ?? 1);
  const kind = (c.req.query("kind") ?? "template").slice(0, 24);

  const existing = await c.env.DB.prepare("SELECT sha256 FROM assets WHERE sha256 = ?").bind(sha).first();
  if (!existing) {
    await c.env.BUCKET.put(assetKey(sha), body, {
      httpMetadata: { contentType: "image/png", cacheControl: "public, max-age=31536000, immutable" },
    });
    await c.env.DB.prepare(
      "INSERT INTO assets (sha256, user_id, r2_key, w, h, scale, kind, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
      .bind(sha, OWNER, assetKey(sha), w, h, scale, kind, now())
      .run();
  }

  return c.json({ ok: true, sha256: sha, url: `/v1/assets/${sha}` });
});

/**
 * Assets are content addressed, so the URL itself is the capability. This route
 * is intentionally unauthenticated: <img> tags cannot send an auth header, and
 * a 256 bit hash is not guessable.
 */
assetRoutes.get("/assets/:sha", async (c) => {
  const sha = c.req.param("sha");
  if (!/^[0-9a-f]{64}$/.test(sha)) return c.json({ error: "bad_sha" }, 400);

  const object = await c.env.BUCKET.get(assetKey(sha));
  if (!object) return c.json({ error: "not_found" }, 404);

  return new Response(object.body, {
    headers: {
      "content-type": object.httpMetadata?.contentType ?? "image/png",
      "cache-control": "public, max-age=31536000, immutable",
      etag: `"${sha}"`,
    },
  });
});

assetRoutes.get("/assets", async (c) => {
  const payload = await requireConsole(c);
  if (isResponse(payload)) return payload;
  const rows = await c.env.DB.prepare(
    "SELECT sha256, w, h, scale, kind, created_at FROM assets WHERE user_id = ? ORDER BY created_at DESC LIMIT 200",
  )
    .bind(OWNER)
    .all();
  return c.json({ assets: rows.results ?? [] });
});
