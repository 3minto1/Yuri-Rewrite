import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RewriteAbChapterDetail, RewriteAbRunDetail } from "../../types";
import { RewriteAbView } from "./RewriteAbView";

const invoke = vi.fn();
const diffMocks = vi.hoisted(() => ({ useChapterDiff: vi.fn() }));
vi.mock("../../tauriApi", () => ({ invokeCommand: (...args: unknown[]) => invoke(...args) }));
vi.mock("./CompareView", () => ({
  useChapterDiff: (...args: unknown[]) => diffMocks.useChapterDiff(...args)
}));

const run: RewriteAbRunDetail = {
  id: "run-1",
  novel_id: "novel-1",
  batch_id: "batch-1",
  batch_label: "第1批",
  batch_fingerprint: "chapter-1:chapter-2",
  status: "ready",
  review_enabled: false,
  model_count: 2,
  chapter_count: 2,
  completed_candidates: 4,
  total_candidates: 4,
  selected_chapters: 0,
  created_at: "2026-07-10",
  updated_at: "2026-07-10",
  models: [
    { slot: "A", profile_id: "profile-a", profile_name: "模型甲", provider: "openai-compatible", model: "model-a" },
    { slot: "B", profile_id: "profile-b", profile_name: "模型乙", provider: "openai-compatible", model: "model-b" }
  ],
  chapters: [1, 2].map((index) => ({
    chapter_id: `chapter-${index}`,
    chapter_index: index,
    title: `第${index}章`,
    selected_slot: null,
    candidate_statuses: { A: "completed", B: "completed" }
  }))
};

function chapterDetail(id: string): RewriteAbChapterDetail {
  const index = Number(id.split("-")[1]);
  return {
    run_id: run.id,
    chapter_id: id,
    chapter_index: index,
    original_title: `第${index}章`,
    original_text: `第${index}章原文`,
    baseline_title: `第${index}章`,
    baseline_rewrite_text: `第${index}章基线`,
    selected_slot: null,
    candidates: run.models.map((model) => ({
      slot: model.slot,
      profile_id: model.profile_id,
      profile_name: model.profile_name,
      model: model.model,
      status: "completed",
      title: `第${index}章`,
      rewrite_text: `${model.slot}候选第${index}章`,
      review_summary: null,
      error: null
    }))
  };
}

