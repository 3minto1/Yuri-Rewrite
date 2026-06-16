import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAutoRunProgress, type AutoRunProgress } from "./useAutoRunProgress";

let progressHandler: ((event: { payload: AutoRunProgress }) => void) | undefined;
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event: string, handler: (event: { payload: AutoRunProgress }) => void) => {
    progressHandler = handler;
    return unlisten;
  })
}));

function progress(overrides: Partial<AutoRunProgress> = {}): AutoRunProgress {
  return {
    id: "job-1",
    novel_id: "novel-1",
    job_type: "auto",
    status: "running",
    current_chapter: 0,
    total_chapters: 3,
    message: "running",
    ...overrides
  };
}

describe("useAutoRunProgress", () => {
  beforeEach(() => {
    progressHandler = undefined;
    unlisten.mockClear();
  });

  it("ignores other novels and stale job ids", async () => {
    const onProgress = vi.fn();
    renderHook(() => useAutoRunProgress("novel-1", onProgress));
    await waitFor(() => expect(progressHandler).toBeDefined());

    act(() => progressHandler?.({ payload: progress({ novel_id: "novel-2" }) }));
    expect(onProgress).not.toHaveBeenCalled();

    act(() => progressHandler?.({ payload: progress() }));
    act(() => progressHandler?.({ payload: progress({ id: "stale-job" }) }));
    expect(onProgress).toHaveBeenCalledTimes(1);
    expect(onProgress).toHaveBeenLastCalledWith(expect.objectContaining({ id: "job-1" }));
  });

  it("accepts a new job after the previous job reaches a terminal state", async () => {
    const onProgress = vi.fn();
    renderHook(() => useAutoRunProgress("novel-1", onProgress));
    await waitFor(() => expect(progressHandler).toBeDefined());

    act(() => progressHandler?.({ payload: progress({ status: "completed" }) }));
    act(() => progressHandler?.({ payload: progress({ status: "running" }) }));
    act(() => progressHandler?.({ payload: progress({ id: "job-2" }) }));
    expect(onProgress).toHaveBeenCalledTimes(2);
    expect(onProgress).toHaveBeenLastCalledWith(expect.objectContaining({ id: "job-2" }));
  });

  it("accepts a resumed job after a paused auto run", async () => {
    const onProgress = vi.fn();
    renderHook(() => useAutoRunProgress("novel-1", onProgress));
    await waitFor(() => expect(progressHandler).toBeDefined());

    act(() => progressHandler?.({ payload: progress({ id: "job-1", status: "paused", message: "限流暂停" }) }));
    act(() => progressHandler?.({ payload: progress({ id: "job-2", status: "running", message: "继续一键分析改写" }) }));

    expect(onProgress).toHaveBeenCalledTimes(2);
    expect(onProgress).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: "job-2", status: "running", message: "继续一键分析改写" })
    );
  });

  it("unsubscribes when the component unmounts", async () => {
    const hook = renderHook(() => useAutoRunProgress("novel-1", vi.fn()));
    await waitFor(() => expect(progressHandler).toBeDefined());
    hook.unmount();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
