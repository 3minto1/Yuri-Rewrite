import { ArrowLeft, GitCompareArrows, Loader2, RefreshCw, RotateCcw, Save, Square, Trash2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invokeCommand as invoke } from "../../tauriApi";
import type {
  RewriteAbApplyResult,
  RewriteAbChapterDetail,
  RewriteAbChoice,
  RewriteAbRunDetail,
  RewriteAbRunSummary,
  RewriteAbSlot
} from "../../types";
import { HighlightedText } from "./HighlightedText";
import { useChapterDiff } from "./CompareView";

type Side = "original" | RewriteAbSlot;
type DiffBasis = "original" | "pair";

type Props = {
  novelId: string;
  initialRunId: string;
  onBack: () => void;
  onRunChange?: (runId: string) => void;
  onNovelChanged: () => Promise<void>;
  onNotice: (message: string) => void;
};

const slotOrder: RewriteAbSlot[] = ["A", "B", "C"];
const emptyDiffRanges: [] = [];
const statusText: Record<string, string> = {
  running: "运行中",
  partial: "部分完成",
  ready: "待选稿",
  applied: "已应用",
  pending: "等待中",
  completed: "已完成",
  failed: "失败"
};

function countText(text: string) {
  return text.replace(/\s/g, "").length;
}

export function RewriteAbView({ novelId, initialRunId, onBack, onRunChange, onNovelChanged, onNotice }: Props) {
  const [runs, setRuns] = useState<RewriteAbRunSummary[]>([]);
  const [run, setRun] = useState<RewriteAbRunDetail | null>(null);
  const [runId, setRunId] = useState(initialRunId);
  const [chapterId, setChapterId] = useState("");
  const [chapter, setChapter] = useState<RewriteAbChapterDetail | null>(null);
  const [leftSide, setLeftSide] = useState<Side>("original");
  const [rightSide, setRightSide] = useState<Side>("A");
  const [diffEnabled, setDiffEnabled] = useState(true);
  const [diffBasis, setDiffBasis] = useState<DiffBasis>("original");
  const [busy, setBusy] = useState("");
  const [terminationRequested, setTerminationRequested] = useState(false);
  const leftRef = useRef<HTMLDivElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);

  const loadRuns = useCallback(async () => {
    const values = await invoke("list_rewrite_ab_runs", { novelId });
    setRuns(values);
    return values;
  }, [novelId]);

  const loadRun = useCallback(async (id: string) => {
    const value = await invoke("get_rewrite_ab_run", { runId: id });
    setRun(value);
    setChapterId((current) => value.chapters.some((item) => item.chapter_id === current) ? current : value.chapters[0]?.chapter_id ?? "");
    const available = value.models.map((model) => model.slot);
    setLeftSide((current) => current === "original" || available.includes(current) ? current : "original");
    setRightSide((current) => current !== "original" && available.includes(current) ? current : available[0] ?? "A");
    return value;
  }, []);

  useEffect(() => {
    setRunId(initialRunId);
  }, [initialRunId]);

  useEffect(() => {
    onRunChange?.(runId);
  }, [onRunChange, runId]);

  useEffect(() => {
    void Promise.all([loadRuns(), loadRun(runId)]).catch((error) => onNotice(String(error)));
  }, [loadRun, loadRuns, onNotice, runId]);

  useEffect(() => {
    if (!chapterId || !runId) {
      setChapter(null);
      return;
    }
    let cancelled = false;
    setChapter(null);
    void invoke("get_rewrite_ab_chapter", { runId, chapterId })
      .then((value) => { if (!cancelled) setChapter(value); })
      .catch((error) => { if (!cancelled) onNotice(String(error)); });
    return () => { cancelled = true; };
  }, [chapterId, onNotice, runId]);

  useEffect(() => {
    if (run?.status !== "running") return undefined;
    const timer = window.setInterval(() => {
      void loadRun(run.id)
        .then(async () => {
          await loadRuns();
          if (chapterId) setChapter(await invoke("get_rewrite_ab_chapter", { runId: run.id, chapterId }));
        })
        .catch((error) => onNotice(String(error)));
    }, 1500);
    return () => window.clearInterval(timer);
  }, [chapterId, loadRun, loadRuns, onNotice, run?.id, run?.status]);

  useEffect(() => {
    if (run?.status !== "running") setTerminationRequested(false);
  }, [run?.status]);

  useEffect(() => {
    if (leftSide === "original" || rightSide === "original") setDiffBasis("original");
  }, [leftSide, rightSide]);

  const sideText = useCallback((side: Side) => {
    if (!chapter) return "";
    if (side === "original") return chapter.original_text;
    return chapter.candidates.find((candidate) => candidate.slot === side)?.rewrite_text ?? "";
  }, [chapter]);
  const sideLabel = useCallback((side: Side) => {
    if (side === "original") return "原文";
    const model = run?.models.find((item) => item.slot === side);
    return `候选 ${side}${model ? ` · ${model.profile_name}` : ""}`;
  }, [run?.models]);
  const leftText = sideText(leftSide);
  const rightText = sideText(rightSide);
  const originalText = chapter?.original_text ?? "";
  const bothCandidates = leftSide !== "original" && rightSide !== "original";
  const usePairDiff = diffEnabled && bothCandidates && diffBasis === "pair";
  const leftOriginalDiff = useChapterDiff(
    `${runId}:${chapterId}:original:${leftSide}`,
    originalText,
    leftText,
    diffEnabled && leftSide !== "original" && !usePairDiff
  );
  const rightOriginalDiff = useChapterDiff(
    `${runId}:${chapterId}:original:${rightSide}`,
    originalText,
    rightText,
    diffEnabled && rightSide !== "original" && !usePairDiff
  );
  const pairDiff = useChapterDiff(
    `${runId}:${chapterId}:pair:${leftSide}:${rightSide}`,
    leftText,
    rightText,
    usePairDiff
  );
  const activeDiffs = usePairDiff
    ? [pairDiff]
    : [
      ...(leftSide !== "original" ? [leftOriginalDiff] : []),
      ...(rightSide !== "original" ? [rightOriginalDiff] : [])
    ];
  const diffLoading = activeDiffs.some((item) => item.loading);
  const diffError = activeDiffs.find((item) => item.error)?.error;
  const diffPlain = activeDiffs.some((item) => item.mode === "plain");
  const diffLine = activeDiffs.some((item) => item.mode === "line");
  const modelProgress = useMemo(() => (run?.models ?? []).map((model) => {
    const statuses = (run?.chapters ?? []).map((item) => item.candidate_statuses[model.slot]);
    const completed = statuses.filter((status) => status === "completed").length;
    const failed = statuses.filter((status) => status === "failed").length;
    const total = run?.chapter_count ?? 0;
    const percent = total > 0 ? Math.round((completed / total) * 100) : 0;
    const label = completed === total && total > 0
      ? "已完成"
      : run?.status === "running"
        ? "生成中"
        : failed > 0
          ? "部分失败"
          : "未完成";
    return { ...model, completed, failed, total, percent, label };
  }), [run]);
  const candidates = chapter?.candidates ?? [];
  const completedSlots = candidates.filter((candidate) => candidate.status === "completed" && candidate.rewrite_text?.trim()).map((candidate) => candidate.slot);

  async function mutate(label: string, action: () => Promise<RewriteAbRunDetail>) {
    setBusy(label);
    try {
      const value = await action();
      setRun(value);
      await loadRuns();
      if (chapterId) setChapter(await invoke("get_rewrite_ab_chapter", { runId: value.id, chapterId }));
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function saveChoices(choices: RewriteAbChoice[], replaceAll = false) {
    await mutate("choice", () => invoke("save_rewrite_ab_choices", {
      runId,
      choices,
      ...(replaceAll ? { replaceAll: true } : {})
    }));
  }

  async function retryRun() {
    if (!run) return;
    setBusy("retry");
    setRun({ ...run, status: "running" });
    try {
      const value = await invoke("retry_rewrite_ab", { runId });
      setRun(value);
      await loadRuns();
      if (chapterId) setChapter(await invoke("get_rewrite_ab_chapter", { runId, chapterId }));
    } catch (error) {
      onNotice(String(error));
      await loadRun(runId).catch(() => undefined);
    } finally {
      setBusy("");
    }
  }

  async function terminateRun() {
    if (!run || terminationRequested) return;
    setTerminationRequested(true);
    try {
      setRun(await invoke("terminate_rewrite_ab", { runId }));
      onNotice("已发送终止请求，正在等待当前请求安全结束。");
    } catch (error) {
      setTerminationRequested(false);
      onNotice(String(error));
    }
  }

  async function applyOrRestore(kind: "apply" | "restore", forceOverwrite = false) {
    if (kind === "apply" && !forceOverwrite) {
      const selectedSlots = new Set(run?.chapters.map((item) => item.selected_slot).filter(Boolean));
      if (selectedSlots.size > 1 && !window.confirm("当前批次混用了多个模型的候选，章节间可能出现文风连续性差异。仍要应用所选吗？")) return;
      if (run?.status === "partial" && !window.confirm("本次实验仍有未完成或失败的候选，但当前所选候选均可用。仍要应用所选吗？")) return;
    }
    setBusy(kind);
    try {
      const action = kind === "apply" ? "apply_rewrite_ab_choices" : "restore_rewrite_ab_baseline";
      let result: RewriteAbApplyResult = await invoke(action, { runId, forceOverwrite });
      if (result.status === "conflict") {
        const labels = result.conflict_chapter_ids.slice(0, 8).map((id) => run?.chapters.find((item) => item.chapter_id === id)?.title ?? id);
        const suffix = result.conflict_chapter_ids.length > 8 ? ` 等 ${result.conflict_chapter_ids.length} 章` : "";
        if (!window.confirm(`${labels.join("、")}${suffix} 的正式稿已发生变化。仍要强制覆盖吗？`)) return;
        result = await invoke(action, { runId, forceOverwrite: true });
      }
      await Promise.all([loadRun(runId), loadRuns(), onNovelChanged()]);
      onNotice(result.status === "restored" ? "已恢复 A/B 实验应用前的整批基线。" : "已将所选候选应用为正式改写稿。");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  async function deleteRun() {
    if (!run || !window.confirm(`删除 ${run.batch_label} 的 A/B 实验及全部候选？正式改写稿不会受影响。`)) return;
    setBusy("delete");
    try {
      await invoke("delete_rewrite_ab_run", { runId: run.id });
      const next = await loadRuns();
      if (next[0]) setRunId(next[0].id);
      else onBack();
      onNotice("A/B 实验已删除。");
    } catch (error) {
      onNotice(String(error));
    } finally {
      setBusy("");
    }
  }

  if (!run) return <div className="rewrite-ab-loading"><Loader2 className="spin" size={24} />正在载入 A/B 实验…</div>;
  const allSelected = run.selected_chapters === run.chapter_count;

  return (
    <div className="compare-page rewrite-ab-page">
      <div className="compare-page-toolbar rewrite-ab-toolbar">
        <div className="rewrite-ab-toolbar-main">
          <label>实验<select aria-label="A/B 实验" value={runId} onChange={(event) => setRunId(event.target.value)}>{runs.map((item) => <option key={item.id} value={item.id}>{item.batch_label} · {statusText[item.status] ?? item.status}</option>)}</select></label>
          <label>章节<select aria-label="A/B 章节" value={chapterId} onChange={(event) => setChapterId(event.target.value)}>{run.chapters.map((item) => <option key={item.chapter_id} value={item.chapter_id}>{item.chapter_index}. {item.title}{item.selected_slot ? ` · 已选 ${item.selected_slot}` : ""}</option>)}</select></label>
          <div className="rewrite-ab-progress" role="status"><strong>{statusText[run.status] ?? run.status}</strong><span>候选 {run.completed_candidates}/{run.total_candidates} · 已选 {run.selected_chapters}/{run.chapter_count}</span></div>
        </div>
        <div className="rewrite-ab-model-progress-list" aria-label="各模型改写进度">
          {modelProgress.map((model) => (
            <div className="rewrite-ab-model-progress" key={model.slot}>
              <div className="rewrite-ab-model-progress-heading">
                <strong>{model.slot} · {model.profile_name}</strong>
                <span>{model.label}</span>
              </div>
              <div
                className="rewrite-ab-model-progress-track"
                role="progressbar"
                aria-label={`候选 ${model.slot} ${model.profile_name} 进度`}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={model.percent}
                aria-valuetext={`已完成 ${model.completed}/${model.total} 章${model.failed ? `，失败 ${model.failed} 章` : ""}`}
              >
                <div className="rewrite-ab-model-progress-fill" style={{ width: `${model.percent}%` }} />
              </div>
              <div className="rewrite-ab-model-progress-meta">
                <span>完成 {model.completed}/{model.total} · {model.percent}%{model.failed ? ` · 失败 ${model.failed}` : ""}</span>
                <span>{run.review_enabled ? "改写 + 独立复检" : "独立改写"}</span>
              </div>
            </div>
          ))}
        </div>
        <div className="rewrite-ab-toolbar-actions">
          {slotOrder.filter((slot) => run.models.some((model) => model.slot === slot)).map((slot) => {
            const coversEveryChapter = run.chapters.length > 0
              && run.chapters.every((item) => item.candidate_statuses[slot] === "completed");
            const descriptionId = `rewrite-ab-bulk-${slot}-description`;
            return (
              <Fragment key={slot}>
                <button
                  type="button"
                  aria-describedby={!coversEveryChapter ? descriptionId : undefined}
                  onClick={() => void saveChoices(
                    run.chapters.map((item) => ({ chapter_id: item.chapter_id, slot })),
                    true
                  )}
                  disabled={!coversEveryChapter || run.status === "running" || busy !== ""}
                >
                  整批采用 {slot}
                </button>
                {!coversEveryChapter && (
                  <span id={descriptionId} className="sr-only">
                    候选 {slot} 尚未覆盖当前实验的所有章节，不能整批采用。
                  </span>
                )}
              </Fragment>
            );
          })}
          {run.status === "partial" && <button type="button" onClick={() => void retryRun()} disabled={busy !== ""}><RefreshCw size={16} />重试失败项</button>}
          {run.status === "running" && <button type="button" onClick={() => void terminateRun()} disabled={terminationRequested || (busy !== "" && busy !== "retry")}><Square size={16} />{terminationRequested ? "终止中" : "终止"}</button>}
          <button className="action-primary" type="button" onClick={() => void applyOrRestore("apply")} disabled={!allSelected || run.status === "running" || busy !== ""}><Save size={16} />应用所选</button>
          {run.status === "applied" && <button type="button" onClick={() => void applyOrRestore("restore")} disabled={busy !== ""}><RotateCcw size={16} />撤销应用</button>}
          <button className="danger-button" type="button" onClick={() => void deleteRun()} disabled={busy !== "" || run.status === "running"}><Trash2 size={16} />删除实验</button>
          <button type="button" onClick={onBack} disabled={busy !== ""}><ArrowLeft size={16} />返回工作台</button>
        </div>
        <div className="rewrite-ab-side-controls">
          <button
            type="button"
            className={diffEnabled ? "active" : ""}
            aria-pressed={diffEnabled}
            onClick={() => setDiffEnabled((value) => !value)}
          >
            <GitCompareArrows size={17} aria-hidden="true" />差异
          </button>
          <label>左侧<select aria-label="左侧对比内容" value={leftSide} onChange={(event) => setLeftSide(event.target.value as Side)}><option value="original">原文</option>{run.models.map((model) => <option key={model.slot} value={model.slot}>候选 {model.slot}</option>)}</select></label>
          <span>对比</span>
          <label>右侧<select aria-label="右侧对比内容" value={rightSide} onChange={(event) => setRightSide(event.target.value as Side)}><option value="original">原文</option>{run.models.map((model) => <option key={model.slot} value={model.slot}>候选 {model.slot}</option>)}</select></label>
          <label title={bothCandidates ? undefined : "双候选模式下可以切换差异基准"}>
            差异基准
            <select
              aria-label="差异基准"
              value={diffBasis}
              onChange={(event) => setDiffBasis(event.target.value as DiffBasis)}
              disabled={!diffEnabled || !bothCandidates}
            >
              <option value="original">各自与原文</option>
              <option value="pair">左右互比</option>
            </select>
          </label>
          <span className="compare-word-count">字数：左 {countText(leftText)} · 右 {countText(rightText)} · 差 {countText(rightText) - countText(leftText) >= 0 ? "+" : ""}{countText(rightText) - countText(leftText)}</span>
        </div>
      </div>
      {chapter ? (
        <>
          <div className="rewrite-ab-choice-strip" role="group" aria-label="选择本章候选">
            <span>本章采用：</span>
            {run.models.map((model) => {
              const candidate = candidates.find((item) => item.slot === model.slot);
              return <button key={model.slot} className={chapter.selected_slot === model.slot ? "active" : ""} aria-pressed={chapter.selected_slot === model.slot} type="button" onClick={() => void saveChoices([{ chapter_id: chapter.chapter_id, slot: model.slot }])} disabled={run.status === "running" || candidate?.status !== "completed" || busy !== ""}>{model.slot} · {candidate ? statusText[candidate.status] ?? candidate.status : "等待中"}</button>;
            })}
            {candidates.some((candidate) => candidate.error) && <span className="field-error">{candidates.filter((candidate) => candidate.error).map((candidate) => `${candidate.slot}: ${candidate.error}`).join("；")}</span>}
          </div>
          {diffEnabled && (diffLoading || diffError || diffPlain || diffLine || (bothCandidates && diffBasis === "original")) && (
            <div className={diffError ? "compare-diff-status error" : "compare-diff-status"} role="status">
              {diffLoading
                ? `正在计算${bothCandidates && diffBasis === "original" ? "两组与原文" : ""}差异…`
                : diffError
                  ? `${diffError}，已显示普通文本。`
                  : diffPlain
                    ? "文本差异过大，已关闭本章差异高亮。"
                    : diffLine
                      ? "长文本或高差异内容已使用行级差异高亮。"
                      : "双候选模式下，两栏分别高亮相对原文的新增或改动；切换任一栏为原文可查看被删除内容。"}
            </div>
          )}
          <div className="large-compare-grid rewrite-ab-compare-grid">
            {([leftSide, rightSide] as const).map((side, index) => {
              const text = index === 0 ? leftText : rightText;
              const ref = index === 0 ? leftRef : rightRef;
              const paneDiff = usePairDiff
                ? pairDiff
                : side === "original"
                  ? (index === 0 ? rightOriginalDiff : leftOriginalDiff)
                  : (index === 0 ? leftOriginalDiff : rightOriginalDiff);
              const paneDiffSide = usePairDiff
                ? (index === 0 ? "original" : "rewrite")
                : (side === "original" ? "original" : "rewrite");
              const reviewSummary = side === "original"
                ? null
                : chapter.candidates.find((candidate) => candidate.slot === side)?.review_summary;
              return (
                <article key={`${index}-${side}`}>
                  <div className="compare-pane-heading">
                    <h2>{sideLabel(side)}</h2>
                    {reviewSummary ? <small>{reviewSummary}</small> : null}
                  </div>
                  <div ref={ref} className="compare-text" aria-label={`${sideLabel(side)}内容`}>
                    {text ? (
                      <HighlightedText
                        text={text}
                        side={paneDiffSide}
                        containerRef={ref}
                        diffRanges={diffEnabled ? paneDiff.ranges : emptyDiffRanges}
                        searchMatches={[]}
                        highlightNamespace={index === 0 ? "rewrite-ab-left" : "rewrite-ab-right"}
                      />
                    ) : (
                      <span className="muted">
                        {completedSlots.includes(side as RewriteAbSlot) ? "候选正文为空。" : "候选尚未完成。"}
                      </span>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        </>
      ) : <div className="rewrite-ab-loading"><Loader2 className="spin" size={22} />正在载入当前章节候选…</div>}
    </div>
  );
}
