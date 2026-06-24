import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChapterRulesPage } from "./ChapterRulesPage";
import type { ChapterRulePreview, Novel } from "../../types";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn()
}));

vi.mock("../../tauriApi", () => ({ invokeCommand: mocks.invoke }));

const novel: Novel = {
  id: "novel-1",
  title: "测试小说",
  source_path: "a.txt",
  encoding: "UTF-8",
  status: "pending_split",
  created_at: "now"
};

const preview: ChapterRulePreview = {
  total_chapters: 3,
  can_apply: true,
  message: "预览已生成",
  chapters: [
    { index: 1, title: "第一章 开始" },
    { index: 2, title: "第二章 转折" },
    { index: 3, title: "第三章 结束" }
  ]
};

describe("ChapterRulesPage", () => {
  afterEach(() => {
    cleanup();
    mocks.invoke.mockReset();
  });

  it("previews, searches and saves chapter rules for a pending novel", async () => {
    const onApplied = vi.fn(async () => undefined);
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "get_chapter_rule") return null;
      if (command === "preview_chapter_rule") return preview;
      if (command === "save_chapter_rule_and_split") return {
        novel_id: novel.id,
        rule: {
          mode: "simple",
          line_start: true,
          prefix: "第",
          number_type: "mixed",
          unit: "章",
          include_pattern: String.raw`^\s*(序言|序章|序曲|前言|后记|番外)`,
          extra_pattern: "未完待续|作者的话",
          regex_pattern: ""
        },
        updated_at: "now"
      };
      return undefined;
    });

    render(
      <ChapterRulesPage
        novel={novel}
        busy=""
        processing={false}
        onBack={vi.fn()}
        onApplied={onApplied}
        onUseBuiltin={vi.fn()}
        showNotice={vi.fn()}
      />
    );

    fireEvent.click(await screen.findByRole("button", { name: "生成预览" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("preview_chapter_rule", expect.objectContaining({
      rule: expect.objectContaining({
        include_pattern: expect.stringContaining("序章"),
        extra_pattern: expect.stringContaining("未完待续")
      })
    })));
    expect(await screen.findByText("第一章 开始")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("搜索预览章节"), { target: { value: "2" } });
    const previewList = screen.getByText(/共 3 章/).closest("section") as HTMLElement;
    expect(within(previewList).getByText("第二章 转折")).toBeInTheDocument();
    expect(within(previewList).queryByText("第一章 开始")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_chapter_rule_and_split", expect.objectContaining({
      novelId: "novel-1"
    })));
    await waitFor(() => expect(onApplied).toHaveBeenCalledWith("novel-1"));
  });
});
