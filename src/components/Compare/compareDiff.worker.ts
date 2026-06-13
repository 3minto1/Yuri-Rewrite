/// <reference lib="webworker" />

import { calculateDiff } from "./compareDiff";

type DiffRequest = { requestId: number; original: string; rewrite: string };

self.onmessage = (event: MessageEvent<DiffRequest>) => {
  try {
    self.postMessage({ requestId: event.data.requestId, result: calculateDiff(event.data.original, event.data.rewrite) });
  } catch (error) {
    self.postMessage({ requestId: event.data.requestId, error: error instanceof Error ? error.message : String(error) });
  }
};
