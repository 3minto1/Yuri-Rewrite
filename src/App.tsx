import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  BookOpen,
  ChartNoAxesCombined,
  CheckCircle2,
  ChevronDown,
  ClipboardList,
  Download,
  FilePlus2,
  Github,
  HelpCircle,
  Loader2,
  MoreHorizontal,
  Pause,
  Play,
  RefreshCw,
  Save,
  Settings,
  Sparkles,
  Square,
  Trash2,
  X
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CompareView } from "./components/Compare/CompareView";
import { DeleteNovelDialog } from "./components/common/DeleteNovelDialog";
import { Modal } from "./components/common/Modal";
import { getStatusTone, StatusBadge } from "./components/common/StatusBadge";
import { UpdateInstallDialog } from "./components/common/UpdateInstallDialog";
import { ModelProfiles } from "./components/Settings/ModelProfiles";
import { NovelSettingsFields, NovelSettingsView } from "./components/Settings/NovelSettings";
import { CoreSettingsPage } from "./components/pages/CoreSettingsPage";
import { LogsPage } from "./components/pages/LogsPage";
import { SettingsPage } from "./components/pages/SettingsPage";
import { TokenStatsPage } from "./components/pages/TokenStatsPage";
import { BatchPanel } from "./components/Workspace/BatchPanel";
import { ChapterList } from "./components/Workspace/ChapterList";
import { ModelConfig } from "./components/Workspace/ModelConfig";
import { TaskEstimate } from "./components/Workspace/TaskEstimate";
import {
  emptyProfile as defaultProfile,
  getModelSuggestions as detectModelSuggestions,
  normalizeThinkingMode
} from "./config/modelRecommendations";
import { useModelProfiles } from "./hooks/useModelProfiles";
import { useNovels } from "./hooks/useNovels";
import { useNotice } from "./hooks/useNotice";
import { useTaskState } from "./hooks/useTaskState";
import { useAppStore } from "./store/appStore";
import { invokeCommand as invoke } from "./tauriApi";
import type {
  AiLog,
  AppSettings,
  AutoRunRecovery,
  CanonAsset,
  Chapter,
  DiagnosisStatus,
  Job,
  JobEstimate,
  ModelDiagnosis,
  ModelProfile,
  Novel,
  NovelDetail,
  NovelSettings,
  NovelSettingsDraft,
  ProfileDraft,
  TokenUsageReport,
  UpdateCheckResult,
  UpdateProgress
} from "./types";
import { type AutoRunProgress, useAutoRunProgress } from "./useAutoRunProgress";

type ModelSuggestion = {
  label: string;
  model: string;
};

type ModelSuggestionGroup = {
  id: string;
  baseTerms: string[];
  modelTerms: string[];
  models: ModelSuggestion[];
};


const emptyProfile: ProfileDraft = {
  name: "OpenAI 兼容接口",
  provider: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  model: "请填写模型名",
  temperature: 0.7,
  top_p: 1,
  thinking_mode: "auto",
  prompt_obfuscation_enabled: false,
  api_key: ""
};

const emptyNovelSettings: NovelSettingsDraft = {
  protagonist_name: "",
  protagonist_aliases: "",
  rewritten_protagonist_name: "",
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: ""
};

const savedApiKeyMask = "********";
const quickStartSeenKey = "yuri-rewrite.quick-start-seen";
const modelSuggestionGroups: ModelSuggestionGroup[] = [
  {
    id: "deepseek",
    baseTerms: ["deepseek"],
    modelTerms: ["deepseek"],
    models: [
      { label: "DeepSeek V4 Pro", model: "deepseek-v4-pro" },
      { label: "DeepSeek V4 Flash", model: "deepseek-v4-flash" }
    ]
  },
  {
    id: "volcengine",
    baseTerms: ["volcengine", "volces", "ark.cn-"],
    modelTerms: ["doubao-", "seed-"],
    models: [
      { label: "Doubao Seed 2.0 Pro", model: "doubao-seed-2-0-pro-260215" },
      { label: "Doubao Seed 2.0 Lite", model: "doubao-seed-2-0-lite-260428" },
      { label: "Doubao Seed 2.0 Mini", model: "doubao-seed-2-0-mini-260428" },
      { label: "Doubao Seed 2.0 Code", model: "doubao-seed-2-0-code-preview-260215" },
      { label: "Doubao 1.5 Pro 32K", model: "doubao-1-5-pro-32k-250115" },
      { label: "Doubao 1.5 Pro 256K", model: "doubao-1-5-pro-256k-250115" },
      { label: "Doubao 1.5 Lite 32K", model: "doubao-1-5-lite-32k-250115" },
      { label: "Doubao 1.5 Thinking Pro", model: "doubao-1-5-thinking-pro-250415" },
      { label: "Doubao 1.5 Vision Pro", model: "doubao-1-5-vision-pro-250328" }
    ]
  },
  {
    id: "openai",
    baseTerms: ["api.openai.com", "openai.azure.com"],
    modelTerms: ["gpt-", "o3", "o4"],
    models: [
      { label: "GPT-5.2", model: "gpt-5.2" },
      { label: "GPT-5.2 Pro", model: "gpt-5.2-pro" },
      { label: "GPT-5.1", model: "gpt-5.1" },
      { label: "GPT-5", model: "gpt-5" },
      { label: "GPT-5 Mini", model: "gpt-5-mini" },
      { label: "GPT-5 Nano", model: "gpt-5-nano" },
      { label: "o3 Pro", model: "o3-pro" },
      { label: "o3", model: "o3" },
      { label: "GPT-4.1", model: "gpt-4.1" },
      { label: "GPT-4.1 Mini", model: "gpt-4.1-mini" },
      { label: "GPT-4o Mini", model: "gpt-4o-mini" }
    ]
  },
  {
    id: "zhipu",
    baseTerms: ["bigmodel", "zhipu", "z.ai", "智谱"],
    modelTerms: ["glm-"],
    models: [
      { label: "GLM-5.2", model: "glm-5.2" },
      { label: "GLM-5.1", model: "glm-5.1" },
      { label: "GLM-5", model: "glm-5" },
      { label: "GLM-5 Turbo", model: "glm-5-turbo" },
      { label: "GLM-4.7", model: "glm-4.7" },
      { label: "GLM-4.6", model: "glm-4.6" },
      { label: "GLM-4.5", model: "glm-4.5" },
      { label: "GLM-4.5 Air", model: "glm-4.5-air" },
      { label: "GLM-4 Plus", model: "glm-4-plus" },
      { label: "GLM-4 Flash", model: "glm-4-flash" }
    ]
  },
  {
    id: "kimi",
    baseTerms: ["moonshot", "kimi"],
    modelTerms: ["moonshot", "kimi"],
    models: [
      { label: "Kimi K2.6", model: "kimi-k2.6" },
      { label: "Kimi K2.5", model: "kimi-k2.5" },
      { label: "Moonshot V1 128K", model: "moonshot-v1-128k" },
      { label: "Moonshot V1 32K", model: "moonshot-v1-32k" },
      { label: "Moonshot V1 8K", model: "moonshot-v1-8k" }
    ]
  },
  {
    id: "minimax",
    baseTerms: ["minimax"],
    modelTerms: ["minimax", "m2-her"],
    models: [
      { label: "MiniMax M3", model: "MiniMax-M3" },
      { label: "MiniMax M2.7", model: "MiniMax-M2.7" },
      { label: "MiniMax M2.7 Highspeed", model: "MiniMax-M2.7-highspeed" },
      { label: "MiniMax M2.5", model: "MiniMax-M2.5" },
      { label: "MiniMax M2.5 Highspeed", model: "MiniMax-M2.5-highspeed" },
      { label: "MiniMax M2.1", model: "MiniMax-M2.1" },
      { label: "MiniMax M2.1 Highspeed", model: "MiniMax-M2.1-highspeed" },
      { label: "MiniMax M2", model: "MiniMax-M2" },
      { label: "M2-her", model: "M2-her" }
    ]
  },
  {
    id: "mimo",
    baseTerms: ["xiaomimimo", "mimo.xiaomi", "mimo.mi.com", "mimo"],
    modelTerms: ["mimo-"],
    models: [
      { label: "MiMo V2.5 Pro", model: "mimo-v2.5-pro" },
      { label: "MiMo V2.5", model: "mimo-v2.5" },
      { label: "MiMo V2 Flash", model: "mimo-v2-flash" }
    ]
  },
  {
    id: "siliconflow",
    baseTerms: ["siliconflow"],
    modelTerms: ["qwen/", "thudm/", "deepseek-ai/", "moonshotai/", "minimaxai/", "zai-org/", "bytedance-seed/", "internlm/", "mistralai/", "openai/"],
    models: [
      { label: "DeepSeek V3.2", model: "deepseek-ai/DeepSeek-V3.2" },
      { label: "DeepSeek V3.2 Exp", model: "deepseek-ai/DeepSeek-V3.2-Exp" },
      { label: "DeepSeek V3.1 Terminus", model: "deepseek-ai/DeepSeek-V3.1-Terminus" },
      { label: "DeepSeek V3.1", model: "deepseek-ai/DeepSeek-V3.1" },
      { label: "DeepSeek R1", model: "deepseek-ai/DeepSeek-R1" },
      { label: "Qwen3.6 27B", model: "Qwen/Qwen3.6-27B" },
      { label: "Qwen3.5 122B A10B", model: "Qwen/Qwen3.5-122B-A10B" },
      { label: "Qwen3.5 35B A3B", model: "Qwen/Qwen3.5-35B-A3B" },
      { label: "Qwen3.5 27B", model: "Qwen/Qwen3.5-27B" },
      { label: "Qwen3 Coder 480B A35B", model: "Qwen/Qwen3-Coder-480B-A35B-Instruct" },
      { label: "Qwen3 Coder 30B A3B", model: "Qwen/Qwen3-Coder-30B-A3B-Instruct" },
      { label: "Kimi K2.6", model: "moonshotai/Kimi-K2.6" },
      { label: "Kimi K2 Instruct 0905", model: "moonshotai/Kimi-K2-Instruct-0905" },
      { label: "GLM-5.1", model: "zai-org/GLM-5.1" },
      { label: "GLM-4.5 Air", model: "zai-org/GLM-4.5-Air" },
      { label: "MiniMax M2.5", model: "MiniMaxAI/MiniMax-M2.5" },
      { label: "MiniMax M2", model: "MiniMaxAI/MiniMax-M2" },
      { label: "GPT OSS 120B", model: "openai/gpt-oss-120b" },
      { label: "Seed OSS 36B Instruct", model: "ByteDance-Seed/Seed-OSS-36B-Instruct" }
    ]
  },
  {
    id: "claude",
    baseTerms: ["anthropic", "claude"],
    modelTerms: ["claude-"],
    models: [
      { label: "Claude Opus 4.8", model: "claude-opus-4-8" },
      { label: "Claude Sonnet 4.6", model: "claude-sonnet-4-6" },
      { label: "Claude Haiku 4.5", model: "claude-haiku-4-5-20251001" }
    ]
  }
];

