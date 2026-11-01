export const now = () => Date.now();
export const uuid = () => crypto.randomUUID();
export const assetKey = (userId: string, sha: string) => `u/${userId}/asset/${sha}`;
export const runLogKey = (userId: string, runId: string) => `u/${userId}/run/${runId}/log.jsonl`;
export const shotKey = (userId: string, runId: string, key: string) => `u/${userId}/run/${runId}/shot/${key}`;

/** Read at most the permitted bytes, even when Content-Length is absent. */
export async function readLimitedBody(request: Request, max: number): Promise<Uint8Array<ArrayBuffer> | null> {
  if (Number(request.headers.get("content-length")) > max) return null;
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > max) { await reader.cancel(); return null; }
      chunks.push(value);
    }
  } finally { reader.releaseLock(); }
  const result = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { result.set(chunk, offset); offset += chunk.byteLength; }
  return result;
}
