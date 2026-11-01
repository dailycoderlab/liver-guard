import { Hono } from "hono";
import { z } from "zod";
import { hashPassword, PASSWORD_ITERATIONS, requireUser, safeEqual, signUserToken, toHex } from "../auth";
import { now, uuid } from "../db";
import type { AppEnv } from "../index";

export const authRoutes = new Hono<AppEnv>();
const Email = z.string().trim().max(254).email();
const Username = z.string().trim().regex(/^[a-zA-Z0-9_.-]{3,64}$/);
// Both fields name the same login identifier; reject conflicting values rather
// than silently authenticating one while the caller believes it used the other.
const Credentials = z.object({
  username: z.union([Username, Email]).optional(),
  email: Email.optional(),
  password: z.string().min(8).max(256),
}).refine((value) => !!(value.username || value.email) &&
  (!value.username || !value.email || value.username.toLowerCase() === value.email.toLowerCase()))
  .transform((value) => ({ identifier: (value.email ?? value.username)!, password: value.password }));

function publicUser(user: { id: string; username: string }) {
  return { id: user.id, username: user.username, email: Email.safeParse(user.username).success ? user.username : null };
}

authRoutes.post("/auth/register", async (c) => {
  const body = Credentials.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const id = uuid();
  const salt = toHex(crypto.getRandomValues(new Uint8Array(16)));
  const hash = await hashPassword(body.data.password, salt, PASSWORD_ITERATIONS);
  // Sign first so misconfiguration cannot leave a registered but unusable account.
  const token = await signUserToken(c.env, id);
  const result = await c.env.DB.prepare("INSERT INTO users (id, username, password_salt, password_hash, password_iterations, created_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(username) DO NOTHING")
    .bind(id, body.data.identifier, salt, hash, PASSWORD_ITERATIONS, now()).run();
  if (!result.meta.changes) return c.json({ error: "username_taken" }, 409);
  return c.json({ token, user: publicUser({ id, username: body.data.identifier }) }, 201);
});

authRoutes.post("/auth/login", async (c) => {
  const body = Credentials.safeParse(await c.req.json().catch(() => null));
  if (!body.success) return c.json({ error: "bad_request" }, 400);
  const user = await c.env.DB.prepare("SELECT * FROM users WHERE username = ? COLLATE NOCASE").bind(body.data.identifier).first<{ id: string; username: string; password_salt: string; password_hash: string; password_iterations: number }>();
  const hash = await hashPassword(body.data.password, user?.password_salt ?? "00000000000000000000000000000000", user?.password_iterations ?? PASSWORD_ITERATIONS);
  if (!user || !safeEqual(hash, user.password_hash)) return c.json({ error: "invalid_credentials" }, 401);
  const token = await signUserToken(c.env, user.id);
  return c.json({ token, user: publicUser(user) });
});

authRoutes.get("/auth/me", async (c) => {
  const id = await requireUser(c);
  if (id instanceof Response) return id;
  const user = await c.env.DB.prepare("SELECT id, username FROM users WHERE id = ?").bind(id).first<{ id: string; username: string }>();
  if (!user) return c.json({ error: "unauthorized" }, 401);
  return c.json({ user: publicUser(user) });
});