describe("RewriteAbView", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });
  beforeEach(() => {
    invoke.mockReset();
    diffMocks.useChapterDiff.mockReset();
    diffMocks.useChapterDiff.mockImplementation((chapterId: string, original: string, rewrite: string) => ({
      chapterId,
      original,
      rewrite,
      loading: false,
      mode: "mixed",
      ranges: [],
      error: null
    }));
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "list_rewrite_ab_runs") return Promise.resolve([{ ...run, models: undefined, chapters: undefined }]);
      if (command === "get_rewrite_ab_run") return Promise.resolve(structuredClone(run));
      if (command === "get_rewrite_ab_chapter") return Promise.resolve(chapterDetail(String(args.chapterId)));
      if (command === "save_rewrite_ab_choices") {
        const choices = args.choices as Array<{ chapter_id: string; slot: "A" | "B" }>;
        return Promise.resolve({
          ...structuredClone(run),
          selected_chapters: choices.length,
          chapters: run.chapters.map((chapter) => ({
            ...chapter,
            selected_slot: choices.find((choice) => choice.chapter_id === chapter.chapter_id)?.slot ?? null
          }))
        });
      }
      if (command === "apply_rewrite_ab_choices") return Promise.resolve({ status: "applied", conflict_chapter_ids: [], chapters: [] });
      throw new Error(command);
    });
  });

  it("loads only the selected chapter and switches between original-based and pair differences", async () => {
    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    await screen.findByText("A候选第1章");
    expect(invoke).toHaveBeenCalledWith("get_rewrite_ab_chapter", { runId: "run-1", chapterId: "chapter-1" });
    expect(invoke).not.toHaveBeenCalledWith("get_rewrite_ab_chapter", { runId: "run-1", chapterId: "chapter-2" });
    expect(diffMocks.useChapterDiff).toHaveBeenCalledWith(
      "run-1:chapter-1:original:A",
      "第1章原文",
      "A候选第1章",
      true
    );

    diffMocks.useChapterDiff.mockClear();
    fireEvent.change(screen.getByRole("combobox", { name: "左侧对比内容" }), { target: { value: "A" } });
    fireEvent.change(screen.getByRole("combobox", { name: "右侧对比内容" }), { target: { value: "B" } });
    expect(screen.getByText("A候选第1章")).toBeInTheDocument();
    expect(screen.getByText("B候选第1章")).toBeInTheDocument();
    await waitFor(() => {
      expect(diffMocks.useChapterDiff).toHaveBeenCalledWith(
        "run-1:chapter-1:original:A",
        "第1章原文",
        "A候选第1章",
        true
      );
      expect(diffMocks.useChapterDiff).toHaveBeenCalledWith(
        "run-1:chapter-1:original:B",
        "第1章原文",
        "B候选第1章",
        true
      );
    });
    expect(screen.getByText(/两栏分别高亮相对原文/)).toBeInTheDocument();

    diffMocks.useChapterDiff.mockClear();
    fireEvent.change(screen.getByRole("combobox", { name: "差异基准" }), { target: { value: "pair" } });
    await waitFor(() => expect(diffMocks.useChapterDiff).toHaveBeenCalledWith(
      "run-1:chapter-1:pair:A:B",
      "A候选第1章",
      "B候选第1章",
      true
    ));

    diffMocks.useChapterDiff.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "差异" }));
    await waitFor(() => {
      expect(diffMocks.useChapterDiff.mock.calls.length).toBeGreaterThanOrEqual(3);
      expect(diffMocks.useChapterDiff.mock.calls.every((call) => call[3] === false)).toBe(true);
    });
  });

  it("reports the last viewed experiment to the persistent app entry", async () => {
    const secondRun: RewriteAbRunDetail = {
      ...structuredClone(run),
      id: "run-2",
      batch_id: "batch-2",
      batch_label: "第2批",
      created_at: "2026-07-11",
      updated_at: "2026-07-11"
    };
    const baseImplementation = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "list_rewrite_ab_runs") return Promise.resolve([run, secondRun]);
      if (command === "get_rewrite_ab_run" && args.runId === "run-2") return Promise.resolve(structuredClone(secondRun));
      return baseImplementation(command, args);
    });
    const onRunChange = vi.fn();

    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onRunChange={onRunChange} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    await screen.findByText("A候选第1章");
    fireEvent.change(screen.getByRole("combobox", { name: "A/B 实验" }), { target: { value: "run-2" } });

    await waitFor(() => expect(onRunChange).toHaveBeenLastCalledWith("run-2"));
    expect(invoke).toHaveBeenCalledWith("get_rewrite_ab_run", { runId: "run-2" });
  });

  it("refreshes each model progress while the experiment is running", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const runningRun: RewriteAbRunDetail = {
      ...structuredClone(run),
      status: "running",
      completed_candidates: 1,
      chapters: run.chapters.map((item, index) => ({
        ...item,
        candidate_statuses: {
          A: index === 0 ? "completed" : "running",
          B: "running"
        }
      }))
    };
    const finishedRun = structuredClone(run);
    let refreshed = false;
    const baseImplementation = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_rewrite_ab_run") {
        return Promise.resolve(structuredClone(refreshed ? finishedRun : runningRun));
      }
      if (command === "list_rewrite_ab_runs") {
        return Promise.resolve([structuredClone(refreshed ? finishedRun : runningRun)]);
      }
      return baseImplementation(command, args);
    });

    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    expect(await screen.findByRole("progressbar", { name: "候选 A 模型甲 进度" })).toHaveAttribute("aria-valuenow", "50");
    expect(screen.getByRole("progressbar", { name: "候选 B 模型乙 进度" })).toHaveAttribute("aria-valuenow", "0");

    refreshed = true;
    await act(async () => {
      vi.advanceTimersByTime(1_500);
      await Promise.resolve();
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByRole("progressbar", { name: "候选 A 模型甲 进度" })).toHaveAttribute("aria-valuenow", "100");
      expect(screen.getByRole("progressbar", { name: "候选 B 模型乙 进度" })).toHaveAttribute("aria-valuenow", "100");
    });
  });

  it("supports bulk choice without applying candidates automatically", async () => {
    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    await screen.findByText("A候选第1章");
    fireEvent.click(screen.getByRole("button", { name: "整批采用 B" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_rewrite_ab_choices", {
      runId: "run-1",
      replaceAll: true,
      choices: [
        { chapter_id: "chapter-1", slot: "B" },
        { chapter_id: "chapter-2", slot: "B" }
      ]
    }));
    expect(invoke).not.toHaveBeenCalledWith("apply_rewrite_ab_choices", expect.anything());
  });

  it("disables a bulk slot that does not cover every chapter", async () => {
    const incompleteRun: RewriteAbRunDetail = {
      ...structuredClone(run),
      status: "partial",
      completed_candidates: 3,
      total_candidates: 6,
      model_count: 3,
      models: [
        ...structuredClone(run.models),
        { slot: "C" as const, profile_id: "profile-c", profile_name: "模型丙", provider: "openai-compatible", model: "model-c" }
      ],
      chapters: run.chapters.map((chapter, index) => ({
        ...chapter,
        candidate_statuses: {
          ...chapter.candidate_statuses,
          B: index === 0 ? "completed" : "failed",
          C: "failed"
        }
      }))
    };
    const baseImplementation = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_rewrite_ab_run") return Promise.resolve(structuredClone(incompleteRun));
      if (command === "list_rewrite_ab_runs") return Promise.resolve([incompleteRun]);
      return baseImplementation(command, args);
    });

    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    await screen.findByText("A候选第1章");

    expect(screen.getByRole("progressbar", { name: "候选 A 模型甲 进度" })).toHaveAttribute("aria-valuenow", "100");
    expect(screen.getByRole("progressbar", { name: "候选 B 模型乙 进度" })).toHaveAttribute("aria-valuenow", "50");
    expect(screen.getByRole("progressbar", { name: "候选 B 模型乙 进度" })).toHaveAttribute(
      "aria-valuetext",
      "已完成 1/2 章，失败 1 章"
    );
    expect(screen.getByRole("progressbar", { name: "候选 C 模型丙 进度" })).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getByRole("progressbar", { name: "候选 C 模型丙 进度" })).toHaveAttribute(
      "aria-valuetext",
      "已完成 0/2 章，失败 2 章"
    );
    expect(screen.getAllByText("部分失败")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "整批采用 A" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "整批采用 B" }))
      .toBeDisabled();
    expect(screen.getByRole("button", { name: "整批采用 B" }))
      .toHaveAccessibleDescription("候选 B 尚未覆盖当前实验的所有章节，不能整批采用。");
  });

  it("warns before applying a mix of model styles", async () => {
    const mixedRun: RewriteAbRunDetail = {
      ...structuredClone(run),
      selected_chapters: 2,
      chapters: run.chapters.map((chapter, index) => ({ ...chapter, selected_slot: index === 0 ? "A" : "B" }))
    };
    const baseImplementation = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args: Record<string, unknown>) => {
      if (command === "get_rewrite_ab_run") return Promise.resolve(structuredClone(mixedRun));
      if (command === "list_rewrite_ab_runs") return Promise.resolve([mixedRun]);
      return baseImplementation(command, args);
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<RewriteAbView novelId="novel-1" initialRunId="run-1" onBack={vi.fn()} onNovelChanged={vi.fn().mockResolvedValue(undefined)} onNotice={vi.fn()} />);
    await screen.findByText("A候选第1章");
    fireEvent.click(screen.getByRole("button", { name: "应用所选" }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("文风连续性差异"));
    expect(invoke).not.toHaveBeenCalledWith("apply_rewrite_ab_choices", expect.anything());
    confirm.mockRestore();
  });
});
