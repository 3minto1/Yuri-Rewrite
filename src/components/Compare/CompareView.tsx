import { ArrowLeft, CaseSensitive, ChevronDown, ChevronUp, Download, GitCompareArrows, Search, X } from "lucide-react";
import { memo, useDeferredValue, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import type { Chapter } from "../../types";
import { calculateDiff, type DiffRange, type DiffResult, type DiffSide } from "./compareDiff";
import { HighlightedText } from "./HighlightedText";
import { buildSearchMatches, initialSearchIndex, moveSearchIndex, type SearchMatch } from "./compareSearch";

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
};

type DiffState = DiffResult & { loading: boolean; error?: string };

const EMPTY_DIFF: DiffState = { ranges: [], mode: "mixed", loading: false };

function useChapterDiff(original: string, rewrite: string, enabled: boolean): DiffState {
  const requestIdRef = useRef(0);
  const [state, setState] = useState<DiffState>(EMPTY_DIFF);

  useEffect(() => {
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    if (!enabled || !rewrite.trim()) {
      setState(EMPTY_DIFF);
      return undefined;
    }

    let cancelled = false;
    setState((current) => ({ ...current, loading: true, error: undefined }));
    if (typeof Worker === "undefined") {
      Promise.resolve().then(() => calculateDiff(original, rewrite)).then((result) => {
        if (!cancelled && requestIdRef.current === requestId) setState({ ...result, loading: false });
      }).catch((error) => {
        if (!cancelled && requestIdRef.current === requestId) setState({ ranges: [], mode: "mixed", loading: false, error: String(error) });
      });
      return () => { cancelled = true; };
    }

    const worker = new Worker(new URL("./compareDiff.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<{ requestId: number; result?: DiffResult; error?: string }>) => {
      if (cancelled || event.data.requestId !== requestId || requestIdRef.current !== requestId) return;
      if (event.data.result) setState({ ...event.data.result, loading: false });
      else setState({ ranges: [], mode: "mixed", loading: false, error: event.data.error || "差异计算失败" });
    };
    worker.onerror = () => {
      if (!cancelled && requestIdRef.current === requestId) setState({ ranges: [], mode: "mixed", loading: false, error: "差异计算失败" });
    };
    worker.postMessage({ requestId, original, rewrite });
    return () => {
      cancelled = true;
      worker.terminate();
    };
  }, [enabled, original, rewrite]);

  return state;
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
  activeMatchRef: RefObject<HTMLElement>;
};

const TextPane = memo(function TextPane(props: TextPaneProps) {
  const { heading, side, text, emptyText, containerRef, diffRanges, searchMatches, activeMatchId, activeMatchRef } = props;
  return (
    <article>
      <h2>{heading}</h2>
      <div ref={containerRef} className="compare-text" aria-label={`${heading}内容`}>
        {text ? (
          <HighlightedText
            text={text}
            side={side}
            diffRanges={diffRanges}
            searchMatches={searchMatches}
            activeMatchId={activeMatchId}
            activeMatchRef={activeMatchRef}
          />
        ) : <span className="muted">{emptyText}</span>}
      </div>
    </article>
  );
});

export const CompareView = memo(function CompareView(props: CompareViewProps) {
  const { chapters, selectedChapter, selectedChapterId, busy, originalRef, rewriteRef, onSelectChapter, onBack, onExport } = props;
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [activeMatchIndex, setActiveMatchIndex] = useState<number | null>(null);
  const [wrapped, setWrapped] = useState(false);
  const [diffEnabled, setDiffEnabled] = useState(true);
  const deferredQuery = useDeferredValue(query);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const activeMatchRef = useRef<HTMLElement | null>(null);
  const navigationTargetRef = useRef<string | null>(null);
  const previousChapterRef = useRef(selectedChapterId);
  const globalMatches = useMemo(
    () => buildSearchMatches(chapters, deferredQuery, caseSensitive),
    [caseSensitive, chapters, deferredQuery]
  );
  const activeMatch = activeMatchIndex === null ? undefined : globalMatches[activeMatchIndex];
  const originalText = selectedChapter?.original_text ?? "";
  const rewriteText = selectedChapter?.rewrite_text ?? "";
  const diff = useChapterDiff(originalText, rewriteText, diffEnabled);
  const visibleDiffRanges = diffEnabled ? diff.ranges : [];
  const chapterMatches = useMemo(() => globalMatches.filter((match) => match.chapter_id === selectedChapterId), [globalMatches, selectedChapterId]);
  const originalMatches = useMemo(() => chapterMatches.filter((match) => match.side === "original"), [chapterMatches]);
  const rewriteMatches = useMemo(() => chapterMatches.filter((match) => match.side === "rewrite"), [chapterMatches]);

  function closeSearch() {
    setSearchOpen(false);
    setQuery("");
    setActiveMatchIndex(null);
    setWrapped(false);
  }

  function selectSearchMatch(index: number | null, didWrap = false) {
    setActiveMatchIndex(index);
    setWrapped(didWrap);
    if (index === null) return;
    const match = globalMatches[index];
    if (match.chapter_id !== selectedChapterId) {
      navigationTargetRef.current = match.chapter_id;
      onSelectChapter(match.chapter_id);
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
    onSelectChapter(chapterId);
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
  }, [caseSensitive, deferredQuery]);

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
    if (!activeMatch || activeMatch.chapter_id !== selectedChapterId) return;
    const frame = window.requestAnimationFrame(() => activeMatchRef.current?.scrollIntoView({ block: "center", inline: "nearest" }));
    return () => window.cancelAnimationFrame(frame);
  }, [activeMatch, selectedChapterId]);

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
          <button className={searchOpen ? "active" : ""} aria-pressed={searchOpen} onClick={() => searchOpen ? closeSearch() : setSearchOpen(true)}><Search size={17} />查找</button>
          <button className={diffEnabled ? "active" : ""} aria-pressed={diffEnabled} onClick={() => setDiffEnabled((value) => !value)}><GitCompareArrows size={17} />差异</button>
          <button onClick={onBack}><ArrowLeft size={17} />返回</button>
          <button onClick={onExport} disabled={busy !== ""}><Download size={17} />TXT</button>
        </div>
      </div>
      {searchOpen && (
        <div className="compare-search-bar" role="search">
          <Search size={18} aria-hidden="true" />
          <input
            ref={searchInputRef}
            aria-label="全局搜索"
            placeholder="同时搜索全部章节的原文和改写稿"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                navigateSearch(event.shiftKey ? -1 : 1);
              }
            }}
          />
          <span className="search-result-count" role="status">
            {deferredQuery ? (globalMatches.length ? `${activeMatchIndex === null ? "—" : activeMatchIndex + 1} / ${globalMatches.length}${wrapped ? " · 已循环" : ""}` : "无结果") : "输入词语开始查找"}
          </span>
          <button className={caseSensitive ? "active icon-button" : "icon-button"} aria-label="区分大小写" aria-pressed={caseSensitive} title="区分大小写" onClick={() => setCaseSensitive((value) => !value)}><CaseSensitive size={20} /></button>
          <button className="icon-button" aria-label="向下搜索" title="向下搜索" disabled={!globalMatches.length} onClick={() => navigateSearch(1)}><ChevronDown size={21} /></button>
          <button className="icon-button" aria-label="向上搜索" title="向上搜索" disabled={!globalMatches.length} onClick={() => navigateSearch(-1)}><ChevronUp size={21} /></button>
          <button className="icon-button" aria-label="关闭查找" title="关闭查找" onClick={closeSearch}><X size={20} /></button>
        </div>
      )}
      {diffEnabled && (diff.loading || diff.mode === "line" || diff.error) && (
        <div className={diff.error ? "compare-diff-status error" : "compare-diff-status"} role="status">
          {diff.loading ? "正在计算差异…" : diff.error ? `${diff.error}，已显示普通文本。` : "长文本已使用行级差异高亮。"}
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
            activeMatchRef={activeMatchRef}
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
            activeMatchRef={activeMatchRef}
          />
        </div>
      ) : <p className="muted">请选择章节。</p>}
    </div>
  );
});