function getModelSuggestions(profile: ProfileDraft) {
  const baseHint = profile.base_url.toLowerCase();
  const modelHint = profile.model.toLowerCase();
  const baseMatched = modelSuggestionGroups.find((group) => group.baseTerms.some((term) => baseHint.includes(term)));
  if (baseMatched) return baseMatched.models;
  const modelMatched = modelSuggestionGroups.find((group) => group.modelTerms.some((term) => modelHint.includes(term)));
  return modelMatched?.models ?? [];
}

const statusText: Record<string, string> = {
  pending: "待处理",
  running: "进行中",
  pausing: "暂停中",
  paused: "已暂停",
  terminating: "终止中",
  terminated: "已终止",
  completed: "完成",
  failed: "失败",
  imported: "已导入"
};

function batchIdContainingChapter(detail: NovelDetail, chapterId: string): string {
  const chapter = detail.chapters.find((item) => item.id === chapterId);
  if (!chapter) return detail.batches[0]?.id ?? "";
  return detail.batches.find(
    (batch) => chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter
  )?.id ?? detail.batches[0]?.id ?? "";
}

const jobPhaseText: Record<string, string> = {
  analysis: "分析",
  rewrite: "改写",
  review: "审查",
  revision: "修复",
  final_review: "终审",
  export: "导出"
};

function localDateString(date: Date) {
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0")
  ].join("-");
}

function defaultTokenStatsDateRange(now = new Date()) {
  const start = new Date(now);
  start.setDate(start.getDate() - 29);
  return {
    startDate: localDateString(start),
    endDate: localDateString(now)
  };
}

