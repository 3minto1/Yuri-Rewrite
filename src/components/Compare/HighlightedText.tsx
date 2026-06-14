import { memo, useEffect, useMemo, useRef, type RefObject } from "react";
import type { DiffRange, DiffSide } from "./compareDiff";
import type { SearchMatch } from "./compareSearch";

type HighlightedTextProps = {
  text: string;
  side: DiffSide;
  containerRef: RefObject<HTMLDivElement>;
  diffRanges: DiffRange[];
  searchMatches: SearchMatch[];
  activeMatchId?: string;
};

type Segment = {
  start: number;
  end: number;
  diffKind?: "removed" | "added";
  searchMatch?: SearchMatch;
};

const HIGHLIGHT_NAMES = {
  original: {
    diff: "compare-original-removed",
    search: "compare-original-search",
    active: "compare-original-active"
  },
  rewrite: {
    diff: "compare-rewrite-added",
    search: "compare-rewrite-search",
    active: "compare-rewrite-active"
  }
} as const;

export function supportsCssCustomHighlights() {
  return typeof CSS !== "undefined" && "highlights" in CSS && typeof Highlight !== "undefined";
}

export function buildTextSegments(text: string, side: DiffSide, diffRanges: DiffRange[], searchMatches: SearchMatch[]): Segment[] {
  const sideDiffs = diffRanges
    .filter((range) => range.side === side)
    .sort((left, right) => left.start - right.start);
  const sortedMatches = [...searchMatches].sort((left, right) => left.start - right.start);
  const boundaries = new Set([0, text.length]);
  for (const range of sideDiffs) {
    boundaries.add(range.start);
    boundaries.add(range.end);
  }
  for (const match of sortedMatches) {
    boundaries.add(match.start);
    boundaries.add(match.end);
  }

  const sortedBoundaries = Array.from(boundaries).sort((left, right) => left - right);
  const segments: Segment[] = [];
  let diffIndex = 0;
  let matchIndex = 0;
  for (let index = 0; index < sortedBoundaries.length - 1; index += 1) {
    const start = sortedBoundaries[index];
    const end = sortedBoundaries[index + 1];
    if (start === end) continue;
    while (sideDiffs[diffIndex]?.end <= start) diffIndex += 1;
    while (sortedMatches[matchIndex]?.end <= start) matchIndex += 1;
    const diffRange = sideDiffs[diffIndex];
    const searchMatch = sortedMatches[matchIndex];
    segments.push({
      start,
      end,
      diffKind: diffRange?.start <= start && diffRange.end >= end ? diffRange.kind : undefined,
      searchMatch: searchMatch?.start <= start && searchMatch.end >= end ? searchMatch : undefined
    });
  }
  return segments;
}

function createRange(textNode: Text, start: number, end: number) {
  const range = document.createRange();
  range.setStart(textNode, Math.max(0, Math.min(start, textNode.length)));
  range.setEnd(textNode, Math.max(0, Math.min(end, textNode.length)));
  return range;
}

function setHighlight(name: string, ranges: Range[], priority: number) {
  if (ranges.length === 0) {
    CSS.highlights.delete(name);
    return;
  }
  const highlight = new Highlight(...ranges);
  highlight.priority = priority;
  CSS.highlights.set(name, highlight);
}

export const HighlightedText = memo(function HighlightedText(props: HighlightedTextProps) {
  const { text, side, containerRef, diffRanges, searchMatches, activeMatchId } = props;
  const sourceRef = useRef<HTMLSpanElement | null>(null);
  const fallbackActiveRef = useRef<HTMLElement | null>(null);
  const customHighlightsSupported = supportsCssCustomHighlights();
  const segments = useMemo(
    () => customHighlightsSupported ? [] : buildTextSegments(text, side, diffRanges, searchMatches),
    [customHighlightsSupported, diffRanges, searchMatches, side, text]
  );

  useEffect(() => {
    if (!customHighlightsSupported) {
      const frame = window.requestAnimationFrame(() => fallbackActiveRef.current?.scrollIntoView({ block: "center", inline: "nearest" }));
      return () => window.cancelAnimationFrame(frame);
    }

    const names = HIGHLIGHT_NAMES[side];
    const textNode = sourceRef.current?.firstChild;
    if (!(textNode instanceof Text)) return undefined;
    const sideDiffs = diffRanges.filter((range) => range.side === side);
    const diffHighlightRanges = sideDiffs.map((range) => createRange(textNode, range.start, range.end));
    const searchHighlightRanges = searchMatches.map((match) => createRange(textNode, match.start, match.end));
    const activeMatch = searchMatches.find((match) => match.id === activeMatchId);
    const activeRange = activeMatch ? createRange(textNode, activeMatch.start, activeMatch.end) : undefined;

    setHighlight(names.diff, diffHighlightRanges, 0);
    setHighlight(names.search, searchHighlightRanges, 1);
    setHighlight(names.active, activeRange ? [activeRange] : [], 2);

    let frame = 0;
    if (activeRange) {
      frame = window.requestAnimationFrame(() => {
        if (typeof activeRange.getBoundingClientRect !== "function") return;
        const container = containerRef.current;
        if (!container) return;
        const matchRect = activeRange.getBoundingClientRect();
        const containerRect = container.getBoundingClientRect();
        container.scrollTop += matchRect.top - containerRect.top - (container.clientHeight / 2) + (matchRect.height / 2);
      });
    }

    return () => {
      if (frame) window.cancelAnimationFrame(frame);
      CSS.highlights.delete(names.diff);
      CSS.highlights.delete(names.search);
      CSS.highlights.delete(names.active);
    };
  }, [activeMatchId, containerRef, customHighlightsSupported, diffRanges, searchMatches, side, text]);

  if (customHighlightsSupported) return <span ref={sourceRef}>{text}</span>;

  return (
    <>
      {segments.map((segment) => {
        const content = text.slice(segment.start, segment.end);
        const active = Boolean(activeMatchId && segment.searchMatch?.id === activeMatchId);
        const className = [
          segment.diffKind ? `diff-${segment.diffKind}` : "",
          segment.searchMatch ? "search-match" : "",
          active ? "active-search-match" : ""
        ].filter(Boolean).join(" ");
        const key = `${segment.start}:${segment.end}`;
        return segment.searchMatch ? (
          <mark key={key} ref={active ? fallbackActiveRef : undefined} className={className} data-match-id={segment.searchMatch.id}>{content}</mark>
        ) : className ? <span key={key} className={className}>{content}</span> : content;
      })}
    </>
  );
});
