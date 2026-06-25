import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { clearDiffCache } from "./components/Compare/compareDiffCache";
import { useAppStore } from "./store/appStore";
import type { AutoRunProgress } from "./useAutoRunProgress";
import type { AiLog, AppSettings, AutoRunRecovery, Job, JobEstimate, ModelProfile, Novel, NovelDetail, UpdateProgress } from "./types";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  progressCallback: undefined as ((progress: AutoRunProgress) => void) | undefined,
  updateCallback: undefined as ((progress: UpdateProgress) => void) | undefined
}));

vi.mock("./tauriApi", () => ({ invokeCommand: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_event: string, callback: (event: { payload: UpdateProgress }) => void) => {
    mocks.updateCallback = (payload: UpdateProgress) => callback({ payload });
    return vi.fn();
  })
}));
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
  top_p: 1,
  thinking_mode: "auto",
  prompt_obfuscation_enabled: false,
  has_api_key: true,
  api_key_storage: "system",
  updated_at: "now"
};

const secondProfile: ModelProfile = {
  ...profile,
  id: "profile-2",
  name: "备用模型",
  model: "second-model",
  updated_at: "later"
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
    protagonist_aliases: "",
    rewritten_protagonist_name: "林茗",
    additional_feminize_names: "",
    bust: "平胸",
    body_type: "少女",
    rewrite_mode: "strict",
    advanced_settings: "",
    relationship_targets: "[]",
    updated_at: "now"
  }
};

const settings: AppSettings = { review_enabled: false, chapter_batch_size: 30, rewrite_parallelism: 6 };
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
  estimated_full_run_seconds: 120,
  recent_success_calls: 0,
  recent_failed_calls: 0
};
let recoveryRows: AutoRunRecovery[] = [];

function installDefaultCommands() {
  mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "list_novels") return novels;
    if (command === "list_model_profiles") return [profile];
    if (command === "get_app_settings") return settings;
    if (command === "list_auto_run_recoveries") return recoveryRows;
    if (command === "get_novel_detail") return detail;
    if (command === "list_ai_logs") return [];
    if (command === "get_token_usage_stats") {
      return { start_date: "2026-05-21", end_date: "2026-06-19", requests: 0, input_tokens: 0, output_tokens: 0, models: [] };
    }
    if (command === "estimate_job_cost") return estimate;
    if (command === "check_for_updates") {
      return { current_version: "0.2.2", latest_version: "0.2.2", latest_tag: "v0.2.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
    }
    if (command === "start_analysis") {
      return { id: "job-1", novel_id: "novel-1", job_type: "analysis", status: "completed", current_chapter: 1, total_chapters: 1, message: "完成" };
    }
    if (command === "start_analyze_rewrite_all") {
      return { id: "job-auto", novel_id: "novel-1", job_type: "auto", status: "paused", current_chapter: 0, total_chapters: 1, message: "已暂停" };
    }
    if (command === "save_model_profile") return profile;
    if (command === "save_selected_profile_id") return settings;
    if (command === "update_chapter_title") {
      const payload = args as { chapterId: string; title: string } | undefined;
      return { ...detail.chapters[0], title: payload?.title ?? detail.chapters[0].title };
    }
    if (command === "export_novel") return { path: "C:/exports/test.txt" };
    return undefined;
  });
}

