import { ArrowLeft, Download } from "lucide-react";
import { memo, type RefObject } from "react";
import type { Chapter } from "../../types";

type CompareViewProps = {
  chapters: Chapter[];
  selectedChapter?: Chapter;
  selectedChapterId: string;
  busy: string;
  originalRef: RefObject<HTMLPreElement>;
  rewriteRef: RefObject<HTMLPreElement>;
  onSelectChapter: (chapterId: string) => void;
  onBack: () => void;
  onExport: () => void;
};

export const CompareView = memo(function CompareView(props: CompareViewProps) {
  const { chapters, selectedChapter, selectedChapterId, busy, originalRef, rewriteRef, onSelectChapter, onBack, onExport } = props;
  return (
    <div className="compare-page">
      <div className="compare-page-toolbar">
        <label>
          章节
          <select value={selectedChapterId} onChange={(event) => onSelectChapter(event.target.value)}>
            {chapters.map((chapter) => <option key={chapter.id} value={chapter.id}>{chapter.index}. {chapter.title}</option>)}
          </select>
        </label>
        <div className="compare-toolbar-actions">
          <button onClick={onBack}><ArrowLeft size={17} />返回</button>
          <button onClick={onExport} disabled={busy !== ""}><Download size={17} />TXT</button>
        </div>
      </div>
      {selectedChapter ? (
        <div className="large-compare-grid">
          <article><h2>原文</h2><pre ref={originalRef}>{selectedChapter.original_text}</pre></article>
          <article><h2>改写稿</h2><pre ref={rewriteRef}>{selectedChapter.rewrite_text || "尚未改写。"}</pre></article>
        </div>
      ) : <p className="muted">请选择章节。</p>}
    </div>
  );
});
