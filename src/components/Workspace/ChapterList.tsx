import type { Chapter } from "../../types";
import { ScrollablePanel } from "../common/ScrollablePanel";

type ChapterListProps = {
  chapters: Chapter[];
  selectedChapterId?: string;
  onSelect: (chapterId: string) => void;
  displayTitle: (chapter: Chapter) => string;
  statusText: Record<string, string>;
};

export function ChapterList({ chapters, selectedChapterId, onSelect, displayTitle, statusText }: ChapterListProps) {
  return (
    <section className="panel chapter-list-panel">
      <div className="panel-heading"><h2>章节</h2></div>
      <ScrollablePanel className="chapter-list">
        {chapters.map((chapter) => (
          <button
            key={chapter.id}
            className={selectedChapterId === chapter.id ? "chapter-item active" : "chapter-item"}
            onClick={() => onSelect(chapter.id)}
          >
            <span className="chapter-title">{chapter.index}. {displayTitle(chapter)}</span>
            <small>分析 {statusText[chapter.analysis_status] ?? chapter.analysis_status} · 改写 {statusText[chapter.rewrite_status] ?? chapter.rewrite_status}</small>
          </button>
        ))}
      </ScrollablePanel>
    </section>
  );
}
