import { memo, useEffect, useMemo, useRef } from "react";
import { List, useListRef, type RowComponentProps } from "react-window";
import type { Chapter } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";

export const CHAPTER_VIRTUALIZATION_THRESHOLD = 300;

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

export const ChapterList = memo(function ChapterList({ chapters, selectedChapterId, onSelect, displayTitle, statusText }: ChapterListProps) {
  const listRef = useListRef(null);
  const selectedButtonRef = useRef<HTMLButtonElement | null>(null);
  const virtualized = chapters.length >= CHAPTER_VIRTUALIZATION_THRESHOLD;
  const selectedIndex = useMemo(() => chapters.findIndex((chapter) => chapter.id === selectedChapterId), [chapters, selectedChapterId]);
  const rowProps = useMemo(() => ({ chapters, selectedChapterId, onSelect, displayTitle, statusText }), [chapters, selectedChapterId, onSelect, displayTitle, statusText]);

  useEffect(() => {
    if (selectedIndex < 0) return;
    if (virtualized) listRef.current?.scrollToRow({ index: selectedIndex, align: "smart" });
    else selectedButtonRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [listRef, selectedIndex, virtualized]);

  return (
    <section className="panel chapter-list-panel">
      <div className="panel-heading"><h2>章节</h2></div>
      {virtualized ? (
        <List
          className="chapter-list virtual-chapter-list"
          listRef={listRef}
          rowComponent={ChapterRow}
          rowCount={chapters.length}
          rowHeight={68}
          rowProps={rowProps}
          overscanCount={4}
          defaultHeight={408}
          style={{ height: "100%" }}
        />
      ) : (
        <ScrollablePanel className="chapter-list">
          {chapters.map((chapter) => (
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
