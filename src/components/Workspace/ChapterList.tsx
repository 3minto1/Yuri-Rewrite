import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { List, useListRef, type RowComponentProps } from "react-window";
import type { Chapter } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";
import { StatusBadge } from "../common/StatusBadge";

export const CHAPTER_VIRTUALIZATION_THRESHOLD = 300;
const CHAPTER_ROW_HEIGHT = 76;

type ChapterListProps = {
  chapters: Chapter[];
  selectedChapterId?: string;
  onSelect: (chapterId: string) => void;
  displayTitle: (chapter: Chapter) => string;
  statusText: Record<string, string>;
  onRenameChapter?: (chapterId: string, title: string) => Promise<void>;
  titleEditDisabledReason?: string;
};

type ChapterRowProps = Pick<ChapterListProps, "chapters" | "selectedChapterId" | "onSelect" | "displayTitle" | "statusText"> & {
  editing: boolean;
  titleDrafts: Record<string, string>;
  onTitleDraftChange: (chapterId: string, title: string) => void;
};

type ChapterButtonProps = Omit<ChapterRowProps, "chapters"> & {
  chapter: Chapter;
  buttonRef?: (node: HTMLButtonElement | null) => void;
};

const ChapterButton = memo(function ChapterButton({
  chapter,
  selectedChapterId,
  onSelect,
  displayTitle,
  statusText,
  buttonRef,
  editing,
  titleDrafts,
  onTitleDraftChange
}: ChapterButtonProps) {
  const title = `${chapter.index}. ${displayTitle(chapter)}`;
  if (editing) {
    return (
      <div className={selectedChapterId === chapter.id ? "chapter-item chapter-item-editing active" : "chapter-item chapter-item-editing"}>
        <div className="chapter-title-edit-label">
          <span className="sr-only">第 {chapter.index} 章名称</span>
          <span className="chapter-title-index" aria-hidden="true">{chapter.index}.</span>
          <input
            className="chapter-title-edit-input"
            value={titleDrafts[chapter.id] ?? displayTitle(chapter)}
            onChange={(event) => onTitleDraftChange(chapter.id, event.target.value)}
            onFocus={() => onSelect(chapter.id)}
            aria-label={`第 ${chapter.index} 章名称`}
          />
        </div>
      </div>
    );
  }
  return (
    <button
      ref={buttonRef}
      className={selectedChapterId === chapter.id ? "chapter-item active" : "chapter-item"}
      onClick={() => onSelect(chapter.id)}
      title={title}
    >
      <span className="chapter-title">{title}</span>
      <span className="chapter-status-row">
        <StatusBadge
          status={chapter.analysis_status}
          label={`分析 ${statusText[chapter.analysis_status] ?? chapter.analysis_status}`}
        />
        <StatusBadge
          status={chapter.rewrite_status}
          label={`改写 ${statusText[chapter.rewrite_status] ?? chapter.rewrite_status}`}
        />
      </span>
    </button>
  );
});

function ChapterRow({ index, style, ariaAttributes, ...props }: RowComponentProps<ChapterRowProps>) {
  return (
    <div {...ariaAttributes} style={style}>
      <ChapterButton chapter={props.chapters[index]} {...props} />
    </div>
  );
}

function normalizeQuery(value: string) {
  return value.trim().toLowerCase();
}

function isIntegerQuery(value: string) {
  return /^\d+$/.test(value);
}

function buildTitleDrafts(chapters: Chapter[], displayTitle: (chapter: Chapter) => string) {
  return Object.fromEntries(chapters.map((chapter) => [chapter.id, displayTitle(chapter)]));
}

