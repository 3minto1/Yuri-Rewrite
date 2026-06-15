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
const MIXED_TOKEN_PATTERN = /[\p{Script=Han}]|[\p{L}\p{N}_]+|\r\n|\r|\n|[^\S\r\n]+|[^\s\p{Script=Han}\p{L}\p{N}_]/gu;
const LINE_TOKEN_PATTERN = /[^\n]*\n|[^\n]+$/g;
const HORIZONTAL_WHITESPACE_PATTERN = /[^\S\r\n]/u;
const LINE_LEADING_WHITESPACE_PATTERN = /^[^\S\r\n]+/u;

type DiffToken = {
  text: string;
  comparison: string;
};

export function tokenizeMixed(text: string): string[] {
  return text.match(MIXED_TOKEN_PATTERN) ?? [];
}

function tokenizeLines(text: string): string[] {
  return text.match(LINE_TOKEN_PATTERN) ?? [];
}

function mixedDiffTokens(text: string): DiffToken[] {
  let atLineStart = true;
  return tokenizeMixed(text).map((token) => {
    const isHorizontalWhitespace = !token.includes("\n")
      && !token.includes("\r")
      && HORIZONTAL_WHITESPACE_PATTERN.test(token);
    const comparison = atLineStart && isHorizontalWhitespace ? "\0line-indent" : token;
    if (token.includes("\n") || token.includes("\r")) {
      atLineStart = true;
    } else if (!isHorizontalWhitespace) {
      atLineStart = false;
    }
    return { text: token, comparison };
  });
}

function lineDiffTokens(text: string): DiffToken[] {
  return tokenizeLines(text).map((line) => ({
    text: line,
    comparison: line.replace(LINE_LEADING_WHITESPACE_PATTERN, "")
  }));
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

function tokenLength(tokens: DiffToken[], start: number, count: number) {
  let length = 0;
  for (let index = start; index < start + count; index += 1) length += tokens[index].text.length;
  return length;
}

function buildRanges(originalTokens: DiffToken[], rewriteTokens: DiffToken[], timeout: number): DiffRange[] | undefined {
  const parts = diffArrays(originalTokens, rewriteTokens, {
    timeout,
    comparator: (left, right) => left.comparison === right.comparison
  });
  if (!parts) return undefined;
  const ranges: DiffRange[] = [];
  let originalOffset = 0;
  let rewriteOffset = 0;
  let originalTokenIndex = 0;
  let rewriteTokenIndex = 0;
  for (const part of parts) {
    const count = part.count ?? part.value.length;
    if (part.removed) {
      const originalLength = tokenLength(originalTokens, originalTokenIndex, count);
      appendRange(ranges, { side: "original", start: originalOffset, end: originalOffset + originalLength, kind: "removed" });
      originalOffset += originalLength;
      originalTokenIndex += count;
    } else if (part.added) {
      const rewriteLength = tokenLength(rewriteTokens, rewriteTokenIndex, count);
      appendRange(ranges, { side: "rewrite", start: rewriteOffset, end: rewriteOffset + rewriteLength, kind: "added" });
      rewriteOffset += rewriteLength;
      rewriteTokenIndex += count;
    } else {
      const originalLength = tokenLength(originalTokens, originalTokenIndex, count);
      const rewriteLength = tokenLength(rewriteTokens, rewriteTokenIndex, count);
      originalOffset += originalLength;
      rewriteOffset += rewriteLength;
      originalTokenIndex += count;
      rewriteTokenIndex += count;
    }
  }
  return ranges;
}

function removeLineLeadingWhitespace(text: string, range: DiffRange): DiffRange[] {
  const ranges: DiffRange[] = [];
  let segmentStart = range.start;
  const lineStart = Math.max(text.lastIndexOf("\n", range.start - 1), text.lastIndexOf("\r", range.start - 1)) + 1;
  let inLineIndent = LINE_LEADING_WHITESPACE_PATTERN.test(text.slice(lineStart, range.start));
  if (lineStart === range.start) inLineIndent = true;

  for (let index = range.start; index < range.end; index += 1) {
    const char = text[index];
    if (char === "\n" || char === "\r") {
      inLineIndent = true;
      continue;
    }
    if (inLineIndent && HORIZONTAL_WHITESPACE_PATTERN.test(char)) {
      if (segmentStart < index) appendRange(ranges, { ...range, start: segmentStart, end: index });
      segmentStart = index + 1;
      continue;
    }
    inLineIndent = false;
  }
  if (segmentStart < range.end) appendRange(ranges, { ...range, start: segmentStart, end: range.end });
  return ranges;
}

function cleanRanges(ranges: DiffRange[], original: string, rewrite: string): DiffRange[] {
  const cleaned: DiffRange[] = [];
  for (const range of ranges) {
    const text = range.side === "original" ? original : rewrite;
    for (const part of removeLineLeadingWhitespace(text, range)) appendRange(cleaned, part);
  }
  return cleaned;
}

export function calculateDiff(original: string, rewrite: string, overrides: Partial<DiffLimits> = {}): DiffResult {
  const limits = { ...DEFAULT_DIFF_LIMITS, ...overrides };
  const originalTokens = mixedDiffTokens(original);
  const rewriteTokens = mixedDiffTokens(rewrite);
  if (originalTokens.length + rewriteTokens.length <= limits.tokenLimit) {
    const ranges = buildRanges(originalTokens, rewriteTokens, limits.mixedTimeoutMs);
    if (ranges) {
      const cleaned = cleanRanges(ranges, original, rewrite);
      if (cleaned.length <= limits.rangeLimit) return { ranges: cleaned, mode: "mixed" };
    }
  }

  const lineRanges = buildRanges(lineDiffTokens(original), lineDiffTokens(rewrite), limits.lineTimeoutMs);
  if (lineRanges) {
    const cleaned = cleanRanges(lineRanges, original, rewrite);
    if (cleaned.length <= limits.rangeLimit) return { ranges: cleaned, mode: "line" };
  }
  return { ranges: [], mode: "plain" };
}
