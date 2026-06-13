import { memo, type RefObject } from "react";
import type { DiffRange, DiffSide } from "./compareDiff";
import type { SearchMatch } from "./compareSearch";

type HighlightedTextProps = {
  text: string;
  side: DiffSide;
  diffRanges: DiffRange[];
  searchMatches: SearchMatch[];
  activeMatchId?: string;
  activeMatchRef: RefObject<HTMLElement>;
};

type Segment = {
  start: number;
  end: number;
  diffKind?: "removed" | "added";
  searchMatch?: SearchMatch;
};

export function buildTextSegments(text: string, side: DiffSide, diffRanges: DiffRange[], searchMatches: SearchMatch[]): Segment[] {
  const sideDiffs = diffRanges.filter((range) => range.side === side);
  const boundaries = new Set([0, text.length]);
  for (const range of sideDiffs) {
    boundaries.add(range.start);
    boundaries.add(range.end);
  }
  for (const match of searchMatches) {
    boundaries.add(match.start);
    boundaries.add(match.end);
  }
  const sorted = Array.from(boundaries).sort((left, right) => left - right);
  const segments: Segment[] = [];
  for (let index = 0; index < sorted.length - 1; index += 1) {
    const start = sorted[index];
    const end = sorted[index + 1];
    if (start === end) continue;
    segments.push({
      start,
      end,
      diffKind: sideDiffs.find((range) => range.start <= start && range.end >= end)?.kind,
      searchMatch: searchMatches.find((match) => match.start <= start && match.end >= end)
    });
  }
  return segments;
}

export const HighlightedText = memo(function HighlightedText(props: HighlightedTextProps) {
  const { text, side, diffRanges, searchMatches, activeMatchId, activeMatchRef } = props;
  const segments = buildTextSegments(text, side, diffRanges, searchMatches);
  return (
    <>
      {segments.map((segment) => {
        const content = text.slice(segment.start, segment.end);
        const active = segment.searchMatch?.id === activeMatchId;
        const className = [
          segment.diffKind ? `diff-${segment.diffKind}` : "",
          segment.searchMatch ? "search-match" : "",
          active ? "active-search-match" : ""
        ].filter(Boolean).join(" ");
        const key = `${segment.start}:${segment.end}`;
        return segment.searchMatch ? (
          <mark key={key} ref={active ? activeMatchRef : undefined} className={className} data-match-id={segment.searchMatch.id}>{content}</mark>
        ) : className ? <span key={key} className={className}>{content}</span> : content;
      })}
    </>
  );
});
