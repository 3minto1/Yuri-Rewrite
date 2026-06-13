import { describe, expect, it } from "vitest";
import { calculateDiff, tokenizeMixed } from "./compareDiff";
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

  it("returns no ranges for equal text and falls back for very long input", () => {
    expect(calculateDiff("相同 text", "相同 text").ranges).toEqual([]);
    expect(calculateDiff("甲".repeat(80_001), "乙").mode).toBe("line");
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
