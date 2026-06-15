import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { useAppStore } from "./store/appStore";
import type { AutoRunProgress } from "./useAutoRunProgress";
import type { AppSettings, JobEstimate, ModelProfile, Novel, NovelDetail } from "./types";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  progressCallback: undefined as ((progress: AutoRunProgress) => void) | undefined
}));

vi.mock("./tauriApi", () => ({ invokeCommand: mocks.invoke }));
vi.mock("./useAutoRunProgress", () => ({
  useAutoRunProgress: (_novelId: string | null, callback: (progress: AutoRunProgress) => void) => {
    mocks.progressCallback = callback;
  }
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(async () => vi.fn()) })
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const novels: Novel[] = [
  { id: "novel-1", title: "测试小说", source_path: "a.txt", encoding: "UTF-8", status: "imported", created_at: "now" },
  { id: "novel-2", title: "第二本", source_path: "b.txt", encoding: "UTF-8", status: "imported", created_at: "now" }
];

const profile: ModelProfile = {
  id: "profile-1",
  name: "测试模型",
  provider: "openai-compatible",
  base_url: "https://example.com/v1",
  model: "test-model",
  temperature: 0.7,
  thinking_mode: "auto",
  has_api_key: true,
  api_key_storage: "system",
  updated_at: "now"
};

const detail: NovelDetail = {
  novel: novels[0],
  chapters: [
    {
      id: "chapter-1",
      novel_id: "novel-1",
      index: 1,
      title: "第一章",
      original_text: "原文内容",
      rewrite_text: "改写内容",
      analysis_status: "completed",
      rewrite_status: "completed"
    }
  ],
  canon_assets: [],
  batches: [
    { id: "batch-1", novel_id: "novel-1", batch_index: 1, label: "第一批", start_chapter: 1, end_chapter: 1, file_path: "1.txt", created_at: "now" },
    { id: "batch-2", novel_id: "novel-1", batch_index: 2, label: "第二批", start_chapter: 2, end_chapter: 2, file_path: "2.txt", created_at: "now" }
  ],
  settings: {
    novel_id: "novel-1",
    protagonist_name: "林明",
    rewritten_protagonist_name: "林茗",
    additional_feminize_names: "",
    bust: "平胸",
    body_type: "少女",
    rewrite_mode: "strict",
    advanced_settings: "",
    updated_at: "now"
  }
};

const settings: AppSettings = { review_enabled: false, rewrite_parallelism: 6 };
const estimate: JobEstimate = {
  novel_chapters: 1,
  novel_chars: 4,
  novel_batches: 2,
  selected_batch_chapters: 1,
  selected_batch_chars: 4,
  parallelism: 6,
  review_enabled: false,
  current_batch_requests: 2,
  full_run_requests: 4,
  recent_success_calls: 0,
  recent_failed_calls: 0
};

function installDefaultCommands() {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === "list_novels") return novels;
    if (command === "list_model_profiles") return [profile];
    if (command === "get_app_settings") return settings;
    if (command === "get_novel_detail") return detail;
    if (command === "list_ai_logs") return [];
    if (command === "estimate_job_cost") return estimate;
    if (command === "check_for_updates") {
      return { current_version: "0.2.1", latest_version: "0.2.1", latest_tag: "v0.2.1", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
    }
    if (command === "start_analysis") {
      return { id: "job-1", novel_id: "novel-1", job_type: "analysis", status: "completed", current_chapter: 1, total_chapters: 1, message: "完成" };
    }
    if (command === "start_analyze_rewrite_all") {
      return { id: "job-auto", novel_id: "novel-1", job_type: "auto", status: "paused", current_chapter: 0, total_chapters: 1, message: "已暂停" };
    }
    if (command === "save_model_profile") return profile;
    if (command === "export_novel") return { path: "C:/exports/test.txt" };
    return undefined;
  });
}

