import { describe, expect, it } from "vitest";
import type { Chapter, NovelSettings } from "../../types";
import { scanRewriteQuality } from "./compareQuality";

const settings: NovelSettings = {
  novel_id: "n1",
  protagonist_name: "萧炎",
  protagonist_aliases: "炎儿，岩枭",
  rewritten_protagonist_name: "萧妍",
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: "",
  relationship_targets: "[]",
  updated_at: "now"
};

function chapter(input: Partial<Chapter>): Chapter {
  return {
    id: input.id ?? "c1",
    novel_id: "n1",
    index: input.index ?? 1,
    title: input.title ?? "第一章",
    original_text: input.original_text ?? "萧炎走进大厅。",
    rewrite_text: input.rewrite_text ?? "萧妍走进大厅。",
    analysis_status: "completed",
    rewrite_status: "completed",
    ...input
  };
}

describe("compare quality scan", () => {
  it("detects protagonist source name and alias residue", () => {
    const issues = scanRewriteQuality([
      chapter({ rewrite_text: "萧妍想起炎儿这个旧称，岩枭也仍被旁人提起。" })
    ], settings);

    expect(issues.filter((issue) => issue.category === "source_name").map((issue) => issue.evidence)).toEqual([
      "炎儿",
      "岩枭"
    ]);
  });

  it("limits masculine residue warnings to sentences related to the rewritten protagonist", () => {
    const safe = scanRewriteQuality([
      chapter({ rewrite_text: "远处的少年走过街角。萧妍只是安静看着窗外。" })
    ], settings);
    const unsafe = scanRewriteQuality([
      chapter({ rewrite_text: "萧妍这个少年仍被众人称作公子。" })
    ], settings);

    expect(safe.some((issue) => issue.category === "gender_residue")).toBe(false);
    expect(unsafe.some((issue) => issue.category === "gender_residue")).toBe(true);
  });

  it("ignores pending empty rewrites but flags completed or edited empty rewrites", () => {
    const pending = scanRewriteQuality([
      chapter({ id: "pending", rewrite_text: "", rewrite_status: "pending" })
    ], settings);
    const completed = scanRewriteQuality([
      chapter({ id: "completed", rewrite_text: "", rewrite_status: "completed" })
    ], settings);
    const edited = scanRewriteQuality([
      chapter({ id: "edited", rewrite_text: "萧妍走进大厅。", rewrite_status: "completed" })
    ], settings, {
      chapterId: "edited",
      rewriteText: ""
    });

    expect(pending).toHaveLength(0);
    expect(completed).toEqual(expect.arrayContaining([
      expect.objectContaining({ category: "missing_rewrite", severity: "error", message: "已完成但改写稿为空。" })
    ]));
    expect(edited).toEqual(expect.arrayContaining([
      expect.objectContaining({ category: "missing_rewrite", severity: "error", message: "已完成但改写稿为空。" })
    ]));
  });

  it("detects unchanged text, ad noise, garbage, duplicate lines, and short completed rewrites", () => {
    const issues = scanRewriteQuality([
      chapter({ id: "short", index: 1, original_text: "很长的原文内容。".repeat(12), rewrite_text: "太短" }),
      chapter({ id: "same", index: 2, original_text: "一 二\n三", rewrite_text: "一二三" }),
      chapter({ id: "noise", index: 3, rewrite_text: "正文继续。\n求票求收藏，加入QQ群。\n□□□□□□" }),
      chapter({ id: "dup", index: 4, rewrite_text: "重复的正文段落内容\n重复的正文段落内容" })
    ], settings);

    expect(issues.map((issue) => issue.category)).toEqual(expect.arrayContaining([
      "missing_rewrite",
      "unchanged",
      "ad_noise",
      "garbage",
      "duplicate"
    ]));
  });

  it("uses the current edit draft override for the selected chapter", () => {
    const rows = [
      chapter({ id: "c1", rewrite_text: "萧妍走进大厅。" }),
      chapter({ id: "c2", index: 2, rewrite_text: "萧妍走进第二章。" })
    ];

    const clean = scanRewriteQuality(rows, settings);
    const dirty = scanRewriteQuality(rows, settings, {
      chapterId: "c1",
      rewriteText: "萧炎仍然走进大厅。"
    });

    expect(clean.some((issue) => issue.chapterId === "c1" && issue.category === "source_name")).toBe(false);
    expect(dirty.some((issue) => issue.chapterId === "c1" && issue.category === "source_name")).toBe(true);
    expect(dirty.some((issue) => issue.chapterId === "c2" && issue.category === "source_name")).toBe(false);
  });
});
