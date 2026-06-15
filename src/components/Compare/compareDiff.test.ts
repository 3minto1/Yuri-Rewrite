import { describe, expect, it } from "vitest";
import { calculateDiff, tokenizeMixed } from "./compareDiff";
import { clearDiffCache, getCachedDiff, getDiffCacheSize, setCachedDiff } from "./compareDiffCache";
import { buildTextSegments } from "./HighlightedText";

describe("compare diff", () => {
  it("tokenizes Chinese by character and English by word", () => {
    expect(tokenizeMixed("你好 hello42 世界")).toEqual(["你", "好", " ", "hello42", " ", "世", "界"]);
  });

  it("keeps punctuation as independent alignment tokens", () => {
    expect(tokenizeMixed("“三段！”")).toEqual(["“", "三", "段", "！", "”"]);
    const result = calculateDiff("“要不是族长是他的父亲。”", "“要不是族长是她的父亲。”");
    const originalChanged = result.ranges
      .filter((range) => range.side === "original")
      .map((range) => "“要不是族长是他的父亲。”".slice(range.start, range.end))
      .join("");
    const rewriteChanged = result.ranges
      .filter((range) => range.side === "rewrite")
      .map((range) => "“要不是族长是她的父亲。”".slice(range.start, range.end))
      .join("");
    expect(originalChanged).toBe("他");
    expect(rewriteChanged).toBe("她");
  });

  it("does not pull matching sentence-end punctuation into a change", () => {
    const original = "“斗之力，三段！”";
    const rewrite = "“斗之力，三段！”";
    expect(calculateDiff(original, rewrite).ranges).toEqual([]);

    const changed = calculateDiff("“少年面无表情。”", "“少女面无表情。”");
    for (const range of changed.ranges) {
      const text = (range.side === "original" ? "“少年面无表情。”" : "“少女面无表情。”").slice(range.start, range.end);
      expect(text).not.toMatch(/[“”。]/);
    }
  });

  it("returns removed and added ranges for replacements", () => {
    const result = calculateDiff("她看向天空", "她望向夜空");
    expect(result.mode).toBe("mixed");
    expect(result.ranges.some((range) => range.side === "original" && range.kind === "removed")).toBe(true);
    expect(result.ranges.some((range) => range.side === "rewrite" && range.kind === "added")).toBe(true);
  });

  it("ignores line-leading indentation differences without changing text offsets", () => {
    const original = "夜已深。\n  群山万壑间。\n　山脉中。";
    const rewrite = "夜已深。\n    群山万壑间。\n山脉中。";

    expect(calculateDiff(original, rewrite).ranges).toEqual([]);
  });

  it("does not highlight indentation when the line content also changes", () => {
    const original = "  少年走进山村。";
    const rewrite = "    少女走进山村。";
    const result = calculateDiff(original, rewrite);
    const originalChanged = result.ranges
      .filter((range) => range.side === "original")
      .map((range) => original.slice(range.start, range.end))
      .join("");
    const rewriteChanged = result.ranges
      .filter((range) => range.side === "rewrite")
      .map((range) => rewrite.slice(range.start, range.end))
      .join("");

    expect(originalChanged).toBe("年");
    expect(rewriteChanged).toBe("女");
  });

  it("keeps sentence-internal whitespace differences visible", () => {
    const original = "她 看向远方。";
    const rewrite = "她  看向远方。";
    const result = calculateDiff(original, rewrite);

    expect(result.ranges.some((range) => (
      range.side === "rewrite" && rewrite.slice(range.start, range.end).includes(" ")
    ))).toBe(true);
  });

  it("ignores indentation-only changes in line fallback mode", () => {
    const original = "  第一行\n  第二行";
    const rewrite = "    第一行\n第二行";

    expect(calculateDiff(original, rewrite, { mixedTimeoutMs: -1 })).toEqual({ ranges: [], mode: "line" });
  });

  it("returns no ranges for equal text and falls back for very long input", () => {
    expect(calculateDiff("相同 text", "相同 text").ranges).toEqual([]);
    expect(calculateDiff("甲".repeat(80_001), "乙").mode).toBe("line");
  });

  it("bounds a 7,500-character high-difference chapter", () => {
    const result = calculateDiff("甲".repeat(3_750), "乙".repeat(3_750), { mixedTimeoutMs: -1 });
    expect(result.mode).toBe("line");
    expect(result.ranges).toHaveLength(2);
  });

  it("falls back to line diff after mixed timeout or too many ranges", () => {
    expect(calculateDiff("甲\n乙", "甲\n丙", { mixedTimeoutMs: -1 }).mode).toBe("line");
    expect(calculateDiff("甲乙丙", "甲丁丙", { rangeLimit: 0 }).mode).toBe("plain");
  });

  it("falls back to plain text when line diff also exceeds its budget", () => {
    expect(calculateDiff("甲\n乙", "丙\n丁", { mixedTimeoutMs: -1, lineTimeoutMs: -1 }).mode).toBe("plain");
  });

  it("keeps search highlighting when it overlaps a diff range", () => {
    const segments = buildTextSegments(
      "目标文本",
      "original",
      [{ side: "original", start: 0, end: 2, kind: "removed" }],
      [{ id: "m1", chapter_id: "c1", side: "original", start: 0, end: 2 }]
    );
    expect(segments[0]).toMatchObject({ diffKind: "removed", searchMatch: { id: "m1" } });
  });
});

describe("compare diff cache", () => {
  it("keeps twelve recent chapters and invalidates changed text", () => {
    clearDiffCache();
    for (let index = 0; index < 13; index += 1) {
      setCachedDiff(`c${index}`, `原文${index}`, `改写${index}`, { ranges: [], mode: "mixed" });
    }
    expect(getDiffCacheSize()).toBe(12);
    expect(getCachedDiff("c0", "原文0", "改写0")).toBeUndefined();
    expect(getCachedDiff("c12", "原文12", "改写12")).toEqual({ ranges: [], mode: "mixed" });
    expect(getCachedDiff("c12", "已变化", "改写12")).toBeUndefined();
  });
});
