import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  BookOpen,
  CheckCircle2,
  ChevronDown,
  ClipboardList,
  Download,
  FilePlus2,
  FolderOpen,
  Github,
  HelpCircle,
  KeyRound,
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
import { useEffect, useMemo, useRef, useState } from "react";

type Novel = {
  id: string;
  title: string;
  source_path: string;
  encoding: string;
  status: string;
  created_at: string;
};

type Chapter = {
  id: string;
  novel_id: string;
  index: number;
  title: string;
  original_text: string;
  analysis_json?: string | null;
  rewrite_text?: string | null;
  analysis_status: string;
  rewrite_status: string;
};

type CanonAsset = {
  novel_id: string;
  kind: string;
  content: string;
  updated_at: string;
};

type ChapterBatch = {
  id: string;
  novel_id: string;
  batch_index: number;
  label: string;
  start_chapter: number;
  end_chapter: number;
  file_path: string;
  created_at: string;
};

type NovelSettings = {
  novel_id: string;
  protagonist_name: string;
  rewritten_protagonist_name: string;
  additional_feminize_names: string;
  bust: string;
  body_type: string;
  rewrite_mode: "strict" | "creative";
  advanced_settings: string;
  updated_at: string;
};

type NovelDetail = {
  novel: Novel;
  chapters: Chapter[];
  canon_assets: CanonAsset[];
  batches: ChapterBatch[];
  settings?: NovelSettings | null;
};

type ModelProfile = {
  id: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
  thinking_mode: "auto" | "off" | "on";
  has_api_key: boolean;
  updated_at: string;
};

type Job = {
  id: string;
  novel_id: string;
  job_type: string;
  status: string;
  current_chapter: number;
  total_chapters: number;
  message: string;
};

type AiLog = {
  id: string;
  novel_id?: string | null;
  profile_id: string;
  action: string;
  chapter_title?: string | null;
  status: string;
  content: string;
  reasoning?: string | null;
  raw_response?: string | null;
  created_at: string;
};

type AppSettings = {
  export_dir?: string | null;
  core_prompt?: string;
  review_enabled?: boolean;
  review_profile_id?: string | null;
  rewrite_parallelism?: 1 | 3 | 6 | 10;
};

type UpdateCheckResult = {
  current_version: string;
  latest_version: string;
  latest_tag: string;
  is_latest: boolean;
  release_url: string;
  asset_name: string;
  asset_download_url: string;
};

type UpdateDownloadResult = {
  path: string;
  version: string;
};

type JobEstimate = {
  novel_chapters: number;
  novel_chars: number;
  novel_batches: number;
  selected_batch_chapters: number;
  selected_batch_chars: number;
  parallelism: number;
  review_enabled: boolean;
  current_batch_requests: number;
  full_run_requests: number;
  average_call_seconds?: number | null;
  estimated_current_batch_seconds?: number | null;
  estimated_full_run_seconds?: number | null;
  recent_success_calls: number;
  recent_failed_calls: number;
  average_input_chars?: number | null;
  average_output_chars?: number | null;
};

type DiagnosisStatus = "ok" | "warning" | "failed";

type ModelDiagnosis = {
  status: DiagnosisStatus;
  recommended_thinking_mode?: "auto" | "off" | "on" | null;
  checks: Array<{
    name: string;
    status: DiagnosisStatus;
    message: string;
  }>;
};

type ProfileDraft = {
  id?: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
  thinking_mode: "auto" | "off" | "on";
  api_key: string;
};

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

type NovelSettingsDraft = {
  protagonist_name: string;
  rewritten_protagonist_name: string;
  additional_feminize_names: string;
  bust: string;
  body_type: string;
  rewrite_mode: "strict" | "creative";
  advanced_settings: string;
};

const emptyProfile: ProfileDraft = {
  name: "OpenAI 兼容接口",
  provider: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  model: "请填写模型名",
  temperature: 0.7,
  thinking_mode: "auto",
  api_key: ""
};

const emptyNovelSettings: NovelSettingsDraft = {
  protagonist_name: "",
  rewritten_protagonist_name: "",
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: ""
};

