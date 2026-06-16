import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { List, useListRef, type RowComponentProps } from "react-window";
import type { Chapter } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";

export const CHAPTER_VIRTUALIZATION_THRESHOLD = 300;
const CHAPTER_ROW_HEIGHT = 68;

type ChapterListProps = {
  chapters: Chapter[];
  selectedChapterId?: string;
  onSelect: (chapterId: string) => void;
  displayTitle: (chapter: Chapter) => string;
  statusText: Record<string, string>;
};

type ChapterRowProps = Pick<ChapterListProps, "chapters" | "selectedChapterId" | "onSelect" | "displayTitle" | "statusText">;

type ChapterButtonProps = Omit<ChapterRowProps, "chapters"> & { chapter: Chapter };

const ChapterButton = memo(function ChapterButton({ chapter, selectedChapterId, onSelect, displayTitle, statusText }: ChapterButtonProps) {
  return (
    <button
      className={selectedChapterId === chapter.id ? "chapter-item active" : "chapter-item"}
      onClick={() => onSelect(chapter.id)}
    >
      <span className="chapter-title">{chapter.index}. {displayTitle(chapter)}</span>
      <small>分析 {statusText[chapter.analysis_status] ?? chapter.analysis_status} · 改写 {statusText[chapter.rewrite_status] ?? chapter.rewrite_status}</small>
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

export const ChapterList = memo(function ChapterList({ chapters, selectedChapterId, onSelect, displayTitle, statusText }: ChapterListProps) {
  const listRef = useListRef(null);
  const selectedButtonRef = useRef<HTMLButtonElement | null>(null);
  const [jumpQuery, setJumpQuery] = useState("");
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
  const selectedIndex = useMemo(() => visibleChapters.findIndex((chapter) => chapter.id === selectedChapterId), [visibleChapters, selectedChapterId]);
  const rowProps = useMemo(() => ({ chapters: visibleChapters, selectedChapterId, onSelect, displayTitle, statusText }), [visibleChapters, selectedChapterId, onSelect, displayTitle, statusText]);
  const firstMatch = visibleChapters[0] ?? null;

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
          ? Math.max(0, index * CHAPTER_ROW_HEIGHT - Math.max(0, (viewportHeight - CHAPTER_ROW_HEIGHT) / 2))
          : index * CHAPTER_ROW_HEIGHT;
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

  useLayoutEffect(() => {
    if (!virtualized || selectedIndex < 0) return;
    scrollVirtualListToIndex(selectedIndex, "center");
    const frame = window.requestAnimationFrame(() => {
      scrollVirtualListToIndex(selectedIndex, "center");
    });
    return () => window.cancelAnimationFrame(frame);
  }, [selectedIndex, virtualized]);

  useEffect(() => {
    if (selectedIndex < 0) return;
    if (virtualized) scrollVirtualListToIndex(selectedIndex, "smart");
    else selectedButtonRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [listRef, selectedIndex, virtualized]);

  return (
    <section className="panel chapter-list-panel">
      <div className="panel-heading chapter-list-heading">
        <h2>章节</h2>
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
      {virtualized ? (
        <List
          className="chapter-list virtual-chapter-list"
          listRef={listRef}
          rowComponent={ChapterRow}
          rowCount={visibleChapters.length}
          rowHeight={CHAPTER_ROW_HEIGHT}
          rowProps={rowProps}
          overscanCount={4}
          defaultHeight={408}
          style={{ height: "100%" }}
        />
      ) : (
        <ScrollablePanel className="chapter-list">
          {visibleChapters.map((chapter) => (
            <button
              key={chapter.id}
              ref={selectedChapterId === chapter.id ? selectedButtonRef : undefined}
              className={selectedChapterId === chapter.id ? "chapter-item active" : "chapter-item"}
              onClick={() => onSelect(chapter.id)}
            >
              <span className="chapter-title">{chapter.index}. {displayTitle(chapter)}</span>
              <small>分析 {statusText[chapter.analysis_status] ?? chapter.analysis_status} · 改写 {statusText[chapter.rewrite_status] ?? chapter.rewrite_status}</small>
            </button>
          ))}
        </ScrollablePanel>
      )}
    </section>
  );
});
