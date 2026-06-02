import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  BookOpen,
  CheckCircle2,
  ClipboardList,
  Download,
  FilePlus2,
  FolderOpen,
  Github,
  KeyRound,
  Loader2,
  MoreHorizontal,
  Play,
  RefreshCw,
  Save,
  Settings,
  Sparkles,
  Trash2
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

type NovelSettingsDraft = {
  protagonist_name: string;
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
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: ""
};

const savedApiKeyMask = "********";
const thinkingModeTooltip = "建议自动\n兼容性：OpenAI/OpenRouter/Gemini 可控；DeepSeek 官方多由模型名决定；不支持时会自动降级";

const statusText: Record<string, string> = {
  pending: "待处理",
  running: "进行中",
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
  const [logs, setLogs] = useState<AiLog[]>([]);
  const [settings, setSettings] = useState<AppSettings>({});
  const [novelSettingsDraft, setNovelSettingsDraft] = useState<NovelSettingsDraft>(emptyNovelSettings);
  const [settingsDialog, setSettingsDialog] = useState<"basic" | "advanced" | null>(null);
  const [activeView, setActiveView] = useState<"workspace" | "compare" | "novel-settings" | "logs" | "settings">("workspace");
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");
  const [noticeDuration, setNoticeDuration] = useState(5000);
  const [pendingUpdate, setPendingUpdate] = useState<UpdateCheckResult | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  const originalCompareRef = useRef<HTMLPreElement | null>(null);
  const rewriteCompareRef = useRef<HTMLPreElement | null>(null);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? detail?.chapters[0],
    [detail, selectedChapterId]
  );

  const selectedBatch = useMemo(
    () => detail?.batches.find((batch) => batch.id === selectedBatchId) ?? detail?.batches[0],
    [detail, selectedBatchId]
  );

  const hasCompleteNovelSettings = Boolean(
    detail?.settings?.protagonist_name?.trim() && detail.settings.bust?.trim() && detail.settings.body_type?.trim()
  );

  useEffect(() => {
    void refreshAll();
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

  async function loadNovel(novelId: string) {
    const next = await invoke<NovelDetail>("get_novel_detail", { novelId });
    setDetail(next);
    setSelectedChapterId(next.chapters[0]?.id ?? "");
    setSelectedBatchId(next.batches[0]?.id ?? "");
    setNovelSettingsDraft(
      next.settings
        ? {
            protagonist_name: next.settings.protagonist_name,
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
        additionalFeminizeNames: novelSettingsDraft.additional_feminize_names,
        bust: novelSettingsDraft.bust,
        bodyType: novelSettingsDraft.body_type,
        rewriteMode: novelSettingsDraft.rewrite_mode,
        advancedSettings: novelSettingsDraft.advanced_settings
      });
      setDetail({ ...detail, settings: saved });
      setNovelSettingsDraft({
        protagonist_name: saved.protagonist_name,
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

  async function importTxt() {
    setBusy("import");
    setNotice("");
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "TXT 小说", extensions: ["txt"] }]
      });
      if (typeof selected !== "string") return;
      const novel = await invoke<Novel>("import_txt", { filePath: selected });
      await refreshAll();
      await loadNovel(novel.id);
      showNotice(`已导入《${novel.title}》。`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
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

  async function testProfile() {
    if (!selectedProfileId) {
      showNotice("请先保存并选择一个模型配置。");
      return;
    }
    setBusy("test");
    setNotice("");
    try {
      const result = await invoke<{ ok: boolean; message: string }>("test_model_profile", {
        profileId: selectedProfileId
      });
      await refreshLogs();
      showNotice(result.ok ? `连接成功：${result.message}` : `连接失败：${result.message}`);
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
    if (kind === "rewrite" && !hasCompleteNovelSettings) {
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
      await loadNovel(detail.novel.id);
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
        setActiveView("compare");
      }
      showNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setBusy("");
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

  async function chooseExportDir() {
    setBusy("choose-export-dir");
    setNotice("");
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const saved = await invoke<AppSettings>("save_app_settings", { settings: { export_dir: selected } });
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
      const saved = await invoke<AppSettings>("save_app_settings", { settings: { export_dir: null } });
      setSettings(saved);
      showNotice("已恢复默认导出目录。");
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
        showNotice(`当前已是最新版：${update.current_version}`, 3000);
        return;
      }

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

  return (
    <main className="app-shell">
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
        <div className="app-menu-spacer" />
        <button className="app-menu-item" onClick={openGithubRepository} disabled={busy !== ""}>
          <Github size={16} />
          GitHub地址
        </button>
        <button className="app-menu-item" onClick={checkForUpdates} disabled={busy !== ""}>
          {busy === "check-updates" || busy === "download-update" ? (
            <Loader2 className="spin" size={16} />
          ) : (
            <RefreshCw size={16} />
          )}
          检查更新
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
                  ? "配置导出目录"
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
              <button
                onClick={runAnalyzeRewriteAll}
                disabled={!detail || !selectedProfileId || busy !== ""}
                title="AI自动分析改写全文，耗时较久"
              >
                {busy === "auto" ? <Loader2 className="spin" size={17} /> : <Sparkles size={17} />}
                一键分析改写
              </button>
              <button onClick={() => runJob("analysis")} disabled={!detail || !selectedProfileId || !selectedBatch || busy !== ""}>
                {busy === "analysis" ? <Loader2 className="spin" size={17} /> : <Play size={17} />}
                分析
              </button>
              <button onClick={() => runJob("rewrite")} disabled={!detail || !selectedProfileId || !selectedBatch || busy !== ""}>
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
        {job && activeView === "workspace" && (
          <div className={`job-strip ${job.status}`}>
            <CheckCircle2 size={17} />
            <span>
              {job.job_type} · {statusText[job.status] ?? job.status} · {job.current_chapter}/{job.total_chapters} ·{" "}
              {job.message}
            </span>
          </div>
        )}

        {activeView === "logs" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>AI 调用日志</h2>
              <div className="panel-actions">
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
                  <label>
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

        {activeView === "settings" && (
          <div className="page-panel">
            <div className="page-heading">
              <h2>设置</h2>
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
              <button onClick={() => exportNovel("txt")} disabled={!detail || busy !== ""}>
                <Download size={17} />
                TXT
              </button>
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
          <div className="content-grid">
            <section className="panel model-panel">
              <div className="panel-heading">
                <h2>模型配置</h2>
                <div className="panel-actions">
                  <button onClick={testProfile} disabled={!selectedProfileId || busy === "test"}>
                    {busy === "test" ? <Loader2 className="spin" size={16} /> : <KeyRound size={16} />}
                    测试模型
                  </button>
                  <button onClick={saveProfile} disabled={busy === "profile"}>
                    {busy === "profile" ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
                    保存
                  </button>
                </div>
              </div>
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
                  <input value={profileDraft.model} onChange={(event) => setProfileDraft({ ...profileDraft, model: event.target.value })} />
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
                    <label>
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
    </main>
  );
}
