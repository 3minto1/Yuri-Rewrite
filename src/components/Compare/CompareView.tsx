import { ArrowLeft, CaseSensitive, ChevronDown, ChevronUp, Download, GitCompareArrows, Pencil, RefreshCw, RotateCcw, Save, Search, X } from "lucide-react";
import { memo, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import type { Chapter } from "../../types";
import { Modal } from "../common/Modal";
import { calculateDiff, type DiffRange, type DiffResult, type DiffSide } from "./compareDiff";
import { getCachedDiff, setCachedDiff } from "./compareDiffCache";
import { HighlightedText } from "./HighlightedText";
import { buildSearchMatches, initialSearchIndex, moveSearchIndex, type SearchMatch, type SearchScope } from "./compareSearch";

type CompareViewProps = {
  chapters: Chapter[];
  selectedChapter?: Chapter;
  selectedChapterId: string;
  busy: string;
  originalRef: RefObject<HTMLDivElement>;
  rewriteRef: RefObject<HTMLDivElement>;
  onSelectChapter: (chapterId: string) => void;
  onBack: () => void;
  onExport: () => void;
  editingAllowed?: boolean;
  editDisabledReason?: string;
  onSaveRewrite?: (chapterId: string, rewriteText: string) => Promise<void>;
  onRestoreRewrite?: (chapterId: string) => Promise<void>;
  onRewriteChapter?: (
    chapterId: string,
    instructions: string,
    sourceMode: "original" | "rewrite"
  ) => Promise<void>;
  onRestoreInitialRewrite?: (chapterId: string) => Promise<void>;
};

type DiffState = DiffResult & {
  loading: boolean;
  chapterId: string;
  original: string;
  rewrite: string;
  cached: boolean;
  error?: string;
};

const EMPTY_RANGES: DiffRange[] = [];

function emptyDiffState(chapterId: string, original: string, rewrite: string, loading = false): DiffState {
  return { ranges: [], mode: "mixed", loading, chapterId, original, rewrite, cached: false };
}

export function useChapterDiff(chapterId: string, original: string, rewrite: string, enabled: boolean): DiffState {
  const requestIdRef = useRef(0);
  const [state, setState] = useState<DiffState>(() => emptyDiffState(chapterId, original, rewrite));

  useEffect(() => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    if (!enabled || !rewrite.trim()) {
      setState(emptyDiffState(chapterId, original, rewrite));
      return undefined;
    }

    const cached = getCachedDiff(chapterId, original, rewrite);
    if (cached) {
      setState({ ...cached, loading: false, chapterId, original, rewrite, cached: true });
      return undefined;
    }

    let cancelled = false;
    setState(emptyDiffState(chapterId, original, rewrite, true));
    if (typeof Worker === "undefined") {
      Promise.resolve().then(() => calculateDiff(original, rewrite)).then((result) => {
        if (!cancelled && requestIdRef.current === requestId) {
          setCachedDiff(chapterId, original, rewrite, result);
          setState({ ...result, loading: false, chapterId, original, rewrite, cached: false });
        }
      }).catch((error) => {
        if (!cancelled && requestIdRef.current === requestId) {
          setState({ ...emptyDiffState(chapterId, original, rewrite), error: String(error) });
        }
      });
      return () => { cancelled = true; };
    }

    const worker = new Worker(new URL("./compareDiff.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<{ requestId: number; result?: DiffResult; error?: string }>) => {
      if (cancelled || event.data.requestId !== requestId || requestIdRef.current !== requestId) return;
      if (event.data.result) {
        setCachedDiff(chapterId, original, rewrite, event.data.result);
        setState({ ...event.data.result, loading: false, chapterId, original, rewrite, cached: false });
      } else {
        setState({ ...emptyDiffState(chapterId, original, rewrite), error: event.data.error || "差异计算失败" });
      }
    };
    worker.onerror = () => {
      if (!cancelled && requestIdRef.current === requestId) {
        setState({ ...emptyDiffState(chapterId, original, rewrite), error: "差异计算失败" });
      }
    };
    worker.postMessage({ requestId, original, rewrite });
    return () => {
      cancelled = true;
      worker.terminate();
    };
  }, [chapterId, enabled, original, rewrite]);

  if (state.chapterId === chapterId && state.original === original && state.rewrite === rewrite) return state;
  return emptyDiffState(chapterId, original, rewrite, enabled && Boolean(rewrite.trim()));
}

type TextPaneProps = {
  heading: string;
  side: DiffSide;
  text: string;
  emptyText?: string;
  containerRef: RefObject<HTMLDivElement>;
  diffRanges: DiffRange[];
  searchMatches: SearchMatch[];
  activeMatchId?: string;
  headerActions?: ReactNode;
  editor?: ReactNode;
};

const TextPane = memo(function TextPane(props: TextPaneProps) {
  const { heading, side, text, emptyText, containerRef, diffRanges, searchMatches, activeMatchId, headerActions, editor } = props;
  return (
    <article>
      <div className="compare-pane-heading">
        <h2>{heading}</h2>
        {headerActions}
      </div>
      {editor ?? (
        <div ref={containerRef} className="compare-text" aria-label={`${heading}内容`}>
          {text ? (
          <HighlightedText
            text={text}
            side={side}
            containerRef={containerRef}
            diffRanges={diffRanges}
            searchMatches={searchMatches}
            activeMatchId={activeMatchId}
          />
          ) : <span className="muted">{emptyText}</span>}
        </div>
      )}
    </article>
  );
});

export const CompareView = memo(function CompareView(props: CompareViewProps) {
  const {
    chapters, selectedChapter, selectedChapterId, busy, originalRef, rewriteRef,
    onSelectChapter, onBack, onExport, editingAllowed = false, editDisabledReason,
    onSaveRewrite = async () => undefined, onRestoreRewrite = async () => undefined,
    onRewriteChapter = async () => undefined,
    onRestoreInitialRewrite = async () => undefined
  } = props;
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [searchScope, setSearchScope] = useState<SearchScope>("both");
  const [activeMatchIndex, setActiveMatchIndex] = useState<number | null>(null);
  const [wrapped, setWrapped] = useState(false);
  const [diffEnabled, setDiffEnabled] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editDraft, setEditDraft] = useState("");
  const [editBusy, setEditBusy] = useState(false);
  const [pendingNavigation, setPendingNavigation] = useState(false);
  const [rewriteDialogOpen, setRewriteDialogOpen] = useState(false);
  const [rewriteSourceMode, setRewriteSourceMode] = useState<"original" | "rewrite">("original");
  const [rewriteMenuOpen, setRewriteMenuOpen] = useState(false);
  const [rewriteInstructions, setRewriteInstructions] = useState("");
  const [rewriteBusy, setRewriteBusy] = useState(false);
  const pendingNavigationRef = useRef<(() => void) | null>(null);
  const deferredQuery = useDeferredValue(query);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const navigationTargetRef = useRef<string | null>(null);
  const previousChapterRef = useRef(selectedChapterId);
  const globalMatches = useMemo(
    () => buildSearchMatches(chapters, deferredQuery, caseSensitive, searchScope),
    [caseSensitive, chapters, deferredQuery, searchScope]
  );
  const activeMatch = activeMatchIndex === null ? undefined : globalMatches[activeMatchIndex];
  const originalText = selectedChapter?.original_text ?? "";
  const rewriteText = selectedChapter?.rewrite_text ?? "";
  const diff = useChapterDiff(selectedChapterId, originalText, rewriteText, diffEnabled);
  const visibleDiffRanges = diffEnabled ? diff.ranges : EMPTY_RANGES;
  const chapterMatches = useMemo(() => globalMatches.filter((match) => match.chapter_id === selectedChapterId), [globalMatches, selectedChapterId]);
  const originalMatches = useMemo(() => chapterMatches.filter((match) => match.side === "original"), [chapterMatches]);
  const rewriteMatches = useMemo(() => chapterMatches.filter((match) => match.side === "rewrite"), [chapterMatches]);
  const editDirty = editing && editDraft !== rewriteText;

  function runOrConfirmNavigation(action: () => void) {
    if (!editDirty) {
      action();
      return;
    }
    pendingNavigationRef.current = action;
    setPendingNavigation(true);
  }

  async function saveEdit(closeAfterSave = true) {
    if (!selectedChapter) return false;
    if (!editDraft.trim()) return false;
    setEditBusy(true);
    try {
      await onSaveRewrite(selectedChapter.id, editDraft);
      if (closeAfterSave) setEditing(false);
      return true;
    } finally {
      setEditBusy(false);
    }
  }

  function finishPendingNavigation() {
    const action = pendingNavigationRef.current;
    pendingNavigationRef.current = null;
    setPendingNavigation(false);
    setEditing(false);
    action?.();
  }

  function closeSearch() {
    setSearchOpen(false);
    setQuery("");
    setActiveMatchIndex(null);
    setWrapped(false);
  }

  async function confirmRewriteChapter() {
    if (!selectedChapter) return;
    setRewriteBusy(true);
    try {
      await onRewriteChapter(selectedChapter.id, rewriteInstructions, rewriteSourceMode);
      setRewriteDialogOpen(false);
      setRewriteInstructions("");
      setEditing(false);
    } finally {
      setRewriteBusy(false);
    }
  }

  async function restoreInitialRewrite() {
    if (!selectedChapter) return;
    if (!window.confirm("恢复到单章重新改写前的初稿？当前重新改写结果和之后的人工修改将被覆盖。")) return;
    setRewriteBusy(true);
    try {
      await onRestoreInitialRewrite(selectedChapter.id);
      setEditing(false);
    } finally {
      setRewriteBusy(false);
    }
  }

  function selectSearchMatch(index: number | null, didWrap = false) {
    setActiveMatchIndex(index);
    setWrapped(didWrap);
    if (index === null) return;
    const match = globalMatches[index];
    if (match.chapter_id !== selectedChapterId) {
      runOrConfirmNavigation(() => {
        navigationTargetRef.current = match.chapter_id;
        onSelectChapter(match.chapter_id);
      });
    }
  }

  function navigateSearch(direction: 1 | -1) {
    if (globalMatches.length === 0) {
      setActiveMatchIndex(null);
      return;
    }
    if (activeMatchIndex === null) {
      selectSearchMatch(initialSearchIndex(globalMatches, selectedChapterId, direction));
      return;
    }
    const next = moveSearchIndex(activeMatchIndex, globalMatches.length, direction);
    selectSearchMatch(next.index, next.wrapped);
  }

  function handleManualChapterSelect(chapterId: string) {
    navigationTargetRef.current = null;
    setActiveMatchIndex(null);
    setWrapped(false);
    runOrConfirmNavigation(() => onSelectChapter(chapterId));
  }

  useEffect(() => {
    if (!searchOpen) return;
    searchInputRef.current?.focus();
  }, [searchOpen]);

  useEffect(() => {
    function handleKeyboard(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        event.stopImmediatePropagation();
        setSearchOpen(true);
        window.requestAnimationFrame(() => searchInputRef.current?.focus());
        return;
      }
      if (event.key === "Escape" && searchOpen) {
        event.preventDefault();
        event.stopImmediatePropagation();
        closeSearch();
      }
    }
    window.addEventListener("keydown", handleKeyboard, true);
    return () => window.removeEventListener("keydown", handleKeyboard, true);
  }, [searchOpen]);

  useEffect(() => {
    if (!deferredQuery) {
      setActiveMatchIndex(null);
      setWrapped(false);
      return;
    }
    const index = initialSearchIndex(globalMatches, selectedChapterId, 1);
    selectSearchMatch(index);
  }, [caseSensitive, deferredQuery, searchScope]);

  useEffect(() => {
    if (previousChapterRef.current === selectedChapterId) return;
    if (navigationTargetRef.current === selectedChapterId) navigationTargetRef.current = null;
    else {
      setActiveMatchIndex(null);
      setWrapped(false);
    }
    previousChapterRef.current = selectedChapterId;
  }, [selectedChapterId]);

  useEffect(() => {
    setEditing(false);
    setEditDraft(selectedChapter?.rewrite_text ?? "");
    setRewriteMenuOpen(false);
    pendingNavigationRef.current = null;
    setPendingNavigation(false);
  }, [selectedChapterId]);

  return (
    <div className="compare-page">
      <div className="compare-page-toolbar">
        <label>
          章节
          <select value={selectedChapterId} onChange={(event) => handleManualChapterSelect(event.target.value)}>
            {chapters.map((chapter) => <option key={chapter.id} value={chapter.id}>{chapter.index}. {chapter.title}</option>)}
          </select>
        </label>
        <div className="compare-toolbar-actions">
          <button
            onClick={() => runOrConfirmNavigation(() => void restoreInitialRewrite())}
            disabled={
              !selectedChapter?.single_rewrite_original_available
              || !rewriteText.trim()
              || !editingAllowed
              || rewriteBusy
              || busy !== ""
            }
            title={
              !selectedChapter?.single_rewrite_original_available
                ? "当前章节没有可恢复的单章重写初稿"
                : editingAllowed
                  ? "恢复到第一次单章重新改写前的初稿"
                  : editDisabledReason
            }
          >
            <RotateCcw size={17} />恢复初稿
          </button>
          <button className={searchOpen ? "active" : ""} aria-pressed={searchOpen} onClick={() => searchOpen ? closeSearch() : setSearchOpen(true)}><Search size={17} />查找</button>
          <button className={diffEnabled ? "active" : ""} aria-pressed={diffEnabled} onClick={() => setDiffEnabled((value) => !value)}><GitCompareArrows size={17} />差异</button>
          <div className="split-button compare-rewrite-split">
            <button
              className="split-button-main"
              onClick={() => runOrConfirmNavigation(() => {
                setRewriteSourceMode("original");
                setRewriteDialogOpen(true);
              })}
              disabled={!editingAllowed || !rewriteText.trim() || busy !== ""}
              title={editingAllowed ? "以原文为主要输入重新生成本章" : editDisabledReason}
            >
              <RefreshCw size={17} />重写本章（原文）
            </button>
            <button
              className="split-button-toggle"
              type="button"
              aria-label="重写本章选项"
              aria-expanded={rewriteMenuOpen}
              onClick={() => setRewriteMenuOpen((open) => !open)}
              disabled={!editingAllowed || !rewriteText.trim() || busy !== ""}
              title="选择单章重写来源"
            >
              <ChevronDown size={16} />
            </button>
            {rewriteMenuOpen && (
              <div className="split-button-menu" role="menu">
                <button
                  role="menuitem"
                  type="button"
                  onClick={() => {
                    setRewriteMenuOpen(false);
                    runOrConfirmNavigation(() => {
                      setRewriteSourceMode("rewrite");
                      setRewriteDialogOpen(true);
                    });
                  }}
                >
                  重写本章（改写稿）
                </button>
              </div>
            )}
          </div>
          <button onClick={() => runOrConfirmNavigation(onBack)}><ArrowLeft size={17} />返回</button>
          <button onClick={onExport} disabled={busy !== ""}><Download size={17} />TXT</button>
        </div>
      </div>
      {searchOpen && (
        <div className="compare-search-bar" role="search">
          <Search size={18} aria-hidden="true" />
          <input
            ref={searchInputRef}
            aria-label="全局搜索"
            placeholder={searchScope === "both" ? "搜索全部章节的原文和改写稿" : searchScope === "original" ? "仅搜索全部章节的原文" : "仅搜索全部章节的改写稿"}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                navigateSearch(event.shiftKey ? -1 : 1);
              }
            }}
          />
          <select
            className="compare-search-scope"
            aria-label="查找范围"
            value={searchScope}
            onChange={(event) => setSearchScope(event.target.value as SearchScope)}
          >
            <option value="both">原文和改写稿</option>
            <option value="original">仅原文</option>
            <option value="rewrite">仅改写稿</option>
          </select>
          <span className="search-result-count" role="status">
            {deferredQuery ? (globalMatches.length ? `${activeMatchIndex === null ? "—" : activeMatchIndex + 1} / ${globalMatches.length}${wrapped ? " · 已循环" : ""}` : "无结果") : "输入词语开始查找"}
          </span>
          <button className={caseSensitive ? "active icon-button" : "icon-button"} aria-label="区分大小写" aria-pressed={caseSensitive} title="区分大小写" onClick={() => setCaseSensitive((value) => !value)}><CaseSensitive size={20} /></button>
          <button className="icon-button" aria-label="向下搜索" title="向下搜索" disabled={!globalMatches.length} onClick={() => navigateSearch(1)}><ChevronDown size={21} /></button>
          <button className="icon-button" aria-label="向上搜索" title="向上搜索" disabled={!globalMatches.length} onClick={() => navigateSearch(-1)}><ChevronUp size={21} /></button>
          <button className="icon-button" aria-label="关闭查找" title="关闭查找" onClick={closeSearch}><X size={20} /></button>
        </div>
      )}
      {diffEnabled && (diff.loading || diff.mode === "line" || diff.mode === "plain" || diff.error) && (
        <div className={diff.error ? "compare-diff-status error" : "compare-diff-status"} role="status">
          {diff.loading
            ? "正在计算差异…"
            : diff.error
              ? `${diff.error}，已显示普通文本。`
              : diff.mode === "plain"
                ? "文本差异过大，已关闭本章差异高亮。"
                : "长文本或高差异内容已使用行级差异高亮。"}
        </div>
      )}
      {selectedChapter ? (
        <div className="large-compare-grid">
          <TextPane
            heading="原文"
            side="original"
            text={originalText}
            containerRef={originalRef}
            diffRanges={visibleDiffRanges}
            searchMatches={originalMatches}
            activeMatchId={activeMatch?.id}
          />
          <TextPane
            heading="改写稿"
            side="rewrite"
            text={rewriteText}
            emptyText="尚未改写。"
            containerRef={rewriteRef}
            diffRanges={visibleDiffRanges}
            searchMatches={rewriteMatches}
            activeMatchId={activeMatch?.id}
            headerActions={(
              <div className="compare-pane-actions">
                {selectedChapter.rewrite_edited && (
                  <button
                    type="button"
                    onClick={() => {
                      if (!window.confirm("恢复到最近一次 AI 生成的改写稿？当前人工修改将被覆盖。")) return;
                      setEditBusy(true);
                      void onRestoreRewrite(selectedChapter.id).finally(() => {
                        setEditBusy(false);
                        setEditing(false);
                      });
                    }}
                    disabled={!editingAllowed || editBusy}
                    title={editingAllowed ? "恢复最近 AI 稿" : editDisabledReason}
                  >
                    <RotateCcw size={15} />恢复 AI 稿
                  </button>
                )}
                {!editing ? (
                  <button
                    type="button"
                    onClick={() => {
                      setEditDraft(rewriteText);
                      setEditing(true);
                    }}
                    disabled={!editingAllowed || !rewriteText.trim()}
                    title={editingAllowed ? "编辑当前章节改写正文" : editDisabledReason}
                  >
                    <Pencil size={15} />编辑
                  </button>
                ) : (
                  <>
                    <button type="button" onClick={() => void saveEdit()} disabled={editBusy || !editDraft.trim() || !editDirty}>
                      <Save size={15} />保存
                    </button>
                    <button
                      type="button"
                      onClick={() => runOrConfirmNavigation(() => setEditing(false))}
                      disabled={editBusy}
                    >
                      <X size={15} />关闭编辑
                    </button>
                  </>
                )}
              </div>
            )}
            editor={editing ? (
              <textarea
                className="compare-edit-textarea"
                aria-label="编辑改写稿正文"
                value={editDraft}
                onChange={(event) => setEditDraft(event.target.value)}
                disabled={editBusy}
              />
            ) : undefined}
          />
        </div>
      ) : <p className="muted">请选择章节。</p>}
      {pendingNavigation && (
        <Modal className="settings-dialog compare-unsaved-dialog" labelledBy="compare-unsaved-title">
          <header className="dialog-titlebar">
            <h2 id="compare-unsaved-title">改写稿尚未保存</h2>
          </header>
          <div className="dialog-body">
            <p>当前章节有未保存的人工修改。保存后继续，还是放弃修改？</p>
          </div>
          <footer className="dialog-actions">
            <button type="button" onClick={() => { pendingNavigationRef.current = null; setPendingNavigation(false); }} disabled={editBusy}>取消</button>
            <button type="button" onClick={finishPendingNavigation} disabled={editBusy}>放弃修改</button>
            <button
              className="dialog-primary"
              type="button"
              disabled={editBusy || !editDraft.trim()}
              onClick={() => void saveEdit(false).then((saved) => { if (saved) finishPendingNavigation(); })}
            >
              保存并继续
            </button>
          </footer>
        </Modal>
      )}
      {rewriteDialogOpen && selectedChapter && (
        <Modal className="settings-dialog rewrite-chapter-dialog" labelledBy="rewrite-chapter-title">
          <header className="dialog-titlebar">
            <h2 id="rewrite-chapter-title">
              {rewriteSourceMode === "original" ? "根据原文重新改写" : "基于改写稿继续修改"}《{selectedChapter.title}》
            </h2>
            <button className="icon-button" type="button" aria-label="关闭单章重写" onClick={() => setRewriteDialogOpen(false)} disabled={rewriteBusy}><X size={17} /></button>
          </header>
          <div className="dialog-body">
            <p>
              {rewriteSourceMode === "original"
                ? "以本章原文为基础重新生成，并复用现有分析、一致性资产、姓名映射和复检设置。"
                : "以当前改写稿为主要底稿，原文、设定、相关一致性资产和相邻章节仅用于辅助理解；本模式不调用审查模型。"}
              完成后会覆盖当前 AI 改写稿，但仍可恢复第一次单章重写前的初稿。
            </p>
            <label className="field">
              <span>本章补充要求（可选）</span>
              <textarea
                aria-label="单章重写补充要求"
                rows={7}
                value={rewriteInstructions}
                onChange={(event) => setRewriteInstructions(event.target.value)}
                placeholder="例如：加强两位女主在本章的情绪互动，但保持事件顺序和伏笔不变。"
                disabled={rewriteBusy}
                autoFocus
              />
            </label>
          </div>
          <footer className="dialog-actions">
            <button type="button" onClick={() => setRewriteDialogOpen(false)} disabled={rewriteBusy}>取消</button>
            <button className="dialog-primary" type="button" onClick={() => void confirmRewriteChapter()} disabled={rewriteBusy}>
              {rewriteBusy ? "正在改写…" : "确定改写"}
            </button>
          </footer>
        </Modal>
      )}
    </div>
  );
});
