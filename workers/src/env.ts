export type Env = {
  DB: D1Database;
  BUCKET: R2Bucket;
  HUB: DurableObjectNamespace;
  ASSETS: Fetcher;
  JWT_SECRET?: string;
  ADMIN_PASSWORD?: string;
};

/** v1 is single-user; every row carries this user id. */
export const OWNER = "owner";
