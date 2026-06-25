import { ArrowLeft, CaseSensitive, ChevronDown, ChevronUp, Download, GitCompareArrows, Pencil, RefreshCw, RotateCcw, Save, Search, ShieldCheck, Square, X } from "lucide-react";
import { memo, useDeferredValue, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import type { Chapter, NovelSettings } from "../../types";
import { Modal } from "../common/Modal";
import { StatusBadge } from "../common/StatusBadge";
import { calculateDiff, type DiffRange, type DiffResult, type DiffSide } from "./compareDiff";
import { getCachedDiff, setCachedDiff } from "./compareDiffCache";
import { HighlightedText } from "./HighlightedText";
import { scanRewriteQuality, type QualityIssue, type QualityIssueSeverity } from "./compareQuality";
import { buildSearchMatches, initialSearchIndex, moveSearchIndex, type SearchMatch, type SearchScope } from "./compareSearch";

type CompareViewProps = {
  chapters: Chapter[];
  selectedChapter?: Chapter;
  selectedChapterId: string;
  novelSettings?: NovelSettings | null;
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
  onTerminateRewrite?: () => Promise<void>;
  onRestoreInitialRewrite?: (chapterId: string) => Promise<void>;
  onDirtyChange?: (dirty: boolean) => void;
};

type QualityFilter = "all" | "error" | "warning";

const qualityIgnoreKeyPrefix = "yuri-rewrite.qualityIgnored.v1.";

type DiffState = DiffResult & {
  loading: boolean;
  chapterId: string;
  original: string;
  rewrite: string;
  cached: boolean;
  error?: string;
};

type ChapterSelectorProps = {
  chapters: Chapter[];
  selectedChapterId: string;
  onSelect: (chapterId: string) => void;
};

const ChapterSelector = memo(function ChapterSelector({
  chapters,
  selectedChapterId,
  onSelect
}: ChapterSelectorProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = Math.max(0, chapters.findIndex((chapter) => chapter.id === selectedChapterId));
  const selectedChapter = chapters[selectedIndex];

  function focusOption(index: number) {
    const nextIndex = Math.max(0, Math.min(index, chapters.length - 1));
    setActiveIndex(nextIndex);
    window.requestAnimationFrame(() => optionRefs.current[nextIndex]?.focus());
  }

  function openMenu(index = selectedIndex) {
    setOpen(true);
    setActiveIndex(index);
    window.requestAnimationFrame(() => {
      const option = optionRefs.current[index];
      option?.focus();
      option?.scrollIntoView({ block: "nearest" });
    });
  }

  function closeMenu() {
    setOpen(false);
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  }

  useEffect(() => {
    if (!open) return;
    function handleOutsidePointer(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    window.addEventListener("mousedown", handleOutsidePointer);
    return () => window.removeEventListener("mousedown", handleOutsidePointer);
  }, [open]);

  useEffect(() => {
    if (!open) setActiveIndex(selectedIndex);
  }, [open, selectedIndex]);

  return (
    <div className="compare-chapter-selector" ref={rootRef}>
      <button
        type="button"
        className="compare-chapter-trigger"
        ref={triggerRef}
        role="combobox"
        aria-label="章节"
        aria-expanded={open}
        aria-controls="compare-chapter-options"
        onClick={() => open ? closeMenu() : openMenu()}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            openMenu(event.key === "ArrowDown" ? selectedIndex : Math.max(0, selectedIndex - 1));
          }
        }}
      >
        <span>{selectedChapter ? `${selectedChapter.index}. ${selectedChapter.title}` : "请选择章节"}</span>
        <ChevronDown size={17} aria-hidden="true" />
      </button>
      {open && (
        <div
          id="compare-chapter-options"
          className="compare-chapter-options"
          role="listbox"
          aria-label="章节列表"
        >
          {chapters.map((chapter, index) => {
            const completed = chapter.rewrite_status === "completed"
              && Boolean(chapter.rewrite_text?.trim());
            return (
              <button
                type="button"
                key={chapter.id}
                ref={(node) => { optionRefs.current[index] = node; }}
                className={chapter.id === selectedChapterId ? "active" : ""}
                role="option"
                aria-selected={chapter.id === selectedChapterId}
                tabIndex={index === activeIndex ? 0 : -1}
                onClick={() => {
                  closeMenu();
                  onSelect(chapter.id);
                }}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    focusOption((index + 1) % chapters.length);
                  } else if (event.key === "ArrowUp") {
                    event.preventDefault();
                    focusOption((index - 1 + chapters.length) % chapters.length);
                  } else if (event.key === "Home" || event.key === "End") {
                    event.preventDefault();
                    focusOption(event.key === "Home" ? 0 : chapters.length - 1);
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    closeMenu();
                  }
                }}
              >
                <span className="compare-chapter-option-title">{chapter.index}. {chapter.title}</span>
                {completed && (
                  <StatusBadge
                    status="completed"
                    label="completed"
                    className="compare-chapter-completed"
                  />
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
});

const EMPTY_RANGES: DiffRange[] = [];

function countNonWhitespaceCharacters(text: string) {
  return text.replace(/\s/g, "").length;
}

function formatCharacterDelta(originalCount: number, rewriteCount: number) {
  if (rewriteCount === 0) return "未改写";
  const delta = rewriteCount - originalCount;
  if (delta === 0) return "持平";
  const prefix = delta > 0 ? "+" : "";
  if (originalCount === 0) return `${prefix}${delta}`;
  return `${prefix}${delta}（${prefix}${((delta / originalCount) * 100).toFixed(1)}%）`;
}

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

function qualitySeverityLabel(severity: QualityIssueSeverity) {
  if (severity === "error") return "严重";
  if (severity === "warning") return "警告";
  return "提示";
}

function qualityCategoryLabel(issue: QualityIssue) {
  switch (issue.category) {
    case "missing_rewrite":
      return "缺失";
    case "source_name":
      return "旧名";
    case "gender_residue":
      return "性别";
    case "group_pronoun":
      return "群体";
    case "ad_noise":
      return "广告";
    case "garbage":
      return "乱码";
    case "duplicate":
      return "重复";
    case "unchanged":
      return "未改";
    default:
      return "检查";
  }
}

function qualityIssueFingerprint(issue: QualityIssue) {
  return [
    issue.chapterId,
    issue.category,
    issue.severity,
    issue.message,
    issue.query,
    issue.evidence
  ].join("\u001f");
}

function loadIgnoredQualityFingerprints(novelId: string) {
  if (!novelId) return new Set<string>();
  try {
    const raw = window.localStorage.getItem(`${qualityIgnoreKeyPrefix}${novelId}`);
    const parsed = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === "string") : []);
  } catch {
    return new Set<string>();
  }
}

function storeIgnoredQualityFingerprints(novelId: string, fingerprints: Set<string>) {
  if (!novelId) return;
  try {
    window.localStorage.setItem(`${qualityIgnoreKeyPrefix}${novelId}`, JSON.stringify([...fingerprints]));
  } catch {
    // Ignore localStorage failures; the in-memory ignore state still works for this session.
  }
}

type QualityPanelProps = {
  open: boolean;
  filter: QualityFilter;
  issues: QualityIssue[];
  currentIssueCount: number;
  totalIssueCount: number;
  canIgnoreCurrent: boolean;
  onClose: () => void;
  onIgnoreCurrent: () => void;
  onFilterChange: (filter: QualityFilter) => void;
  onIssueClick: (issue: QualityIssue) => void;
};

const QualityPanel = memo(function QualityPanel(props: QualityPanelProps) {
  const {
    open,
    filter,
    issues,
    currentIssueCount,
    totalIssueCount,
    canIgnoreCurrent,
    onClose,
    onIgnoreCurrent,
    onFilterChange,
    onIssueClick
  } = props;
  if (!open) return null;
  return (
    <aside className="compare-quality-panel" aria-label="本地质量检查">
      <header className="compare-quality-header">
        <div>
          <h2>本地检查</h2>
          <p>当前章 {currentIssueCount} · 全书 {totalIssueCount}</p>
        </div>
        <div className="compare-quality-header-actions">
          <button type="button" className="compare-quality-ignore-button" onClick={onIgnoreCurrent} disabled={!canIgnoreCurrent}>
            忽略当前问题
          </button>
          <button type="button" className="icon-button" aria-label="关闭检查" onClick={onClose}><X size={18} /></button>
        </div>
      </header>
      <div className="compare-quality-filters" role="group" aria-label="检查筛选">
        <button type="button" className={filter === "all" ? "active" : ""} onClick={() => onFilterChange("all")}>全部</button>
        <button type="button" className={filter === "error" ? "active" : ""} onClick={() => onFilterChange("error")}>严重</button>
        <button type="button" className={filter === "warning" ? "active" : ""} onClick={() => onFilterChange("warning")}>警告</button>
      </div>
      {issues.length ? (
        <div className="compare-quality-list">
          {issues.map((issue) => (
            <div
              role="button"
              tabIndex={0}
              key={issue.id}
              className={`compare-quality-issue ${issue.severity}`}
              onClick={() => onIssueClick(issue)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onIssueClick(issue);
                }
              }}
            >
              <span className="compare-quality-meta">
                <span>第 {issue.chapterIndex} 章</span>
                <span>{qualityCategoryLabel(issue)}</span>
                <span>{qualitySeverityLabel(issue.severity)}</span>
              </span>
              <strong>{issue.message}</strong>
              {issue.evidence && <span className="compare-quality-evidence">{issue.evidence}</span>}
            </div>
          ))}
        </div>
      ) : (
        <p className="compare-quality-empty">
          {totalIssueCount === 0 ? "当前章未发现本地规则问题。" : "当前筛选下没有问题。"}
        </p>
      )}
    </aside>
  );
});

export const CompareView = memo(function CompareView(props: CompareViewProps) {
  const {
    chapters, selectedChapter, selectedChapterId, novelSettings, busy, originalRef, rewriteRef,
    onSelectChapter, onBack, onExport, editingAllowed = false, editDisabledReason,
    onSaveRewrite = async () => undefined, onRestoreRewrite = async () => undefined,
    onRewriteChapter = async () => undefined,
    onTerminateRewrite = async () => undefined,
    onRestoreInitialRewrite = async () => undefined,
    onDirtyChange
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
  const [terminateRewriteBusy, setTerminateRewriteBusy] = useState(false);
  const [qualityOpen, setQualityOpen] = useState(false);
  const [qualityFilter, setQualityFilter] = useState<QualityFilter>("all");
  const [ignoredQualityFingerprints, setIgnoredQualityFingerprints] = useState<Set<string>>(() => new Set());
  const pendingNavigationRef = useRef<(() => void) | null>(null);
  const pendingQualityIssueRef = useRef<QualityIssue | null>(null);
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
  const visibleRewriteText = editing ? editDraft : rewriteText;
  const originalCharacterCount = useMemo(() => countNonWhitespaceCharacters(originalText), [originalText]);
  const rewriteCharacterCount = useMemo(() => countNonWhitespaceCharacters(visibleRewriteText), [visibleRewriteText]);
  const characterDelta = useMemo(
    () => formatCharacterDelta(originalCharacterCount, rewriteCharacterCount),
    [originalCharacterCount, rewriteCharacterCount]
  );
  const diff = useChapterDiff(selectedChapterId, originalText, rewriteText, diffEnabled);
  const visibleDiffRanges = diffEnabled ? diff.ranges : EMPTY_RANGES;
  const chapterMatches = useMemo(() => globalMatches.filter((match) => match.chapter_id === selectedChapterId), [globalMatches, selectedChapterId]);
  const originalMatches = useMemo(() => chapterMatches.filter((match) => match.side === "original"), [chapterMatches]);
  const rewriteMatches = useMemo(() => chapterMatches.filter((match) => match.side === "rewrite"), [chapterMatches]);
  const editDirty = editing && editDraft !== rewriteText;
  const novelId = selectedChapter?.novel_id ?? chapters[0]?.novel_id ?? "";
  const qualityIssues = useMemo(() => scanRewriteQuality(
    chapters,
    novelSettings,
    editing ? { chapterId: selectedChapterId, rewriteText: editDraft } : null
  ), [chapters, editDraft, editing, novelSettings, selectedChapterId]);
  const activeQualityIssues = useMemo(
    () => qualityIssues.filter((issue) => !ignoredQualityFingerprints.has(qualityIssueFingerprint(issue))),
    [ignoredQualityFingerprints, qualityIssues]
  );
  const currentQualityIssues = useMemo(
    () => activeQualityIssues.filter((issue) => issue.chapterId === selectedChapterId),
    [activeQualityIssues, selectedChapterId]
  );
  const visibleQualityIssues = useMemo(() => {
    const filtered = qualityFilter === "all"
      ? activeQualityIssues
      : activeQualityIssues.filter((issue) => issue.severity === qualityFilter);
    return [...filtered].sort((left, right) => {
      if (left.chapterId === selectedChapterId && right.chapterId !== selectedChapterId) return -1;
      if (right.chapterId === selectedChapterId && left.chapterId !== selectedChapterId) return 1;
      return left.chapterIndex - right.chapterIndex;
    });
  }, [activeQualityIssues, qualityFilter, selectedChapterId]);

  useEffect(() => {
    setIgnoredQualityFingerprints(loadIgnoredQualityFingerprints(novelId));
  }, [novelId]);

  useEffect(() => {
    onDirtyChange?.(editDirty);
  }, [editDirty, onDirtyChange]);

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

  function activateIssueSearch(issue: QualityIssue) {
    if (!issue.query.trim()) return;
    setSearchOpen(true);
    setSearchScope("rewrite");
    setCaseSensitive(false);
    setQuery(issue.query);
    setWrapped(false);
    const nextMatches = buildSearchMatches(chapters, issue.query, false, "rewrite");
    const nextIndex = nextMatches.findIndex((match) => match.chapter_id === issue.chapterId);
    setActiveMatchIndex(nextIndex >= 0 ? nextIndex : null);
  }

  function focusQualityIssue(issue: QualityIssue) {
    if (issue.chapterId === selectedChapterId) {
      activateIssueSearch(issue);
      return;
    }
    runOrConfirmNavigation(() => {
      pendingQualityIssueRef.current = issue;
      navigationTargetRef.current = issue.chapterId;
      onSelectChapter(issue.chapterId);
    });
  }

  function ignoreCurrentQualityIssues() {
    if (!novelId || activeQualityIssues.length === 0) return;
    const next = new Set(ignoredQualityFingerprints);
    for (const issue of activeQualityIssues) {
      if (issue.severity === "error" || issue.severity === "warning") {
        next.add(qualityIssueFingerprint(issue));
      }
    }
    setIgnoredQualityFingerprints(next);
    storeIgnoredQualityFingerprints(novelId, next);
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

  async function terminateRewriteChapter() {
    setTerminateRewriteBusy(true);
    try {
      await onTerminateRewrite();
    } finally {
      setTerminateRewriteBusy(false);
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
    if (chapterId === selectedChapterId) return;
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

  useEffect(() => {
    const issue = pendingQualityIssueRef.current;
    if (!issue || issue.chapterId !== selectedChapterId) return;
    pendingQualityIssueRef.current = null;
    activateIssueSearch(issue);
  }, [selectedChapterId]);

  return (
    <div className="compare-page">
      <div className="compare-page-toolbar">
        <div className="compare-toolbar-row compare-toolbar-primary-row">
          <label>
            章节
            <ChapterSelector
              chapters={chapters}
              selectedChapterId={selectedChapterId}
              onSelect={handleManualChapterSelect}
            />
          </label>
          <div className="split-button compare-rewrite-split">
            <button
              className="split-button-main action-primary"
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
              className="split-button-toggle action-primary"
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
        </div>
        <div className="compare-toolbar-row compare-toolbar-secondary-row">
          <div className="compare-toolbar-actions compare-toolbar-left-actions">
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
          <button className={qualityOpen ? "active compare-quality-toggle" : "compare-quality-toggle"} aria-pressed={qualityOpen} onClick={() => setQualityOpen((open) => !open)}>
            <ShieldCheck size={17} />检查
            {activeQualityIssues.length > 0 && <span className="compare-quality-badge">{activeQualityIssues.length}</span>}
          </button>
          </div>
          <div className="compare-word-count" aria-label="字数对比">
            字数：原文 {originalCharacterCount} · 改写稿 {rewriteCharacterCount} · {characterDelta}
          </div>
          <div className="compare-toolbar-actions compare-toolbar-right-actions">
          <button onClick={() => runOrConfirmNavigation(onBack)}><ArrowLeft size={17} />返回</button>
          <button onClick={onExport} disabled={busy !== ""}><Download size={17} />TXT</button>
          </div>
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
      <QualityPanel
        open={qualityOpen}
        filter={qualityFilter}
        issues={visibleQualityIssues}
        currentIssueCount={currentQualityIssues.length}
        totalIssueCount={activeQualityIssues.length}
        canIgnoreCurrent={activeQualityIssues.some((issue) => issue.severity === "error" || issue.severity === "warning")}
        onClose={() => setQualityOpen(false)}
        onIgnoreCurrent={ignoreCurrentQualityIssues}
        onFilterChange={setQualityFilter}
        onIssueClick={focusQualityIssue}
      />
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
            {rewriteBusy && (
              <button
                className="dialog-danger"
                type="button"
                onClick={() => void terminateRewriteChapter()}
                disabled={terminateRewriteBusy}
              >
                <Square size={16} />{terminateRewriteBusy ? "终止中…" : "终止"}
              </button>
            )}
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
