import { diffArrays } from "diff";

export type DiffSide = "original" | "rewrite";
export type DiffRange = { side: DiffSide; start: number; end: number; kind: "removed" | "added" };
export type DiffResult = { ranges: DiffRange[]; mode: "mixed" | "line" | "plain" };

export type DiffLimits = {
  tokenLimit: number;
  rangeLimit: number;
  mixedTimeoutMs: number;
  lineTimeoutMs: number;
};

export const DEFAULT_DIFF_LIMITS: DiffLimits = {
  tokenLimit: 80_000,
  rangeLimit: 10_000,
  mixedTimeoutMs: 600,
  lineTimeoutMs: 600
};
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

function buildRanges(originalTokens: string[], rewriteTokens: string[], timeout: number): DiffRange[] | undefined {
  const parts = diffArrays(originalTokens, rewriteTokens, { timeout });
  if (!parts) return undefined;
  const ranges: DiffRange[] = [];
  let originalOffset = 0;
  let rewriteOffset = 0;
  for (const part of parts) {
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
  return ranges;
}

export function calculateDiff(original: string, rewrite: string, overrides: Partial<DiffLimits> = {}): DiffResult {
  const limits = { ...DEFAULT_DIFF_LIMITS, ...overrides };
  const originalTokens = tokenizeMixed(original);
  const rewriteTokens = tokenizeMixed(rewrite);
  if (originalTokens.length + rewriteTokens.length <= limits.tokenLimit) {
    const ranges = buildRanges(originalTokens, rewriteTokens, limits.mixedTimeoutMs);
    if (ranges && ranges.length <= limits.rangeLimit) return { ranges, mode: "mixed" };
  }

  const lineRanges = buildRanges(tokenizeLines(original), tokenizeLines(rewrite), limits.lineTimeoutMs);
  if (lineRanges && lineRanges.length <= limits.rangeLimit) return { ranges: lineRanges, mode: "line" };
  return { ranges: [], mode: "plain" };
}