export const ChapterList = memo(function ChapterList({
  chapters,
  selectedChapterId,
  onSelect,
  displayTitle,
  statusText,
  onRenameChapter,
  titleEditDisabledReason
}: ChapterListProps) {
  const listRef = useListRef(null);
  const selectedButtonRef = useRef<HTMLButtonElement | null>(null);
  const [jumpQuery, setJumpQuery] = useState("");
  const [editingTitles, setEditingTitles] = useState(false);
  const [savingTitles, setSavingTitles] = useState(false);
  const [titleDrafts, setTitleDrafts] = useState<Record<string, string>>({});
  const [titleEditError, setTitleEditError] = useState("");
  const normalizedJumpQuery = normalizeQuery(jumpQuery);
  const visibleChapters = useMemo(() => {
    const query = normalizedJumpQuery;
    if (!query) return chapters;
    const numericQuery = isIntegerQuery(query) ? Number.parseInt(query, 10) : NaN;
    const exactChapter = Number.isFinite(numericQuery)
      ? chapters.find((chapter) => chapter.index === numericQuery)
      : undefined;
    if (exactChapter) return [exactChapter];
    return chapters.filter((chapter) => displayTitle(chapter).toLowerCase().includes(query));
  }, [chapters, displayTitle, normalizedJumpQuery]);
  const virtualized = visibleChapters.length >= CHAPTER_VIRTUALIZATION_THRESHOLD;
  const rowHeight = CHAPTER_ROW_HEIGHT;
  const selectedIndex = useMemo(() => visibleChapters.findIndex((chapter) => chapter.id === selectedChapterId), [visibleChapters, selectedChapterId]);
  const rowProps = useMemo(() => ({
    chapters: visibleChapters,
    selectedChapterId,
    onSelect,
    displayTitle,
    statusText,
    editing: editingTitles,
    titleDrafts,
    onTitleDraftChange
  }), [visibleChapters, selectedChapterId, onSelect, displayTitle, statusText, editingTitles, titleDrafts]);
  const firstMatch = visibleChapters[0] ?? null;
  const titleEditingDisabled = Boolean(titleEditDisabledReason || !onRenameChapter || savingTitles);

  function virtualListElement() {
    return listRef.current?.element ?? null;
  }

  function scrollVirtualListToIndex(index: number, align: "center" | "smart" = "smart") {
    if (index < 0) return;
    listRef.current?.scrollToRow({ index, align, behavior: "auto" });
    const element = virtualListElement();
    if (element) {
      const viewportHeight = element.clientHeight || 408;
      const offset =
        align === "center"
          ? Math.max(0, index * rowHeight - Math.max(0, (viewportHeight - rowHeight) / 2))
          : index * rowHeight;
      if (typeof element.scrollTo === "function") {
        element.scrollTo({ top: offset, behavior: "auto" });
      } else {
        element.scrollTop = offset;
      }
      element.dispatchEvent(new Event("scroll", { bubbles: true }));
    }
  }

  function selectFirstMatch() {
    if (!firstMatch) return;
    onSelect(firstMatch.id);
  }

  function onTitleDraftChange(chapterId: string, title: string) {
    setTitleEditError("");
    setTitleDrafts((drafts) => ({ ...drafts, [chapterId]: title }));
  }

  function startTitleEditing() {
    if (titleEditingDisabled) return;
    setTitleEditError("");
    setTitleDrafts(buildTitleDrafts(chapters, displayTitle));
    setEditingTitles(true);
  }

  function cancelTitleEditing() {
    if (savingTitles) return;
    setTitleEditError("");
    setTitleDrafts({});
    setEditingTitles(false);
  }

  async function saveTitleEdits() {
    if (!onRenameChapter || savingTitles) return;
    const emptyChapter = chapters.find((chapter) => (titleDrafts[chapter.id] ?? displayTitle(chapter)).trim().length === 0);
    if (emptyChapter) {
      setTitleEditError(`第 ${emptyChapter.index} 章名称不能为空。`);
      return;
    }
    const changedChapters = chapters
      .map((chapter) => ({ chapter, title: (titleDrafts[chapter.id] ?? displayTitle(chapter)).trim() }))
      .filter(({ chapter, title }) => title !== displayTitle(chapter).trim());
    if (changedChapters.length === 0) {
      cancelTitleEditing();
      return;
    }
    setSavingTitles(true);
    setTitleEditError("");
    try {
      for (const { chapter, title } of changedChapters) {
        await onRenameChapter(chapter.id, title);
      }
      setTitleDrafts({});
      setEditingTitles(false);
    } catch (error) {
      setTitleEditError(String(error));
    } finally {
      setSavingTitles(false);
    }
  }

  useLayoutEffect(() => {
    if (!virtualized || selectedIndex < 0) return;
    scrollVirtualListToIndex(selectedIndex, "center");
    const frame = window.requestAnimationFrame(() => {
      scrollVirtualListToIndex(selectedIndex, "center");
    });
    return () => window.cancelAnimationFrame(frame);
  }, [rowHeight, selectedIndex, virtualized]);

  useEffect(() => {
    if (selectedIndex < 0) return;
    if (virtualized) scrollVirtualListToIndex(selectedIndex, "smart");
    else selectedButtonRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [listRef, rowHeight, selectedIndex, virtualized]);

  return (
    <section className="panel chapter-list-panel">
      <div className="panel-heading chapter-list-heading">
        <h2>章节</h2>
        {editingTitles ? (
          <div className="chapter-title-edit-actions">
            <button type="button" className="secondary-button compact-button" onClick={cancelTitleEditing} disabled={savingTitles}>
              取消
            </button>
            <button type="button" className="action-primary compact-button" onClick={saveTitleEdits} disabled={savingTitles}>
              {savingTitles ? "保存中…" : "保存"}
            </button>
          </div>
        ) : (
          <button
            type="button"
            className="secondary-button compact-button"
            onClick={startTitleEditing}
            disabled={titleEditingDisabled}
            title={titleEditDisabledReason}
          >
            编辑
          </button>
        )}
        <input
          aria-label="搜索章节"
          className="chapter-jump-input"
          placeholder="搜索章号/标题"
          value={jumpQuery}
          onChange={(event) => setJumpQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") selectFirstMatch();
          }}
        />
      </div>
      {titleEditError && <p className="chapter-title-edit-error">{titleEditError}</p>}
      {virtualized ? (
        <List
          className="chapter-list virtual-chapter-list"
          listRef={listRef}
          rowComponent={ChapterRow}
          rowCount={visibleChapters.length}
          rowHeight={rowHeight}
          rowProps={rowProps}
          overscanCount={4}
          defaultHeight={408}
          style={{ height: "100%" }}
        />
      ) : (
        <ScrollablePanel className="chapter-list">
          {visibleChapters.map((chapter) => (
            <ChapterButton
              key={chapter.id}
              chapter={chapter}
              selectedChapterId={selectedChapterId}
              buttonRef={selectedChapterId === chapter.id ? (node) => { selectedButtonRef.current = node; } : undefined}
              onSelect={onSelect}
              displayTitle={displayTitle}
              statusText={statusText}
              editing={editingTitles}
              titleDrafts={titleDrafts}
              onTitleDraftChange={onTitleDraftChange}
            />
          ))}
        </ScrollablePanel>
      )}
    </section>
  );
});
