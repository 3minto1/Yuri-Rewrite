import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  BookOpen,
  CheckCircle2,
  ClipboardList,
  Download,
  FilePlus2,
  KeyRound,
  Loader2,
  MoreHorizontal,
  Play,
  RefreshCw,
  Save,
  Sparkles,
  Trash2
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

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

type NovelDetail = {
  novel: Novel;
  chapters: Chapter[];
  canon_assets: CanonAsset[];
};

type ModelProfile = {
  id: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
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
  created_at: string;
};

type ProfileDraft = {
  id?: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
  api_key: string;
};

const emptyProfile: ProfileDraft = {
  name: "OpenAI 兼容接口",
  provider: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  model: "请填写模型名",
  temperature: 0.7,
  api_key: ""
};

const savedApiKeyMask = "********";

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
  const [openNovelMenuId, setOpenNovelMenuId] = useState("");
  const [logs, setLogs] = useState<AiLog[]>([]);
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");
  const [job, setJob] = useState<Job | null>(null);

  const selectedChapter = useMemo(
    () => detail?.chapters.find((chapter) => chapter.id === selectedChapterId) ?? detail?.chapters[0],
    [detail, selectedChapterId]
  );

  useEffect(() => {
    void refreshAll();
  }, []);

  useEffect(() => {
    void refreshLogs();
  }, [detail?.novel.id]);

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
      api_key: profile.has_api_key ? savedApiKeyMask : ""
    });
  }, [profiles, selectedProfileId]);

  async function refreshAll() {
    const [novelRows, profileRows] = await Promise.all([
      invoke<Novel[]>("list_novels"),
      invoke<ModelProfile[]>("list_model_profiles")
    ]);
    setNovels(novelRows);
    setProfiles(profileRows);
    if (!selectedProfileId && profileRows[0]) {
      setSelectedProfileId(profileRows[0].id);
    }
    if (!detail && novelRows[0]) {
      await loadNovel(novelRows[0].id);
    }
    await refreshLogs();
  }

  async function loadNovel(novelId: string) {
    const next = await invoke<NovelDetail>("get_novel_detail", { novelId });
    setDetail(next);
    setSelectedChapterId(next.chapters[0]?.id ?? "");
    setOpenNovelMenuId("");
    await refreshLogs(next.novel.id);
  }

  async function refreshLogs(novelId = detail?.novel.id) {
    const rows = await invoke<AiLog[]>("list_ai_logs", { novelId: novelId ?? null });
    setLogs(rows);
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
      setNotice(`已导入《${novel.title}》。`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function deleteNovel(novel: Novel) {
    const confirmed = window.confirm(`删除《${novel.title}》及其本地分析、改写和日志数据？`);
    if (!confirmed) return;
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
          setLogs([]);
        }
      }
      setNotice(`已删除《${novel.title}》。`);
    } catch (error) {
      setNotice(String(error));
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
      setProfileDraft({
        ...profileDraft,
        id: saved.id,
        api_key: saved.has_api_key ? savedApiKeyMask : ""
      });
      await refreshAll();
      setNotice(saved.has_api_key ? "模型配置和 API Key 已保存。" : "模型配置已保存，尚未保存 API Key。");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function testProfile() {
    if (!selectedProfileId) {
      setNotice("请先保存并选择一个模型配置。");
      return;
    }
    setBusy("test");
    setNotice("");
    try {
      const result = await invoke<{ ok: boolean; message: string }>("test_model_profile", {
        profileId: selectedProfileId
      });
      await refreshLogs();
      setNotice(result.ok ? `连接成功：${result.message}` : `连接失败：${result.message}`);
    } catch (error) {
      setNotice(String(error));
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
      setNotice("一致性资产已保存。");
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function runJob(kind: "analysis" | "rewrite") {
    if (!detail || !selectedProfileId) {
      setNotice("请先导入小说并选择模型配置。");
      return;
    }
    setBusy(kind);
    setNotice("");
    try {
      const result = await invoke<Job>(kind === "analysis" ? "start_analysis" : "start_rewrite", {
        novelId: detail.novel.id,
        profileId: selectedProfileId
      });
      setJob(result);
      await loadNovel(detail.novel.id);
      await refreshLogs(detail.novel.id);
      setNotice(result.status === "completed" ? result.message : `${result.status}：${result.message}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function exportNovel(format: "txt" | "markdown") {
    if (!detail) return;
    setBusy(`export-${format}`);
    setNotice("");
    try {
      const result = await invoke<{ path: string }>("export_novel", {
        novelId: detail.novel.id,
        format
      });
      setNotice(`已导出：${result.path}`);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  function updateCanon(kind: string, content: string) {
    if (!detail) return;
    setDetail({
      ...detail,
      canon_assets: detail.canon_assets.map((asset) =>
        asset.kind === kind ? { ...asset, content } : asset
      )
    });
  }

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <Sparkles size={22} />
          <div>
            <strong>Yuri Rewrite</strong>
            <span>本地小说分析与改写</span>
          </div>
        </div>

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
          <select value={selectedProfileId} onChange={(event) => setSelectedProfileId(event.target.value)}>
            <option value="">未选择</option>
            {profiles.map((profile) => (
              <option key={profile.id} value={profile.id}>
                {profile.model}
              </option>
            ))}
          </select>
        </div>

        <div className="side-section log-section">
          <div className="section-label">日志</div>
          <div className="log-list">
            {logs.map((log) => (
              <article className={`log-item ${log.status}`} key={log.id}>
                <header>
                  <ClipboardList size={14} />
                  <span>{log.action}</span>
                  <time>{new Date(log.created_at).toLocaleTimeString()}</time>
                </header>
                {log.chapter_title && <strong>{log.chapter_title}</strong>}
                <pre>{log.content}</pre>
              </article>
            ))}
            {logs.length === 0 && <p className="muted">暂无 AI 调用日志。</p>}
          </div>
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <h1>{detail?.novel.title ?? "工作台"}</h1>
            <p>
              {detail
                ? `${detail.chapters.length} 章 · ${detail.novel.encoding} · ${statusText[detail.novel.status] ?? detail.novel.status}`
                : "导入 TXT 后开始分析和改写"}
            </p>
          </div>
          <div className="topbar-actions">
            <button onClick={() => runJob("analysis")} disabled={!detail || !selectedProfileId || busy !== ""}>
              {busy === "analysis" ? <Loader2 className="spin" size={17} /> : <Play size={17} />}
              分析
            </button>
            <button onClick={() => runJob("rewrite")} disabled={!detail || !selectedProfileId || busy !== ""}>
              {busy === "rewrite" ? <Loader2 className="spin" size={17} /> : <RefreshCw size={17} />}
              改写
            </button>
            <button onClick={() => exportNovel("txt")} disabled={!detail || busy !== ""}>
              <Download size={17} />
              TXT
            </button>
            <button onClick={() => exportNovel("markdown")} disabled={!detail || busy !== ""}>
              <Download size={17} />
              MD
            </button>
          </div>
        </header>

        {notice && <div className="notice">{notice}</div>}
        {job && (
          <div className={`job-strip ${job.status}`}>
            <CheckCircle2 size={17} />
            <span>
              {job.job_type} · {statusText[job.status] ?? job.status} · {job.current_chapter}/
              {job.total_chapters} · {job.message}
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
            <div className="form-grid">
              <label>
                名称
                <input
                  value={profileDraft.name}
                  onChange={(event) => setProfileDraft({ ...profileDraft, name: event.target.value })}
                />
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
                <input
                  value={profileDraft.base_url}
                  onChange={(event) => setProfileDraft({ ...profileDraft, base_url: event.target.value })}
                />
              </label>
              <label>
                模型名
                <input
                  value={profileDraft.model}
                  onChange={(event) => setProfileDraft({ ...profileDraft, model: event.target.value })}
                />
              </label>
              <label>
                Temperature
                <input
                  type="number"
                  min="0"
                  max="2"
                  step="0.1"
                  value={profileDraft.temperature}
                  onChange={(event) =>
                    setProfileDraft({ ...profileDraft, temperature: Number(event.target.value) })
                  }
                />
              </label>
              <label>
                API Key
                <input
                  type="password"
                  value={profileDraft.api_key}
                  placeholder={selectedProfileId ? "留空则不保存 Key" : "填写 API Key 后保存"}
                  onFocus={() => {
                    if (profileDraft.api_key === savedApiKeyMask) {
                      setProfileDraft({ ...profileDraft, api_key: "" });
                    }
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
                  <textarea
                    value={asset.content}
                    onChange={(event) => updateCanon(asset.kind, event.target.value)}
                    placeholder="分析后会自动生成，也可以手动补充。"
                  />
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
                  <span>{chapter.index}. {chapter.title}</span>
                  <small>
                    分析 {statusText[chapter.analysis_status] ?? chapter.analysis_status} · 改写{" "}
                    {statusText[chapter.rewrite_status] ?? chapter.rewrite_status}
                  </small>
                </button>
              ))}
            </div>
          </section>

          <section className="panel compare-panel">
            <div className="panel-heading">
              <h2>{selectedChapter?.title ?? "原文 / 改写"}</h2>
            </div>
            {selectedChapter ? (
              <div className="compare-grid">
                <article>
                  <h3>原文</h3>
                  <pre>{selectedChapter.original_text}</pre>
                </article>
                <article>
                  <h3>改写稿</h3>
                  <pre>{selectedChapter.rewrite_text || "尚未改写。"}</pre>
                </article>
                <article className="analysis-output">
                  <h3>分析 JSON</h3>
                  <pre>{selectedChapter.analysis_json || "尚未分析。"}</pre>
                </article>
              </div>
            ) : (
              <p className="muted">选择章节后查看内容。</p>
            )}
          </section>
        </div>
      </section>
    </main>
  );
}
