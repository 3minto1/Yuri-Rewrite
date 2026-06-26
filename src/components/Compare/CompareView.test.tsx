import { createRef, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Chapter, NovelSettings } from "../../types";
import { clearDiffCache } from "./compareDiffCache";
import { CompareView } from "./CompareView";

const chapters: Chapter[] = [
  { id: "c1", novel_id: "n1", index: 1, title: "第一章", original_text: "Alpha 目标 原文", rewrite_text: "alpha 目标 改写", analysis_status: "completed", rewrite_status: "completed" },
  { id: "c2", novel_id: "n1", index: 2, title: "第二章", original_text: "第二章也有目标", rewrite_text: "最终目标", analysis_status: "completed", rewrite_status: "completed" }
];

const novelSettings: NovelSettings = {
  novel_id: "n1",
  protagonist_name: "萧炎",
  protagonist_aliases: "炎儿",
  rewritten_protagonist_name: "萧妍",
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: "",
  relationship_targets: "[]",
  updated_at: "now"
};

function Harness({
  onBack = vi.fn(),
  onRewriteChapter = vi.fn(async () => undefined),
  onTerminateRewrite = vi.fn(async () => undefined),
  onRestoreInitialRewrite = vi.fn(async () => undefined),
  onDirtyChange,
  initialChapters = chapters
}: {
  onBack?: () => void;
  onRewriteChapter?: (
    chapterId: string,
    instructions: string,
    sourceMode: "original" | "rewrite"
  ) => Promise<void>;
  onTerminateRewrite?: () => Promise<void>;
  onRestoreInitialRewrite?: (chapterId: string) => Promise<void>;
  onDirtyChange?: (dirty: boolean) => void;
  initialChapters?: Chapter[];
}) {
  const [chapterRows, setChapterRows] = useState(initialChapters);
  const [selectedChapterId, setSelectedChapterId] = useState("c1");
  return (
    <CompareView
      chapters={chapterRows}
      selectedChapter={chapterRows.find((chapter) => chapter.id === selectedChapterId)}
      selectedChapterId={selectedChapterId}
      novelSettings={novelSettings}
      busy=""
      originalRef={createRef<HTMLDivElement>()}
      rewriteRef={createRef<HTMLDivElement>()}
      onSelectChapter={setSelectedChapterId}
      onBack={onBack}
      onExport={vi.fn()}
      editingAllowed
      onSaveRewrite={async (chapterId, rewriteText) => {
        setChapterRows((rows) => rows.map((chapter) => chapter.id === chapterId
          ? { ...chapter, rewrite_text: rewriteText, rewrite_edited: true }
          : chapter));
      }}
      onRestoreRewrite={async (chapterId) => {
        setChapterRows((rows) => rows.map((chapter) => chapter.id === chapterId
          ? { ...chapter, rewrite_text: chapters.find((source) => source.id === chapterId)?.rewrite_text, rewrite_edited: false }
          : chapter));
      }}
      onRewriteChapter={onRewriteChapter}
      onTerminateRewrite={onTerminateRewrite}
      onRestoreInitialRewrite={onRestoreInitialRewrite}
      onDirtyChange={onDirtyChange}
    />
  );
}

function selectChapter(title: string) {
  const selector = screen.getByRole("combobox", { name: "章节" });
  if (selector.getAttribute("aria-expanded") !== "true") fireEvent.click(selector);
  fireEvent.click(screen.getByRole("option", { name: new RegExp(title) }));
}

