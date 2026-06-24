import { describe, expect, it } from "vitest";
import type { AppSettings, Chapter, ModelDiagnosis, NovelDetail } from "../types";
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
});
