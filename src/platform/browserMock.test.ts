import { describe, expect, it } from "vitest";
import type {
  AppSettings,
  Chapter,
  ModelDiagnosis,
  NovelDetail,
  RewriteAbApplyResult,
  RewriteAbChapterDetail,
  RewriteAbEstimate,
  RewriteAbRunDetail
} from "../types";
import { invokeBrowserMock } from "./browserMock";

describe("browser test mode", () => {
  it("loads representative data and supports common UI mutations", async () => {
    const detail = await invokeBrowserMock("get_novel_detail") as NovelDetail;
    expect(detail.novel.title).toBe("浏览器测试小说");
    expect(detail.chapters).toHaveLength(20);
    expect(detail.batches).toHaveLength(2);

    const savedSettings = await invokeBrowserMock("save_app_settings", {
      settings: { review_enabled: false, rewrite_parallelism: 6 }
    }) as AppSettings;
    expect(savedSettings.review_enabled).toBe(false);
    expect(savedSettings.rewrite_parallelism).toBe(6);
    const autoContinueSettings = await invokeBrowserMock("set_auto_continue_enabled", {
      enabled: true
    }) as AppSettings;
    expect(autoContinueSettings.auto_continue_enabled).toBe(true);

    const renamed = await invokeBrowserMock("update_chapter_title", {
      chapterId: detail.chapters[0].id,
      title: "浏览器测试新标题"
    }) as Chapter;
    expect(renamed.title).toBe("浏览器测试新标题");

    const diagnosis = await invokeBrowserMock("diagnose_model_profile", {
      profileId: "browser-profile-deepseek"
    }) as ModelDiagnosis;
    expect(diagnosis.status).toBe("ok");
    expect(diagnosis.checks).toHaveLength(3);
  });

  it("supports the complete multi-model A/B candidate lifecycle without changing formal rewrites before apply", async () => {
    const before = await invokeBrowserMock("get_novel_detail") as NovelDetail;
    const beforeRewrite = before.chapters[0].rewrite_text;
    const estimate = await invokeBrowserMock("estimate_rewrite_ab", {
      novelId: before.novel.id,
      batchId: before.batches[0].id,
      profileIds: ["browser-profile-deepseek", "browser-profile-claude"],
      reviewEnabled: false
    }) as RewriteAbEstimate;
    expect(estimate.model_count).toBe(2);
    expect(estimate.chapter_count).toBe(10);

    const run = await invokeBrowserMock("start_rewrite_ab", {
      novelId: before.novel.id,
      batchId: before.batches[0].id,
      profileIds: ["browser-profile-deepseek", "browser-profile-claude"],
      reviewEnabled: true
    }) as RewriteAbRunDetail;
    expect(run.status).toBe("ready");
    expect(run.completed_candidates).toBe(20);
    expect((await invokeBrowserMock("get_novel_detail") as NovelDetail).chapters[0].rewrite_text).toBe(beforeRewrite);
    const replacementEstimate = await invokeBrowserMock("estimate_rewrite_ab", {
      novelId: before.novel.id,
      batchId: before.batches[0].id,
      profileIds: ["browser-profile-deepseek", "browser-profile-claude"],
      reviewEnabled: false
    }) as RewriteAbEstimate;
    expect(replacementEstimate.existing_run_id).toBe(run.id);

    const candidateChapter = await invokeBrowserMock("get_rewrite_ab_chapter", {
      runId: run.id,
      chapterId: run.chapters[0].chapter_id
    }) as RewriteAbChapterDetail;
    expect(candidateChapter.candidates).toHaveLength(2);
    expect(candidateChapter.candidates[0].review_summary).toMatch(/复检通过/);

    const selected = await invokeBrowserMock("save_rewrite_ab_choices", {
      runId: run.id,
      choices: run.chapters.map((chapter) => ({ chapter_id: chapter.chapter_id, slot: "B" })),
      replaceAll: true
    }) as RewriteAbRunDetail;
    expect(selected.selected_chapters).toBe(10);

    const applied = await invokeBrowserMock("apply_rewrite_ab_choices", {
      runId: run.id,
      forceOverwrite: false
    }) as RewriteAbApplyResult;
    expect(applied.status).toBe("applied");
    expect((await invokeBrowserMock("get_novel_detail") as NovelDetail).chapters[0].rewrite_text).toMatch(/^B 候选/);

    const restored = await invokeBrowserMock("restore_rewrite_ab_baseline", {
      runId: run.id,
      forceOverwrite: false
    }) as RewriteAbApplyResult;
    expect(restored.status).toBe("restored");
    expect((await invokeBrowserMock("get_novel_detail") as NovelDetail).chapters[0].rewrite_text).toBe(beforeRewrite);
  });
});
