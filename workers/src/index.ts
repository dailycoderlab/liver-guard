import { Hono } from "hono";
import { cors } from "hono/cors";
import { bearerToken, verifyConsoleToken } from "./auth";
import { deviceByToken } from "./db";
import type { Env } from "./env";
import { OWNER } from "./env";
import { RecordingHub } from "./hub";
import { assetRoutes } from "./routes/assets";
import { authRoutes } from "./routes/devices";
import { runRoutes } from "./routes/runs";
import { scriptRoutes } from "./routes/scripts";

const app = new Hono<{ Bindings: Env }>();

app.use(
  "/v1/*",
  cors({
    origin: (origin) => origin ?? "*",
    allowHeaders: ["authorization", "content-type"],
    allowMethods: ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
    maxAge: 86400,
  }),
);

app.get("/v1/health", (c) => c.json({ ok: true, service: "sky-platform", ts: Date.now() }));

app.route("/v1", authRoutes);
app.route("/v1", scriptRoutes);
app.route("/v1", assetRoutes);
app.route("/v1", runRoutes);

/** Device socket. Authenticated by the opaque device token. */
app.get("/v1/ws/device", async (c) => {
  const url = new URL(c.req.url);
  const token = url.searchParams.get("token") ?? bearerToken(c.req.header("authorization"));
  const device = await deviceByToken(c.env, token);
  if (!device) return c.json({ error: "unauthorized" }, 401);

  const target = new URL("https://hub/ws");
  target.searchParams.set("role", "device");
  target.searchParams.set("deviceId", device.id);
  return c.env.HUB.get(c.env.HUB.idFromName(OWNER)).fetch(new Request(target.toString(), c.req.raw));
});

/** Console socket. Authenticated by the console JWT. */
app.get("/v1/ws/console", async (c) => {
  const url = new URL(c.req.url);
  const token = url.searchParams.get("token") ?? bearerToken(c.req.header("authorization"));
  if (!token) return c.json({ error: "missing_token" }, 401);
  const payload = await verifyConsoleToken(c.env, token);
  if (!payload) return c.json({ error: "invalid_token" }, 401);

  const target = new URL("https://hub/ws");
  target.searchParams.set("role", "console");
  return c.env.HUB.get(c.env.HUB.idFromName(OWNER)).fetch(new Request(target.toString(), c.req.raw));
});

app.notFound((c) => c.json({ error: "not_found", path: new URL(c.req.url).pathname }, 404));

app.onError((err, c) => {
  console.error("unhandled", err);
  return c.json({ error: "internal_error", message: String(err?.message ?? err) }, 500);
});

export { RecordingHub };
export default app;
