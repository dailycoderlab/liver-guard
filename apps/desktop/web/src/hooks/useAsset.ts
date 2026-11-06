import { useEffect, useState } from 'react';
import { backend } from '@/api';

const cache = new Map<string, Promise<string>>();

/** Object URL for a template asset; each asset is fetched once per session. */
export function assetUrl(sha: string): Promise<string> {
  let hit = cache.get(sha);
  if (!hit) {
    hit = backend.assetPng(sha).then((blob) => URL.createObjectURL(blob));
    cache.set(sha, hit);
    hit.catch(() => cache.delete(sha));
  }
  return hit;
}

export function useAssetUrl(sha: string | undefined): string | undefined {
  const [url, setUrl] = useState<string>();
  useEffect(() => {
    setUrl(undefined);
    if (!sha) return;
    let alive = true;
    assetUrl(sha)
      .then((u) => alive && setUrl(u))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [sha]);
  return url;
}
