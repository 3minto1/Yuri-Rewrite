import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChapterList, CHAPTER_VIRTUALIZATION_THRESHOLD } from "./ChapterList";
import type { Chapter } from "../../types";

function chapters(count: number): Chapter[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `chapter-${index + 1}`,
    novel_id: "novel-1",
    index: index + 1,
    title: `第${index + 1}章`,
    original_text: "text",
    analysis_status: "pending",
    rewrite_status: "pending"
  }));
}

const statusText = { pending: "待处理", completed: "完成" };
const displayTitle = (chapter: Chapter) => chapter.title;

describe("ChapterList", () => {
  afterEach(cleanup);

  it("keeps the normal DOM list below the threshold", () => {
    const rows = chapters(CHAPTER_VIRTUALIZATION_THRESHOLD - 1);
    render(<ChapterList chapters={rows} selectedChapterId="chapter-1" onSelect={vi.fn()} displayTitle={displayTitle} statusText={statusText} />);
    expect(screen.getAllByRole("button").filter((button) => button.className.includes("chapter-item"))).toHaveLength(rows.length);
    expect(screen.getAllByText("分析 待处理")[0].closest(".status-badge")).toHaveClass("status-neutral");
    expect(screen.getAllByText("改写 待处理")[0].closest(".status-badge")).toHaveClass("status-neutral");
  });

  it("shows independent analysis and rewrite status tones", () => {
    const rows = [{
      ...chapters(1)[0],
      analysis_status: "completed",
      rewrite_status: "running"
    }];
    render(<ChapterList chapters={rows} selectedChapterId="chapter-1" onSelect={vi.fn()} displayTitle={displayTitle} statusText={{ ...statusText, running: "进行中" }} />);

    expect(screen.getByText("分析 完成").closest(".status-badge")).toHaveClass("status-success");
    expect(screen.getByText("改写 进行中").closest(".status-badge")).toHaveClass("status-progress");
    expect(screen.getByRole("button", { name: /第1章/ })).toHaveAttribute("title", "1. 第1章");
  });

  it("virtualizes at the threshold and for very large novels", () => {
    const rows = chapters(3_000);
    render(<ChapterList chapters={rows} selectedChapterId="chapter-1" onSelect={vi.fn()} displayTitle={displayTitle} statusText={statusText} />);
    expect(screen.getAllByRole("button").length).toBeLessThan(CHAPTER_VIRTUALIZATION_THRESHOLD);
    expect(screen.getByText("1. 第1章")).toBeInTheDocument();
  });

  it("selects rows and scrolls a distant selected chapter into view", async () => {
    const onSelect = vi.fn();
    const rows = chapters(3_000);
    const view = render(<ChapterList chapters={rows} selectedChapterId="chapter-1" onSelect={onSelect} displayTitle={displayTitle} statusText={statusText} />);
    fireEvent.click(screen.getByText("1. 第1章"));
    expect(onSelect).toHaveBeenCalledWith("chapter-1");
    view.rerender(<ChapterList chapters={rows} selectedChapterId="chapter-3000" onSelect={onSelect} displayTitle={displayTitle} statusText={statusText} />);
    await waitFor(() => expect(screen.getByText("3000. 第3000章")).toBeInTheDocument());
  });

  it("filters to a chapter by number and selects it with Enter", () => {
    const onSelect = vi.fn();
    const rows = chapters(3_000);
    render(<ChapterList chapters={rows} selectedChapterId="chapter-1" onSelect={onSelect} displayTitle={displayTitle} statusText={statusText} />);

    fireEvent.change(screen.getByRole("textbox", { name: "搜索章节" }), { target: { value: "250" } });
    expect(screen.getByText((_, node) => node?.textContent === "250. 第250章")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "跳转" })).not.toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("textbox", { name: "搜索章节" }), { key: "Enter" });

    expect(onSelect).toHaveBeenCalledWith("chapter-250");
  });

  it("edits chapter titles and saves only changed rows", async () => {
    const onRenameChapter = vi.fn(async () => undefined);
    const rows = chapters(3);
    render(
      <ChapterList
        chapters={rows}
        selectedChapterId="chapter-1"
        onSelect={vi.fn()}
        displayTitle={displayTitle}
        statusText={statusText}
        onRenameChapter={onRenameChapter}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "第 2 章名称" }), {
      target: { value: "第二章 新标题" }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onRenameChapter).toHaveBeenCalledWith("chapter-2", "第二章 新标题"));
    expect(onRenameChapter).toHaveBeenCalledTimes(1);
  });

  it("hides analysis and rewrite status badges while editing titles", () => {
    const rows = chapters(2);
    render(
      <ChapterList
        chapters={rows}
        selectedChapterId="chapter-1"
        onSelect={vi.fn()}
        displayTitle={displayTitle}
        statusText={statusText}
        onRenameChapter={vi.fn()}
      />
    );

    expect(screen.getAllByText("分析 待处理")).toHaveLength(2);
    expect(screen.getAllByText("改写 待处理")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));

    expect(screen.queryByText("分析 待处理")).not.toBeInTheDocument();
    expect(screen.queryByText("改写 待处理")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "第 1 章名称" })).toHaveValue("第1章");
  });

  it("keeps editing mode and blocks empty chapter titles", async () => {
    const onRenameChapter = vi.fn(async () => undefined);
    const rows = chapters(2);
    render(
      <ChapterList
        chapters={rows}
        selectedChapterId="chapter-1"
        onSelect={vi.fn()}
        displayTitle={displayTitle}
        statusText={statusText}
        onRenameChapter={onRenameChapter}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "第 1 章名称" }), {
      target: { value: "   " }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(await screen.findByText("第 1 章名称不能为空。")).toBeInTheDocument();
    expect(onRenameChapter).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "第 1 章名称" })).toBeInTheDocument();
  });

  it("disables title editing when the parent reports an unsafe state", () => {
    render(
      <ChapterList
        chapters={chapters(1)}
        selectedChapterId="chapter-1"
        onSelect={vi.fn()}
        displayTitle={displayTitle}
        statusText={statusText}
        onRenameChapter={vi.fn()}
        titleEditDisabledReason="任务运行期间不能修改章节名称"
      />
    );

    const editButton = screen.getByRole("button", { name: "编辑" });
    expect(editButton).toBeDisabled();
    expect(editButton).toHaveAttribute("title", "任务运行期间不能修改章节名称");
  });
});
