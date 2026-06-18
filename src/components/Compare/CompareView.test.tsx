import { createRef, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Chapter } from "../../types";
import { clearDiffCache } from "./compareDiffCache";
import { CompareView } from "./CompareView";

const chapters: Chapter[] = [
  { id: "c1", novel_id: "n1", index: 1, title: "第一章", original_text: "Alpha 目标 原文", rewrite_text: "alpha 目标 改写", analysis_status: "completed", rewrite_status: "completed" },
  { id: "c2", novel_id: "n1", index: 2, title: "第二章", original_text: "第二章也有目标", rewrite_text: "最终目标", analysis_status: "completed", rewrite_status: "completed" }
];

function Harness({ onBack = vi.fn() }: { onBack?: () => void }) {
  const [chapterRows, setChapterRows] = useState(chapters);
  const [selectedChapterId, setSelectedChapterId] = useState("c1");
  return (
    <CompareView
      chapters={chapterRows}
      selectedChapter={chapterRows.find((chapter) => chapter.id === selectedChapterId)}
      selectedChapterId={selectedChapterId}
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
    />
  );
}

describe("CompareView", () => {
  beforeEach(() => {
    clearDiffCache();
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

  it("asks before navigating away from unsaved edits", async () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "尚未保存的正文" }
    });
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c2" } });

    expect(screen.getByRole("dialog", { name: "改写稿尚未保存" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "章节" })).toHaveValue("c1");
    fireEvent.click(screen.getByRole("button", { name: "放弃修改" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveValue("c2"));
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
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c2" } });
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
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c2" } });
    await waitFor(() => expect(workers).toHaveLength(2));
    fireEvent.change(screen.getByRole("combobox", { name: "章节" }), { target: { value: "c1" } });
    await waitFor(() => expect(screen.getByRole("combobox", { name: "章节" })).toHaveValue("c1"));
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