describe("App feature behavior", () => {
  afterEach(cleanup);

  beforeEach(() => {
    vi.useRealTimers();
    clearDiffCache();
    useAppStore.getState().reset();
    mocks.invoke.mockReset();
    mocks.progressCallback = undefined;
    mocks.updateCallback = undefined;
    recoveryRows = [];
    window.localStorage.setItem("yuri-rewrite.quick-start-seen", "true");
    window.localStorage.removeItem("yuri-rewrite.theme");
    delete document.documentElement.dataset.theme;
    installDefaultCommands();
  });

  it("loads the initial novel, model and batch", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "测试小说" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "当前批次" })).toHaveValue("batch-1");
    expect(screen.getByRole("combobox", { name: "改写模型" })).toHaveDisplayValue("测试模型");
    expect(screen.getByRole("option", { name: "测试模型" })).toHaveAttribute(
      "title",
      expect.stringContaining("模型名：test-model")
    );
    expect(screen.getAllByDisplayValue("test-model")).not.toHaveLength(0);
  });

  it("toggles and stores the local theme preference", async () => {
    render(<App />);
    const themeButton = await screen.findByRole("button", { name: "夜间模式" });

    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem("yuri-rewrite.theme")).toBe("light");

    fireEvent.click(themeButton);

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("yuri-rewrite.theme")).toBe("dark");
    expect(screen.getByRole("button", { name: "日间模式" })).toHaveAttribute("aria-pressed", "true");
  });

  it("confirms an available update and shows throttled download progress", async () => {
    let resolveDownload: ((value: unknown) => void) | undefined;
    const download = new Promise((resolve) => {
      resolveDownload = resolve;
    });
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "take_update_install_result") return null;
      if (command === "check_for_updates") {
        return {
          current_version: "0.3.10",
          latest_version: "0.3.11",
          latest_tag: "v0.3.11",
          is_latest: false,
          release_url: "https://github.com/3minto1/Yuri-Rewrite/releases/tag/v0.3.11",
          asset_name: "YuriRewrite-v0.3.11-windows-x64.zip",
          asset_download_url: "https://github.com/download.zip",
          asset_digest: `sha256:${"a".repeat(64)}`,
          asset_size: 10_000_000,
          auto_install_supported: true,
          auto_install_reason: null
        };
      }
      if (command === "download_latest_update") return download;
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: /检查更新/ }));
    expect(await screen.findByRole("button", { name: "下载并安装最新版" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "下载并安装最新版" }));
    expect(screen.getByRole("dialog", { name: "安装 Yuri Rewrite v0.3.11" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "下载并安装" }));

    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("download_latest_update"));
    act(() => {
      mocks.updateCallback?.({
        stage: "downloading",
        source: "国内镜像 1",
        downloaded_bytes: 5_000_000,
        total_bytes: 10_000_000,
        message: "正在从国内镜像 1 下载最新版：4.8 / 9.5 MB"
      });
    });
    expect(screen.getByText("正在从国内镜像 1 下载最新版：4.8 / 9.5 MB")).toBeInTheDocument();
    expect(screen.getByText("50%")).toBeInTheDocument();

    await act(async () => {
      resolveDownload?.({
        path: "C:/Users/test/Downloads/YuriRewrite-v0.3.11-windows-x64.zip",
        version: "0.3.11",
        install_started: false,
        manual_install_required: true,
        message: "未获得 GitHub SHA-256 摘要，仅保存为手动安装包，不能自动安装。"
      });
      await download;
    });
    expect(await screen.findByText(/已下载 v0.3.11/)).toBeInTheDocument();
    expect(screen.getByText(/未获得 GitHub SHA-256 摘要/)).toBeInTheDocument();
  });

  it("keeps the release-page fallback after automatic update download fails", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "take_update_install_result") return null;
      if (command === "check_for_updates") {
        return {
          current_version: "0.3.10",
          latest_version: "0.3.11",
          latest_tag: "v0.3.11",
          is_latest: false,
          release_url: "https://github.com/3minto1/Yuri-Rewrite/releases/tag/v0.3.11",
          asset_name: "YuriRewrite-v0.3.11-windows-x64.zip",
          asset_download_url: "https://github.com/download.zip",
          auto_install_supported: true
        };
      }
      if (command === "download_latest_update") {
        throw new Error("自动下载失败，请手动访问 GitHub 发布页下载 portable ZIP");
      }
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: /检查更新/ }));
    fireEvent.click(await screen.findByRole("button", { name: "下载并安装最新版" }));
    fireEvent.click(screen.getByRole("button", { name: "下载并安装" }));
    expect(await screen.findByText(/自动下载失败/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "查看发布页" })).toBeInTheDocument();
  });

  it("switches the workspace content between the main panels and canon assets", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    expect(screen.getByRole("heading", { name: "模型配置" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "章节" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "一致性资产" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "一致性资产" }));
    expect(screen.getByRole("heading", { name: "一致性资产" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "模型配置" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "返回工作台" }));
    expect(screen.getByRole("heading", { name: "模型配置" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "章节" })).toBeInTheDocument();
  });

  it("shows and saves protagonist aliases in novel settings", async () => {
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "save_novel_settings") {
        return {
          ...detail.settings,
          protagonist_aliases: (args as { protagonistAliases: string }).protagonistAliases,
          relationship_targets: (args as { relationshipTargets?: string }).relationshipTargets ?? "[]"
        };
      }
      if (command === "check_for_updates") {
        return { current_version: "0.3.3", latest_version: "0.3.3", latest_tag: "v0.3.3", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      }
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    fireEvent.change(screen.getByLabelText("主角别名（选填）"), {
      target: { value: "小明，林公子" }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_novel_settings", expect.objectContaining({
      protagonistAliases: "小明，林公子"
    })));
  });

  it.each([
    ["返回", () => fireEvent.click(screen.getByRole("button", { name: "返回" })), "章节"],
    ["设置", () => fireEvent.click(screen.getByRole("button", { name: "设置" })), "设置"],
    ["品牌返回", () => fireEvent.click(screen.getByRole("button", { name: /Yuri Rewrite/ })), "章节"],
    ["Esc", () => fireEvent.keyDown(window, { key: "Escape" }), "章节"]
  ])("guards unsaved novel settings before navigation: %s", async (_label, triggerNavigation, expectedHeading) => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    fireEvent.change(screen.getByLabelText("主角姓名（必填）"), {
      target: { value: "未保存主角" }
    });

    triggerNavigation();
    const dialog = screen.getByRole("dialog", { name: "基本设定尚未保存" });
    expect(within(dialog).getByText(/当前基本设定有未保存的修改/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "继续编辑" }));
    expect(screen.getByLabelText("主角姓名（必填）")).toHaveValue("未保存主角");

    triggerNavigation();
    fireEvent.click(within(screen.getByRole("dialog", { name: "基本设定尚未保存" })).getByRole("button", { name: "不保存退出" }));
    expect(screen.getByRole("heading", { name: expectedHeading })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    expect(screen.getByLabelText("主角姓名（必填）")).toHaveValue("林明");
  });

  it("saves dirty novel settings before leaving when requested", async () => {
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "save_novel_settings") {
        const payload = args as {
          protagonistName: string;
          relationshipTargets: string;
        };
        return {
          ...detail.settings,
          protagonist_name: payload.protagonistName,
          relationship_targets: payload.relationshipTargets,
          updated_at: "saved"
        };
      }
      if (command === "check_for_updates") {
        return { current_version: "0.3.3", latest_version: "0.3.3", latest_tag: "v0.3.3", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      }
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    fireEvent.change(screen.getByLabelText("主角姓名（必填）"), {
      target: { value: "保存主角" }
    });
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "基本设定尚未保存" })).getByRole("button", { name: "保存并退出" }));

    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_novel_settings", expect.objectContaining({
      protagonistName: "保存主角",
      relationshipTargets: "[]"
    })));
    expect(screen.getByRole("heading", { name: "设置" })).toBeInTheDocument();
  });

  it("restores an interrupted auto run as paused on startup", async () => {
    recoveryRows = [{
      novel_id: "novel-1",
      start_batch_index: 0,
      next_batch_index: 1,
      status: "paused",
      pause_reason: "软件意外关闭。",
      profile_ids: ["profile-1"],
      job: {
        id: "job-recovery",
        novel_id: "novel-1",
        job_type: "auto",
        status: "paused",
        current_chapter: 1,
        total_chapters: 2,
        message: "旧消息"
      }
    }];
    render(<App />);

    expect(await screen.findByRole("button", { name: "继续" })).toBeEnabled();
    expect(screen.getByText(/检测到上次未完成的一键任务，将继续处理第 2 批的未完成分片/)).toBeInTheDocument();
    expect(screen.getByText("未完成")).toBeInTheDocument();
  });

  it("shows the updated quick start help text", async () => {
    window.localStorage.removeItem("yuri-rewrite.quick-start-seen");
    render(<App />);

    const dialog = await screen.findByRole("dialog", { name: "快速上手" });
    expect(within(dialog).getByText(/建议先处理一个批次/)).toBeInTheDocument();
    expect(within(dialog).getByText(/限流\/网络中断后也可调整设置再继续/)).toBeInTheDocument();
    expect(within(dialog).getByText(/com\.local\.yurirewrite/)).toBeInTheDocument();
  });

  it("restores the last selected rewrite model from app settings", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile, secondProfile];
      if (command === "get_app_settings") return { ...settings, selected_profile_id: "profile-2" };
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "check_for_updates") {
        return { current_version: "0.2.2", latest_version: "0.2.2", latest_tag: "v0.2.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      }
      return undefined;
    });

    render(<App />);

    await waitFor(() => expect(screen.getByRole("combobox", { name: "改写模型" })).toHaveValue("profile-2"));
    expect(screen.getByRole("combobox", { name: "改写模型" })).toHaveDisplayValue("备用模型");
    expect(screen.getAllByDisplayValue("second-model")).not.toHaveLength(0);
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

  it("shows detailed progress and terminate-only controls for the current-batch auto task", async () => {
    let resolveBatchJob!: (job: Job) => void;
    const batchJob = new Promise<Job>((resolve) => {
      resolveBatchJob = resolve;
    });
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "start_analyze_rewrite_batch") return batchJob;
      if (command === "check_for_updates") {
        return { current_version: "0.3.1", latest_version: "0.3.1", latest_tag: "v0.3.1", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      }
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "一键分析改写当前批次" }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("start_analyze_rewrite_batch", {
        novelId: "novel-1",
        profileId: "profile-1",
        batchId: "batch-1"
      })
    );
    expect(screen.getByRole("button", { name: "终止" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "暂停" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "继续" })).not.toBeInTheDocument();

    act(() => {
      mocks.progressCallback?.({
        id: "auto-batch-1",
        novel_id: "novel-1",
        job_type: "auto_batch",
        status: "running",
        current_chapter: 0,
        total_chapters: 1,
        message: "第 1/1 批 · 分析 · 分片已完成 1/6",
        phase: "analysis",
        batch_index: 1,
        batch_total: 1,
        shard_completed: 1,
        shard_total: 6,
        chapter_completed: 40,
        chapter_total: 100,
        active_shards: [{
          index: 2,
          total: 6,
          start_chapter: 2,
          end_chapter: 2,
          phase: "analysis"
        }]
      });
    });

    expect(await screen.findByLabelText("一键分析改写进度 20%")).toBeInTheDocument();
    expect(screen.getByText(/章节 40\/100 · 分片 1\/6/)).toBeInTheDocument();
    expect(screen.getByText("2/6 第2章（分析）")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "暂停" })).not.toBeInTheDocument();

    await act(async () => {
      resolveBatchJob({
        id: "auto-batch-1",
        novel_id: "novel-1",
        job_type: "auto_batch",
        status: "terminated",
        current_chapter: 0,
        total_chapters: 1,
        message: "当前批次任务已终止"
      });
      await batchJob;
    });
    expect(await screen.findByText("terminated：当前批次任务已终止")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "终止" })).not.toBeInTheDocument()
    );
  });

  it("allows parallelism changes while an auto job is paused", async () => {
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile, secondProfile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "save_app_settings") return { ...settings, rewrite_parallelism: (args as { settings: AppSettings }).settings.rewrite_parallelism };
      if (command === "save_selected_profile_id") return { ...settings, selected_profile_id: "profile-2" };
      if (command === "check_for_updates") return { current_version: "0.2.2", latest_version: "0.2.2", latest_tag: "v0.2.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    act(() => {
      mocks.progressCallback?.({ id: "auto-1", novel_id: "novel-1", job_type: "auto", status: "paused", current_chapter: 0, total_chapters: 2, message: "限流暂停" });
    });

    await waitFor(() => expect(screen.getByRole("button", { name: "继续" })).toBeEnabled());
    fireEvent.change(screen.getByRole("combobox", { name: "改写模型" }), { target: { value: "profile-2" } });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_selected_profile_id", { profileId: "profile-2" }));

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("radio", { name: "50 章" })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: "3" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("save_app_settings", {
        settings: expect.objectContaining({ rewrite_parallelism: 3 })
      })
    );
  });

  it("enables high concurrency only for compatible batch sizes", async () => {
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "save_app_settings") {
        const requested = (args as { settings: AppSettings }).settings;
        return { ...settings, ...requested };
      }
      if (command === "check_for_updates") return { current_version: "0.2.2", latest_version: "0.2.2", latest_tag: "v0.2.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    expect(screen.getByRole("radio", { name: "25" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "50" })).toBeDisabled();

    expect(screen.getByRole("radio", { name: "10 章" })).toBeEnabled();
    fireEvent.click(screen.getByRole("radio", { name: "10 章" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_app_settings", {
      settings: expect.objectContaining({ chapter_batch_size: 10 })
    }));
    expect(screen.getByRole("radio", { name: "10" })).toBeEnabled();
    expect(screen.getByRole("radio", { name: "25" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "50" })).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: "50 章" }));
    await waitFor(() => expect(screen.getByRole("radio", { name: "25" })).toBeEnabled());
    expect(screen.getByRole("radio", { name: "50" })).toBeDisabled();

    fireEvent.click(screen.getByRole("radio", { name: "100 章" }));
    await waitFor(() => expect(screen.getByRole("radio", { name: "50" })).toBeEnabled());
    expect(mocks.invoke).toHaveBeenCalledWith("save_app_settings", {
      settings: expect.objectContaining({ chapter_batch_size: 100 })
    });
  });

  it("saves a separate analysis model while keeping the sidebar model as rewrite model", async () => {
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile, secondProfile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "save_app_settings") return { ...settings, ...(args as { settings: AppSettings }).settings };
      if (command === "check_for_updates") return { current_version: "0.3.2", latest_version: "0.3.2", latest_tag: "v0.3.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    expect(screen.getByText("改写模型")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.change(screen.getByTitle("选择独立分析模型；留空则使用左侧当前改写模型"), {
      target: { value: "profile-2" }
    });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("save_app_settings", {
      settings: expect.objectContaining({ analysis_profile_id: "profile-2" })
    }));
    expect(screen.getByText("已设置独立分析模型。")).toBeInTheDocument();
  });

  it("opens token statistics below logs and loads the selected date range", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "get_token_usage_stats") {
        return {
          start_date: "2026-05-21",
          end_date: "2026-06-19",
          requests: 2,
          input_tokens: 1000,
          output_tokens: 250,
          models: [{
            profile_id: "profile-1",
            profile_name: "测试模型",
            model: "test-model",
            requests: 2,
            input_tokens: 1000,
            output_tokens: 250,
            days: [{ date: "2026-06-19", requests: 2, input_tokens: 1000, output_tokens: 250 }]
          }]
        };
      }
      if (command === "check_for_updates") return { current_version: "0.3.2", latest_version: "0.3.2", latest_tag: "v0.3.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "Token统计" }));
    expect(await screen.findByRole("heading", { name: "Token 统计" })).toBeInTheDocument();
    expect(screen.getAllByText("test-model").length).toBeGreaterThan(0);
    expect(mocks.invoke).toHaveBeenCalledWith("get_token_usage_stats", expect.objectContaining({
      startDate: expect.any(String),
      endDate: expect.any(String)
    }));
  });

  it("rolls the default token statistics range forward after the app crosses midnight", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(new Date("2026-06-22T12:00:00+08:00"));
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    fireEvent.click(screen.getByRole("button", { name: "Token统计" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("get_token_usage_stats", {
      startDate: "2026-05-24",
      endDate: "2026-06-22"
    }));
    fireEvent.click(screen.getByRole("button", { name: "返回" }));

    vi.setSystemTime(new Date("2026-06-23T00:10:00+08:00"));
    fireEvent.click(screen.getByRole("button", { name: "Token统计" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("get_token_usage_stats", {
      startDate: "2026-05-25",
      endDate: "2026-06-23"
    }));
  });

  it("refreshes token statistics once at task terminal state instead of on every progress event", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "Token统计" }));
    await waitFor(() => expect(
      mocks.invoke.mock.calls.filter(([command]) => command === "get_token_usage_stats")
    ).toHaveLength(1));

    act(() => mocks.progressCallback?.({
      id: "job-auto-batch",
      novel_id: "novel-1",
      job_type: "auto_batch",
      status: "running",
      current_chapter: 0,
      total_chapters: 1,
      message: "分片 1/10"
    }));
    act(() => mocks.progressCallback?.({
      id: "job-auto-batch",
      novel_id: "novel-1",
      job_type: "auto_batch",
      status: "running",
      current_chapter: 0,
      total_chapters: 1,
      message: "分片 9/10"
    }));
    expect(
      mocks.invoke.mock.calls.filter(([command]) => command === "get_token_usage_stats")
    ).toHaveLength(1);

    act(() => mocks.progressCallback?.({
      id: "job-auto-batch",
      novel_id: "novel-1",
      job_type: "auto_batch",
      status: "completed",
      current_chapter: 1,
      total_chapters: 1,
      message: "完成"
    }));
    await waitFor(() => expect(
      mocks.invoke.mock.calls.filter(([command]) => command === "get_token_usage_stats")
    ).toHaveLength(2));

    act(() => mocks.progressCallback?.({
      id: "job-auto-batch",
      novel_id: "novel-1",
      job_type: "auto_batch",
      status: "completed",
      current_chapter: 1,
      total_chapters: 1,
      message: "重复终态"
    }));
    expect(
      mocks.invoke.mock.calls.filter(([command]) => command === "get_token_usage_stats")
    ).toHaveLength(2);
  });

  it("removes the repeated topbar from secondary operational pages", async () => {
    const { container } = render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    expect(container.querySelector(".topbar")).not.toBeNull();

    for (const pageName of ["对比", "日志", "Token统计", "设置"]) {
      fireEvent.click(screen.getByRole("button", { name: pageName }));
      await waitFor(() => expect(container.querySelector(".topbar")).toBeNull());
      fireEvent.click(screen.getByRole("button", { name: "返回" }));
      await waitFor(() => expect(container.querySelector(".topbar")).not.toBeNull());
    }
  });

  it("uses two primary one-click actions and secondary manual actions", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    expect(screen.getByRole("button", { name: "一键分析改写" })).toHaveClass("action-primary");
    expect(screen.getByRole("button", { name: "一键分析改写当前批次" })).toHaveClass("action-primary");
    expect(screen.getByRole("button", { name: "分析" })).not.toHaveClass("action-primary");
    expect(screen.getByRole("button", { name: "改写" })).not.toHaveClass("action-primary");
  });

  it("rewrites one completed chapter from the compare page and replaces its text", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return detail;
      if (command === "list_ai_logs") return [];
      if (command === "estimate_job_cost") return estimate;
      if (command === "rewrite_single_chapter") {
        return {
          ...detail.chapters[0],
          rewrite_text: "单章重新生成后的正文",
          single_rewrite_original_available: true
        };
      }
      if (command === "restore_single_chapter_rewrite") {
        return {
          ...detail.chapters[0],
          single_rewrite_original_available: false
        };
      }
      if (command === "check_for_updates") return { current_version: "0.3.2", latest_version: "0.3.2", latest_tag: "v0.3.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    fireEvent.click(screen.getByRole("button", { name: "重写本章（原文）" }));
    fireEvent.change(screen.getByRole("textbox", { name: "单章重写补充要求" }), {
      target: { value: "强化情绪互动" }
    });
    fireEvent.click(screen.getByRole("button", { name: "确定改写" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("rewrite_single_chapter", {
      novelId: "novel-1",
      profileId: "profile-1",
      chapterId: "chapter-1",
      instructions: "强化情绪互动",
      sourceMode: "original"
    }));
    await waitFor(() => expect(useAppStore.getState().detail?.chapters[0].rewrite_text).toBe("单章重新生成后的正文"));
    await waitFor(() => expect(screen.getByLabelText("改写稿内容")).toHaveTextContent("单章重新生成后的正文"));
    expect(screen.getByText("已重新改写完成《第一章》。")).toBeInTheDocument();

    vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "恢复初稿" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("restore_single_chapter_rewrite", {
      chapterId: "chapter-1"
    }));
    await waitFor(() => expect(screen.getByRole("button", { name: "重写本章（原文）" })).toBeInTheDocument());
    expect(screen.getByLabelText("改写稿内容")).toHaveTextContent("改写内容");
    expect(screen.getByText("已恢复《第一章》的初稿。")).toBeInTheDocument();
  });

  it("refreshes the estimate immediately after parallelism and batch size changes", async () => {
    let currentSettings = { ...settings };
    let currentDetail = detail;
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return currentSettings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "get_novel_detail") return currentDetail;
      if (command === "list_ai_logs") return [];
      if (command === "save_app_settings") {
        const requested = (args as { settings: AppSettings }).settings;
        currentSettings = { ...currentSettings, ...requested };
        if (requested.chapter_batch_size === 50) {
          currentDetail = {
            ...detail,
            batches: [{
              ...detail.batches[0],
              id: "batch-rebuilt",
              label: "重新分批"
            }]
          };
        }
        return currentSettings;
      }
      if (command === "estimate_job_cost") {
        return {
          ...estimate,
          parallelism: currentSettings.rewrite_parallelism ?? 6,
          novel_batches: currentDetail.batches.length,
          estimated_current_batch_seconds:
            currentSettings.rewrite_parallelism === 10 ? 45 : 90
        };
      }
      if (command === "check_for_updates") {
        return { current_version: "0.3.1", latest_version: "0.3.1", latest_tag: "v0.3.1", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      }
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("radio", { name: "10" }));
    await waitFor(() => {
      const saveIndex = mocks.invoke.mock.calls.findIndex(
        ([command, args]) =>
          command === "save_app_settings"
          && (args as { settings: AppSettings }).settings.rewrite_parallelism === 10
      );
      expect(saveIndex).toBeGreaterThanOrEqual(0);
      expect(
        mocks.invoke.mock.calls
          .slice(saveIndex + 1)
          .some(([command]) => command === "estimate_job_cost")
      ).toBe(true);
    });
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(await screen.findByText("并发 10 · 复检关闭")).toBeInTheDocument();
    expect(screen.getByText("当前 45.0 秒 · 全文 2 分 0 秒")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(screen.getByRole("radio", { name: "50 章" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("estimate_job_cost", {
        novelId: "novel-1",
        batchId: "batch-rebuilt",
        profileId: "profile-1"
      })
    );
  });

  it("refreshes rewritten chapters after an auto batch finishes without changing pages", async () => {
    let currentDetail = detail;
    let currentLogs: AiLog[] = [];
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "get_novel_detail") return currentDetail;
      if (command === "list_novels") return novels;
      if (command === "list_model_profiles") return [profile];
      if (command === "get_app_settings") return settings;
      if (command === "list_auto_run_recoveries") return [];
      if (command === "list_ai_logs") return currentLogs;
      if (command === "estimate_job_cost") return estimate;
      if (command === "check_for_updates") return { current_version: "0.2.2", latest_version: "0.2.2", latest_tag: "v0.2.2", is_latest: true, release_url: "", asset_name: "", asset_download_url: "" };
      return undefined;
    });

    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    expect(screen.getByText("改写内容")).toBeInTheDocument();

    currentDetail = {
      ...detail,
      chapters: [
        {
          ...detail.chapters[0],
          rewrite_text: "第一批更新后的改写内容"
        }
      ]
    };
    currentLogs = [{
      id: "log-1",
      novel_id: "novel-1",
      profile_id: "profile-1",
      action: "批次改写",
      chapter_title: "第一批",
      status: "success",
      content: "第一批日志内容",
      reasoning: "",
      raw_response: "",
      created_at: "2026-06-16T00:00:00Z"
    }];
    act(() => {
      mocks.progressCallback?.({
        id: "auto-1",
        novel_id: "novel-1",
        job_type: "auto",
        status: "running",
        current_chapter: 1,
        total_chapters: 2,
        message: "已更新合并导出至第 1 批"
      });
    });

    await waitFor(() => expect(screen.getByText("第一批更新后的改写内容")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "TXT" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "日志" }));
    await waitFor(() => expect(screen.getAllByText("第一批日志内容").length).toBeGreaterThan(0));
  });

  it("updates auto run remaining time as batches complete", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    act(() => {
      mocks.progressCallback?.({
        id: "auto-1",
        novel_id: "novel-1",
        job_type: "auto",
        status: "running",
        current_chapter: 1,
        total_chapters: 2,
        message: "已更新合并导出至第 1 批"
      });
    });

    await waitFor(() => expect(screen.getByText(/预计剩余 1 分 0 秒/)).toBeInTheDocument());
  });

  it("shows detailed batch and active shard progress", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    act(() => {
      mocks.progressCallback?.({
        id: "auto-detail",
        novel_id: "novel-1",
        job_type: "auto",
        status: "running",
        current_chapter: 1,
        total_chapters: 10,
        message: "详细进度",
        phase: "review",
        batch_index: 2,
        batch_total: 10,
        shard_completed: 3,
        shard_total: 6,
        chapter_completed: 40,
        chapter_total: 100,
        active_shards: [{
          index: 4,
          total: 6,
          start_chapter: 40,
          end_chapter: 42,
          phase: "review"
        }]
      });
    });

    expect(await screen.findByText(/第 2\/10 批 · 审查 · 章节 40\/100 · 分片 3\/6/)).toBeInTheDocument();
    expect(screen.getByText(/4\/6 第40-42章（审查）/)).toBeInTheDocument();
  });

  it("limits active shard details to three entries", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    act(() => {
      mocks.progressCallback?.({
        id: "auto-many-shards",
        novel_id: "novel-1",
        job_type: "auto",
        status: "running",
        current_chapter: 0,
        total_chapters: 2,
        message: "多个分片",
        phase: "rewrite",
        batch_index: 1,
        batch_total: 2,
        shard_completed: 0,
        shard_total: 6,
        active_shards: [1, 2, 3, 4].map((index) => ({
          index,
          total: 6,
          start_chapter: index,
          end_chapter: index,
          phase: "rewrite"
        }))
      });
    });

    expect(await screen.findByText("1/6 第1章（改写）")).toBeInTheDocument();
    expect(screen.getByText("3/6 第3章（改写）")).toBeInTheDocument();
    expect(screen.queryByText("4/6 第4章（改写）")).not.toBeInTheDocument();
    expect(screen.getByText("另有 1 个处理中")).toBeInTheDocument();
  });

  it.each(["completed", "paused", "failed"])(
    "refreshes task estimate when auto job becomes %s",
    async (status) => {
      render(<App />);
      await screen.findByRole("heading", { name: "测试小说" });
      await waitFor(() =>
        expect(mocks.invoke.mock.calls.some(([command]) => command === "estimate_job_cost")).toBe(true)
      );
      const estimateCallsBefore = mocks.invoke.mock.calls.filter(
        ([command]) => command === "estimate_job_cost"
      ).length;

      act(() => {
        mocks.progressCallback?.({
          id: `auto-${status}`,
          novel_id: "novel-1",
          job_type: "auto",
          status,
          current_chapter: status === "completed" ? 2 : 1,
          total_chapters: 2,
          message: status
        });
      });

      await waitFor(() =>
        expect(
          mocks.invoke.mock.calls.filter(([command]) => command === "estimate_job_cost").length
        ).toBeGreaterThan(estimateCallsBefore)
      );
    }
  );

  it("opens novel settings and exports from the compare view", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "设定" }));
    expect(screen.getByRole("heading", { name: "基本设定" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "基本设定" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    expect(screen.getByText("原文内容")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "TXT" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("export_novel", { novelId: "novel-1", format: "txt" }));
  });

  it("keeps renamed chapter titles in sync with the compare chapter selector", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "第 1 章名称" }), {
      target: { value: "第一章 修改后" }
    });
    const titleSaveButton = screen
      .getAllByRole("button", { name: "保存" })
      .find((button) => button.className.includes("compact-button"));
    expect(titleSaveButton).toBeDefined();
    fireEvent.click(titleSaveButton!);

    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("update_chapter_title", {
      chapterId: "chapter-1",
      title: "第一章 修改后"
    }));
    await waitFor(() => expect(useAppStore.getState().detail?.chapters[0].title).toBe("第一章 修改后"));

    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    expect(screen.getByRole("combobox", { name: "章节" })).toHaveTextContent("1. 第一章 修改后");
    fireEvent.click(screen.getByRole("combobox", { name: "章节" }));
    expect(screen.getByRole("option", { name: /1\. 第一章 修改后/ })).toBeInTheDocument();
  });

  it.each([
    ["设置", () => fireEvent.click(screen.getByRole("button", { name: "设置" })), "设置"],
    ["日志", () => fireEvent.click(screen.getByRole("button", { name: "日志" })), "AI 调用日志"],
    ["品牌返回", () => fireEvent.click(screen.getByRole("button", { name: /Yuri Rewrite/ })), "章节"],
    ["Esc 返回", () => fireEvent.keyDown(window, { key: "Escape" }), "章节"]
  ])("guards unsaved compare edits before app-level navigation: %s", async (_label, triggerNavigation, expectedHeading) => {
    render(<App />);
    await screen.findByRole("heading", { name: "测试小说" });
    fireEvent.click(screen.getByRole("button", { name: "对比" }));
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByRole("textbox", { name: "编辑改写稿正文" }), {
      target: { value: "尚未保存的 App 级导航草稿" }
    });

    triggerNavigation();
    const dialog = screen.getByRole("dialog", { name: "改写稿尚未保存" });
    expect(within(dialog).getByText(/离开对比页面会放弃这些修改/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "继续编辑" }));
    expect(screen.getByRole("textbox", { name: "编辑改写稿正文" })).toHaveValue("尚未保存的 App 级导航草稿");

    triggerNavigation();
    fireEvent.click(screen.getByRole("button", { name: "放弃并离开" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: expectedHeading })).toBeInTheDocument());
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
