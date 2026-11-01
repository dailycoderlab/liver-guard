import { SignJWT, jwtVerify } from "jose";
import type { Context } from "hono";
import type { AppEnv } from "./index";
import type { Env } from "./env";

const ISSUER = "sky-platform";
const AUDIENCE = "sky-user-api";
// Cloudflare production may cap PBKDF2 at 100,000 iterations per invocation.
export const PASSWORD_ITERATIONS = 100_000;
const encoder = new TextEncoder();

export async function sha256Hex(input: string | ArrayBuffer | Uint8Array<ArrayBuffer>): Promise<string> {
  const bytes = typeof input === "string" ? encoder.encode(input) : input;
  return toHex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
}
export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
export function safeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let difference = 0;
  for (let i = 0; i < a.length; i++) difference |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return difference === 0;
}
export async function hashPassword(password: string, salt: string, iterations: number): Promise<string> {
  const saltBytes = new Uint8Array(salt.match(/.{2}/g)!.map((n) => parseInt(n, 16)));
  const key = await crypto.subtle.importKey("raw", encoder.encode(password), "PBKDF2", false, ["deriveBits"]);
  return toHex(new Uint8Array(await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: saltBytes, iterations }, key, 256)));
}
function secretKey(env: Env): Uint8Array {
  if (!env.JWT_SECRET || encoder.encode(env.JWT_SECRET).length < 32) throw new Error("JWT_SECRET must be at least 32 bytes");
  return encoder.encode(env.JWT_SECRET);
}
export async function signUserToken(env: Env, userId: string): Promise<string> {
  return new SignJWT({ role: "user" }).setProtectedHeader({ alg: "HS256" }).setSubject(userId)
    .setIssuer(ISSUER).setAudience(AUDIENCE).setIssuedAt().setExpirationTime("7d").sign(secretKey(env));
}
export async function requireUser(c: Context<AppEnv>): Promise<string | Response> {
  const token = /^Bearer\s+(\S+)$/i.exec(c.req.header("authorization") ?? "")?.[1];
  if (!token) return c.json({ error: "unauthorized" }, 401);
  try {
    const { payload } = await jwtVerify(token, secretKey(c.env), { issuer: ISSUER, audience: AUDIENCE, algorithms: ["HS256"] });
    if (payload.role !== "user" || typeof payload.sub !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(payload.sub)) throw new Error("invalid subject");
    const user = await c.env.DB.prepare("SELECT id FROM users WHERE id = ?").bind(payload.sub).first();
    if (!user) throw new Error("unknown user");
    return payload.sub;
  } catch {
    return c.json({ error: "unauthorized" }, 401);
  }
}
