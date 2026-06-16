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
});
