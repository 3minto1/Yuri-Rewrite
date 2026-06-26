import { ArrowLeft, Loader2, Play, Save, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { invokeCommand } from "../../tauriApi";
import type { CanonAsset, Chapter, ChapterRule, ChapterRulePreview, Novel } from "../../types";

const defaultRule: ChapterRule = {
  mode: "simple",
  line_start: true,
  prefix: "第",
  number_type: "mixed",
  unit: "章",
  include_pattern: String.raw`^\s*(序言|序章|序卷|序[1-9]|序曲|楔子|引子|引言|序幕|前言|终章|最终章|尾声|后记|卷末后记|完本感言|番外|番外篇|番外章|特别篇|外传|插曲|间章)`,
  extra_pattern: "未完待续|作者的话|求月票|求推荐票|第二更|第三更",
  regex_pattern: String.raw`^\s*第[0-9０-９零〇一二两三四五六七八九十百千万]+章\s*.+$`
};

const unitOptions = ["章", "回", "卷", "节", "集", "部", "篇", "话", "幕", "更", "[章回卷节集部篇话幕更]"];
const prefixOptions = [
  { value: "第", label: "第" },
  { value: "卷", label: "卷" },
  { value: "[第卷]", label: "[第卷]" }
];

type ChapterRulesPageProps = {
  novel: Novel | null;
  chapters: Chapter[];
  canonAssets: CanonAsset[];
  busy: string;
  processing: boolean;
  onBack: () => void;
  onApplied: (novelId: string) => Promise<void>;
  onUseBuiltin: (novelId: string, splitLongChapters?: boolean) => Promise<void>;
  showNotice: (message: string) => void;
};

function normalizeQuery(value: string) {
  return value.trim().toLowerCase();
}

function chapterHasProcessingTrace(chapter: Chapter) {
  return chapter.analysis_status !== "pending"
    || chapter.rewrite_status !== "pending"
    || Boolean(chapter.analysis_json?.trim())
    || Boolean(chapter.rewrite_text?.trim())
    || Boolean(chapter.rewrite_edited);
}

function chapterRuleApplyState(
  novel: Novel | null,
  chapters: Chapter[],
  canonAssets: CanonAsset[],
  processing: boolean
) {
  if (!novel) {
    return {
      canApply: false,
      message: "请先导入或选择一本小说。"
    };
  }
  if (processing) {
    return {
      canApply: false,
      message: "当前任务运行或暂停中，不能重新生成章节列表。"
    };
  }
  if (novel.status === "pending_split") {
    return {
      canApply: true,
      message: `为《${novel.title}》生成章节列表。`
    };
  }
  if (novel.status !== "imported") {
    return {
      canApply: false,
      message: "当前小说状态不支持重新拆分。"
    };
  }
  if (chapters.some(chapterHasProcessingTrace)) {
    return {
      canApply: false,
      message: "已开始分析或改写，不能重新拆分；如需修改章节规则，请重新导入小说。"
    };
  }
  if (canonAssets.some((asset) => asset.content.trim() !== "")) {
    return {
      canApply: false,
      message: "已有手动一致性资产内容，不能重新拆分；如需修改章节规则，请重新导入小说。"
    };
  }
  return {
    canApply: true,
    message: "尚未开始分析或改写，可重新生成章节列表；这会替换当前章节划分。"
  };
}

function confirmApplyChapterRule(pendingSplit: boolean, splitLongChapters: boolean) {
  if (pendingSplit) return true;
  const splitNotice = splitLongChapters
    ? "\n\n已开启长章节自动拆分，超过 5000 字的单章会按拆分结果重建为多个章节。"
    : "";
  return window.confirm(
    `重新生成章节列表会替换当前章节划分和批次范围，并清空尚未使用的一致性资产。${splitNotice}\n\n不会删除原始 TXT、小说设定、模型配置或应用设置。\n\n确定继续吗？`
  );
}

export function ChapterRulesPage({
  novel,
  chapters,
  canonAssets,
  busy,
  processing,
  onBack,
  onApplied,
  onUseBuiltin,
  showNotice
}: ChapterRulesPageProps) {
  const [rule, setRule] = useState<ChapterRule>(defaultRule);
  const [preview, setPreview] = useState<ChapterRulePreview | null>(null);
  const [query, setQuery] = useState("");
  const [loadingRule, setLoadingRule] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [splitLongChapters, setSplitLongChapters] = useState(false);

  const pendingSplit = novel?.status === "pending_split";
  const applyState = useMemo(
    () => chapterRuleApplyState(novel, chapters, canonAssets, processing),
    [canonAssets, chapters, novel, processing]
  );
  const disabled = processing || !novel || loadingRule || previewing || saving;
  const normalizedQuery = normalizeQuery(query);
  const visiblePreviewChapters = useMemo(() => {
    const rows = preview?.chapters ?? [];
    if (!normalizedQuery) return rows;
    const numeric = /^\d+$/.test(normalizedQuery) ? Number.parseInt(normalizedQuery, 10) : NaN;
    if (Number.isFinite(numeric)) {
      return rows.filter((chapter) => chapter.index === numeric || chapter.title.includes(normalizedQuery));
    }
    return rows.filter((chapter) => chapter.title.toLowerCase().includes(normalizedQuery));
  }, [normalizedQuery, preview?.chapters]);

  useEffect(() => {
    setPreview(null);
    setQuery("");
    setSplitLongChapters(false);
    if (!novel) {
      setRule(defaultRule);
      return;
    }
    let cancelled = false;
    setLoadingRule(true);
    invokeCommand("get_chapter_rule", { novelId: novel.id })
      .then((stored) => {
        if (cancelled) return;
        setRule(stored?.rule ?? defaultRule);
      })
      .catch((error) => {
        if (!cancelled) showNotice(String(error));
      })
      .finally(() => {
        if (!cancelled) setLoadingRule(false);
      });
    return () => {
      cancelled = true;
    };
  }, [novel?.id, showNotice]);

  async function generatePreview() {
    if (!novel) return;
    setPreviewing(true);
    setPreview(null);
    try {
      const result = await invokeCommand("preview_chapter_rule", {
        novelId: novel.id,
        rule,
        splitLongChapters
      });
      setPreview(result);
      showNotice(result.message);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setPreviewing(false);
    }
  }

  async function saveAndApply() {
    if (!novel || !preview?.can_apply || !applyState.canApply) return;
    if (!confirmApplyChapterRule(Boolean(pendingSplit), splitLongChapters)) return;
    setSaving(true);
    try {
      await invokeCommand("save_chapter_rule_and_split", {
        novelId: novel.id,
        rule,
        splitLongChapters
      });
      await onApplied(novel.id);
    } catch (error) {
      showNotice(String(error));
    } finally {
      setSaving(false);
    }
  }

  async function useBuiltinRule() {
    if (!novel || !applyState.canApply) return;
    if (!confirmApplyChapterRule(Boolean(pendingSplit), splitLongChapters)) return;
    setSaving(true);
    try {
      await onUseBuiltin(novel.id, splitLongChapters);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="page-panel chapter-rules-page">
      <div className="page-heading">
        <div>
          <h2>章节规则</h2>
          <p>{applyState.message}</p>
        </div>
        <div className="panel-actions">
          <button onClick={onBack}><ArrowLeft size={16} />返回</button>
          {novel && (
            <button
              onClick={useBuiltinRule}
              disabled={disabled || !applyState.canApply || busy === "chapter-split"}
              title={!applyState.canApply ? applyState.message : undefined}
            >
              {busy === "chapter-split" || saving ? <Loader2 className="spin" size={16} /> : <Play size={16} />}
              使用内置规则
            </button>
          )}
          {novel && (
            <button
              className="action-primary"
              onClick={saveAndApply}
              disabled={disabled || !applyState.canApply || !preview?.can_apply || busy === "chapter-rule"}
              title={!applyState.canApply ? applyState.message : !preview?.can_apply ? "请先生成有效预览" : undefined}
            >
              {busy === "chapter-rule" || saving ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
              保存
            </button>
          )}
        </div>
      </div>

      {novel && (
        <div className={applyState.canApply ? "chapter-rule-status can-apply" : "chapter-rule-status blocked"}>
          {applyState.message}
        </div>
      )}

      <section className="settings-section chapter-rule-editor">
        <div className="segmented-control chapter-rule-mode" role="radiogroup" aria-label="章节规则模式">
          <button
            type="button"
            className={rule.mode === "simple" ? "active" : ""}
            aria-checked={rule.mode === "simple"}
            role="radio"
            onClick={() => setRule({ ...rule, mode: "simple" })}
            disabled={disabled}
          >
            简易规则
          </button>
          <button
            type="button"
            className={rule.mode === "regex" ? "active" : ""}
            aria-checked={rule.mode === "regex"}
            role="radio"
            onClick={() => setRule({ ...rule, mode: "regex" })}
            disabled={disabled}
          >
            正则表达式
          </button>
        </div>

        {rule.mode === "simple" ? (
          <div className="chapter-rule-grid">
            <label className="chapter-rule-checkbox">
              <input
                type="checkbox"
                checked={rule.line_start}
                onChange={(event) => setRule({ ...rule, line_start: event.target.checked })}
                disabled={disabled}
              />
              行首标识
            </label>
            <label>
              前缀
              <select
                value={rule.prefix}
                onChange={(event) => setRule({ ...rule, prefix: event.target.value })}
                disabled={disabled}
              >
                {prefixOptions.map((option, index) => (
                  <option key={`${option.value}-${index}`} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              数字类型
              <select
                value={rule.number_type}
                onChange={(event) => setRule({ ...rule, number_type: event.target.value as ChapterRule["number_type"] })}
                disabled={disabled}
              >
                <option value="mixed">混合型数字</option>
                <option value="chinese">纯中文数字</option>
                <option value="arabic">纯阿拉伯数字</option>
              </select>
            </label>
            <label>
              单位
              <select
                value={rule.unit}
                onChange={(event) => setRule({ ...rule, unit: event.target.value })}
                disabled={disabled}
              >
                {unitOptions.map((unit) => <option key={unit} value={unit}>{unit}</option>)}
              </select>
            </label>
            <label className="chapter-rule-wide">
              附加规则
              <input
                value={rule.include_pattern}
                onChange={(event) => setRule({ ...rule, include_pattern: event.target.value })}
                placeholder={String.raw`^\s*(序言|序章|楔子|前言|终章|番外)`}
                disabled={disabled}
              />
            </label>
            <label className="chapter-rule-wide">
              排除规则
              <input
                value={rule.extra_pattern}
                onChange={(event) => setRule({ ...rule, extra_pattern: event.target.value })}
                placeholder="未完待续|作者的话|求月票|求推荐票|第二更|第三更"
                disabled={disabled}
              />
            </label>
          </div>
        ) : (
          <label className="chapter-rule-regex">
            正则表达式
            <input
              value={rule.regex_pattern}
              onChange={(event) => setRule({ ...rule, regex_pattern: event.target.value })}
              placeholder={String.raw`^\s*第(\d+)\s*章\s+(.+)$`}
              disabled={disabled}
            />
          </label>
        )}

        <div className="chapter-rule-actions">
          <button type="button" onClick={generatePreview} disabled={disabled || !novel}>
            {previewing ? <Loader2 className="spin" size={16} /> : <Play size={16} />}
            生成预览
          </button>
          <span className="muted">
            预览不会修改数据库；满意后点击保存才会生成章节列表。
          </span>
        </div>

        <div className="setting-toggle-row chapter-rule-long-split">
          <button
            type="button"
            className={splitLongChapters ? "setting-switch active" : "setting-switch"}
            onClick={() => setSplitLongChapters((enabled) => !enabled)}
            disabled={disabled}
            aria-pressed={splitLongChapters}
          >
            {splitLongChapters ? "开启" : "关闭"}
          </button>
          <span>长章节自动拆分：仅影响本次预览和生成章节列表；单章正文超过 5000 字时会拆成（1）（2）等多个章节。</span>
        </div>
      </section>

      <section className="panel chapter-rule-preview-panel">
        <div className="panel-heading">
          <div>
            <h2>章节预览</h2>
            <p className="muted">
              {preview
                ? `共 ${preview.total_chapters} 章，当前显示 ${visiblePreviewChapters.length} 章。`
                : "暂无预览。"}
            </p>
          </div>
          <label className="chapter-rule-search">
            <Search size={16} />
            <input
              aria-label="搜索预览章节"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索章号/标题"
              disabled={!preview}
            />
          </label>
        </div>
        <div className="chapter-rule-preview-list">
          {visiblePreviewChapters.map((chapter) => (
            <div className="chapter-rule-preview-item" key={`${chapter.index}-${chapter.title}`}>
              <span className="chapter-rule-preview-index">{chapter.index}.</span>
              <span>{chapter.title}</span>
            </div>
          ))}
          {preview && visiblePreviewChapters.length === 0 && (
            <p className="settings-empty-hint">没有匹配当前搜索的章节。</p>
          )}
          {!preview && (
            <p className="settings-empty-hint">点击“生成预览”后，这里会显示按当前规则识别到的章节。</p>
          )}
        </div>
      </section>
    </div>
  );
}
