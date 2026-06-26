import type { Chapter, NovelSettings } from "../../types";

export type QualityIssueSeverity = "error" | "warning" | "info";

export type QualityIssueCategory =
  | "missing_rewrite"
  | "source_name"
  | "gender_residue"
  | "group_pronoun"
  | "ad_noise"
  | "garbage"
  | "duplicate"
  | "unchanged"
  | "length_delta";

export type QualityIssue = {
  id: string;
  chapterId: string;
  chapterIndex: number;
  category: QualityIssueCategory;
  severity: QualityIssueSeverity;
  message: string;
  evidence: string;
  query: string;
};

export type RewriteQualityOverride = {
  chapterId: string;
  rewriteText: string;
};

const AD_PATTERNS = [
  "求票",
  "求收藏",
  "最新网址",
  "最新章节",
  "QQ群",
  "书友群",
  "加群",
  "作者有话说",
  "月票",
  "推荐票"
];

const MASCULINE_TERMS = [
  "少年郎",
  "少年",
  "公子",
  "少爷",
  "男子",
  "男人",
  "男儿",
  "男主",
  "作为男人"
];

const GROUP_MALE_CUES = [
  "父亲",
  "兄弟",
  "师父",
  "师兄",
  "公子",
  "少爷",
  "男子",
  "男人",
  "少年",
  "他们",
  "他"
];

const severityRank: Record<QualityIssueSeverity, number> = {
  error: 0,
  warning: 1,
  info: 2
};

const LENGTH_DELTA_WARNING_THRESHOLD = 1000;

function normalizeText(text: string) {
  return text.replace(/\s/g, "");
}

function splitConfiguredNames(input: string) {
  return input
    .split(/[\n\r,，、;；]+/u)
    .map((name) => {
      const trimmed = name.trim();
      for (const delimiter of ["->", "=>", "→"]) {
        const index = trimmed.indexOf(delimiter);
        if (index >= 0) return trimmed.slice(0, index).trim();
      }
      return trimmed;
    })
    .filter(Boolean);
}

function protagonistSourceNames(settings?: NovelSettings | null) {
  if (!settings) return [];
  const rewritten = settings.rewritten_protagonist_name.trim();
  const names = [settings.protagonist_name.trim(), ...splitConfiguredNames(settings.protagonist_aliases)]
    .filter((name) => name && name !== rewritten);
  return Array.from(new Set(names));
}

function splitSentences(text: string) {
  const sentences: string[] = [];
  let current = "";
  for (const char of text) {
    current += char;
    if ("。！？!?；;\n".includes(char)) {
      if (current.trim()) sentences.push(current.trim());
      current = "";
    }
  }
  if (current.trim()) sentences.push(current.trim());
  return sentences;
}

function truncateEvidence(text: string) {
  const compact = text.replace(/\s+/g, " ").trim();
  return compact.length > 80 ? `${compact.slice(0, 80)}…` : compact;
}

function containsStandaloneMalePronoun(sentence: string) {
  return /(^|[，。！？；：“”、\s])他([，。！？；：“”、\s]|$)/u.test(sentence);
}

function firstMatchingTerm(text: string, terms: string[]) {
  return terms.find((term) => text.includes(term));
}

function pushIssue(
  issues: QualityIssue[],
  chapter: Chapter,
  category: QualityIssueCategory,
  severity: QualityIssueSeverity,
  message: string,
  evidence: string,
  query = evidence
) {
  const compactEvidence = truncateEvidence(evidence);
  issues.push({
    id: `${chapter.id}:${category}:${issues.length}`,
    chapterId: chapter.id,
    chapterIndex: chapter.index,
    category,
    severity,
    message,
    evidence: compactEvidence,
    query: query.trim() || compactEvidence
  });
}

function detectDuplicateLine(text: string) {
  const lines = text
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.length >= 8);
  for (let index = 1; index < lines.length; index += 1) {
    if (lines[index] === lines[index - 1]) return lines[index];
  }
  return "";
}

