import { describe, expect, it } from "vitest";
import {
  autoContinueDelaySeconds,
  autoContinueProgressFingerprint,
  isAutomaticPauseKind
} from "./autoContinue";
import type { AutoRunRecovery } from "./types";

describe("auto continue policy", () => {
  it("only treats runtime failure pauses as automatic", () => {
    for (const kind of ["rate_limit", "network", "temporary_gateway", "model_format", "content_filter"] as const) {
      expect(isAutomaticPauseKind(kind)).toBe(true);
    }
    for (const kind of ["user", "interrupted", "unknown", ""] as const) {
      expect(isAutomaticPauseKind(kind)).toBe(false);
    }
  });

  it("uses capped reason-specific backoff", () => {
    expect(autoContinueDelaySeconds("network", 0)).toBe(10);
    expect(autoContinueDelaySeconds("network", 10)).toBe(300);
    expect(autoContinueDelaySeconds("content_filter", 0)).toBe(30);
    expect(autoContinueDelaySeconds("model_format", 10)).toBe(600);
    expect(autoContinueDelaySeconds("rate_limit", 0)).toBe(300);
    expect(autoContinueDelaySeconds("rate_limit", 10)).toBe(900);
    expect(autoContinueDelaySeconds("user", 0)).toBe(0);
  });

  it("changes the progress fingerprint only when recoverable work advances", () => {
    const recovery: AutoRunRecovery = {
      novel_id: "novel-1",
      start_batch_index: 0,
      next_batch_index: 0,
      status: "paused",
      pause_reason: "网络异常",
      pause_kind: "network",
      phase: "rewrite",
      batch_index: 1,
      profile_ids: ["profile-1"],
      summary: {
        phase: "rewrite",
        batch_index: 1,
        batch_id: "batch-1",
        batch_label: "第1批",
        total_chapters: 10,
        staged_chapters: 2,
        pending_chapters: 8,
        pending_ranges: ["3-10"],
        pending_ranges_truncated: false
      }
    };
    const before = autoContinueProgressFingerprint(recovery);
    const after = autoContinueProgressFingerprint({
      ...recovery,
      summary: { ...recovery.summary!, staged_chapters: 3, pending_chapters: 7 }
    });
    expect(after).not.toBe(before);
  });
});
