import type { Chapter } from "../../types";

export type SearchSide = "original" | "rewrite";

export type SearchMatch = {
  id: string;
  chapter_id: string;
  side: SearchSide;
  start: number;
  end: number;
};

export function findTextMatches(text: string, query: string, caseSensitive: boolean): Array<{ start: number; end: number }> {
  if (!query) return [];
  const haystack = caseSensitive ? text : text.toLocaleLowerCase();
  const needle = caseSensitive ? query : query.toLocaleLowerCase();
  const matches: Array<{ start: number; end: number }> = [];
  let offset = 0;
  while (offset <= haystack.length - needle.length) {
    const start = haystack.indexOf(needle, offset);
    if (start < 0) break;
    matches.push({ start, end: start + needle.length });
    offset = start + Math.max(needle.length, 1);
  }
  return matches;
}

export function buildSearchMatches(chapters: Chapter[], query: string, caseSensitive: boolean): SearchMatch[] {
  if (!query) return [];
  const matches: SearchMatch[] = [];
  for (const chapter of chapters) {
    const sides: Array<[SearchSide, string]> = [["original", chapter.original_text]];
    if (chapter.rewrite_text?.trim()) sides.push(["rewrite", chapter.rewrite_text]);
    for (const [side, text] of sides) {
      for (const match of findTextMatches(text, query, caseSensitive)) {
        matches.push({
          id: `${chapter.id}:${side}:${match.start}:${match.end}`,
          chapter_id: chapter.id,
          side,
          ...match
        });
      }
    }
  }
  return matches;
}

export function initialSearchIndex(matches: SearchMatch[], selectedChapterId: string, direction: 1 | -1): number | null {
  if (matches.length === 0) return null;
  if (direction === 1) {
    const index = matches.findIndex((match) => match.chapter_id === selectedChapterId);
    return index >= 0 ? index : 0;
  }
  for (let index = matches.length - 1; index >= 0; index -= 1) {
    if (matches[index].chapter_id === selectedChapterId) return index;
  }
  return matches.length - 1;
}

export function moveSearchIndex(current: number | null, count: number, direction: 1 | -1): { index: number | null; wrapped: boolean } {
  if (count === 0) return { index: null, wrapped: false };
  if (current === null) return { index: direction === 1 ? 0 : count - 1, wrapped: false };
  const next = current + direction;
  if (next >= count) return { index: 0, wrapped: true };
  if (next < 0) return { index: count - 1, wrapped: true };
  return { index: next, wrapped: false };
}
