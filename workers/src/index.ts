import { Hono } from "hono";
import { cors } from "hono/cors";
import { requireUser } from "./auth";
import type { Env } from "./env";
import { authRoutes } from "./routes/auth";
import { scriptRoutes } from "./routes/scripts";
import { assetRoutes } from "./routes/assets";
import { runRoutes } from "./routes/runs";

export type AppEnv = { Bindings: Env; Variables: { userId: string } };
const app = new Hono<AppEnv>();
app.use("/v1/*", cors({ origin: "*", allowHeaders: ["authorization", "content-type"], allowMethods: ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"], maxAge: 86400 }));
app.get("/v1/health", (c) => c.json({ ok: true, service: "sky-platform", ts: Date.now() }));
app.route("/v1", authRoutes);
const api = new Hono<AppEnv>();
api.use("*", async (c, next) => {
  const user = await requireUser(c);
  if (user instanceof Response) return user;
  c.set("userId", user);
  await next();
});
api.route("/", scriptRoutes);
api.route("/", assetRoutes);
api.route("/", runRoutes);
app.route("/v1", api);
export default app;