export default function App() {
  const {
    novels, setNovels, detail, setDetail, selectedChapterId, setSelectedChapterId,
    selectedBatchId, setSelectedBatchId, selectedChapter, selectedBatch,
    novelSettingsDraft, setNovelSettingsDraft
  } = useNovels(emptyNovelSettings);
  const {
    profiles, setProfiles, profileDraft, setProfileDraft,
    selectedProfileId, setSelectedProfileId, selectedProfile
  } = useModelProfiles(defaultProfile);
  const {
    busy, setBusy, autoRunState, setAutoRunState, autoControlBusy,
    setAutoControlBusy, job, setJob, processingTaskActive
  } = useTaskState();
  const [openNovelMenuId, setOpenNovelMenuId] = useState("");
  const [openModelMenu, setOpenModelMenu] = useState(false);
  const [openModelSuggestions, setOpenModelSuggestions] = useState(false);
  const [logs, setLogs] = useState<AiLog[]>([]);
  const [settings, setSettings] = useState<AppSettings>({});
  const [corePromptDraft, setCorePromptDraft] = useState("");
  const [jobEstimate, setJobEstimate] = useState<JobEstimate | null>(null);
  const [estimateCollapsed, setEstimateCollapsed] = useState(false);
  const [modelDiagnosis, setModelDiagnosis] = useState<ModelDiagnosis | null>(null);
  const [settingsDialog, setSettingsDialog] = useState<"basic" | "advanced" | null>(null);
  const [novelPendingDeletion, setNovelPendingDeletion] = useState<Novel | null>(null);
  const [activeView, setActiveView] = useState<"workspace" | "compare" | "novel-settings" | "core-settings" | "logs" | "token-stats" | "settings">("workspace");
  const [workspaceSection, setWorkspaceSection] = useState<"main" | "canon">("main");
  const [tokenStats, setTokenStats] = useState<TokenUsageReport | null>(null);
  const initialTokenStatsRangeRef = useRef(defaultTokenStatsDateRange());
  const [tokenStatsStartDate, setTokenStatsStartDate] = useState(initialTokenStatsRangeRef.current.startDate);
  const [tokenStatsEndDate, setTokenStatsEndDate] = useState(initialTokenStatsRangeRef.current.endDate);
  const [tokenStatsLoading, setTokenStatsLoading] = useState(false);
  const [pendingUpdate, setPendingUpdate] = useState<UpdateCheckResult | null>(null);
  const { notice, setNotice, showNotice } = useNotice(setPendingUpdate);
  const [hasAvailableUpdate, setHasAvailableUpdate] = useState(false);
  const [showUpdateInstallDialog, setShowUpdateInstallDialog] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<UpdateProgress | null>(null);
  const [showQuickStart, setShowQuickStart] = useState(false);
  const [autoRunRecoveries, setAutoRunRecoveries] = useState<AutoRunRecovery[]>([]);
  const [autoRunMode, setAutoRunMode] = useState<"range" | "batch" | null>(null);
  const [autoRunMenuOpen, setAutoRunMenuOpen] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const originalCompareRef = useRef<HTMLDivElement | null>(null);
  const rewriteCompareRef = useRef<HTMLDivElement | null>(null);
  const detailRef = useRef<NovelDetail | null>(null);
  const selectedBatchIdRef = useRef("");
  const selectedChapterIdRef = useRef("");
  const lastAutoExportedBatchRef = useRef(0);
  const estimateRequestIdRef = useRef(0);
  const silentNovelRefreshInFlightRef = useRef(false);
  const busyRef = useRef("");
  const importInProgressRef = useRef(false);
  const processingTaskActiveRef = useRef(processingTaskActive);
  const autoRunMenuRef = useRef<HTMLDivElement | null>(null);
  const activeViewRef = useRef(activeView);
  const tokenStatsRangeCustomizedRef = useRef(false);
  const tokenStatsDirtyRef = useRef(false);
  const tokenStatsLoadingRef = useRef(false);

  const autoProgressPercent = useMemo(() => {
    if (!job || !["auto", "auto_batch"].includes(job.job_type) || job.total_chapters <= 0) return 0;
    if (job.job_type === "auto_batch" && job.status === "running") {
      const stageRatio = job.chapter_total
        ? (job.chapter_completed ?? 0) / job.chapter_total
        : job.shard_total
          ? (job.shard_completed ?? 0) / job.shard_total
        : 0;
      if (job.phase === "analysis") return Math.round(stageRatio * 50);
      if (["rewrite", "review", "revision", "final_review"].includes(job.phase ?? "")) {
        return Math.round(50 + stageRatio * 50);
      }
    }
    return Math.min(100, Math.max(0, Math.round((job.current_chapter / job.total_chapters) * 100)));
  }, [job]);

  const detectedModelSuggestions = useMemo(
    () => detectModelSuggestions(profileDraft),
    [profileDraft.provider, profileDraft.base_url, profileDraft.model]
  );

  const hasCompleteNovelSettings = Boolean(
    detail?.settings?.protagonist_name?.trim() && detail.settings.bust?.trim() && detail.settings.body_type?.trim()
  );
  const pausedAutoRun = autoRunState === "paused";
  const adjustableWhilePaused = processingTaskActive && !pausedAutoRun;

  const requestJobEstimate = useCallback(async (
    novelId: string | null | undefined,
    batchId: string | null,
    profileId: string | null
  ) => {
    const requestId = ++estimateRequestIdRef.current;
    if (!novelId) {
      setJobEstimate(null);
      return;
    }
    try {
      const estimate = await invoke("estimate_job_cost", {
        novelId,
        batchId,
        profileId
      });
      if (estimateRequestIdRef.current === requestId) setJobEstimate(estimate);
    } catch {
      if (estimateRequestIdRef.current === requestId) setJobEstimate(null);
    }
  }, []);

  const refreshJobEstimate = useCallback(async () => {
    await requestJobEstimate(
      detail?.novel.id,
      selectedBatchId || null,
      selectedProfileId || null
    );
  }, [detail?.novel.id, requestJobEstimate, selectedBatchId, selectedProfileId]);

  useEffect(() => {
    void refreshAll();
    try {
      if (window.localStorage.getItem(quickStartSeenKey) !== "true") {
        setShowQuickStart(true);
      }
    } catch {
      setShowQuickStart(true);
    }
  }, []);

  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  useEffect(() => {
    detailRef.current = detail;
  }, [detail]);

  useEffect(() => {
    selectedBatchIdRef.current = selectedBatchId;
  }, [selectedBatchId]);

  useEffect(() => {
    selectedChapterIdRef.current = selectedChapterId;
  }, [selectedChapterId]);

  useEffect(() => {
    processingTaskActiveRef.current = processingTaskActive;
  }, [processingTaskActive]);

  useEffect(() => {
    activeViewRef.current = activeView;
  }, [activeView]);

  useEffect(() => {
    if (!autoRunMenuOpen) return undefined;
    const closeMenu = (event: MouseEvent) => {
      if (!autoRunMenuRef.current?.contains(event.target as Node)) {
        setAutoRunMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", closeMenu);
    return () => window.removeEventListener("mousedown", closeMenu);
  }, [autoRunMenuOpen]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    function handleDragDrop(event: { payload: DragDropEvent }) {
      const payload = event.payload;
      if (payload.type === "enter") {
        setDragActive(payload.paths.some(isTxtFilePath));
        return;
      }
      if (payload.type === "leave") {
        setDragActive(false);
        return;
      }
      if (payload.type !== "drop") return;

      setDragActive(false);
      const txtPath = payload.paths.find(isTxtFilePath);
      if (!txtPath) {
        showNotice("请拖入 TXT 小说文件。");
        return;
      }
      if (busyRef.current || processingTaskActiveRef.current) {
        showNotice("当前有任务正在进行，请稍后再导入。");
        return;
      }
      void importTxtFile(txtPath);
    }

    void getCurrentWebview().onDragDropEvent(handleDragDrop).then((handler) => {
      if (cancelled) {
        handler();
      } else {
        unlisten = handler;
      }
    });

    return () => {
      cancelled = true;
      setDragActive(false);
      if (unlisten) unlisten();
    };
  }, []);

  useAutoRunProgress(detail?.novel.id ?? null, (progress: AutoRunProgress) => {
      setJob(progress);
      setAutoRunMode(progress.job_type === "auto_batch" ? "batch" : "range");
      if (progress.status === "running") {
        tokenStatsDirtyRef.current = true;
        setAutoRunState("running");
        if (progress.current_chapter > lastAutoExportedBatchRef.current) {
          lastAutoExportedBatchRef.current = progress.current_chapter;
          void refreshCurrentNovelSilently(progress.novel_id);
        }
      } else if (progress.status === "paused") {
        setAutoRunState("paused");
      } else if (progress.status === "pausing" || progress.status === "terminating") {
        setAutoRunState("stopping");
      } else if (["completed", "failed", "terminated"].includes(progress.status)) {
        setAutoRunState("idle");
        setAutoRunMode(null);
        lastAutoExportedBatchRef.current = 0;
      }
      if (["completed", "paused", "failed"].includes(progress.status)) {
        void refreshJobEstimate();
      }
      if (["completed", "paused", "failed", "terminated"].includes(progress.status)) {
        void invoke("list_auto_run_recoveries").then(setAutoRunRecoveries);
        if (tokenStatsDirtyRef.current && activeViewRef.current === "token-stats") {
          tokenStatsDirtyRef.current = false;
          const range = tokenStatsRangeCustomizedRef.current
            ? { startDate: tokenStatsStartDate, endDate: tokenStatsEndDate }
            : defaultTokenStatsDateRange();
          if (!tokenStatsRangeCustomizedRef.current) {
            setTokenStatsStartDate(range.startDate);
            setTokenStatsEndDate(range.endDate);
          }
          void refreshTokenStats(range.startDate, range.endDate).then((refreshed) => {
            if (!refreshed) tokenStatsDirtyRef.current = true;
          });
        }
      }
  });

  useEffect(() => {
    if (detectedModelSuggestions.length === 0) setOpenModelSuggestions(false);
  }, [detectedModelSuggestions.length]);

  useEffect(() => {
    setCorePromptDraft(settings.core_prompt ?? "");
  }, [settings.core_prompt]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<UpdateProgress>("update-progress", (event) => {
      if (!cancelled) setUpdateProgress(event.payload);
    }).then((handler) => {
      if (cancelled) handler();
      else unlisten = handler;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function checkStartupUpdate() {
      try {
        const result = await invoke("take_update_install_result");
        if (!cancelled && result) {
          showNotice(
            result.status === "success"
              ? result.message
              : `${result.message} 更新日志：${result.log_path}`,
            result.status === "success" ? 5000 : 60_000
          );
        }
      } catch {
        // A missing or unreadable updater result must not block normal startup.
      }
      try {
        const update = await invoke("check_for_updates");
        if (!cancelled) {
          setHasAvailableUpdate(!update.is_latest);
        }
      } catch {
        // Startup update checks are intentionally silent when offline or rate-limited.
      }
    }

    void checkStartupUpdate();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (novelPendingDeletion && busy !== "delete-novel") {
        setNovelPendingDeletion(null);
        return;
      }
      if (activeView !== "workspace" && !settingsDialog && !novelPendingDeletion) {
        setActiveView("workspace");
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeView, busy, novelPendingDeletion, settingsDialog]);

  useEffect(() => {
    const profile = selectedProfile;
    setModelDiagnosis(null);
    if (!profile) return;
    setProfileDraft(normalizeThinkingMode({
      id: profile.id,
      name: profile.name,
      provider: profile.provider,
      base_url: profile.base_url,
      model: profile.model,
      temperature: profile.temperature,
      top_p: profile.top_p,
      thinking_mode: profile.thinking_mode === "off" || profile.thinking_mode === "on" ? profile.thinking_mode : "auto",
      prompt_obfuscation_enabled: profile.prompt_obfuscation_enabled,
      api_key: profile.has_api_key ? savedApiKeyMask : ""
    }));
  }, [selectedProfile]);

  useEffect(() => {
    if (originalCompareRef.current) originalCompareRef.current.scrollTop = 0;
    if (rewriteCompareRef.current) rewriteCompareRef.current.scrollTop = 0;
  }, [selectedChapterId]);

  useEffect(() => {
    void refreshJobEstimate();
  }, [refreshJobEstimate]);

  async function refreshAll() {
    const [novelRows, profileRows, appSettings, recoveryRows] = await Promise.all([
      invoke("list_novels"),
      invoke("list_model_profiles"),
      invoke("get_app_settings"),
      invoke("list_auto_run_recoveries")
    ]);
    setNovels(novelRows);
    setProfiles(profileRows);
    setSettings(appSettings);
    setAutoRunRecoveries(recoveryRows);
    const currentProfileIsValid = selectedProfileId && profileRows.some((profile) => profile.id === selectedProfileId);
    const savedProfileId = appSettings.selected_profile_id ?? "";
    const savedProfileIsValid = savedProfileId && profileRows.some((profile) => profile.id === savedProfileId);
    if (!currentProfileIsValid) {
      setSelectedProfileId(savedProfileIsValid ? savedProfileId : profileRows[0]?.id ?? "");
    }
    if (!detail && novelRows[0]) {
      const firstRecoveryNovel = recoveryRows.find((recovery) => novelRows.some((novel) => novel.id === recovery.novel_id));
      await loadNovel(firstRecoveryNovel?.novel_id ?? novelRows[0].id, { recoveries: recoveryRows });
    } else if (detail) {
      applyRecoveryForNovel(detail.novel.id, recoveryRows);
    }
    await refreshLogs();
  }

  function applyRecoveryForNovel(novelId: string, recoveries = autoRunRecoveries) {
    const recovery = recoveries.find((item) => item.novel_id === novelId);
    if (!recovery) {
      if (autoRunState === "paused") {
        setAutoRunState("idle");
        setJob(null);
      }
      return;
    }
    setAutoRunState("paused");
    setAutoRunMode("range");
    if (recovery.job) {
      setJob({
        ...recovery.job,
        status: "paused",
        message: `检测到上次未完成的一键任务，将继续处理第 ${recovery.next_batch_index + 1} 批的未完成分片。${recovery.pause_reason ? ` ${recovery.pause_reason}` : ""}`
      });
    }
  }

  async function loadNovel(
    novelId: string,
    options: { preserveBatchId?: string; preserveChapterId?: string; recoveries?: AutoRunRecovery[] } = {}
  ) {
    if ((autoRunState === "running" || autoRunState === "stopping") && detail?.novel.id !== novelId) {
      showNotice("当前任务运行或暂停中，不能切换小说。请先完成或终止任务。");
      return;
    }
    const next = await invoke("get_novel_detail", { novelId });
    setDetail(next);
    const nextChapterId =
      options.preserveChapterId && next.chapters.some((chapter) => chapter.id === options.preserveChapterId)
        ? options.preserveChapterId
        : next.chapters[0]?.id ?? "";
    const nextBatchId =
      options.preserveBatchId && next.batches.some((batch) => batch.id === options.preserveBatchId)
        ? options.preserveBatchId
        : batchIdContainingChapter(next, nextChapterId);
    setSelectedChapterId(nextChapterId);
    setSelectedBatchId(nextBatchId);
    setNovelSettingsDraft(
      next.settings
        ? {
            protagonist_name: next.settings.protagonist_name,
            protagonist_aliases: next.settings.protagonist_aliases ?? "",
            rewritten_protagonist_name: next.settings.rewritten_protagonist_name ?? "",
            additional_feminize_names: next.settings.additional_feminize_names,
            bust: next.settings.bust,
            body_type: next.settings.body_type,
            rewrite_mode: next.settings.rewrite_mode === "creative" ? "creative" : "strict",
            advanced_settings: next.settings.advanced_settings
          }
        : emptyNovelSettings
    );
    setOpenNovelMenuId("");
    setWorkspaceSection("main");
    setActiveView("workspace");
    applyRecoveryForNovel(novelId, options.recoveries);
    await refreshLogs(next.novel.id);
  }

  async function refreshCurrentNovelSilently(novelId: string) {
    if (silentNovelRefreshInFlightRef.current) return;
    if (detailRef.current?.novel.id !== novelId) return;
    silentNovelRefreshInFlightRef.current = true;
    try {
      const next = await invoke("get_novel_detail", { novelId });
      if (detailRef.current?.novel.id !== novelId) return;
      setDetail(next);
      const preservedChapterId = selectedChapterIdRef.current;
      const preservedBatchId = selectedBatchIdRef.current;
      if (!next.chapters.some((chapter) => chapter.id === preservedChapterId)) {
        setSelectedChapterId(next.chapters[0]?.id ?? "");
      }
      if (!next.batches.some((batch) => batch.id === preservedBatchId)) {
        setSelectedBatchId(batchIdContainingChapter(next, preservedChapterId));
      }
      if (next.settings) {
        setNovelSettingsDraft({
          protagonist_name: next.settings.protagonist_name,
          protagonist_aliases: next.settings.protagonist_aliases ?? "",
          rewritten_protagonist_name: next.settings.rewritten_protagonist_name ?? "",
          additional_feminize_names: next.settings.additional_feminize_names,
          bust: next.settings.bust,
          body_type: next.settings.body_type,
          rewrite_mode: next.settings.rewrite_mode === "creative" ? "creative" : "strict",
          advanced_settings: next.settings.advanced_settings
        });
      }
      await refreshLogs(novelId);
    } catch {
      // Progress refresh is best-effort. The final task result still performs a full refresh.
    } finally {
      silentNovelRefreshInFlightRef.current = false;
    }
  }

  async function refreshLogs(novelId = detail?.novel.id) {
    const rows = await invoke("list_ai_logs", { novelId: novelId ?? null });
    setLogs(rows);
  }

  async function refreshTokenStats(
    startDate = tokenStatsStartDate,
    endDate = tokenStatsEndDate
  ) {
    if (!startDate || !endDate || tokenStatsLoadingRef.current) return false;
    tokenStatsLoadingRef.current = true;
    setTokenStatsLoading(true);
    setNotice("");
    try {
      const report = await invoke("get_token_usage_stats", {
        startDate,
        endDate
      });
      setTokenStats(report);
      tokenStatsDirtyRef.current = false;
      return true;
    } catch (error) {
      showNotice(String(error));
      return false;
    } finally {
      tokenStatsLoadingRef.current = false;
      setTokenStatsLoading(false);
    }
  }

  function openTokenStats() {
    const range = tokenStatsRangeCustomizedRef.current
      ? { startDate: tokenStatsStartDate, endDate: tokenStatsEndDate }
      : defaultTokenStatsDateRange();
    if (!tokenStatsRangeCustomizedRef.current) {
      setTokenStatsStartDate(range.startDate);
      setTokenStatsEndDate(range.endDate);
    }
    setActiveView("token-stats");
    void refreshTokenStats(range.startDate, range.endDate);
  }

  function changeTokenStatsStartDate(value: string) {
    tokenStatsRangeCustomizedRef.current = true;
    setTokenStatsStartDate(value);
  }

  function changeTokenStatsEndDate(value: string) {
    tokenStatsRangeCustomizedRef.current = true;
    setTokenStatsEndDate(value);
  }

  async function persistSelectedProfileId(profileId: string) {
    try {
      const saved = await invoke("save_selected_profile_id", { profileId: profileId || null });
      setSettings(saved);
    } catch (error) {
      console.error("Failed to persist selected model profile", error);
    }
  }

  function selectModelProfile(profileId: string) {
    setSelectedProfileId(profileId);
    setOpenModelMenu(false);
    void persistSelectedProfileId(profileId);
  }

  async function clearLogs() {
    const targetText = detail ? `《${detail.novel.title}》相关日志和全局日志` : "所有日志";
    if (!window.confirm(`清空${targetText}？Token 调用统计将保留。`)) return;
    setBusy("clear-logs");
    setNotice("");
    try {
      await invoke("clear_ai_logs", { novelId: detail?.novel.id ?? null });
      await refreshLogs();
      showNotice("日志已清空，Token 调用统计已保留。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function isTxtFilePath(filePath: string) {
    return filePath.trim().toLowerCase().endsWith(".txt");
  }

  function openNovelSettings() {
    if (!detail) {
      showNotice("请先上传小说文件");
      return;
    }
    setSettingsDialog("basic");
  }

  async function saveNovelSettings() {
    if (!detail) return;
    if (!novelSettingsDraft.protagonist_name.trim()) {
      showNotice("请填写主角姓名。");
      return;
    }
    setBusy("novel-settings");
    setNotice("");
    try {
      const saved = await invoke("save_novel_settings", {
        novelId: detail.novel.id,
        protagonistName: novelSettingsDraft.protagonist_name,
        protagonistAliases: novelSettingsDraft.protagonist_aliases,
        rewrittenProtagonistName: novelSettingsDraft.rewritten_protagonist_name,
        additionalFeminizeNames: novelSettingsDraft.additional_feminize_names,
        bust: novelSettingsDraft.bust,
        bodyType: novelSettingsDraft.body_type,
        rewriteMode: novelSettingsDraft.rewrite_mode,
        advancedSettings: novelSettingsDraft.advanced_settings
      });
      setDetail({ ...detail, settings: saved });
      setNovelSettingsDraft({
        protagonist_name: saved.protagonist_name,
        protagonist_aliases: saved.protagonist_aliases ?? "",
        rewritten_protagonist_name: saved.rewritten_protagonist_name ?? "",
        additional_feminize_names: saved.additional_feminize_names,
        bust: saved.bust,
        body_type: saved.body_type,
        rewrite_mode: saved.rewrite_mode === "creative" ? "creative" : "strict",
        advanced_settings: saved.advanced_settings
      });
      setSettingsDialog(null);
      setActiveView("workspace");
      showNotice("基本设定已保存。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function importTxtFile(filePath: string) {
    if (!isTxtFilePath(filePath)) {
      showNotice("当前仅支持导入 TXT 小说文件。");
      return;
    }
    if (importInProgressRef.current) return;
    importInProgressRef.current = true;
    busyRef.current = "import";
    setBusy("import");
    setNotice("");
    try {
      const novel = await invoke("import_txt", { filePath });
      await refreshAll();
      await loadNovel(novel.id);
      showNotice(`已导入《${novel.title}》。`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      importInProgressRef.current = false;
      busyRef.current = "";
      setBusy("");
    }
  }

  async function importTxt() {
    busyRef.current = "import";
    setBusy("import");
    setNotice("");
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "TXT 小说", extensions: ["txt"] }]
      });
      if (typeof selected !== "string") return;
      await importTxtFile(selected);
    } catch (error) {
      showNotice(String(error));
    } finally {
      if (!importInProgressRef.current) {
        busyRef.current = "";
        setBusy("");
      }
    }
  }

  function deleteNovel(novel: Novel) {
    if (processingTaskActive) {
      showNotice("当前任务运行或暂停中，不能删除小说。");
      return;
    }
    setOpenNovelMenuId("");
    setNovelPendingDeletion(novel);
  }

  async function confirmDeleteNovel() {
    const novel = novelPendingDeletion;
    if (!novel) return;
    if (processingTaskActive) {
      setNovelPendingDeletion(null);
      showNotice("当前任务运行或暂停中，不能删除小说。");
      return;
    }
    setBusy("delete-novel");
    setNotice("");
    try {
      await invoke("delete_novel", { novelId: novel.id });
      const remaining = await invoke("list_novels");
      setNovels(remaining);
      setOpenNovelMenuId("");
      if (detail?.novel.id === novel.id) {
        if (remaining[0]) {
          await loadNovel(remaining[0].id);
        } else {
          setDetail(null);
          setSelectedChapterId("");
          setSelectedBatchId("");
          setNovelSettingsDraft(emptyNovelSettings);
          setSettingsDialog(null);
          setLogs([]);
        }
      }
      setNovelPendingDeletion(null);
      showNotice(`已删除《${novel.title}》。`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function saveProfile() {
    setBusy("profile");
    setNotice("");
    try {
      const input = {
        ...profileDraft,
        id: profileDraft.id && selectedProfileId === profileDraft.id ? profileDraft.id : undefined,
        name: profileDraft.name.trim(),
        provider: profileDraft.provider.trim(),
        base_url: profileDraft.base_url.trim(),
        model: profileDraft.model.trim(),
        api_key: profileDraft.api_key === savedApiKeyMask ? undefined : profileDraft.api_key
      };
      const saved = await invoke("save_model_profile", { input });
      setSelectedProfileId(saved.id);
      setProfileDraft({ ...profileDraft, id: saved.id, api_key: saved.has_api_key ? savedApiKeyMask : "" });
      await persistSelectedProfileId(saved.id);
      await refreshAll();
      showNotice(saved.has_api_key ? "模型配置和 API Key 已保存。" : "模型配置已保存，尚未保存 API Key。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function createNewModelProfile() {
    setSelectedProfileId("");
    setProfileDraft(defaultProfile);
    setOpenModelMenu(false);
    void persistSelectedProfileId("");
    showNotice("已切换为新建模型配置，填写后点击保存。");
  }

  async function deleteSelectedModelProfile() {
    if (processingTaskActive) {
      showNotice("当前任务运行或暂停中，不能删除模型配置。");
      return;
    }
    const profile = profiles.find((item) => item.id === selectedProfileId);
    if (!profile) {
      showNotice("请先选择一个模型配置。");
      return;
    }
    if (!window.confirm(`删除模型配置「${profile.model}」及其保存的 API Key？`)) return;
    setBusy("delete-model");
    setNotice("");
    try {
      await invoke("delete_model_profile", { profileId: profile.id });
      const nextProfiles = await invoke("list_model_profiles");
      setProfiles(nextProfiles);
      const nextSelected = nextProfiles[0]?.id ?? "";
      setSelectedProfileId(nextSelected);
      setOpenModelMenu(false);
      await persistSelectedProfileId(nextSelected);
      if (!nextSelected) setProfileDraft(defaultProfile);
      await refreshLogs();
      showNotice(`已删除模型配置「${profile.model}」。`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function diagnoseProfile() {
    if (!selectedProfileId) {
      showNotice("请先保存并选择一个模型配置。");
      return;
    }
    setBusy("diagnose");
    setNotice("");
    setModelDiagnosis(null);
    try {
      const result = await invoke("diagnose_model_profile", {
        profileId: selectedProfileId
      });
      setModelDiagnosis(result);
      await refreshLogs();
      const label = result.status === "ok" ? "诊断通过" : result.status === "warning" ? "诊断有警告" : "诊断失败";
      showNotice(label);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function saveCanonAssets() {
    if (!detail) return;
    setBusy("canon");
    setNotice("");
    try {
      const assets = detail.canon_assets.map(({ kind, content }) => ({ kind, content }));
      const updated = await invoke("update_canon_assets", {
        novelId: detail.novel.id,
        assets
      });
      setDetail({ ...detail, canon_assets: updated });
      showNotice("一致性资产已保存。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function confirmRewriteOverwrite(chapters: Chapter[]) {
    const editedCount = chapters.filter((chapter) => chapter.rewrite_edited).length;
    if (editedCount === 0) return true;
    return window.confirm(
      `当前处理范围有 ${editedCount} 章包含人工编辑内容。重新改写会用新的 AI 稿覆盖这些修改，是否继续？`
    );
  }

  async function runJob(kind: "analysis" | "rewrite") {
    if (!detail || !selectedProfileId) {
      showNotice("请先导入小说并选择模型配置。");
      return;
    }
    if (!hasCompleteNovelSettings) {
      showNotice("请先填写设定");
      setSettingsDialog("basic");
      return;
    }
    if (!selectedBatch) {
      showNotice("当前小说没有可处理的批次。");
      return;
    }
    if (
      kind === "rewrite"
      && !confirmRewriteOverwrite(
        detail.chapters.filter(
          (chapter) => chapter.index >= selectedBatch.start_chapter && chapter.index <= selectedBatch.end_chapter
        )
      )
    ) return;
    setBusy(kind);
    setNotice("");
    try {
      const result = await invoke(kind === "analysis" ? "start_analysis" : "start_rewrite", {
        novelId: detail.novel.id,
        profileId: selectedProfileId,
        batchId: selectedBatch.id
      });
      setJob(result);
      await loadNovel(detail.novel.id, { preserveBatchId: selectedBatch.id, preserveChapterId: selectedChapterId });
      await refreshLogs(detail.novel.id);
      if (kind === "rewrite" && result.status === "completed") {
        setActiveView("compare");
      }
      showNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function runAnalyzeRewriteCurrentBatch() {
    if (!detail || !selectedProfileId) {
      showNotice("请先导入小说并选择模型配置。");
      return;
    }
    if (!hasCompleteNovelSettings) {
      showNotice("请先填写设定");
      setSettingsDialog("basic");
      return;
    }
    if (!selectedBatch) {
      showNotice("当前小说没有可处理的批次。");
      return;
    }
    const batchId = selectedBatch.id;
    const novelId = detail.novel.id;
    if (!confirmRewriteOverwrite(
      detail.chapters.filter(
        (chapter) => chapter.index >= selectedBatch.start_chapter && chapter.index <= selectedBatch.end_chapter
      )
    )) return;
    setBusy("auto-batch");
    setAutoRunState("running");
    setAutoRunMode("batch");
    setNotice("");
    try {
      const result = await invoke("start_analyze_rewrite_batch", {
        novelId,
        profileId: selectedProfileId,
        batchId
      });
      setJob(result);
      await loadNovel(novelId, { preserveBatchId: batchId, preserveChapterId: selectedChapterId });
      await refreshLogs(novelId);
      if (result.status === "completed") {
        setActiveView("compare");
      }
      showNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setAutoRunState("idle");
      setAutoRunMode(null);
      setBusy("");
    }
  }

  async function runAnalyzeRewriteAll(startBatchId: string | null = null) {
    if (!detail || !selectedProfileId) {
      showNotice("请先导入小说并选择模型配置。");
      return;
    }
    if (!hasCompleteNovelSettings) {
      showNotice("请先填写设定");
      setSettingsDialog("basic");
      return;
    }
    const recovery = autoRunRecoveries.find((item) => item.novel_id === detail.novel.id);
    const startPosition = autoRunState === "paused" && recovery
      ? recovery.next_batch_index
      : startBatchId
        ? Math.max(0, detail.batches.findIndex((batch) => batch.id === startBatchId))
        : 0;
    const firstIncludedChapter = detail.batches[startPosition]?.start_chapter ?? Number.MAX_SAFE_INTEGER;
    if (!confirmRewriteOverwrite(detail.chapters.filter((chapter) => chapter.index >= firstIncludedChapter))) return;
    setBusy("auto");
    setAutoRunState("running");
    setAutoRunMode("range");
    setAutoRunMenuOpen(false);
    setNotice("");
    try {
      const result = await invoke("start_analyze_rewrite_all", {
        novelId: detail.novel.id,
        profileId: selectedProfileId,
        startBatchId
      });
      setJob(result);
      await loadNovel(detail.novel.id, {
        preserveBatchId: startBatchId ?? selectedBatchId,
        preserveChapterId: selectedChapterId
      });
      await refreshLogs(detail.novel.id);
      if (result.status === "completed") {
        setAutoRunState("idle");
        setAutoRunMode(null);
        setActiveView("compare");
      } else if (result.status === "paused") {
        setAutoRunState("paused");
      } else if (result.status === "terminated" || result.status === "failed") {
        setAutoRunState("idle");
        setAutoRunMode(null);
      }
      showNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      setAutoRunState("idle");
      setAutoRunMode(null);
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function pauseAnalyzeRewriteAll() {
    if (!detail || autoRunState !== "running") return;
    setAutoControlBusy(true);
    try {
      const result = await invoke("pause_analyze_rewrite_all", { novelId: detail.novel.id });
      setJob(result);
      setAutoRunState("stopping");
      showNotice(result.message);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setAutoControlBusy(false);
    }
  }

  async function terminateAnalyzeRewriteAll() {
    if (!detail || autoRunState === "idle") return;
    setAutoControlBusy(true);
    try {
      const result = await invoke("terminate_analyze_rewrite_all", { novelId: detail.novel.id });
      setJob(result);
      setAutoRunState("idle");
      setAutoRunMode(null);
      showNotice(result.message);
    } catch (error) {
      setAutoRunState("idle");
      setAutoRunMode(null);
      showNotice(String(error));
    } finally {
      setAutoControlBusy(false);
    }
  }

  const exportNovel = useCallback(async (format: "txt") => {
    if (!detail) return;
    setBusy(`export-${format}`);
    setNotice("");
    try {
      const result = await invoke("export_novel", {
        novelId: detail.novel.id,
        format
      });
      showNotice(`已导出：${result.path}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }, [detail, setBusy, showNotice]);

  function appSettingsPayload(overrides: Partial<AppSettings> = {}): AppSettings {
    return {
      export_dir: settings.export_dir ?? null,
      core_prompt: settings.core_prompt ?? "",
      review_enabled: settings.review_enabled ?? false,
      review_profile_id: settings.review_profile_id ?? null,
      analysis_profile_id: settings.analysis_profile_id ?? null,
      selected_profile_id: selectedProfileId || null,
      chapter_batch_size: settings.chapter_batch_size ?? 30,
      rewrite_parallelism: settings.rewrite_parallelism ?? 10,
      ...overrides
    };
  }

  async function chooseExportDir() {
    setBusy("choose-export-dir");
    setNotice("");
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ export_dir: selected })
      });
      setSettings(saved);
      showNotice(`已设置导出目录：${saved.export_dir}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function clearExportDir() {
    setBusy("clear-export-dir");
    setNotice("");
    try {
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ export_dir: null })
      });
      setSettings(saved);
      showNotice("已恢复默认导出目录。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function toggleReviewEnabled() {
    setBusy("review-setting");
    setNotice("");
    try {
      const nextEnabled = !(settings.review_enabled ?? false);
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ review_enabled: nextEnabled })
      });
      setSettings(saved);
      await refreshJobEstimate();
      showNotice(nextEnabled ? "已开启改写复检。" : "已关闭改写复检。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function setChapterBatchSize(value: 10 | 30 | 50 | 100) {
    setBusy("batch-size-setting");
    setNotice("");
    try {
      const previousParallelism = settings.rewrite_parallelism ?? 10;
      const preservedChapterId = selectedChapterId;
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ chapter_batch_size: value })
      });
      setSettings(saved);
      let nextBatchId = selectedBatchId || null;
      if (detail) {
        const next = await invoke("get_novel_detail", { novelId: detail.novel.id });
        setDetail(next);
        const nextChapterId = next.chapters.some((chapter) => chapter.id === preservedChapterId)
          ? preservedChapterId
          : next.chapters[0]?.id ?? "";
        nextBatchId = batchIdContainingChapter(next, nextChapterId) || null;
        setSelectedChapterId(nextChapterId);
        setSelectedBatchId(nextBatchId ?? "");
        await requestJobEstimate(
          next.novel.id,
          nextBatchId,
          selectedProfileId || null
        );
      }
      const clamped = saved.rewrite_parallelism !== previousParallelism;
      showNotice(
        clamped
          ? `已设置每批 ${value} 章并重新生成批次；当前并发已自动调整为 ${saved.rewrite_parallelism}。`
          : `已设置每批 ${value} 章并重新生成现有章节型小说批次。`
      );
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function setRewriteParallelism(value: 1 | 3 | 6 | 10 | 25 | 50) {
    setBusy("parallelism-setting");
    setNotice("");
    try {
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ rewrite_parallelism: value })
      });
      setSettings(saved);
      await refreshJobEstimate();
      showNotice(value === 1 ? "已切换为不并发处理。" : `已设置分析/改写并发请求数：${value}。`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function setReviewProfileId(value: string) {
    setBusy("review-profile-setting");
    setNotice("");
    try {
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ review_profile_id: value || null })
      });
      setSettings(saved);
      showNotice(value ? "已设置审查专家模型。" : "已恢复使用当前改写模型审查。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function setAnalysisProfileId(value: string) {
    setBusy("analysis-profile-setting");
    setNotice("");
    try {
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ analysis_profile_id: value || null })
      });
      setSettings(saved);
      await refreshJobEstimate();
      showNotice(value ? "已设置独立分析模型。" : "已恢复使用当前改写模型分析。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function saveCoreSettings() {
    setBusy("core-settings");
    setNotice("");
    try {
      const saved = await invoke("save_app_settings", {
        settings: appSettingsPayload({ core_prompt: corePromptDraft })
      });
      setSettings(saved);
      showNotice(corePromptDraft.trim() ? "核心设定已保存。" : "核心设定已清空。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function openGithubRepository() {
    setBusy("open-github");
    setNotice("");
    try {
      await invoke("open_github_url");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function openGithubReleasePage() {
    setBusy("open-github");
    try {
      await invoke("open_github_release_url");
    } catch (error) {
      showNotice(String(error), 60_000, Boolean(pendingUpdate));
    } finally {
      setBusy("");
    }
  }

  async function checkForUpdates() {
    setBusy("check-updates");
    setNotice("");
    setPendingUpdate(null);
    try {
      const update = await invoke("check_for_updates");
      if (update.is_latest) {
        setHasAvailableUpdate(false);
        showNotice(`当前已是最新版：${update.current_version}`, 3000);
        return;
      }

      setHasAvailableUpdate(true);
      setPendingUpdate(update);
      showNotice(`当前版本：${update.current_version}，最新版本：${update.latest_version}`, 60_000, true);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function downloadPendingUpdate() {
    setShowUpdateInstallDialog(false);
    setBusy("download-update");
    setUpdateProgress(null);
    setNotice("正在准备下载最新版…");
    let installStarted = false;
    try {
      const result = await invoke("download_latest_update");
      if (result.install_started) {
        installStarted = true;
        setNotice(result.message);
        return;
      }
      setPendingUpdate(null);
      showNotice(
        `已下载 v${result.version}：${result.path}。${result.message}`,
        60_000
      );
    } catch (error) {
      showNotice(String(error), 60_000, true);
    } finally {
      if (!installStarted) {
        setUpdateProgress(null);
        setBusy("");
      }
    }
  }

  function cancelPendingUpdateDownload() {
    setShowUpdateInstallDialog(false);
    setPendingUpdate(null);
    setNotice("");
    setActiveView("workspace");
  }

  function updateCanon(kind: string, content: string) {
    if (!detail) return;
    setDetail({
      ...detail,
      canon_assets: detail.canon_assets.map((asset) => (asset.kind === kind ? { ...asset, content } : asset))
    });
  }

  const displayChapterTitle = useCallback((chapter: Chapter) => {
    const title = chapter.title.replace(/\s+/g, " ").trim();
    return title || `第 ${chapter.index} 章`;
  }, []);

  const formatNumber = useCallback((value?: number | null) => {
    if (value === null || value === undefined) return "暂无";
    return new Intl.NumberFormat("zh-CN").format(Math.round(value));
  }, []);

  const formatSeconds = useCallback((value?: number | null) => {
    if (value === null || value === undefined) return "暂无历史数据";
    if (value < 60) return `${value.toFixed(1)} 秒`;
    const minutes = Math.floor(value / 60);
    const seconds = Math.round(value % 60);
    if (minutes < 60) return `${minutes} 分 ${seconds} 秒`;
    const hours = Math.floor(minutes / 60);
    const restMinutes = minutes % 60;
    return `${hours} 小时 ${restMinutes} 分`;
  }, []);

  const autoRemainingSeconds = useMemo(() => {
    if (!job || job.job_type !== "auto" || job.total_chapters <= 0) return null;
    if (!jobEstimate?.estimated_full_run_seconds) return null;
    const remainingBatches = Math.max(0, job.total_chapters - job.current_chapter);
    if (remainingBatches <= 0) return 0;
    const estimatedBatchCount = jobEstimate.novel_batches > 0 ? jobEstimate.novel_batches : job.total_chapters;
    const estimatedSecondsPerBatch = jobEstimate.estimated_full_run_seconds / estimatedBatchCount;
    return estimatedSecondsPerBatch * remainingBatches;
  }, [job, jobEstimate]);
  const displayedNotice = busy === "download-update" && updateProgress
    ? updateProgress.message
    : notice;
  const updateProgressPercent = updateProgress?.total_bytes
    ? Math.min(100, Math.round((updateProgress.downloaded_bytes / updateProgress.total_bytes) * 100))
    : null;

  const selectedRecovery = useMemo(
    () => autoRunRecoveries.find((recovery) => recovery.novel_id === detail?.novel.id),
    [autoRunRecoveries, detail?.novel.id]
  );
  const selectedChapterBatchPosition = useMemo(() => {
    if (!detail || !selectedChapter) return -1;
    return detail.batches.findIndex(
      (batch) => selectedChapter.index >= batch.start_chapter && selectedChapter.index <= batch.end_chapter
    );
  }, [detail, selectedChapter]);
  const compareEditingAllowed = Boolean(
    selectedChapter
      && busy === ""
      && autoRunState !== "running"
      && autoRunState !== "stopping"
      && (
        autoRunState !== "paused"
        || (selectedRecovery && selectedChapterBatchPosition >= 0 && selectedChapterBatchPosition < selectedRecovery.next_batch_index)
      )
  );
  const compareEditDisabledReason = autoRunState === "paused"
    ? "暂停任务当前未完成批次及后续批次不能编辑"
    : processingTaskActive
      ? "任务运行期间不能编辑改写稿"
      : busy
        ? "当前操作完成后可编辑"
        : undefined;

  const handleCompareBack = useCallback(() => setActiveView("workspace"), []);
  const handleCompareExport = useCallback(() => {
    void exportNovel("txt");
  }, [exportNovel]);

  const updateChapterFromBackend = useCallback((chapter: Chapter) => {
    const current = detailRef.current ?? useAppStore.getState().detail;
    if (!current || current.novel.id !== chapter.novel_id) return;
    const next = {
      ...current,
      chapters: current.chapters.map((item) => item.id === chapter.id ? chapter : item)
    };
    detailRef.current = next;
    setDetail(next);
  }, [setDetail]);

  const saveChapterRewriteEdit = useCallback(async (chapterId: string, rewriteText: string) => {
    try {
      const chapter = await invoke("save_chapter_rewrite_edit", { chapterId, rewriteText });
      updateChapterFromBackend(chapter);
      showNotice("当前章节人工修改已保存。");
    } catch (error) {
      showNotice(String(error));
      throw error;
    }
  }, [showNotice, updateChapterFromBackend]);

  const restoreChapterRewriteEdit = useCallback(async (chapterId: string) => {
    try {
      const chapter = await invoke("restore_chapter_rewrite_edit", { chapterId });
      updateChapterFromBackend(chapter);
      showNotice("当前章节已恢复到最近 AI 稿。");
    } catch (error) {
      showNotice(String(error));
      throw error;
    }
  }, [showNotice, updateChapterFromBackend]);

  const updateChapterTitle = useCallback(async (chapterId: string, title: string) => {
    try {
      const chapter = await invoke("update_chapter_title", { chapterId, title });
      updateChapterFromBackend(chapter);
      showNotice("章节名称已保存。");
    } catch (error) {
      showNotice(String(error));
      throw error;
    }
  }, [showNotice, updateChapterFromBackend]);

  const rewriteSingleChapter = useCallback(async (
    chapterId: string,
    instructions: string,
    sourceMode: "original" | "rewrite"
  ) => {
    const currentDetail = detailRef.current;
    const chapter = currentDetail?.chapters.find((item) => item.id === chapterId);
    if (!currentDetail || !chapter || !selectedProfileId) {
      throw new Error("当前小说、章节或改写模型不可用。");
    }
    setBusy("rewrite-chapter");
    setNotice(`正在重新改写《${chapter.title}》…`);
    try {
      const updated = await invoke("rewrite_single_chapter", {
        novelId: currentDetail.novel.id,
        profileId: selectedProfileId,
        chapterId,
        instructions,
        sourceMode
      });
      updateChapterFromBackend(updated);
      await refreshLogs(currentDetail.novel.id);
      showNotice(`已重新改写完成《${updated.title}》。`);
    } catch (error) {
      showNotice(String(error));
      throw error;
    } finally {
      setBusy("");
    }
  }, [selectedProfileId, setBusy, setNotice, showNotice, updateChapterFromBackend]);

  const restoreSingleChapterRewrite = useCallback(async (chapterId: string) => {
    const currentDetail = detailRef.current;
    const chapter = currentDetail?.chapters.find((item) => item.id === chapterId);
    if (!currentDetail || !chapter) {
      throw new Error("当前小说或章节不可用。");
    }
    setBusy("restore-rewrite-chapter");
    try {
      const restored = await invoke("restore_single_chapter_rewrite", { chapterId });
      updateChapterFromBackend(restored);
      showNotice(`已恢复《${restored.title}》的初稿。`);
    } catch (error) {
      showNotice(String(error));
      throw error;
    } finally {
      setBusy("");
    }
  }, [setBusy, showNotice, updateChapterFromBackend]);

  const terminateSingleChapterRewrite = useCallback(async () => {
    const currentDetail = detailRef.current;
    if (!currentDetail) {
      throw new Error("当前小说不可用。");
    }
    await invoke("terminate_single_chapter_rewrite", {
      novelId: currentDetail.novel.id
    });
    setNotice("正在终止当前单章重写任务…");
  }, [setNotice]);

  function diagnosisStatusText(status: DiagnosisStatus) {
    if (status === "ok") return "通过";
    if (status === "warning") return "警告";
    return "失败";
  }

  function closeQuickStart() {
    try {
      window.localStorage.setItem(quickStartSeenKey, "true");
    } catch {
      // Ignore storage failures; closing the dialog for this session still works.
    }
    setShowQuickStart(false);
  }

  return (
    <main className={dragActive ? "app-shell drag-active" : "app-shell"}>
      {dragActive && (
        <div className="drop-import-overlay" aria-live="polite">
          <div className="drop-import-card">
            <FilePlus2 size={30} />
            <strong>松开导入 TXT 小说</strong>
            <span>软件会自动识别章节并载入工作台</span>
          </div>
        </div>
      )}
      <nav className="app-menu">
        <button
          className={activeView === "compare" ? "app-menu-item active" : "app-menu-item"}
          onClick={() => setActiveView("compare")}
          disabled={!detail}
        >
          对比
        </button>
        <button
          className={activeView === "novel-settings" ? "app-menu-item active" : "app-menu-item"}
          onClick={openNovelSettings}
          disabled={!detail}
        >
          设定
        </button>
        <button
          className={activeView === "core-settings" ? "app-menu-item active" : "app-menu-item"}
          onClick={() => setActiveView("core-settings")}
        >
          核心设定
        </button>
        <button className="app-menu-item" onClick={() => setShowQuickStart(true)}>
          <HelpCircle size={16} />
          帮助
        </button>
        <div className="app-menu-spacer" />
        <button className="app-menu-item" onClick={openGithubRepository} disabled={busy !== ""}>
          <Github size={16} />
          GitHub地址
        </button>
        <button className="app-menu-item update-menu-item" onClick={checkForUpdates} disabled={busy !== ""}>
          {busy === "check-updates" || busy === "download-update" ? (
            <Loader2 className="spin" size={16} />
          ) : (
            <RefreshCw size={16} />
          )}
          检查更新
          {hasAvailableUpdate && <span className="update-dot" aria-label="发现新版本" />}
        </button>
      </nav>

      <aside className="sidebar">
        <button className="brand brand-button" onClick={() => setActiveView("workspace")}>
          <Sparkles size={22} />
          <div>
            <strong>Yuri Rewrite</strong>
            <span>本地小说分析与改写</span>
          </div>
        </button>

        <button className="primary-action" onClick={importTxt} disabled={busy === "import" || processingTaskActive}>
          {busy === "import" ? <Loader2 className="spin" size={18} /> : <FilePlus2 size={18} />}
          导入 TXT
        </button>

        <div className="side-section">
          <div className="section-label">小说</div>
          <div className="novel-list">
            {novels.map((novel) => (
              <div className="novel-row" key={novel.id}>
                <button
                  className={detail?.novel.id === novel.id ? "novel-item active" : "novel-item"}
                  onClick={() => loadNovel(novel.id)}
                  disabled={(autoRunState === "running" || autoRunState === "stopping" || ["analysis", "rewrite", "auto-batch"].includes(busy)) && detail?.novel.id !== novel.id}
                >
                  <BookOpen size={16} />
                  <span>{novel.title}</span>
                  {autoRunRecoveries.some((recovery) => recovery.novel_id === novel.id) && (
                    <small className="novel-recovery-badge">未完成</small>
                  )}
                </button>
                <button
                  className="icon-button menu-trigger"
                  aria-label={`打开《${novel.title}》菜单`}
                  onClick={() => setOpenNovelMenuId(openNovelMenuId === novel.id ? "" : novel.id)}
                  disabled={processingTaskActive}
                >
                  <MoreHorizontal size={17} />
                </button>
                {openNovelMenuId === novel.id && (
                  <div className="context-menu">
                    <button onClick={() => deleteNovel(novel)} disabled={busy === "delete-novel" || processingTaskActive}>
                      <Trash2 size={15} />
                      删除当前小说
                    </button>
                  </div>
                )}
              </div>
            ))}
            {novels.length === 0 && <p className="muted">尚未导入小说。</p>}
          </div>
        </div>

        <div className="side-section">
          <div className="section-label">改写模型</div>
          <ModelProfiles
            profiles={profiles}
            selectedProfileId={selectedProfileId}
            menuOpen={openModelMenu}
            processing={adjustableWhilePaused}
            busy={busy}
            onSelect={selectModelProfile}
            onMenuOpenChange={setOpenModelMenu}
            onDelete={deleteSelectedModelProfile}
          />
        </div>

        <div className="side-section nav-section">
          <button
            className={activeView === "logs" ? "nav-button active" : "nav-button"}
            onClick={() => setActiveView("logs")}
          >
            <ClipboardList size={17} />
            日志
          </button>
          <button
            className={activeView === "token-stats" ? "nav-button active" : "nav-button"}
            onClick={openTokenStats}
            disabled={busy !== ""}
          >
            <ChartNoAxesCombined size={17} />
            Token统计
          </button>
        </div>

        <div className="sidebar-spacer" />

        <button
          className={activeView === "settings" ? "nav-button active" : "nav-button"}
          onClick={() => setActiveView("settings")}
        >
          <Settings size={17} />
          设置
        </button>
      </aside>

      <section className="workspace">
        {!["compare", "settings", "token-stats", "logs"].includes(activeView) && (
        <header className="topbar">
          <div>
            <h1>
              {activeView === "logs"
                ? "日志"
                : activeView === "token-stats"
                  ? "Token统计"
                : activeView === "settings"
                  ? "设置"
                  : activeView === "novel-settings"
                    ? "基本设定"
                  : activeView === "compare"
                    ? "对比"
                    : detail?.novel.title ?? "工作台"}
            </h1>
            <p>
              {activeView === "logs"
                ? "查看 AI 调用的思考过程与原始输出"
                : activeView === "token-stats"
                  ? "按模型和日期查看请求次数、输入与输出 Token"
                : activeView === "settings"
                  ? ""
                  : activeView === "novel-settings"
                    ? detail
                      ? `绑定《${detail.novel.title}》的改写规则`
                      : "导入小说后配置基本设定"
                  : activeView === "compare"
                    ? "左侧原文，右侧改写稿"
                    : detail
                      ? `${detail.chapters.length} 章 · ${detail.novel.encoding} · ${statusText[detail.novel.status] ?? detail.novel.status}`
                      : "导入 TXT 后开始分析和改写"}
            </p>
          </div>
          {activeView === "workspace" && (
            <div className="topbar-actions">
              {autoRunState !== "idle" && (
                <>
                  {autoRunMode !== "batch" && (
                    <button
                      className="task-control-warning"
                      onClick={autoRunState === "paused" ? () => void runAnalyzeRewriteAll() : pauseAnalyzeRewriteAll}
                      disabled={autoControlBusy || autoRunState === "stopping"}
                      title={autoRunState === "paused" ? "继续一键分析改写" : "暂停一键分析改写"}
                    >
                      {autoControlBusy || autoRunState === "stopping" ? (
                        <Loader2 className="spin" size={17} />
                      ) : autoRunState === "paused" ? (
                        <Play size={17} />
                      ) : (
                        <Pause size={17} />
                      )}
                      {autoRunState === "paused" ? "继续" : "暂停"}
                    </button>
                  )}
                  <button className="task-control-danger" onClick={terminateAnalyzeRewriteAll} disabled={autoControlBusy} title="终止一键分析改写">
                    {autoControlBusy ? <Loader2 className="spin" size={17} /> : <Square size={17} />}
                    终止
                  </button>
                </>
              )}
              <div className="split-button task-primary-split" ref={autoRunMenuRef}>
                <button
                  className="split-button-main action-primary"
                  onClick={() => void runAnalyzeRewriteAll()}
                  disabled={!detail || !selectedProfileId || busy !== "" || autoRunState !== "idle"}
                  title="AI自动分析改写全文，耗时较久"
                >
                  {busy === "auto" ? <Loader2 className="spin" size={17} /> : <Sparkles size={17} />}
                  一键分析改写
                </button>
                <button
                  className="split-button-toggle action-primary"
                  aria-label="一键分析改写选项"
                  aria-expanded={autoRunMenuOpen}
                  onClick={() => setAutoRunMenuOpen((open) => !open)}
                  disabled={!detail || !selectedProfileId || !selectedBatch || busy !== "" || autoRunState !== "idle"}
                  title="更多一键分析改写选项"
                >
                  <ChevronDown size={16} />
                </button>
                {autoRunMenuOpen && selectedBatch && (
                  <div className="split-button-menu" role="menu">
                    <button
                      role="menuitem"
                      onClick={() => void runAnalyzeRewriteAll(selectedBatch.id)}
                    >
                      从当前批次开始一键分析改写
                    </button>
                  </div>
                )}
              </div>
              <button
                className="action-primary"
                onClick={runAnalyzeRewriteCurrentBatch}
                disabled={!detail || !selectedProfileId || !selectedBatch || busy !== "" || autoRunState !== "idle"}
                title="AI自动分析并改写当前选中批次"
              >
                {busy === "auto-batch" ? <Loader2 className="spin" size={17} /> : <Sparkles size={17} />}
                一键分析改写当前批次
              </button>
              <div className="task-secondary-group">
                <button
                  onClick={() => runJob("analysis")}
                  disabled={!detail || !selectedProfileId || !selectedBatch || busy !== "" || autoRunState !== "idle"}
                >
                  {busy === "analysis" ? <Loader2 className="spin" size={17} /> : <Play size={17} />}
                  分析
                </button>
                <button
                  onClick={() => runJob("rewrite")}
                  disabled={!detail || !selectedProfileId || !selectedBatch || busy !== "" || autoRunState !== "idle"}
                >
                  {busy === "rewrite" ? <Loader2 className="spin" size={17} /> : <RefreshCw size={17} />}
                  改写
                </button>
              </div>
            </div>
          )}
        </header>
        )}

        {displayedNotice && (
          <div className="notice notice-panel">
            <span>{displayedNotice}</span>
            {busy === "download-update" && updateProgress?.stage === "downloading" && (
              <div className="update-download-progress" aria-label="更新包下载进度">
                <div className="update-download-progress-track">
                  <div
                    className="update-download-progress-fill"
                    style={{ width: updateProgressPercent === null ? "18%" : `${updateProgressPercent}%` }}
                  />
                </div>
                <strong>{updateProgressPercent === null ? "下载中" : `${updateProgressPercent}%`}</strong>
              </div>
            )}
            {pendingUpdate && busy !== "download-update" && (
              <div className="notice-actions">
                <button
                  onClick={() => setShowUpdateInstallDialog(true)}
                  disabled={processingTaskActive || busy !== ""}
                  title={processingTaskActive ? "当前任务结束后才能安装更新" : undefined}
                >
                  <Download size={16} />
                  下载并安装最新版
                </button>
                <button onClick={openGithubReleasePage} disabled={busy !== ""}>
                  查看发布页
                </button>
                <button onClick={cancelPendingUpdateDownload} disabled={busy !== ""}>
                  暂不更新
                </button>
              </div>
            )}
          </div>
        )}
        {modelDiagnosis && (
          <div className={`diagnosis-panel diagnosis-top-panel status-container status-${getStatusTone(modelDiagnosis.status)}`}>
            <div className="diagnosis-heading">
              <strong>诊断结果</strong>
              <StatusBadge
                status={modelDiagnosis.status}
                label={diagnosisStatusText(modelDiagnosis.status)}
              />
              {modelDiagnosis.recommended_thinking_mode && (
                <span className="diagnosis-recommendation">建议思考模式：{modelDiagnosis.recommended_thinking_mode}</span>
              )}
              <button
                className="icon-button diagnosis-close"
                type="button"
                aria-label="关闭诊断结果"
                onClick={() => setModelDiagnosis(null)}
              >
                <X size={16} />
              </button>
            </div>
            <div className="diagnosis-list">
              {modelDiagnosis.checks.map((check) => (
                <div className="diagnosis-item" key={`${check.name}-${check.message}`}>
                  <StatusBadge
                    status={check.status}
                    label={diagnosisStatusText(check.status)}
                    showDot={false}
                  />
                  <div>
                    <strong>{check.name}</strong>
                    <p>{check.message}</p>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
        {job && activeView === "workspace" && (
          <div className={`job-strip status-container status-${getStatusTone(job.status)}`}>
            <CheckCircle2 size={17} />
            <div className="job-content">
              <span className="job-summary">
                <span>{job.job_type === "auto_batch" ? "当前批次一键任务" : job.job_type}</span>
                <StatusBadge status={job.status} label={statusText[job.status] ?? job.status} />
                <span>{job.current_chapter}/{job.total_chapters} · {job.message}</span>
                {job.job_type === "auto" && autoRemainingSeconds !== null && job.status === "running"
                  ? ` · 预计剩余 ${formatSeconds(autoRemainingSeconds)}`
                  : ""}
              </span>
              {["auto", "auto_batch"].includes(job.job_type) && (
                <>
                  <div className="job-progress-row" aria-label={`一键分析改写进度 ${autoProgressPercent}%`}>
                    <div className="job-progress-bar">
                      <div className="job-progress-fill" style={{ width: `${autoProgressPercent}%` }} />
                    </div>
                    <strong>{autoProgressPercent}%</strong>
                  </div>
                  {job.shard_total !== undefined && job.shard_total > 0 && (
                    <div className="job-stage-progress">
                      <div className="job-stage-summary">
                        <span>
                          第 {job.batch_index ?? "—"}/{job.batch_total ?? job.total_chapters} 批
                          {job.phase ? ` · ${jobPhaseText[job.phase] ?? job.phase}` : ""}
                          {job.chapter_total !== undefined
                            ? ` · 章节 ${job.chapter_completed ?? 0}/${job.chapter_total}`
                            : ""}
                          {` · 分片 ${job.shard_completed ?? 0}/${job.shard_total}`}
                        </span>
                        <div
                          className="job-stage-bar"
                          aria-label={
                            job.chapter_total !== undefined
                              ? `当前阶段章节进度 ${job.chapter_completed ?? 0}/${job.chapter_total}`
                              : `当前阶段分片进度 ${job.shard_completed ?? 0}/${job.shard_total}`
                          }
                        >
                          <div
                            className="job-stage-fill"
                            style={{
                              width: `${Math.round(
                                job.chapter_total
                                  ? ((job.chapter_completed ?? 0) / job.chapter_total) * 100
                                  : ((job.shard_completed ?? 0) / job.shard_total) * 100
                              )}%`
                            }}
                          />
                        </div>
                      </div>
                      {job.active_shards && job.active_shards.length > 0 && (
                        <div className="job-active-shards">
                          {job.active_shards.slice(0, 3).map((shard) => (
                            <span key={`${shard.index}-${shard.phase}`}>
                              {`${shard.index}/${shard.total} 第${shard.start_chapter}${shard.end_chapter === shard.start_chapter ? "" : `-${shard.end_chapter}`}章（${jobPhaseText[shard.phase] ?? shard.phase}）`}
                            </span>
                          ))}
                          {job.active_shards.length > 3 && <span>另有 {job.active_shards.length - 3} 个处理中</span>}
                        </div>
                      )}
                    </div>
                  )}
                </>
              )}
            </div>
            {["completed", "failed", "paused", "terminated"].includes(job.status) && (
              <button
                className="icon-button job-strip-close"
                type="button"
                aria-label="关闭任务提示"
                onClick={() => setJob(null)}
              >
                <X size={15} />
              </button>
            )}
          </div>
        )}

        {activeView === "logs" && (
          <LogsPage
            logs={logs}
            busy={busy}
            onBack={() => setActiveView("workspace")}
            onClear={clearLogs}
            onRefresh={() => refreshLogs()}
          />
        )}

        {activeView === "token-stats" && (
          <TokenStatsPage
            report={tokenStats}
            startDate={tokenStatsStartDate}
            endDate={tokenStatsEndDate}
            busy={tokenStatsLoading}
            onStartDateChange={changeTokenStatsStartDate}
            onEndDateChange={changeTokenStatsEndDate}
            onRefresh={() => { void refreshTokenStats(); }}
            onBack={() => setActiveView("workspace")}
          />
        )}

        {activeView === "novel-settings" && (
          <NovelSettingsView
            draft={novelSettingsDraft}
            setDraft={setNovelSettingsDraft}
            disabled={processingTaskActive}
            hasNovel={Boolean(detail)}
            busy={busy}
            onBack={() => setActiveView("workspace")}
            onSave={saveNovelSettings}
          />
        )}

        {activeView === "core-settings" && (
          <CoreSettingsPage
            value={corePromptDraft}
            busy={busy === "core-settings"}
            disabled={processingTaskActive}
            onChange={setCorePromptDraft}
            onBack={() => setActiveView("workspace")}
            onSave={saveCoreSettings}
          />
        )}

        {activeView === "settings" && (
          <SettingsPage
            settings={settings}
            profiles={profiles}
            busy={busy}
            processing={processingTaskActive}
            pausedAutoRun={pausedAutoRun}
            onBack={() => setActiveView("workspace")}
            onChooseExportDir={chooseExportDir}
            onClearExportDir={clearExportDir}
            onToggleReview={toggleReviewEnabled}
            onReviewProfileChange={setReviewProfileId}
            onAnalysisProfileChange={setAnalysisProfileId}
            onBatchSizeChange={setChapterBatchSize}
            onParallelismChange={setRewriteParallelism}
          />
        )}

        {activeView === "compare" && (
          <CompareView
            chapters={detail?.chapters ?? []}
            selectedChapter={selectedChapter}
            selectedChapterId={selectedChapterId}
            busy={busy}
            originalRef={originalCompareRef}
            rewriteRef={rewriteCompareRef}
            onSelectChapter={setSelectedChapterId}
            onBack={handleCompareBack}
            onExport={handleCompareExport}
            editingAllowed={compareEditingAllowed}
            editDisabledReason={compareEditDisabledReason}
            onSaveRewrite={saveChapterRewriteEdit}
            onRestoreRewrite={restoreChapterRewriteEdit}
            onRewriteChapter={rewriteSingleChapter}
            onTerminateRewrite={terminateSingleChapterRewrite}
            onRestoreInitialRewrite={restoreSingleChapterRewrite}
          />
        )}

        {activeView === "workspace" && (
          <>
          {detail && (
            <BatchPanel
              batches={detail.batches}
              selectedBatch={selectedBatch}
              selectedBatchId={selectedBatchId}
              onSelect={setSelectedBatchId}
              onOpenCanon={() => setWorkspaceSection("canon")}
            />
          )}
          {detail && jobEstimate && (
            <TaskEstimate
              estimate={jobEstimate}
              collapsed={estimateCollapsed}
              onToggle={() => setEstimateCollapsed((value) => !value)}
              formatNumber={formatNumber}
              formatSeconds={formatSeconds}
            />
          )}
          {workspaceSection === "main" ? (
            <div className="content-grid workspace-main-grid">
              <ModelConfig
                draft={profileDraft}
                setDraft={setProfileDraft}
                selectedProfile={selectedProfile}
                selectedProfileId={selectedProfileId}
                suggestions={detectedModelSuggestions}
                suggestionsOpen={openModelSuggestions}
                busy={busy}
                processing={adjustableWhilePaused}
                savedApiKeyMask={savedApiKeyMask}
                onSuggestionsOpenChange={setOpenModelSuggestions}
                onCreate={createNewModelProfile}
                onDiagnose={diagnoseProfile}
                onSave={saveProfile}
              />
              <ChapterList
                chapters={detail?.chapters ?? []}
                selectedChapterId={selectedChapter?.id}
                onSelect={setSelectedChapterId}
                displayTitle={displayChapterTitle}
                statusText={statusText}
                onRenameChapter={updateChapterTitle}
                titleEditDisabledReason={processingTaskActive || autoRunState !== "idle"
                  ? "任务运行或暂停期间不能修改章节名称"
                  : busy
                    ? "当前操作完成后可编辑章节名称"
                    : undefined}
              />
            </div>
          ) : (
            <section className="panel canon-workspace-page">
              <div className="panel-heading">
                <div className="canon-workspace-heading">
                  <button type="button" onClick={() => setWorkspaceSection("main")}>
                    <ArrowLeft size={16} />返回工作台
                  </button>
                  <h2>一致性资产</h2>
                </div>
                <button onClick={saveCanonAssets} disabled={!detail || busy === "canon" || processingTaskActive}>
                  {busy === "canon" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                  保存
                </button>
              </div>
              <div className="asset-stack">
                {detail?.canon_assets.map((asset) => (
                  <label key={asset.kind}>
                    {asset.kind}
                    <textarea
                      value={asset.content}
                      onChange={(event) => updateCanon(asset.kind, event.target.value)}
                      placeholder="分析后会自动生成，也可以手动补充。"
                      disabled={processingTaskActive}
                    />
                  </label>
                ))}
              </div>
            </section>
          )}
          </>
        )}
      </section>
      {settingsDialog && (
        <Modal labelledBy="settings-dialog-title">
            {settingsDialog === "basic" ? (
              <>
                <header className="dialog-titlebar">
                  <h2 id="settings-dialog-title">基本设定</h2>
                  <button
                    className="dialog-close"
                    type="button"
                    aria-label="关闭基本设定"
                    title="关闭"
                    onClick={() => setSettingsDialog(null)}
                  >
                    <X size={16} />
                  </button>
                </header>
                <div className="dialog-body">
                  <NovelSettingsFields
                    draft={novelSettingsDraft}
                    setDraft={setNovelSettingsDraft}
                    disabled={processingTaskActive}
                  />
                </div>
                <footer className="dialog-actions">
                  <button onClick={() => setSettingsDialog("advanced")} disabled={busy === "novel-settings" || processingTaskActive}>
                    高级设定
                  </button>
                  <button className="dialog-primary" onClick={saveNovelSettings} disabled={!detail || busy === "novel-settings" || processingTaskActive}>
                    确定
                  </button>
                </footer>
              </>
            ) : (
              <>
                <header className="dialog-titlebar">
                  <h2 id="settings-dialog-title">高级设定</h2>
                  <button
                    className="dialog-close"
                    type="button"
                    aria-label="关闭高级设定"
                    title="关闭"
                    onClick={() => setSettingsDialog(null)}
                  >
                    <X size={16} />
                  </button>
                </header>
                <div className="dialog-body">
                  <label>
                    自定义设定
                    <textarea
                      className="advanced-settings-input"
                      value={novelSettingsDraft.advanced_settings}
                      onChange={(event) =>
                        setNovelSettingsDraft({ ...novelSettingsDraft, advanced_settings: event.target.value })
                      }
                      placeholder="你可以自由输入你需要加入的设定"
                    />
                  </label>
                </div>
                <footer className="dialog-actions">
                  <button className="dialog-primary" onClick={() => setSettingsDialog("basic")}>
                    确定
                  </button>
                </footer>
              </>
            )}
        </Modal>
      )}
      {novelPendingDeletion && (
        <DeleteNovelDialog
          busy={busy === "delete-novel"}
          novel={novelPendingDeletion}
          onCancel={() => setNovelPendingDeletion(null)}
          onConfirm={confirmDeleteNovel}
        />
      )}
      {pendingUpdate && showUpdateInstallDialog && (
        <UpdateInstallDialog
          busy={busy === "download-update"}
          processingTaskActive={processingTaskActive}
          update={pendingUpdate}
          onCancel={() => setShowUpdateInstallDialog(false)}
          onConfirm={downloadPendingUpdate}
        />
      )}
      {showQuickStart && (
        <Modal className="quickstart-dialog" labelledBy="quickstart-title">
            <div className="quickstart-content">
              <h2 id="quickstart-title">快速上手</h2>
              <ol>
                <li>点击导入 TXT，软件会自动识别章节，并按批次整理。</li>
                <li>先配置模型，填写 Base URL、模型 ID 和 API Key，保存后点击诊断模型。</li>
                <li>进入设定，填写主角原名、改写后姓名、身材体型、改写模式和额外要求。</li>
                <li>建议先处理一个批次：点击分析，再点击改写，确认效果稳定后再使用一键分析改写。</li>
                <li>如需更严格检查，可在设置中开启改写复检；复检会增加请求数、等待时间和 token 消耗。</li>
                <li>一键分析改写会按批次连续处理；运行中可暂停、继续或终止，限流/网络中断后也可调整设置再继续。</li>
                <li>改写完成后进入对比页面，可搜索、查看差异并导出 TXT；导出只包含已完成改写的章节。</li>
              </ol>
              <p className="quickstart-tip">
                温馨提示：如果发现 API 调用异常缓慢，可以尝试删除{" "}
                <code>C:\Users\你的用户名\AppData\Roaming\com.local.yurirewrite</code>{" "}
                文件夹后重试。
              </p>
              <button className="dialog-primary quickstart-confirm" onClick={closeQuickStart}>
                确定
              </button>
            </div>
        </Modal>
      )}
    </main>
  );
}