describe("App feature behavior", () => {
  afterEach(cleanup);

  beforeEach(() => {
    useAppStore.getState().reset();
    mocks.invoke.mockReset();
    mocks.progressCallback = undefined;
    window.localStorage.setItem("yuri-rewrite.quick-start-seen", "true");
    installDefaultCommands();
  });

  it("loads the initial novel, model and batch", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "测试小说" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "当前批次" })).toHaveValue("batch-1");
    expect(screen.getAllByDisplayValue("test-model")).not.toHaveLength(0);
  });

  it("preserves the selected batch after an analysis refresh", async () => {
    render(<App />);
    const batchSelect = await screen.findByRole("combobox", { name: "当前批次" });
    fireEvent.change(batchSelect, { target: { value: "batch-2" } });
    const analysisButton = screen.getAllByRole("button", { name: "分析" }).find((button) => !button.hasAttribute("disabled"));
    fireEvent.click(analysisButton as HTMLElement);
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("start_analysis", expect.objectContaining({ batchId: "batch-2" })));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "当前批次" })).toHaveValue("batch-2"));
  });

  it("starts the full auto workflow from the selected batch through the end", async () => {
    render(<App />);
    const batchSelect = await screen.findByRole("combobox", { name: "当前批次" });
    fireEvent.change(batchSelect, { target: { value: "batch-2" } });
    fireEvent.click(screen.getByRole("button", { name: "一键分析改写选项" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "从当前批次开始一键分析改写" }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("start_analyze_rewrite_all", {
        novelId: "novel-1",
        profileId: "profile-1",
        startBatchId: "batch-2"
      })
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "继续" })).toBeEnabled());
  });

  it("saves a replacement API key and restores the mask", async () => {
    render(<App />);
    const apiKey = await screen.findByLabelText("API Key");
    fireEvent.focus(apiKey);
    fireEvent.change(apiKey, { target: { value: "replacement-secret" } });
    const modelPanel = screen.getByRole("heading", { name: "模型配置" }).closest("section");
    fireEvent.click(within(modelPanel as HTMLElement).getByRole("button", { name: "保存" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_model_profile", expect.objectContaining({ input: expect.objectContaining({ api_key: "replacement-secret" }) })));
    await waitFor(() => expect(screen.getByLabelText("API Key")).toHaveValue("********"));
  });

  it("locks conflicting controls while an auto job is active", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    act(() => {
      mocks.progressCallback?.({ id: "auto-1", novel_id: "novel-1", job_type: "auto", status: "running", current_chapter: 1, total_chapters: 10, message: "运行中" });
    });
    await waitFor(() => expect(screen.getByRole("button", { name: /导入 TXT/ })).toBeDisabled());
    expect(screen.getByRole("button", { name: "第二本" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "当前批次" })).toBeEnabled();
  });

  it("opens novel settings and exports from the compare view", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    expect(screen.getByRole("dialog", { name: "基本设定" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "关闭基本设定" }));
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    expect(screen.getByText("原文内容")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "TXT" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("export_novel", { novelId: "novel-1", format: "txt" }));
  });

  it("closes compare search before Escape returns to the workspace", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    await screen.findByRole("textbox", { name: "全局搜索" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("textbox", { name: "全局搜索" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "TXT" })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("heading", { name: "章节" })).toBeInTheDocument();
  });

  it("requires explicit confirmation and describes novel deletion scope", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    fireEvent.click(screen.getByRole("button", { name: "打开《测试小说》菜单" }));
    fireEvent.click(screen.getByRole("button", { name: "删除当前小说" }));

    const dialog = screen.getByRole("dialog", { name: "确认删除小说" });
    expect(within(dialog).getByText("该小说生成的审查警告日志")).toBeInTheDocument();
    expect(within(dialog).getByText("最初导入的原始 TXT 文件")).toBeInTheDocument();
    expect(within(dialog).getByText("已经导出到输出目录的改写 TXT 文件")).toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalledWith("delete_novel", expect.anything());

    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("dialog", { name: "确认删除小说" })).not.toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalledWith("delete_novel", expect.anything());

    fireEvent.click(screen.getByRole("button", { name: "打开《测试小说》菜单" }));
    fireEvent.click(screen.getByRole("button", { name: "删除当前小说" }));
    fireEvent.click(screen.getByRole("button", { name: "确认删除" }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("delete_novel", { novelId: "novel-1" })
    );
  });
});
