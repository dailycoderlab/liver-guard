import { SignJWT, jwtVerify } from "jose";
import type { Context } from "hono";
import { OWNER, type Env } from "./env";

const ISSUER = "sky-platform";

export async function sha256Hex(input: string | ArrayBuffer | Uint8Array): Promise<string> {
  const bytes =
    typeof input === "string"
      ? new TextEncoder().encode(input)
      : input instanceof Uint8Array
        ? input
        : new Uint8Array(input);
  const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Constant-time compare on equal-length hex strings. */
export function safeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

/**
 * Password check hashes both sides first so the comparison length is fixed and
 * the check stays constant-time regardless of what the user typed.
 */
export async function checkPassword(env: Env, supplied: string): Promise<boolean> {
  const expected = env.ADMIN_PASSWORD;
  if (!expected) return false;
  const [a, b] = await Promise.all([sha256Hex(supplied), sha256Hex(expected)]);
  return safeEqual(a, b);
}

function secretKey(env: Env): Uint8Array {
  const secret = env.JWT_SECRET;
  if (!secret) throw new Error("JWT_SECRET is not configured");
  return new TextEncoder().encode(secret);
}

export async function signConsoleToken(env: Env, ttlSeconds = 60 * 60 * 24 * 30): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  return new SignJWT({ sub: OWNER, role: "console" })
    .setProtectedHeader({ alg: "HS256" })
    .setIssuer(ISSUER)
    .setIssuedAt(now)
    .setExpirationTime(now + ttlSeconds)
    .sign(secretKey(env));
}

export type TokenPayload = { sub: string; role: string };

export async function verifyConsoleToken(env: Env, token: string): Promise<TokenPayload | null> {
  try {
    const { payload } = await jwtVerify(token, secretKey(env), { issuer: ISSUER });
    if (payload.role !== "console") return null;
    return { sub: String(payload.sub ?? ""), role: String(payload.role) };
  } catch {
    return null;
  }
}

export function bearerToken(header: string | null | undefined): string | null {
  if (!header) return null;
  const match = /^Bearer\s+(.+)$/i.exec(header.trim());
  return match ? match[1].trim() : null;
}

/** Device tokens are opaque random strings; only their hash is stored. */
export function newDeviceToken(): string {
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  return "sky_" + [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function requireConsole(c: Context<{ Bindings: Env }>): Promise<TokenPayload | Response> {
  const token = bearerToken(c.req.header("authorization"));
  if (!token) return c.json({ error: "missing_token" }, 401);
  const payload = await verifyConsoleToken(c.env, token);
  if (!payload) return c.json({ error: "invalid_token" }, 401);
  return payload;
}

export function isResponse(value: unknown): value is Response {
  return value instanceof Response;
}
