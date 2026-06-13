import { diffArrays } from "diff";

export type DiffSide = "original" | "rewrite";
export type DiffRange = { side: DiffSide; start: number; end: number; kind: "removed" | "added" };
export type DiffResult = { ranges: DiffRange[]; mode: "mixed" | "line" };

const TOKEN_LIMIT = 80_000;
const MIXED_TOKEN_PATTERN = /[\p{Script=Han}]|[\p{L}\p{N}_]+|\s+|[^\s\p{Script=Han}\p{L}\p{N}_]/gu;
const LINE_TOKEN_PATTERN = /[^\n]*\n|[^\n]+$/g;

export function tokenizeMixed(text: string): string[] {
  return text.match(MIXED_TOKEN_PATTERN) ?? [];
}

function tokenizeLines(text: string): string[] {
  return text.match(LINE_TOKEN_PATTERN) ?? [];
}

function appendRange(ranges: DiffRange[], range: DiffRange) {
  if (range.start === range.end) return;
  const previous = ranges[ranges.length - 1];
  if (previous && previous.side === range.side && previous.kind === range.kind && previous.end === range.start) {
    previous.end = range.end;
  } else {
    ranges.push(range);
  }
}

export function calculateDiff(original: string, rewrite: string): DiffResult {
  let originalTokens = tokenizeMixed(original);
  let rewriteTokens = tokenizeMixed(rewrite);
  let mode: DiffResult["mode"] = "mixed";
  if (originalTokens.length + rewriteTokens.length > TOKEN_LIMIT) {
    originalTokens = tokenizeLines(original);
    rewriteTokens = tokenizeLines(rewrite);
    mode = "line";
  }

  const ranges: DiffRange[] = [];
  let originalOffset = 0;
  let rewriteOffset = 0;
  for (const part of diffArrays(originalTokens, rewriteTokens)) {
    const length = part.value.reduce((total, token) => total + token.length, 0);
    if (part.removed) {
      appendRange(ranges, { side: "original", start: originalOffset, end: originalOffset + length, kind: "removed" });
      originalOffset += length;
    } else if (part.added) {
      appendRange(ranges, { side: "rewrite", start: rewriteOffset, end: rewriteOffset + length, kind: "added" });
      rewriteOffset += length;
    } else {
      originalOffset += length;
      rewriteOffset += length;
    }
  }
  return { ranges, mode };
}
