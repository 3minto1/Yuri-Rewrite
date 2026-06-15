import { describe, expect, it } from "vitest";
import type { Chapter } from "../../types";
import { buildSearchMatches, findTextMatches, initialSearchIndex, moveSearchIndex } from "./compareSearch";

const chapters: Chapter[] = [
  { id: "c1", novel_id: "n1", index: 1, title: "一", original_text: "Alpha 目标", rewrite_text: "alpha 目标", analysis_status: "completed", rewrite_status: "completed" },
  { id: "c2", novel_id: "n1", index: 2, title: "二", original_text: "目标", rewrite_text: "", analysis_status: "completed", rewrite_status: "pending" }
];

describe("compare search", () => {
  it("matches literal text and handles case sensitivity", () => {
    expect(findTextMatches("a.b aXb", "a.b", false)).toEqual([{ start: 0, end: 3 }]);
    expect(buildSearchMatches(chapters, "alpha", false)).toHaveLength(2);
    expect(buildSearchMatches(chapters, "alpha", true)).toHaveLength(1);
  });

  it("orders each chapter original before rewrite and skips empty rewrites", () => {
    const matches = buildSearchMatches(chapters, "目标", false);
    expect(matches.map((match) => `${match.chapter_id}:${match.side}`)).toEqual(["c1:original", "c1:rewrite", "c2:original"]);
    expect(initialSearchIndex(matches, "c2", 1)).toBe(2);
  });

  it("filters matches by original or rewrite scope", () => {
    expect(buildSearchMatches(chapters, "目标", false, "original").map((match) => match.side))
      .toEqual(["original", "original"]);
    expect(buildSearchMatches(chapters, "目标", false, "rewrite").map((match) => match.side))
      .toEqual(["rewrite"]);
    expect(buildSearchMatches(chapters, "目标", false, "both").map((match) => match.side))
      .toEqual(["original", "rewrite", "original"]);
  });

  it("moves in both directions and reports wrapping", () => {
    expect(moveSearchIndex(2, 3, 1)).toEqual({ index: 0, wrapped: true });
    expect(moveSearchIndex(0, 3, -1)).toEqual({ index: 2, wrapped: true });
    expect(moveSearchIndex(null, 3, -1)).toEqual({ index: 2, wrapped: false });
  });
});
