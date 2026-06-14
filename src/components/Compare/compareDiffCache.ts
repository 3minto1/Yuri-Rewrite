import type { DiffResult } from "./compareDiff";

type DiffCacheEntry = {
  original: string;
  rewrite: string;
  result: DiffResult;
};

const CACHE_LIMIT = 12;
const cache = new Map<string, DiffCacheEntry>();

export function getCachedDiff(chapterId: string, original: string, rewrite: string): DiffResult | undefined {
  const entry = cache.get(chapterId);
  if (!entry || entry.original !== original || entry.rewrite !== rewrite) return undefined;
  cache.delete(chapterId);
  cache.set(chapterId, entry);
  return entry.result;
}

export function setCachedDiff(chapterId: string, original: string, rewrite: string, result: DiffResult) {
  cache.delete(chapterId);
  cache.set(chapterId, { original, rewrite, result });
  while (cache.size > CACHE_LIMIT) {
    const oldestKey = cache.keys().next().value;
    if (oldestKey === undefined) break;
    cache.delete(oldestKey);
  }
}

export function clearDiffCache() {
  cache.clear();
}

export function getDiffCacheSize() {
  return cache.size;
}
