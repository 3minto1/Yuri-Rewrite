import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelProfile, RewriteAbRunDetail } from "../../types";
import { RewriteAbStartDialog } from "./RewriteAbStartDialog";

const invoke = vi.fn();
vi.mock("../../tauriApi", () => ({ invokeCommand: (...args: unknown[]) => invoke(...args) }));

const profiles: ModelProfile[] = ["A", "B", "C"].map((name) => ({
  id: `profile-${name}`,
  name: `模型 ${name}`,
  provider: "openai-compatible",
  base_url: "https://example.com/v1",
  model: `model-${name}`,
  temperature: 0.7,
  top_p: 1,
  thinking_mode: "auto",
  prompt_obfuscation_enabled: false,
  has_api_key: true,
  api_key_storage: "system",
  updated_at: "2026-07-10"
}));

const startedRun = { id: "run-1", status: "ready" } as RewriteAbRunDetail;

describe("RewriteAbStartDialog", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((command: string) => {
      if (command === "list_rewrite_ab_runs") return Promise.resolve([]);
      if (command === "estimate_rewrite_ab") return Promise.resolve({
        chapter_count: 10,
        model_count: 2,
        shard_count: 4,
        estimated_requests: 8,
        estimated_seconds: 120,
        average_call_seconds: 15,
        recent_success_calls: 12
      });
      if (command === "start_rewrite_ab") return Promise.resolve(startedRun);
      throw new Error(command);
    });
  });

  it("requires distinct models, shows cost and supports an optional third model", async () => {
    render(<RewriteAbStartDialog
      novelId="novel-1"
      batchId="batch-1"
      batchLabel="第1批"
      profiles={profiles}
      defaultProfileId="profile-A"
      onCancel={vi.fn()}
      onOpenRun={vi.fn()}
      onStarted={vi.fn()}
      onNotice={vi.fn()}
      onTaskBusyChange={vi.fn()}
    />);

    expect(screen.getByText(/普通改写的 2 倍调用量/)).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /分别复检每个候选/ })).not.toBeChecked();
    await waitFor(() => expect(screen.getByText(/10 章 · 2 个模型 · 约 8 次请求/)).toBeInTheDocument());

    fireEvent.change(screen.getByRole("combobox", { name: "模型 B" }), { target: { value: "profile-A" } });
    expect(screen.getByRole("button", { name: "开始 A/B 改写" })).toBeDisabled();
    expect(screen.getByText(/请选择 2–3 个不同/)).toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "模型 B" }), { target: { value: "profile-B" } });
    fireEvent.click(screen.getByRole("button", { name: /增加模型 C/ }));
    expect(screen.getByRole("combobox", { name: "模型 C" })).toHaveValue("profile-C");
    expect(screen.getByText(/普通改写的 3 倍调用量/)).toBeInTheDocument();
  });

  it("closes into the progress surface and reports the completed run", async () => {
    const onCancel = vi.fn();
    const onStarted = vi.fn();
    const onTaskBusyChange = vi.fn();
    render(<RewriteAbStartDialog
      novelId="novel-1"
      batchId="batch-1"
      batchLabel="第1批"
      profiles={profiles}
      defaultProfileId="profile-A"
      onCancel={onCancel}
      onOpenRun={vi.fn()}
      onStarted={onStarted}
      onNotice={vi.fn()}
      onTaskBusyChange={onTaskBusyChange}
    />);

    await waitFor(() => expect(screen.getByRole("button", { name: "开始 A/B 改写" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "开始 A/B 改写" }));
    expect(onTaskBusyChange).toHaveBeenCalledWith(true);
    expect(onCancel).toHaveBeenCalled();
    await waitFor(() => expect(onStarted).toHaveBeenCalledWith(startedRun));
    expect(onTaskBusyChange).toHaveBeenLastCalledWith(false);
    expect(invoke).toHaveBeenCalledWith("start_rewrite_ab", expect.objectContaining({
      profileIds: ["profile-A", "profile-B"],
      reviewEnabled: false
    }));
  });

  it("uses the backend scope match when replacing a run whose batch id changed", async () => {
    const oldRun = {
      id: "run-old-scope",
      novel_id: "novel-1",
      batch_id: "old-batch-id",
      batch_label: "旧第1批",
      batch_fingerprint: "same-chapter-scope",
      status: "ready",
      review_enabled: false,
      model_count: 2,
      chapter_count: 10,
      completed_candidates: 20,
      total_candidates: 20,
      selected_chapters: 0,
      created_at: "2026-07-09",
      updated_at: "2026-07-09"
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_rewrite_ab_runs") return Promise.resolve([oldRun]);
      if (command === "estimate_rewrite_ab") return Promise.resolve({
        chapter_count: 10,
        model_count: 2,
        shard_count: 4,
        estimated_requests: 8,
        estimated_seconds: 120,
        average_call_seconds: 15,
        recent_success_calls: 12,
        existing_run_id: oldRun.id
      });
      if (command === "start_rewrite_ab") return Promise.resolve(startedRun);
      throw new Error(command);
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<RewriteAbStartDialog
      novelId="novel-1"
      batchId="new-batch-id"
      batchLabel="新第1批"
      profiles={profiles}
      defaultProfileId="profile-A"
      onCancel={vi.fn()}
      onOpenRun={vi.fn()}
      onStarted={vi.fn()}
      onNotice={vi.fn()}
      onTaskBusyChange={vi.fn()}
    />);

    await waitFor(() => expect(screen.getByRole("button", { name: "开始 A/B 改写" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "开始 A/B 改写" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_rewrite_ab", expect.objectContaining({
      batchId: "new-batch-id",
      replaceRunId: oldRun.id
    })));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("相同章节范围已有一次 A/B 实验"));
    confirm.mockRestore();
  });

  it("discovers the persisted running experiment before the long start command returns", async () => {
    let resolveStart!: (run: RewriteAbRunDetail) => void;
    let startPending = false;
    invoke.mockImplementation((command: string) => {
      if (command === "list_rewrite_ab_runs") return Promise.resolve(startPending ? [{
        id: "run-running",
        novel_id: "novel-1",
        batch_id: "batch-1",
        batch_label: "第1批",
        batch_fingerprint: "chapter-1",
        status: "running",
        review_enabled: false,
        model_count: 2,
        chapter_count: 1,
        completed_candidates: 0,
        total_candidates: 2,
        selected_chapters: 0,
        created_at: "2026-07-10",
        updated_at: "2026-07-10"
      }] : []);
      if (command === "estimate_rewrite_ab") return Promise.resolve({
        chapter_count: 1,
        model_count: 2,
        shard_count: 2,
        estimated_requests: 2,
        estimated_seconds: 30,
        average_call_seconds: 15,
        recent_success_calls: 3
      });
      if (command === "start_rewrite_ab") {
        startPending = true;
        return new Promise<RewriteAbRunDetail>((resolve) => { resolveStart = resolve; });
      }
      throw new Error(command);
    });
    const onOpenRun = vi.fn();
    const onStarted = vi.fn();
    render(<RewriteAbStartDialog
      novelId="novel-1"
      batchId="batch-1"
      batchLabel="第1批"
      profiles={profiles}
      defaultProfileId="profile-A"
      onCancel={vi.fn()}
      onOpenRun={onOpenRun}
      onStarted={onStarted}
      onNotice={vi.fn()}
      onTaskBusyChange={vi.fn()}
    />);
    await waitFor(() => expect(screen.getByRole("button", { name: "开始 A/B 改写" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "开始 A/B 改写" }));
    await waitFor(() => expect(onOpenRun).toHaveBeenCalledWith("run-running"), { timeout: 1200 });
    expect(onStarted).not.toHaveBeenCalled();
    resolveStart({ ...startedRun, id: "run-running" });
    await waitFor(() => expect(onStarted).toHaveBeenCalled());
  });
});