const savedApiKeyMask = "********";
const quickStartSeenKey = "yuri-rewrite.quick-start-seen";
const thinkingModeTooltip =
  "建议自动；分析阶段通常关闭更快\n兼容性：OpenAI 推理模型可控；DeepSeek V4 与 Kimi K2.5 支持 thinking 开关；Gemini 2.5 用 thinkingBudget；SiliconFlow 推理模型用 thinking_budget；Claude 原生 API 支持 extended/adaptive thinking；MiniMax/MiMo/Claude 转发取决于服务商，不支持时会自动降级";
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
    modelTerms: ["qwen/", "thudm/", "deepseek-ai/", "internlm/", "mistralai/"],
    models: [
      { label: "Qwen2 72B Instruct", model: "Qwen/Qwen2-72B-Instruct" },
      { label: "Qwen2 57B A14B Instruct", model: "Qwen/Qwen2-57B-A14B-Instruct" },
      { label: "Qwen2 7B Instruct", model: "Qwen/Qwen2-7B-Instruct" },
      { label: "Qwen2 1.5B Instruct", model: "Qwen/Qwen2-1.5B-Instruct" },
      { label: "GLM-4 9B Chat", model: "THUDM/glm-4-9b-chat" },
      { label: "ChatGLM3 6B", model: "THUDM/chatglm3-6b" },
      { label: "DeepSeek Coder V2 Instruct", model: "deepseek-ai/DeepSeek-Coder-V2-Instruct" },
      { label: "DeepSeek V2 Chat", model: "deepseek-ai/DeepSeek-V2-Chat" },
      { label: "InternLM2.5 7B Chat", model: "internlm/internlm2_5-7b-chat" },
      { label: "Mistral 7B Instruct v0.2", model: "mistralai/Mistral-7B-Instruct-v0.2" }
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

export default function App() {
  const [novels, setNovels] = useState<Novel[]>([]);
  const [detail, setDetail] = useState<NovelDetail | null>(null);
  const [profiles, setProfiles] = useState<ModelProfile[]>([]);
  const [profileDraft, setProfileDraft] = useState<ProfileDraft>(emptyProfile);
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [selectedChapterId, setSelectedChapterId] = useState("");
  const [selectedBatchId, setSelectedBatchId] = useState("");
  const [openNovelMenuId, setOpenNovelMenuId] = useState("");
  const [openModelMenu, setOpenModelMenu] = useState(false);
  const [openModelSuggestions, setOpenModelSuggestions] = useState(false);
  const [logs, setLogs] = useState<AiLog[]>([]);
  const [settings, setSettings] = useState<AppSettings>({});
  const [corePromptDraft, setCorePromptDraft] = useState("");
  const [jobEstimate, setJobEstimate] = useState<JobEstimate | null>(null);
  const [estimateCollapsed, setEstimateCollapsed] = useState(false);
  const [modelDiagnosis, setModelDiagnosis] = useState<ModelDiagnosis | null>(null);
  const [novelSettingsDraft, setNovelSettingsDraft] = useState<NovelSettingsDraft>(emptyNovelSettings);
  const [settingsDialog, setSettingsDialog] = useState<"basic" | "advanced" | null>(null);
  const [activeView, setActiveView] = useState<"workspace" | "compare" | "novel-settings" | "core-settings" | "logs" | "settings">("workspace");
  const [busy, setBusy] = useState("");
  const [autoRunState, setAutoRunState] = useState<"idle" | "running" | "paused" | "stopping">("idle");
  const [autoControlBusy, setAutoControlBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [noticeDuration, setNoticeDuration] = useState(5000);
  const [pendingUpdate, setPendingUpdate] = useState<UpdateCheckResult | null>(null);
  const [hasAvailableUpdate, setHasAvailableUpdate] = useState(false);
  const [job, setJob] = useState<Job | null>(null);
  const [showQuickStart, setShowQuickStart] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const originalCompareRef = useRef<HTMLPreElement | null>(null);
  const rewriteCompareRef = useRef<HTMLPreElement | null>(null);
  const busyRef = useRef("");
  const importInProgressRef = useRef(false);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? detail?.chapters[0],
    [detail, selectedChapterId]
  );

  const selectedBatch = useMemo(
    () => detail?.batches.find((batch) => batch.id === selectedBatchId) ?? detail?.batches[0],
    [detail, selectedBatchId]
  );

  const autoProgressPercent = useMemo(() => {
    if (!job || job.job_type !== "auto" || job.total_chapters <= 0) return 0;
    return Math.min(100, Math.max(0, Math.round((job.current_chapter / job.total_chapters) * 100)));
  }, [job]);

  const detectedModelSuggestions = useMemo(
    () => getModelSuggestions(profileDraft),
    [profileDraft.provider, profileDraft.base_url, profileDraft.model]
  );

  const hasCompleteNovelSettings = Boolean(
    detail?.settings?.protagonist_name?.trim() && detail.settings.bust?.trim() && detail.settings.body_type?.trim()
  );

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
      if (busyRef.current) {
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

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<Job>("job-progress", (event) => {
      if (event.payload.job_type !== "auto") return;
      setJob(event.payload);
      if (event.payload.status === "running") {
        setAutoRunState("running");
      } else if (event.payload.status === "paused") {
        setAutoRunState("paused");
      } else if (event.payload.status === "pausing" || event.payload.status === "terminating") {
        setAutoRunState("stopping");
      } else if (["completed", "failed", "terminated"].includes(event.payload.status)) {
        setAutoRunState("idle");
      }
    }).then((handler) => {
      if (cancelled) {
        handler();
      } else {
        unlisten = handler;
      }
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    if (detectedModelSuggestions.length === 0) setOpenModelSuggestions(false);
  }, [detectedModelSuggestions.length]);

  useEffect(() => {
    setCorePromptDraft(settings.core_prompt ?? "");
  }, [settings.core_prompt]);

  useEffect(() => {
    let cancelled = false;
    async function checkStartupUpdate() {
      try {
        const update = await invoke<UpdateCheckResult>("check_for_updates");
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
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(""), noticeDuration);
    return () => window.clearTimeout(timer);
  }, [notice, noticeDuration]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && activeView !== "workspace" && !settingsDialog) {
        setActiveView("workspace");
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [activeView, settingsDialog]);

  useEffect(() => {
    const profile = profiles.find((item) => item.id === selectedProfileId);
    setModelDiagnosis(null);
    if (!profile) return;
    setProfileDraft({
      id: profile.id,
      name: profile.name,
      provider: profile.provider,
      base_url: profile.base_url,
      model: profile.model,
      temperature: profile.temperature,
      thinking_mode: profile.thinking_mode === "off" || profile.thinking_mode === "on" ? profile.thinking_mode : "auto",
      api_key: profile.has_api_key ? savedApiKeyMask : ""
    });
  }, [profiles, selectedProfileId]);

  useEffect(() => {
    if (originalCompareRef.current) originalCompareRef.current.scrollTop = 0;
    if (rewriteCompareRef.current) rewriteCompareRef.current.scrollTop = 0;
  }, [selectedChapterId]);

  useEffect(() => {
    let cancelled = false;
    async function loadEstimate() {
      if (!detail) {
        setJobEstimate(null);
        return;
      }
      try {
        const estimate = await invoke<JobEstimate>("estimate_job_cost", {
          novelId: detail.novel.id,
          batchId: selectedBatchId || null,
          profileId: selectedProfileId || null
        });
        if (!cancelled) setJobEstimate(estimate);
      } catch {
        if (!cancelled) setJobEstimate(null);
      }
    }
    void loadEstimate();
    return () => {
      cancelled = true;
    };
  }, [detail?.novel.id, selectedBatchId, selectedProfileId, settings.review_enabled, settings.rewrite_parallelism]);

  async function refreshAll() {
    const [novelRows, profileRows, appSettings] = await Promise.all([
      invoke<Novel[]>("list_novels"),
      invoke<ModelProfile[]>("list_model_profiles"),
      invoke<AppSettings>("get_app_settings")
    ]);
    setNovels(novelRows);
    setProfiles(profileRows);
    setSettings(appSettings);
    if (!selectedProfileId && profileRows[0]) setSelectedProfileId(profileRows[0].id);
    if (!detail && novelRows[0]) await loadNovel(novelRows[0].id);
    await refreshLogs();
  }

  async function loadNovel(novelId: string, options: { preserveBatchId?: string; preserveChapterId?: string } = {}) {
    const next = await invoke<NovelDetail>("get_novel_detail", { novelId });
    setDetail(next);
    const nextChapterId =
      options.preserveChapterId && next.chapters.some((chapter) => chapter.id === options.preserveChapterId)
        ? options.preserveChapterId
        : next.chapters[0]?.id ?? "";
    const nextBatchId =
      options.preserveBatchId && next.batches.some((batch) => batch.id === options.preserveBatchId)
        ? options.preserveBatchId
        : next.batches[0]?.id ?? "";
    setSelectedChapterId(nextChapterId);
    setSelectedBatchId(nextBatchId);
    setNovelSettingsDraft(
      next.settings
        ? {
            protagonist_name: next.settings.protagonist_name,
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
    setActiveView("workspace");
    await refreshLogs(next.novel.id);
  }

  async function refreshLogs(novelId = detail?.novel.id) {
    const rows = await invoke<AiLog[]>("list_ai_logs", { novelId: novelId ?? null });
    setLogs(rows);
  }

  async function clearLogs() {
    const targetText = detail ? `《${detail.novel.title}》相关日志和全局日志` : "所有日志";
    if (!window.confirm(`清空${targetText}？`)) return;
    setBusy("clear-logs");
    setNotice("");
    try {
      await invoke<void>("clear_ai_logs", { novelId: detail?.novel.id ?? null });
      await refreshLogs();
      showNotice("日志已清空。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function showNotice(message: string, duration = 5000, keepPendingUpdate = false) {
    if (!keepPendingUpdate) setPendingUpdate(null);
    setNoticeDuration(duration);
    setNotice(message);
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
      const saved = await invoke<NovelSettings>("save_novel_settings", {
        novelId: detail.novel.id,
        protagonistName: novelSettingsDraft.protagonist_name,
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
      const novel = await invoke<Novel>("import_txt", { filePath });
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

  async function deleteNovel(novel: Novel) {
    if (!window.confirm(`删除《${novel.title}》及其本地分析、改写和日志数据？`)) return;
    setBusy("delete-novel");
    setNotice("");
    try {
      await invoke<void>("delete_novel", { novelId: novel.id });
      const remaining = await invoke<Novel[]>("list_novels");
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
      const saved = await invoke<ModelProfile>("save_model_profile", { input });
      setSelectedProfileId(saved.id);
      setProfileDraft({ ...profileDraft, id: saved.id, api_key: saved.has_api_key ? savedApiKeyMask : "" });
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
    setProfileDraft(emptyProfile);
    setOpenModelMenu(false);
    showNotice("已切换为新建模型配置，填写后点击保存。");
  }

  async function deleteSelectedModelProfile() {
    const profile = profiles.find((item) => item.id === selectedProfileId);
    if (!profile) {
      showNotice("请先选择一个模型配置。");
      return;
    }
    if (!window.confirm(`删除模型配置「${profile.model}」及其保存的 API Key？`)) return;
    setBusy("delete-model");
    setNotice("");
    try {
      await invoke<void>("delete_model_profile", { profileId: profile.id });
      const nextProfiles = await invoke<ModelProfile[]>("list_model_profiles");
      setProfiles(nextProfiles);
      const nextSelected = nextProfiles[0]?.id ?? "";
      setSelectedProfileId(nextSelected);
      setOpenModelMenu(false);
      if (!nextSelected) setProfileDraft(emptyProfile);
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
      const result = await invoke<ModelDiagnosis>("diagnose_model_profile", {
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
      const updated = await invoke<CanonAsset[]>("update_canon_assets", {
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
    setBusy(kind);
    setNotice("");
    try {
      const result = await invoke<Job>(kind === "analysis" ? "start_analysis" : "start_rewrite", {
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
    setBusy("auto-batch");
    setNotice("");
    try {
      const analysisResult = await invoke<Job>("start_analysis", {
        novelId,
        profileId: selectedProfileId,
        batchId
      });
      setJob(analysisResult);
      if (analysisResult.status !== "completed") {
        await loadNovel(novelId, { preserveBatchId: batchId, preserveChapterId: selectedChapterId });
        await refreshLogs(novelId);
        showNotice(`${analysisResult.status}：${analysisResult.message}`);
        return;
      }

      const rewriteResult = await invoke<Job>("start_rewrite", {
        novelId,
        profileId: selectedProfileId,
        batchId
      });
      setJob(rewriteResult);
      await loadNovel(novelId, { preserveBatchId: batchId, preserveChapterId: selectedChapterId });
      await refreshLogs(novelId);
      if (rewriteResult.status === "completed") {
        setActiveView("compare");
      }
      showNotice(rewriteResult.status === "completed" ? rewriteResult.message : `${rewriteResult.status}：${rewriteResult.message}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function runAnalyzeRewriteAll() {
    if (!detail || !selectedProfileId) {
      showNotice("请先导入小说并选择模型配置。");
      return;
    }
    if (!hasCompleteNovelSettings) {
      showNotice("请先填写设定");
      setSettingsDialog("basic");
      return;
    }
    setBusy("auto");
    setAutoRunState("running");
    setNotice("");
    try {
      const result = await invoke<Job>("start_analyze_rewrite_all", {
        novelId: detail.novel.id,
        profileId: selectedProfileId
      });
      setJob(result);
      await loadNovel(detail.novel.id);
      await refreshLogs(detail.novel.id);
      if (result.status === "completed") {
        setAutoRunState("idle");
        setActiveView("compare");
      } else if (result.status === "paused") {
        setAutoRunState("paused");
      } else if (result.status === "terminated" || result.status === "failed") {
        setAutoRunState("idle");
      }
      showNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      setAutoRunState("idle");
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function pauseAnalyzeRewriteAll() {
    if (!detail || autoRunState !== "running") return;
    setAutoControlBusy(true);
    try {
      const result = await invoke<Job>("pause_analyze_rewrite_all", { novelId: detail.novel.id });
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
      const result = await invoke<Job>("terminate_analyze_rewrite_all", { novelId: detail.novel.id });
      setJob(result);
      setAutoRunState("idle");
      showNotice(result.message);
    } catch (error) {
      setAutoRunState("idle");
      showNotice(String(error));
    } finally {
      setAutoControlBusy(false);
    }
  }

  async function exportNovel(format: "txt") {
    if (!detail) return;
    setBusy(`export-${format}`);
    setNotice("");
    try {
      const result = await invoke<{ path: string }>("export_novel", {
        novelId: detail.novel.id,
        format
      });
      showNotice(`已导出：${result.path}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function appSettingsPayload(overrides: Partial<AppSettings> = {}): AppSettings {
    return {
      export_dir: settings.export_dir ?? null,
      core_prompt: settings.core_prompt ?? "",
      review_enabled: settings.review_enabled ?? false,
      review_profile_id: settings.review_profile_id ?? null,
      rewrite_parallelism: settings.rewrite_parallelism ?? 6,
      ...overrides
    };
  }

  async function chooseExportDir() {
    setBusy("choose-export-dir");
    setNotice("");
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const saved = await invoke<AppSettings>("save_app_settings", {
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
      const saved = await invoke<AppSettings>("save_app_settings", {
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
      const saved = await invoke<AppSettings>("save_app_settings", {
        settings: appSettingsPayload({ review_enabled: nextEnabled })
      });
      setSettings(saved);
      showNotice(nextEnabled ? "已开启改写复检。" : "已关闭改写复检。");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function setRewriteParallelism(value: 1 | 3 | 6 | 10) {
    setBusy("parallelism-setting");
    setNotice("");
    try {
      const saved = await invoke<AppSettings>("save_app_settings", {
        settings: appSettingsPayload({ rewrite_parallelism: value })
      });
      setSettings(saved);
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
      const saved = await invoke<AppSettings>("save_app_settings", {
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

  async function saveCoreSettings() {
    setBusy("core-settings");
    setNotice("");
    try {
      const saved = await invoke<AppSettings>("save_app_settings", {
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
      await invoke<void>("open_github_url");
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function checkForUpdates() {
    setBusy("check-updates");
    setNotice("");
    setPendingUpdate(null);
    try {
      const update = await invoke<UpdateCheckResult>("check_for_updates");
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
    setBusy("download-update");
    try {
      const result = await invoke<UpdateDownloadResult>("download_latest_update");
      setPendingUpdate(null);
      setHasAvailableUpdate(false);
      showNotice(`已下载 ${result.version}：${result.path}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function cancelPendingUpdateDownload() {
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

  function displayChapterTitle(chapter: Chapter) {
    const title = chapter.title.replace(/\s+/g, " ").trim();
    return title || `第 ${chapter.index} 章`;
  }

  function formatNumber(value?: number | null) {
    if (value === null || value === undefined) return "暂无";
    return new Intl.NumberFormat("zh-CN").format(Math.round(value));
  }

  function formatSeconds(value?: number | null) {
    if (value === null || value === undefined) return "暂无历史数据";
    if (value < 60) return `${value.toFixed(1)} 秒`;
    const minutes = Math.floor(value / 60);
    const seconds = Math.round(value % 60);
    if (minutes < 60) return `${minutes} 分 ${seconds} 秒`;
    const hours = Math.floor(minutes / 60);
    const restMinutes = minutes % 60;
    return `${hours} 小时 ${restMinutes} 分`;
  }

  function diagnosisStatusText(status: DiagnosisStatus) {
    if (status === "ok") return "通过";
    if (status === "warning") return "警告";
    return "失败";
  }

  function renderRewriteModeControl() {
    return (
      <div className="mode-field">
        <span>改写模式</span>
        <div className="mode-toggle" role="radiogroup" aria-label="改写模式">
          <button
            type="button"
            className={novelSettingsDraft.rewrite_mode === "strict" ? "active" : ""}
            title="AI会更加忠于原文，不做过大改动"
            aria-checked={novelSettingsDraft.rewrite_mode === "strict"}
            role="radio"
            onClick={() => setNovelSettingsDraft({ ...novelSettingsDraft, rewrite_mode: "strict" })}
          >
            严谨模式
          </button>
          <button
            type="button"
            className={novelSettingsDraft.rewrite_mode === "creative" ? "active" : ""}
            title="AI会更加有创意，可能产生较大改动"
            aria-checked={novelSettingsDraft.rewrite_mode === "creative"}
            role="radio"
            onClick={() => setNovelSettingsDraft({ ...novelSettingsDraft, rewrite_mode: "creative" })}
          >
            创意模式
          </button>
        </div>
      </div>
    );
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

        <button className="primary-action" onClick={importTxt} disabled={busy === "import"}>
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
                >
                  <BookOpen size={16} />
                  <span>{novel.title}</span>
                </button>
                <button
                  className="icon-button menu-trigger"
                  aria-label={`打开《${novel.title}》菜单`}
                  onClick={() => setOpenNovelMenuId(openNovelMenuId === novel.id ? "" : novel.id)}
                >
                  <MoreHorizontal size={17} />
                </button>
                {openNovelMenuId === novel.id && (
                  <div className="context-menu">
                    <button onClick={() => deleteNovel(novel)} disabled={busy === "delete-novel"}>
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
          <div className="section-label">模型</div>
          <div className="model-row">
            <select value={selectedProfileId} onChange={(event) => setSelectedProfileId(event.target.value)}>
              <option value="">未选择</option>
              {profiles.map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.model}
                </option>
              ))}
            </select>
            <button
              className="icon-button menu-trigger"
              aria-label="打开模型菜单"
              onClick={() => setOpenModelMenu(!openModelMenu)}
              disabled={!selectedProfileId}
            >
              <MoreHorizontal size={17} />
            </button>
            {openModelMenu && selectedProfileId && (
              <div className="context-menu">
                <button onClick={deleteSelectedModelProfile} disabled={busy === "delete-model"}>
                  <Trash2 size={15} />
                  删除当前模型
                </button>
              </div>
            )}
          </div>
        </div>

        <div className="side-section nav-section">
          <button
            className={activeView === "logs" ? "nav-button active" : "nav-button"}
            onClick={() => setActiveView("logs")}
          >
            <ClipboardList size={17} />
            日志
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
        <header className="topbar">
          <div>
            <h1>
              {activeView === "logs"
                ? "日志"
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
                  <button
                    onClick={autoRunState === "paused" ? runAnalyzeRewriteAll : pauseAnalyzeRewriteAll}
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
                  <button onClick={terminateAnalyzeRewriteAll} disabled={autoControlBusy} title="终止一键分析改写">
                    {autoControlBusy ? <Loader2 className="spin" size={17} /> : <Square size={17} />}
                    终止
                  </button>
                </>
              )}
              <button
                onClick={runAnalyzeRewriteAll}
                disabled={!detail || !selectedProfileId || busy !== "" || autoRunState !== "idle"}
                title="AI自动分析改写全文，耗时较久"
              >
                {busy === "auto" ? <Loader2 className="spin" size={17} /> : <Sparkles size={17} />}
                一键分析改写
              </button>
              <button
                onClick={runAnalyzeRewriteCurrentBatch}
                disabled={!detail || !selectedProfileId || !selectedBatch || busy !== "" || autoRunState !== "idle"}
                title="AI自动分析并改写当前选中批次"
              >
                {busy === "auto-batch" ? <Loader2 className="spin" size={17} /> : <Sparkles size={17} />}
                一键分析改写当前批次
              </button>
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
          )}
        </header>

        {notice && (
          <div className="notice notice-panel">
            <span>{notice}</span>
            {pendingUpdate && (
              <div className="notice-actions">
                <button onClick={downloadPendingUpdate} disabled={busy === "download-update"}>
                  {busy === "download-update" ? <Loader2 className="spin" size={16} /> : <Download size={16} />}
                  下载最新版
                </button>
                <button onClick={cancelPendingUpdateDownload} disabled={busy === "download-update"}>
                  不下载最新版
                </button>
              </div>
            )}
          </div>
        )}
        {modelDiagnosis && (
          <div className={`diagnosis-panel diagnosis-top-panel ${modelDiagnosis.status}`}>
            <div className="diagnosis-heading">
              <strong>诊断结果：{diagnosisStatusText(modelDiagnosis.status)}</strong>
              {modelDiagnosis.recommended_thinking_mode && (
                <span>建议思考模式：{modelDiagnosis.recommended_thinking_mode}</span>
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
                <div className={`diagnosis-item ${check.status}`} key={`${check.name}-${check.message}`}>
                  <span>{diagnosisStatusText(check.status)}</span>
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
          <div className={`job-strip ${job.status}`}>
            <CheckCircle2 size={17} />
            <div className="job-content">
              <span>
                {job.job_type} · {statusText[job.status] ?? job.status} · {job.current_chapter}/{job.total_chapters} ·{" "}
                {job.message}
              </span>
              {job.job_type === "auto" && (
                <div className="job-progress-row" aria-label={`一键分析改写进度 ${autoProgressPercent}%`}>
                  <div className="job-progress-bar">
                    <div className="job-progress-fill" style={{ width: `${autoProgressPercent}%` }} />
                  </div>
                  <strong>{autoProgressPercent}%</strong>
                </div>
              )}
            </div>
          </div>
        )}

        {activeView === "logs" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>AI 调用日志</h2>
              <div className="panel-actions">
                <button onClick={() => setActiveView("workspace")}>
                  <ArrowLeft size={16} />
                  返回
                </button>
                <button onClick={clearLogs} disabled={busy !== "" || logs.length === 0}>
                  <Trash2 size={16} />
                  清空
                </button>
                <button onClick={() => refreshLogs()} disabled={busy !== ""}>
                  <RefreshCw size={16} />
                  刷新
                </button>
              </div>
            </div>
            <div className="full-log-list">
              {logs.map((log) => (
                <article className={`full-log-item ${log.status}`} key={log.id}>
                  <header>
                    <div>
                      <strong>{log.action}</strong>
                      <span>
                        {log.chapter_title || "全局调用"} · {new Date(log.created_at).toLocaleString()}
                      </span>
                    </div>
                    <span className="log-status">{log.status}</span>
                  </header>
                  {log.reasoning && (
                    <section>
                      <h3>思考过程</h3>
                      <pre>{log.reasoning}</pre>
                    </section>
                  )}
                  <section>
                    <h3>输出文本</h3>
                    <pre>{log.content || "无正文内容。"}</pre>
                  </section>
                  <section>
                    <h3>原始响应</h3>
                    <pre>{log.raw_response || log.content || "无原始响应。"}</pre>
                  </section>
                </article>
              ))}
              {logs.length === 0 && <p className="muted">暂无 AI 调用日志。</p>}
            </div>
          </div>
        )}

        {activeView === "novel-settings" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>基本设定</h2>
              <div className="panel-actions">
                <button onClick={() => setActiveView("workspace")}>
                  <ArrowLeft size={16} />
                  返回
                </button>
                <button onClick={saveNovelSettings} disabled={!detail || busy === "novel-settings"}>
                  {busy === "novel-settings" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                  保存
                </button>
              </div>
            </div>
            {detail ? (
              <section className="settings-section novel-settings-section">
                <h3>改写基础规则</h3>
                <div className="form-grid">
                  <label>
                    主角姓名（必填）
                    <input
                      value={novelSettingsDraft.protagonist_name}
                      onChange={(event) =>
                        setNovelSettingsDraft({ ...novelSettingsDraft, protagonist_name: event.target.value })
                      }
                      placeholder="例如：萧炎"
                    />
                  </label>
                  <label className="settings-rewritten-name-field">
                    改写后姓名（选填）
                    <input
                      value={novelSettingsDraft.rewritten_protagonist_name}
                      onChange={(event) =>
                        setNovelSettingsDraft({
                          ...novelSettingsDraft,
                          rewritten_protagonist_name: event.target.value
                        })
                      }
                      placeholder="留空则让AI生成改写后姓名"
                    />
                  </label>
                  <label className="settings-additional-names-field">
                    其他需要女性化的人名（选填）
                    <textarea
                      value={novelSettingsDraft.additional_feminize_names}
                      onChange={(event) =>
                        setNovelSettingsDraft({ ...novelSettingsDraft, additional_feminize_names: event.target.value })
                      }
                      placeholder="支持逗号或换行分隔"
                    />
                  </label>
                  <label>
                    身材
                    <select
                      value={novelSettingsDraft.bust}
                      onChange={(event) => setNovelSettingsDraft({ ...novelSettingsDraft, bust: event.target.value })}
                    >
                      <option value="平胸">平胸</option>
                      <option value="巨乳">巨乳</option>
                    </select>
                  </label>
                  <label>
                    体型
                    <select
                      value={novelSettingsDraft.body_type}
                      onChange={(event) => setNovelSettingsDraft({ ...novelSettingsDraft, body_type: event.target.value })}
                    >
                      <option value="萝莉">萝莉</option>
                      <option value="御姐">御姐</option>
                      <option value="少女">少女</option>
                    </select>
                  </label>
                  {renderRewriteModeControl()}
                </div>
                <p className="settings-note">
                  分析和改写会自动附带这些设定。主角姓名会按同音或近音原则女性化，例如萧炎改为萧妍，李火旺改为李火婉。
                </p>
              </section>
            ) : (
              <p className="muted">请先导入小说。</p>
            )}
          </div>
        )}

        {activeView === "core-settings" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>核心设定</h2>
              <div className="panel-actions">
                <button onClick={() => setActiveView("workspace")}>
                  <ArrowLeft size={16} />
                  返回
                </button>
                <button onClick={saveCoreSettings} disabled={busy === "core-settings"}>
                  {busy === "core-settings" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                  保存
                </button>
              </div>
            </div>
            <section className="settings-section core-settings-section">
              <h3>全局改写风格</h3>
              <p className="settings-note">
                核心设定不随小说变化，会在每一次改写和打回重写时发送给 AI，并作为最高优先级的写作要求。建议主要填写文风、叙述节奏、描写密度、语气、对白风格、情绪氛围等全局写法；不要写某一本小说的主角姓名、剧情设定、章节内容或临时任务，避免影响其他小说。
              </p>
              <textarea
                className="core-settings-input"
                value={corePromptDraft}
                onChange={(event) => setCorePromptDraft(event.target.value)}
                placeholder="例如：保持原文轻小说风格，句子自然流畅；减少机械替换感；动作描写细腻但不过度堆砌；对白保留角色原本语气；百合互动要循序渐进，不要突然强行亲密。"
              />
              {!corePromptDraft.trim() && (
                <p className="settings-empty-hint">
                  当前未填写核心设定。留空也可以正常改写；如果填写，建议只写长期通用的文风和描写偏好。
                </p>
              )}
            </section>
          </div>
        )}

        {activeView === "settings" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>设置</h2>
              <div className="panel-actions">
                <button onClick={() => setActiveView("workspace")}>
                  <ArrowLeft size={16} />
                  返回
                </button>
              </div>
            </div>
            <section className="settings-section">
              <h3>导出目录</h3>
              <div className="setting-row">
                <input readOnly value={settings.export_dir || "默认应用数据目录"} />
                <button onClick={chooseExportDir} disabled={busy === "choose-export-dir"}>
                  <FolderOpen size={16} />
                  选择目录
                </button>
                <button onClick={clearExportDir} disabled={!settings.export_dir || busy === "clear-export-dir"}>
                  恢复默认
                </button>
              </div>
            </section>
            <section className="settings-section">
              <div className="settings-section-heading">
                <h3>改写复检</h3>
                <span className="setting-help" tabIndex={0} aria-label="改写复检说明">
                  <HelpCircle size={16} />
                  <span className="setting-help-tooltip" role="tooltip">
                    双专家审查会显著增加请求数和等待时间，但能让改写后的文本逻辑更顺、质量更稳。开启后，每个分片最多可能经历“分析、初稿改写、审查判定、打回重写、审查复判、再次打回重写、第三次审查”七次模型请求。建议为审查专家选择逻辑能力强、JSON 输出稳定、长文本一致性检查更可靠的模型。
                  </span>
                </span>
              </div>
              <div className="setting-toggle-row">
                <button
                  className={settings.review_enabled ? "setting-switch active" : "setting-switch"}
                  onClick={toggleReviewEnabled}
                  disabled={busy === "review-setting"}
                  title="开启复检时AI改写完成后会检查一遍是否有疏漏，会增加改写时间"
                >
                  {settings.review_enabled ? "开启" : "关闭"}
                </button>
                <span>默认关闭，开启后每批改写会由审查专家判定；不通过时打回改写模型重写并复判。</span>
              </div>
              <div className="setting-row">
                <select
                  value={settings.review_profile_id ?? ""}
                  onChange={(event) => setReviewProfileId(event.target.value)}
                  disabled={busy === "review-profile-setting"}
                  title="选择第二个 AI 作为审查专家；留空则使用当前改写模型审查"
                >
                  <option value="">使用当前改写模型审查</option>
                  {profiles.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.model}
                    </option>
                  ))}
                </select>
                <span>审查专家只判定并列出问题；不通过时会打回改写模型重写，再由审查专家复判。</span>
              </div>
            </section>
            <section className="settings-section">
              <h3>分析/改写并发</h3>
              <div
                className="setting-toggle-row"
                title="较高并发通常可以缩短分析和改写等待时间，但会同时增加请求数量；请求越多，触发限流、网络失败或分片解析失败的概率越高，也可能因每个分片都携带设定和一致性资产而略微增加 token 消耗。若频繁失败或服务商限流，请降低并发数。"
              >
                <div className="mode-toggle mode-toggle-four setting-parallelism" role="radiogroup" aria-label="分析和改写并发请求数">
                  {([10, 6, 3, 1] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      className={(settings.rewrite_parallelism ?? 6) === value ? "active" : ""}
                      aria-checked={(settings.rewrite_parallelism ?? 6) === value}
                      role="radio"
                      disabled={busy === "parallelism-setting"}
                      onClick={() => setRewriteParallelism(value)}
                    >
                      {value === 1 ? "不并发" : value}
                    </button>
                  ))}
                </div>
                <span>默认 6：30 章会拆成 6 个请求，每个约 5 章；分析和改写共用该设置，并尽量共享设定和一致性资产。</span>
              </div>
            </section>
          </div>
        )}

        {activeView === "compare" && (
          <div className="compare-page">
            <div className="compare-page-toolbar">
              <label>
                章节
                <select value={selectedChapterId} onChange={(event) => setSelectedChapterId(event.target.value)}>
                  {detail?.chapters.map((chapter) => (
                    <option key={chapter.id} value={chapter.id}>
                      {chapter.index}. {chapter.title}
                    </option>
                  ))}
                </select>
              </label>
              <div className="compare-toolbar-actions">
                <button onClick={() => setActiveView("workspace")}>
                  <ArrowLeft size={17} />
                  返回
                </button>
                <button onClick={() => exportNovel("txt")} disabled={!detail || busy !== ""}>
                  <Download size={17} />
                  TXT
                </button>
              </div>
            </div>
            {selectedChapter ? (
              <div className="large-compare-grid">
                <article>
                  <h2>原文</h2>
                  <pre ref={originalCompareRef}>{selectedChapter.original_text}</pre>
                </article>
                <article>
                  <h2>改写稿</h2>
                  <pre ref={rewriteCompareRef}>{selectedChapter.rewrite_text || "尚未改写。"}</pre>
                </article>
              </div>
            ) : (
              <p className="muted">请选择章节。</p>
            )}
          </div>
        )}

        {activeView === "workspace" && (
          <>
          {detail && (
            <div className="batch-strip">
              <label>
                当前批次
                <select value={selectedBatchId} onChange={(event) => setSelectedBatchId(event.target.value)}>
                  {detail.batches.map((batch) => (
                    <option key={batch.id} value={batch.id}>
                      {batch.label}
                    </option>
                  ))}
                </select>
              </label>
              <span>
                {selectedBatch
                  ? `将处理第 ${selectedBatch.start_chapter} - ${selectedBatch.end_chapter} 段/章`
                  : "暂无批次"}
              </span>
            </div>
          )}
          {detail && jobEstimate && (
            <section className={`estimate-panel ${estimateCollapsed ? "collapsed" : ""}`} aria-label="任务预估">
              <div className="estimate-heading">
                <h2>任务预估</h2>
                <div className="estimate-heading-actions">
                  {!estimateCollapsed && (
                    <span>
                      并发 {jobEstimate.parallelism} · 复检{jobEstimate.review_enabled ? "开启" : "关闭"}
                    </span>
                  )}
                  <button
                    className="icon-button estimate-toggle"
                    title={estimateCollapsed ? "展开任务预估详情" : "隐藏任务预估详情"}
                    aria-label={estimateCollapsed ? "展开任务预估详情" : "隐藏任务预估详情"}
                    aria-expanded={!estimateCollapsed}
                    onClick={() => setEstimateCollapsed((value) => !value)}
                  >
                    <ChevronDown size={17} />
                  </button>
                </div>
              </div>
              {!estimateCollapsed && (
                <div className="estimate-grid">
                  <div>
                    <span>全文规模</span>
                    <strong>
                      {formatNumber(jobEstimate.novel_chapters)} 章 · {formatNumber(jobEstimate.novel_chars)} 字 ·{" "}
                      {formatNumber(jobEstimate.novel_batches)} 批
                    </strong>
                  </div>
                  <div>
                    <span>当前批次</span>
                    <strong>
                      {formatNumber(jobEstimate.selected_batch_chapters)} 章 ·{" "}
                      {formatNumber(jobEstimate.selected_batch_chars)} 字
                    </strong>
                  </div>
                  <div>
                    <span>预计请求数</span>
                    <strong>
                      当前 {formatNumber(jobEstimate.current_batch_requests)} · 全文{" "}
                      {formatNumber(jobEstimate.full_run_requests)}
                    </strong>
                  </div>
                  <div>
                    <span>预计等待</span>
                    <strong>
                      当前 {formatSeconds(jobEstimate.estimated_current_batch_seconds)} · 全文{" "}
                      {formatSeconds(jobEstimate.estimated_full_run_seconds)}
                    </strong>
                  </div>
                  <div>
                    <span>历史调用</span>
                    <strong>
                      成功 {formatNumber(jobEstimate.recent_success_calls)} · 失败{" "}
                      {formatNumber(jobEstimate.recent_failed_calls)} · 平均{" "}
                      {formatSeconds(jobEstimate.average_call_seconds)}
                    </strong>
                  </div>
                  <div>
                    <span>历史字符</span>
                    <strong>
                      输入 {formatNumber(jobEstimate.average_input_chars)} · 输出{" "}
                      {formatNumber(jobEstimate.average_output_chars)}
                    </strong>
                  </div>
                </div>
              )}
            </section>
          )}
          <div className="content-grid">
            <section className="panel model-panel">
              <div className="panel-heading">
                <h2>模型配置</h2>
                <div className="panel-actions">
                  <button onClick={createNewModelProfile} disabled={busy !== ""}>
                    <FilePlus2 size={16} />
                    新建
                  </button>
                  <button onClick={diagnoseProfile} disabled={!selectedProfileId || busy === "diagnose"}>
                    {busy === "diagnose" ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}
                    诊断模型
                  </button>
                  <button onClick={saveProfile} disabled={busy === "profile"}>
                    {busy === "profile" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                    保存
                  </button>
                </div>
              </div>
              <div className="model-scroll">
                <div className="form-grid model-form-grid">
                  <label>
                    名称
                    <input value={profileDraft.name} onChange={(event) => setProfileDraft({ ...profileDraft, name: event.target.value })} />
                  </label>
                  <label>
                    Provider
                    <select
                      value={profileDraft.provider}
                      onChange={(event) =>
                        setProfileDraft({
                          ...profileDraft,
                          provider: event.target.value,
                          base_url:
                            event.target.value === "gemini"
                              ? "https://generativelanguage.googleapis.com/v1beta"
                              : profileDraft.base_url
                        })
                      }
                    >
                      <option value="openai-compatible">OpenAI 兼容</option>
                      <option value="gemini">Google Gemini</option>
                    </select>
                  </label>
                  <label>
                    Base URL
                    <input value={profileDraft.base_url} onChange={(event) => setProfileDraft({ ...profileDraft, base_url: event.target.value })} />
                  </label>
                  <label>
                    模型名
                    <div className="model-name-control">
                      <input
                        value={profileDraft.model}
                        onChange={(event) => setProfileDraft({ ...profileDraft, model: event.target.value })}
                      />
                      {detectedModelSuggestions.length > 0 && (
                        <button
                          type="button"
                          className="model-suggestion-trigger"
                          title="选择检测到的服务商模型"
                          aria-label="选择检测到的服务商模型"
                          aria-expanded={openModelSuggestions}
                          onClick={() => setOpenModelSuggestions((open) => !open)}
                        >
                          <ChevronDown size={16} />
                        </button>
                      )}
                      {openModelSuggestions && detectedModelSuggestions.length > 0 && (
                        <div className="model-suggestion-menu" role="listbox">
                          {detectedModelSuggestions.map((suggestion) => (
                            <button
                              type="button"
                              key={suggestion.model}
                              role="option"
                              aria-selected={profileDraft.model === suggestion.model}
                              onClick={() => {
                                setProfileDraft((draft) => ({ ...draft, model: suggestion.model }));
                                setOpenModelSuggestions(false);
                              }}
                            >
                              <span>{suggestion.label}</span>
                              <small>{suggestion.model}</small>
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  </label>
                  <label>
                    Temperature
                    <input
                      type="number"
                      min="0"
                      max="2"
                      step="0.1"
                      value={profileDraft.temperature}
                      onChange={(event) => setProfileDraft({ ...profileDraft, temperature: Number(event.target.value) })}
                    />
                  </label>
                  <label className="mode-field thinking-mode-field form-full" title={thinkingModeTooltip}>
                    <span>思考模式</span>
                    <div className="mode-toggle mode-toggle-three" role="radiogroup" aria-label="思考模式">
                      <button
                        type="button"
                        className={profileDraft.thinking_mode === "auto" ? "active" : ""}
                        role="radio"
                        aria-checked={profileDraft.thinking_mode === "auto"}
                        title={thinkingModeTooltip}
                        onClick={() => setProfileDraft({ ...profileDraft, thinking_mode: "auto" })}
                      >
                        自动
                      </button>
                      <button
                        type="button"
                        className={profileDraft.thinking_mode === "off" ? "active" : ""}
                        role="radio"
                        aria-checked={profileDraft.thinking_mode === "off"}
                        title={thinkingModeTooltip}
                        onClick={() => setProfileDraft({ ...profileDraft, thinking_mode: "off" })}
                      >
                        关闭
                      </button>
                      <button
                        type="button"
                        className={profileDraft.thinking_mode === "on" ? "active" : ""}
                        role="radio"
                        aria-checked={profileDraft.thinking_mode === "on"}
                        title={thinkingModeTooltip}
                        onClick={() => setProfileDraft({ ...profileDraft, thinking_mode: "on" })}
                      >
                        开启
                      </button>
                    </div>
                  </label>
                  <label>
                    API Key
                    <input
                      type="password"
                      value={profileDraft.api_key}
                      placeholder={selectedProfileId ? "留空则不保存 Key" : "填写 API Key 后保存"}
                      onFocus={() => {
                        if (profileDraft.api_key === savedApiKeyMask) setProfileDraft({ ...profileDraft, api_key: "" });
                      }}
                      onChange={(event) => setProfileDraft({ ...profileDraft, api_key: event.target.value })}
                    />
                  </label>
                </div>
              </div>
            </section>

            <section className="panel canon-panel">
              <div className="panel-heading">
                <h2>一致性资产</h2>
                <button onClick={saveCanonAssets} disabled={!detail || busy === "canon"}>
                  {busy === "canon" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                  保存
                </button>
              </div>
              <div className="asset-stack">
                {detail?.canon_assets.map((asset) => (
                  <label key={asset.kind}>
                    {asset.kind}
                    <textarea value={asset.content} onChange={(event) => updateCanon(asset.kind, event.target.value)} placeholder="分析后会自动生成，也可以手动补充。" />
                  </label>
                ))}
                {!detail && <p className="muted">导入小说后显示人物卡、关系、地点、伏笔和术语表。</p>}
              </div>
            </section>

            <section className="panel chapter-list-panel">
              <div className="panel-heading">
                <h2>章节</h2>
              </div>
              <div className="chapter-list">
                {detail?.chapters.map((chapter) => (
                  <button
                    key={chapter.id}
                    className={selectedChapter?.id === chapter.id ? "chapter-item active" : "chapter-item"}
                    onClick={() => setSelectedChapterId(chapter.id)}
                  >
                    <span className="chapter-title">
                      {chapter.index}. {displayChapterTitle(chapter)}
                    </span>
                    <small>
                      分析 {statusText[chapter.analysis_status] ?? chapter.analysis_status} · 改写{" "}
                      {statusText[chapter.rewrite_status] ?? chapter.rewrite_status}
                    </small>
                  </button>
                ))}
              </div>
            </section>

          </div>
          </>
        )}
      </section>
      {settingsDialog && (
        <div className="modal-backdrop">
          <div className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-dialog-title">
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
                  <div className="form-grid">
                    <label>
                      主角姓名（必填）
                      <input
                        value={novelSettingsDraft.protagonist_name}
                        onChange={(event) =>
                          setNovelSettingsDraft({ ...novelSettingsDraft, protagonist_name: event.target.value })
                        }
                        placeholder="例如：萧炎"
                    />
                  </label>
                    <label className="settings-rewritten-name-field">
                      改写后姓名（选填）
                      <input
                        value={novelSettingsDraft.rewritten_protagonist_name}
                        onChange={(event) =>
                          setNovelSettingsDraft({
                            ...novelSettingsDraft,
                            rewritten_protagonist_name: event.target.value
                          })
                        }
                        placeholder="留空则让AI生成改写后姓名"
                      />
                    </label>
                    <label className="settings-additional-names-field">
                      其他需要女性化的人名（选填）
                      <textarea
                        value={novelSettingsDraft.additional_feminize_names}
                        onChange={(event) =>
                          setNovelSettingsDraft({
                            ...novelSettingsDraft,
                            additional_feminize_names: event.target.value
                          })
                        }
                        placeholder="支持逗号或换行分隔"
                      />
                    </label>
                    <label>
                      身材
                      <select
                        value={novelSettingsDraft.bust}
                        onChange={(event) => setNovelSettingsDraft({ ...novelSettingsDraft, bust: event.target.value })}
                      >
                        <option value="平胸">平胸</option>
                        <option value="巨乳">巨乳</option>
                      </select>
                    </label>
                    <label>
                      体型
                      <select
                        value={novelSettingsDraft.body_type}
                        onChange={(event) =>
                          setNovelSettingsDraft({ ...novelSettingsDraft, body_type: event.target.value })
                        }
                      >
                        <option value="萝莉">萝莉</option>
                        <option value="御姐">御姐</option>
                        <option value="少女">少女</option>
                      </select>
                    </label>
                    {renderRewriteModeControl()}
                  </div>
                </div>
                <footer className="dialog-actions">
                  <button onClick={() => setSettingsDialog("advanced")} disabled={busy === "novel-settings"}>
                    高级设定
                  </button>
                  <button className="dialog-primary" onClick={saveNovelSettings} disabled={!detail || busy === "novel-settings"}>
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
          </div>
        </div>
      )}
      {showQuickStart && (
        <div className="modal-backdrop">
          <div className="quickstart-dialog" role="dialog" aria-modal="true" aria-labelledby="quickstart-title">
            <div className="quickstart-content">
              <h2 id="quickstart-title">快速上手</h2>
              <ol>
                <li>点击导入 TXT，软件会自动识别章节并按批次整理。</li>
                <li>填写模型配置，保存后点击诊断模型，确认 API Key、JSON 输出和思考模式可用。</li>
                <li>进入设定，填写主角姓名、改写后姓名、身材体型、改写模式和额外要求。</li>
                <li>如需更严格质量控制，可在设置里开启改写复检，并选择审查专家模型。</li>
                <li>回到工作台，先看任务预估，确认请求数、历史耗时和预计等待时间。</li>
                <li>点击一键分析改写当前批次或全文；全文任务运行中可暂停、继续或终止。</li>
                <li>改写完成后进入对比页面，检查原文和改写稿，只导出已完成章节的 TXT。</li>
              </ol>
              <button className="dialog-primary quickstart-confirm" onClick={closeQuickStart}>
                确定
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  );
}
