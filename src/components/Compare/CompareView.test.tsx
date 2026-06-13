import { createRef, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Chapter } from "../../types";
import { CompareView } from "./CompareView";

const chapters: Chapter[] = [
  { id: "c1", novel_id: "n1", index: 1, title: "第一章", original_text: "Alpha 目标 原文", rewrite_text: "alpha 目标 改写", analysis_status: "completed", rewrite_status: "completed" },
  { id: "c2", novel_id: "n1", index: 2, title: "第二章", original_text: "第二章也有目标", rewrite_text: "最终目标", analysis_status: "completed", rewrite_status: "completed" }
];

function Harness({ onBack = vi.fn() }: { onBack?: () => void }) {
  const [selectedChapterId, setSelectedChapterId] = useState("c1");
  return (
    <CompareView
      chapters={chapters}
      selectedChapter={chapters.find((chapter) => chapter.id === selectedChapterId)}
      selectedChapterId={selectedChapterId}
      busy=""
      originalRef={createRef<HTMLDivElement>()}
      rewriteRef={createRef<HTMLDivElement>()}
      onSelectChapter={setSelectedChapterId}
      onBack={onBack}
      onExport={vi.fn()}
    />
  );
}

describe("CompareView", () => {
  beforeEach(() => {
    vi.stubGlobal("Worker", undefined);
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
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveValue("c2"));
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
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c2" } });
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
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c2" } });
    await waitFor(() => expect(workers).toHaveLength(2));
    expect(workers[0].terminate).toHaveBeenCalledOnce();
  });
});