export function scanRewriteQuality(
  chapters: Chapter[],
  settings?: NovelSettings | null,
  override?: RewriteQualityOverride | null
): QualityIssue[] {
  const issues: QualityIssue[] = [];
  const sourceNames = protagonistSourceNames(settings);
  const rewrittenName = settings?.rewritten_protagonist_name.trim() ?? "";

  for (const chapter of chapters) {
    const hasOverride = override?.chapterId === chapter.id;
    const rewriteText = hasOverride ? override.rewriteText : chapter.rewrite_text ?? "";
    const rewriteTitleAndText = `${chapter.title}\n${rewriteText}`;
    const originalCount = normalizeText(chapter.original_text).length;
    const rewriteCount = normalizeText(rewriteText).length;

    if (rewriteCount === 0) {
      if (hasOverride || chapter.rewrite_status === "completed") {
        pushIssue(issues, chapter, "missing_rewrite", "error", "已完成但改写稿为空。", "空改写稿", "");
      }
      continue;
    }

    if (originalCount >= 80 && (rewriteCount < 20 || rewriteCount / originalCount < 0.15)) {
      pushIssue(issues, chapter, "missing_rewrite", "warning", "改写稿明显过短。", rewriteText, rewriteText.slice(0, 20));
    }

    if (originalCount > 0 && normalizeText(chapter.original_text) === normalizeText(rewriteText)) {
      pushIssue(issues, chapter, "unchanged", "warning", "改写稿与原文去空白后一致，疑似未改写。", rewriteText, rewriteText.slice(0, 20));
    }

    const skipLengthDelta = hasOverride || chapter.rewrite_edited || chapter.single_rewrite_original_available;
    const lengthDelta = Math.abs(originalCount - rewriteCount);
    if (
      !skipLengthDelta
      && chapter.rewrite_status === "completed"
      && originalCount > 0
      && lengthDelta >= LENGTH_DELTA_WARNING_THRESHOLD
    ) {
      pushIssue(
        issues,
        chapter,
        "length_delta",
        "warning",
        `原文 ${originalCount} 字，改写稿 ${rewriteCount} 字，相差 ${lengthDelta} 字。`,
        rewriteText,
        rewriteText.slice(0, 20)
      );
    }

    for (const name of sourceNames) {
      if (rewriteTitleAndText.includes(name)) {
        pushIssue(issues, chapter, "source_name", "error", `仍残留主角原名或别名：${name}`, name, name);
      }
    }

    const sentences = splitSentences(rewriteTitleAndText);
    if (rewrittenName) {
      for (const sentence of sentences) {
        if (!sentence.includes(rewrittenName)) continue;
        const masculineTerm = firstMatchingTerm(sentence, MASCULINE_TERMS);
        if (masculineTerm || containsStandaloneMalePronoun(sentence)) {
          pushIssue(
            issues,
            chapter,
            "gender_residue",
            "warning",
            "主角相关句疑似残留男性称谓或代词。",
            sentence,
            masculineTerm ?? "他"
          );
          break;
        }
      }

      for (const sentence of sentences) {
        if (!sentence.includes(rewrittenName) || !sentence.includes("她们")) continue;
        const cue = firstMatchingTerm(sentence, GROUP_MALE_CUES);
        if (!cue) continue;
        pushIssue(
          issues,
          chapter,
          "group_pronoun",
          "warning",
          "主角相关群体句使用“她们”，但句中也有男性角色线索。",
          sentence,
          "她们"
        );
        break;
      }
    }

    const adTerm = firstMatchingTerm(rewriteText, AD_PATTERNS);
    if (adTerm) {
      pushIssue(issues, chapter, "ad_noise", "warning", "疑似广告、求票或读者互动残留。", adTerm, adTerm);
    }

    const garbageMatch = rewriteText.match(/[�□]|[~～*＊=_＿\-—·。]{6,}/u);
    if (garbageMatch) {
      pushIssue(issues, chapter, "garbage", "warning", "疑似乱码或无关装饰标点。", garbageMatch[0], garbageMatch[0]);
    }

    const duplicate = detectDuplicateLine(rewriteText);
    if (duplicate) {
      pushIssue(issues, chapter, "duplicate", "warning", "发现连续重复段落。", duplicate, duplicate);
    }
  }

  return issues.sort((left, right) =>
    left.chapterIndex - right.chapterIndex
    || severityRank[left.severity] - severityRank[right.severity]
    || left.category.localeCompare(right.category)
  );
}