describe("CompareView", () => {
  beforeEach(() => {
    clearDiffCache();
    window.localStorage.clear();
    vi.stubGlobal("Worker", undefined);
  });

  it("can limit global search to original or rewrite text", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "全局搜索" }), {
      target: { value: "Alpha" }
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("1 / 2"));

    fireEvent.change(screen.getByRole("combobox", { name: "查找范围" }), {
      target: { value: "original" }
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("1 / 1"));

    fireEvent.change(screen.getByRole("combobox", { name: "查找范围" }), {
      target: { value: "rewrite" }
    });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("1 / 1"));
    expect(screen.getByLabelText("改写稿内容").textContent).toContain("alpha");
  });

  it("saves an edited rewrite and recalculates visible text", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "人工修改后的目标正文" }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(screen.getByLabelText("改写稿内容")).toHaveTextContent("人工修改后的目标正文"));
    expect(screen.getByRole("button", { name: "恢复 AI 稿" })).toBeInTheDocument();
  });

  it("reports dirty state while editing and clears it after saving or cancelling", async () => {
    const onDirtyChange = vi.fn();
    render(<Harness onDirtyChange={onDirtyChange} />);

    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "人工修改后的目标正文" }
    });

    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "再次修改但不保存" }
    });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    fireEvent.click(screen.getByRole("button", { name: "关闭编辑" }));
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(false));
  });

  it("asks before navigating away from unsaved edits", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "尚未保存的正文" }
    });
    selectChapter("第二章");

    expect(screen.getByRole("dialog", { name: "改写稿尚未保存" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("1. 第一章");
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("2. 第二章"));
  });

  it("shows completed as presentation-only status in the chapter menu", () => {
    render(<Harness initialChapters={[
      chapters[0],
      { ...chapters[1], rewrite_text: null, rewrite_status: "pending" }
    ]} />);

    fireEvent.click(screen.getByRole("combobox", { name: "章节" }));
    const completedOption = screen.getByRole("option", { name: /第一章 completed/ });
    expect(within(completedOption).getByText("completed").closest(".status-badge")).toHaveClass(
      "compare-chapter-completed",
      "status-success"
    );
    expect(screen.getByRole("option", { name: "2. 第二章" })).not.toHaveTextContent("completed");
    expect(chapters[0].title).toBe("第一章");
  });

  it("keeps all compare actions in the compact two-row toolbar", () => {
    const { container } = render(<Harness />);
    expect(container.querySelectorAll(".compare-toolbar-row")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "恢复初稿" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "查找" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "差异" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /检查/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重写本章（原文）" })).toHaveClass("action-primary");
    expect(screen.getByRole("button", { name: "返回" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "TXT" })).toBeInTheDocument();
  });

  it("opens a compact quality panel with current and full-book counts", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    fireEvent.click(screen.getByRole("button", { name: /检查/ }));

    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 0 · 全书 1");
    expect(within(panel).getByRole("button", { name: "全部" })).toHaveClass("active");
    const ignoreButton = within(panel).getByRole("button", { name: "忽略当前问题" });
    const closeButton = within(panel).getByRole("button", { name: "关闭检查" });
    expect(ignoreButton.compareDocumentPosition(closeButton) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(panel).getByText("仍残留主角原名或别名：萧炎")).toBeInTheDocument();
  });

  it("ignores current quality issues across filters and updates counts", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍 ******" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    expect(screen.getByText("检查").closest("button")).toHaveTextContent("2");
    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 1 · 全书 2");
    fireEvent.click(within(panel).getByRole("button", { name: "警告" }));
    expect(within(panel).queryByText("仍残留主角原名或别名：萧炎")).not.toBeInTheDocument();

    fireEvent.click(within(panel).getByRole("button", { name: "忽略当前问题" }));

    expect(screen.getByRole("button", { name: "检查" })).not.toHaveTextContent(/\d/);
    expect(panel).toHaveTextContent("当前章 0 · 全书 0");
    expect(panel).toHaveTextContent("当前章未发现本地规则问题。");
    expect(JSON.parse(window.localStorage.getItem("yuri-rewrite.qualityIgnored.v1.n1") ?? "[]")).toHaveLength(2);
  });

  it("keeps ignored quality issues in localStorage but shows new evidence", () => {
    const first = render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    fireEvent.click(within(screen.getByLabelText("本地质量检查")).getByRole("button", { name: "忽略当前问题" }));
    expect(screen.getByRole("button", { name: "检查" })).not.toHaveTextContent(/\d/);

    first.unmount();
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" },
      { ...chapters[1], id: "c3", index: 3, title: "第三章", rewrite_text: "炎儿又在第三章。" }
    ]} />);

    expect(screen.getByText("检查").closest("button")).toHaveTextContent("1");
    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).not.toHaveTextContent("仍残留主角原名或别名：萧炎");
    expect(panel).toHaveTextContent("仍残留主角原名或别名：炎儿");
    expect(panel).toHaveTextContent("当前章 0 · 全书 1");
  });

  it("does not count pending empty rewrites as quality issues", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: null, rewrite_status: "pending" },
      { ...chapters[1], rewrite_text: "萧妍走进第二章。" }
    ]} />);

    expect(screen.getByRole("button", { name: "检查" })).not.toHaveTextContent(/\d/);
    fireEvent.click(screen.getByRole("button", { name: "检查" }));

    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 0 · 全书 0");
    expect(panel).toHaveTextContent("当前章未发现本地规则问题。");
    expect(screen.getByLabelText("改写稿内容")).toHaveTextContent("尚未改写。");
  });

  it("shows length delta warnings in the quality panel", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], original_text: "原".repeat(2000), rewrite_text: "改".repeat(1000) },
      { ...chapters[1], rewrite_text: "萧妍走进第二章。" }
    ]} />);

    expect(screen.getByRole("button", { name: /检查/ })).toHaveTextContent("1");
    fireEvent.click(screen.getByRole("button", { name: /检查/ }));

    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 1 · 全书 1");
    expect(panel).toHaveTextContent("字数");
    expect(panel).toHaveTextContent("原文 2000 字，改写稿 1000 字，相差 1000 字。");
  });

  it("virtualizes quality issues while keeping later issues reachable", () => {
    const manyIssueChapters = Array.from({ length: 85 }, (_, index) => ({
      ...chapters[0],
      id: `c${index + 1}`,
      index: index + 1,
      title: `第${index + 1}章`,
      rewrite_text: `萧炎仍在第${index + 1}章。`
    }));
    render(<Harness initialChapters={manyIssueChapters} />);

    fireEvent.click(screen.getByRole("button", { name: /检查/ }));

    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 1 · 全书 85");
    expect(panel).not.toHaveTextContent("已显示前 80 条");
    expect(within(panel).getAllByText("仍残留主角原名或别名：萧炎").length).toBeLessThan(85);

    const list = screen.getByLabelText("本地检查问题列表");
    fireEvent.scroll(list, { target: { scrollTop: 84 * 126 } });

    expect(panel).toHaveTextContent("第 85 章");
  });

  it("ignores one quality issue without opening search or switching chapters", () => {
    const { rerender } = render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧炎仍在第一章。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    const checkButton = screen.getByText("检查").closest("button");
    expect(checkButton).toHaveTextContent("2");
    fireEvent.click(checkButton!);

    const panel = screen.getByLabelText("本地质量检查");
    expect(panel).toHaveTextContent("当前章 1 · 全书 2");
    fireEvent.click(within(panel).getAllByRole("button", { name: "忽略此问题" })[0]);

    expect(screen.getByText("检查").closest("button")).toHaveTextContent("1");
    expect(panel).toHaveTextContent("当前章 0 · 全书 1");
    expect(screen.queryByRole("textbox", { name: "全局搜索" })).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("1. 第一章");

    rerender(<Harness key="with-new-issue" initialChapters={[
      { ...chapters[0], rewrite_text: "萧炎仍在第一章。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" },
      { ...chapters[0], id: "c3", index: 3, title: "第三章", rewrite_text: "萧炎仍在第三章。" }
    ]} />);

    expect(screen.getByText("检查").closest("button")).toHaveTextContent("2");
  });

  it("updates quality counts while editing the current rewrite", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" }
    ]} />);

    expect(screen.getByRole("button", { name: /检查/ })).not.toHaveTextContent("1");
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "萧炎仍然走进大厅。" }
    });

    expect(screen.getByRole("button", { name: /检查/ })).toHaveTextContent("1");
    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    expect(screen.getByLabelText("本地质量检查")).toHaveTextContent("当前章 1 · 全书 1");
  });

  it("opens rewrite-only search when a quality issue is clicked", async () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    fireEvent.click(screen.getByRole("button", { name: /仍残留主角原名或别名：萧炎/ }));

    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("2. 第二章"));
    expect(screen.getByRole("textbox", { name: "全局搜索" })).toHaveValue("萧炎");
    expect(screen.getByRole("combobox", { name: "查找范围" })).toHaveValue("rewrite");
    await waitFor(() => expect(within(screen.getByLabelText("改写稿内容")).getByText("萧炎")).toHaveClass("active-search-match"));
  });

  it("keeps unsaved edit protection when clicking a cross-chapter quality issue", async () => {
    render(<Harness initialChapters={[
      { ...chapters[0], rewrite_text: "萧妍走进大厅。" },
      { ...chapters[1], rewrite_text: "萧炎仍在第二章。" }
    ]} />);

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "尚未保存的正文" }
    });
    fireEvent.click(screen.getByRole("button", { name: /检查/ }));
    fireEvent.click(screen.getByRole("button", { name: /仍残留主角原名或别名：萧炎/ }));

    expect(screen.getByRole("dialog", { name: "改写稿尚未保存" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("1. 第一章");
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("2. 第二章"));
  });

  it("shows compact non-whitespace character counts in the second toolbar row", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], original_text: "一 二\n三", rewrite_text: "一二三四" }
    ]} />);

    expect(screen.getByLabelText("字数对比")).toHaveTextContent("字数：原文 3 · 改写稿 4 · +1（+33.3%）");
  });

  it("shows an empty rewrite character count without per-chapter progress", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], original_text: "一 二\n三", rewrite_text: null, rewrite_status: "pending" }
    ]} />);

    expect(screen.getByLabelText("字数对比")).toHaveTextContent("字数：原文 3 · 改写稿 0 · 未改写");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("updates the rewrite character count while editing", () => {
    render(<Harness initialChapters={[
      { ...chapters[0], original_text: "一二三四", rewrite_text: "一二" }
    ]} />);

    expect(screen.getByLabelText("字数对比")).toHaveTextContent("字数：原文 4 · 改写稿 2 · -2（-50.0%）");
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "一 二 三 四 五" }
    });

    expect(screen.getByLabelText("字数对比")).toHaveTextContent("字数：原文 4 · 改写稿 5 · +1（+25.0%）");
  });

  it("collects optional instructions before rewriting the current chapter", async () => {
    const onRewriteChapter = vi.fn(async () => undefined);
    render(<Harness onRewriteChapter={onRewriteChapter} />);
    expect(screen.getByRole("button", { name: "恢复初稿" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "重写本章（原文）" }));
    const dialog = screen.getByRole("dialog", { name: "根据原文重新改写《第一章》" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "单章重写补充要求" }), {
      target: { value: "加强双女主互动，但不要改变伏笔。" }
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "确定改写" }));
    await waitFor(() => expect(onRewriteChapter).toHaveBeenCalledWith(
      "c1",
      "加强双女主互动，但不要改变伏笔。",
      "original"
    ));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "根据原文重新改写《第一章》" })).not.toBeInTheDocument());
  });

  it("shows a terminate button while a single-chapter rewrite is running", async () => {
    let finishRewrite: (() => void) | undefined;
    const onRewriteChapter = vi.fn(() => new Promise<void>((resolve) => {
      finishRewrite = resolve;
    }));
    const onTerminateRewrite = vi.fn(async () => undefined);
    render(
      <Harness
        onRewriteChapter={onRewriteChapter}
        onTerminateRewrite={onTerminateRewrite}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "重写本章（原文）" }));
    const dialog = screen.getByRole("dialog", { name: "根据原文重新改写《第一章》" });
    expect(within(dialog).queryByRole("button", { name: "终止" })).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "确定改写" }));

    const terminateButton = await within(dialog).findByRole("button", { name: "终止" });
    expect(terminateButton).toBeEnabled();
    expect(within(dialog).getByRole("button", { name: "取消" })).toBeDisabled();
    fireEvent.click(terminateButton);
    await waitFor(() => expect(onTerminateRewrite).toHaveBeenCalledOnce());

    finishRewrite?.();
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "根据原文重新改写《第一章》" })).not.toBeInTheDocument());
  });

  it("can rewrite from the current draft without hiding the initial-draft restore action", async () => {
    const onRewriteChapter = vi.fn(async () => undefined);
    render(
      <Harness
        initialChapters={[
          { ...chapters[0], single_rewrite_original_available: true },
          chapters[1]
        ]}
        onRewriteChapter={onRewriteChapter}
      />
    );

    expect(screen.getByRole("button", { name: "恢复初稿" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "重写本章（原文）" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "重写本章选项" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "重写本章（改写稿）" }));
    const dialog = screen.getByRole("dialog", { name: "基于改写稿继续修改《第一章》" });
    expect(within(dialog).getByText(/当前改写稿为主要底稿/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "确定改写" }));
    await waitFor(() => expect(onRewriteChapter).toHaveBeenCalledWith("c1", "", "rewrite"));
  });

  it("offers to restore the initial draft after a single-chapter rewrite", async () => {
    const onRestoreInitialRewrite = vi.fn(async () => undefined);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <Harness
        initialChapters={[
          { ...chapters[0], single_rewrite_original_available: true },
          chapters[1]
        ]}
        onRestoreInitialRewrite={onRestoreInitialRewrite}
      />
    );

    expect(screen.getByRole("button", { name: "重写本章（原文）" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "恢复初稿" }));
    await waitFor(() => expect(onRestoreInitialRewrite).toHaveBeenCalledWith("c1"));
    expect(window.confirm).toHaveBeenCalledWith(
      "恢复到单章重新改写前的初稿？当前重新改写结果和之后的人工修改将被覆盖。"
    );
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("searches original and rewrite across chapters in order and wraps", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "全局搜索" }), { target: { value: "目标" } });
    const searchStatus = () => within(screen.getByRole("search")).getByRole("status");
    await waitFor(() => expect(searchStatus()).toHaveTextContent("1 / 4"));
    expect(within(screen.getByLabelText("原文内容")).getByText("目标")).toHaveClass("active-search-match");

    fireEvent.click(screen.getByRole("button", { name: "向下搜索" }));
    expect(within(screen.getByLabelText("改写稿内容")).getByText("目标")).toHaveClass("active-search-match");
    fireEvent.click(screen.getByRole("button", { name: "向下搜索" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("2. 第二章"));
    fireEvent.click(screen.getByRole("button", { name: "向下搜索" }));
    fireEvent.click(screen.getByRole("button", { name: "向下搜索" }));
    await waitFor(() => expect(searchStatus()).toHaveTextContent("1 / 4 · 已循环"));
  });

  it("supports case-sensitive search and manual chapter reset", async () => {
    render(<Harness />);
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const input = await screen.findByRole("textbox", { name: "全局搜索" });
    fireEvent.change(input, { target: { value: "Alpha" } });
    const searchStatus = () => within(screen.getByRole("search")).getByRole("status");
    await waitFor(() => expect(searchStatus()).toHaveTextContent("1 / 2"));
    fireEvent.click(screen.getByRole("button", { name: "区分大小写" }));
    await waitFor(() => expect(searchStatus()).toHaveTextContent("1 / 1"));
    selectChapter("第二章");
    expect(screen.getByRole("textbox", { name: "全局搜索" })).toHaveValue("Alpha");
    await waitFor(() => expect(searchStatus()).toHaveTextContent("— / 1"));
  });

  it("closes search on Escape before allowing the page back shortcut", async () => {
    const onBack = vi.fn();
    render(<Harness onBack={onBack} />);
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    await screen.findByRole("textbox", { name: "全局搜索" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "全局搜索" })).not.toBeInTheDocument();
    expect(onBack).not.toHaveBeenCalled();
  });

  it("toggles diff highlighting", async () => {
    render(<Harness />);
    await waitFor(() => expect(document.querySelectorAll(".diff-removed, .diff-added").length).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole("button", { name: "差异" }));
    expect(document.querySelector(".diff-removed, .diff-added")).not.toBeInTheDocument();
  });

  it("terminates an obsolete worker after chapter changes", async () => {
    const workers: Array<{ terminate: ReturnType<typeof vi.fn> }> = [];
    class WorkerMock {
      onmessage = null;
      onerror = null;
      terminate = vi.fn();
      postMessage() {}
      constructor() { workers.push(this); }
    }
    vi.stubGlobal("Worker", WorkerMock);
    render(<Harness />);
    await waitFor(() => expect(workers).toHaveLength(1));
    selectChapter("第二章");
    await waitFor(() => expect(workers).toHaveLength(2));
    expect(workers[0].terminate).toHaveBeenCalledOnce();
  });

  it("ignores an obsolete worker result after switching chapters", async () => {
    const workers: Array<{
      onmessage: ((event: MessageEvent) => void) | null;
      terminate: ReturnType<typeof vi.fn>;
      requestId?: number;
    }> = [];
    class WorkerMock {
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror = null;
      terminate = vi.fn();
      requestId?: number;
      postMessage(message: { requestId: number }) { this.requestId = message.requestId; }
      constructor() { workers.push(this); }
    }
    vi.stubGlobal("Worker", WorkerMock);
    render(<Harness />);
    await waitFor(() => expect(workers).toHaveLength(1));
    selectChapter("第二章");
    await waitFor(() => expect(workers).toHaveLength(2));
    workers[0].onmessage?.({
      data: {
        requestId: workers[0].requestId,
        result: { mode: "mixed", ranges: [{ side: "original", start: 0, end: 2, kind: "removed" }] }
      }
    } as MessageEvent);
    expect(screen.getByLabelText("原文内容")).toHaveTextContent("第二章也有目标");
    expect(screen.getByLabelText("原文内容").querySelector(".diff-removed")).toBeNull();
  });

  it("reuses a completed chapter diff from the LRU cache", async () => {
    const workers: Array<{ onmessage: ((event: MessageEvent) => void) | null; requestId?: number }> = [];
    class WorkerMock {
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror = null;
      terminate = vi.fn();
      requestId?: number;
      postMessage(message: { requestId: number }) {
        this.requestId = message.requestId;
        this.onmessage?.({ data: { requestId: message.requestId, result: { mode: "mixed", ranges: [] } } } as MessageEvent);
      }
      constructor() { workers.push(this); }
    }
    vi.stubGlobal("Worker", WorkerMock);
    render(<Harness />);
    await waitFor(() => expect(workers).toHaveLength(1));
    selectChapter("第二章");
    await waitFor(() => expect(workers).toHaveLength(2));
    selectChapter("第一章");
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("1. 第一章"));
    expect(workers).toHaveLength(2);
  });

  it("uses CSS Custom Highlight without expanding the text into diff nodes", async () => {
    const registry = new Map<string, unknown>();
    class HighlightMock {
      priority = 0;
      constructor(..._ranges: Range[]) {}
    }
    vi.stubGlobal("Highlight", HighlightMock);
    vi.stubGlobal("CSS", {
      highlights: {
        set: (name: string, value: unknown) => registry.set(name, value),
        delete: (name: string) => registry.delete(name)
      }
    });
    render(<Harness />);
    await waitFor(() => expect(registry.has("compare-original-removed")).toBe(true));
    expect(screen.getByLabelText("原文内容").querySelectorAll(".diff-removed, mark")).toHaveLength(0);
    expect(screen.getByLabelText("原文内容").children).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "全局搜索" }), { target: { value: "目标" } });
    await waitFor(() => expect(registry.has("compare-original-search")).toBe(true));
    expect(registry.has("compare-original-active")).toBe(true);
    expect(screen.getByLabelText("原文内容").children).toHaveLength(1);
  });
});
