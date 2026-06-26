use crate::domain::{Chapter, ChapterRule, SplitResult};
use regex::Regex;
use std::sync::OnceLock;
use uuid::Uuid;

const LONG_CHAPTER_SPLIT_LIMIT: usize = 5_000;

pub(crate) fn split_chapters(novel_id: &str, text: &str) -> SplitResult {
    let matches = chapter_heading_matches(text);
    if matches.is_empty() {
        return SplitResult {
            chapters: chunk_without_headings(novel_id, text),
            detected_chapters: false,
        };
    }

    let mut segments = Vec::new();
    for (idx, mat) in matches.iter().enumerate() {
        let start = mat.start();
        let content_start = mat.end();
        let end = matches.get(idx + 1).map_or(text.len(), |next| next.start());
        let title = text[start..content_start].trim();
        let title = if title.is_empty() {
            format!("第{}章", idx + 1)
        } else {
            title.to_string()
        };
        let original_text = text[content_start..end].trim().to_string();
        segments.push(DetectedChapterSegment {
            title,
            original_text,
        });
    }
    let segments = normalize_detected_chapter_segments(segments);
    let chapters = segments
        .into_iter()
        .enumerate()
        .map(|(idx, segment)| Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title: segment.title,
            original_text: segment.original_text,
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        })
        .collect();
    SplitResult {
        chapters,
        detected_chapters: true,
    }
}

pub(crate) fn split_chapters_with_custom_rule(
    novel_id: &str,
    text: &str,
    rule: &ChapterRule,
) -> Result<SplitResult, String> {
    let heading_re = custom_chapter_heading_regex(rule)?;
    let include_re = custom_optional_regex(rule.include_pattern.trim(), "附加规则")?;
    let exclude_re = custom_optional_regex(rule.extra_pattern.trim(), "排除规则")?;
    let mut headings = Vec::new();
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let line_start = offset;
        offset += segment.len();
        let line = segment.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        if line.chars().count() > 120 {
            continue;
        }
        if !custom_rule_matches_heading(&heading_re, include_re.as_ref(), line) {
            continue;
        }
        if exclude_re
            .as_ref()
            .is_some_and(|exclude| exclude.is_match(line))
        {
            continue;
        }
        let content_start = offset;
        headings.push(CustomHeadingMatch {
            start: line_start,
            content_start,
            title: line.trim().to_string(),
        });
    }
    if offset < text.len() {
        let line = &text[offset..];
        if !line.trim().is_empty()
            && line.chars().count() <= 120
            && custom_rule_matches_heading(&heading_re, include_re.as_ref(), line)
            && !exclude_re
                .as_ref()
                .is_some_and(|exclude| exclude.is_match(line))
        {
            headings.push(CustomHeadingMatch {
                start: offset,
                content_start: text.len(),
                title: line.trim().to_string(),
            });
        }
    }
    if headings.is_empty() {
        return Err("自定义章节规则没有匹配到章节标题。".to_string());
    }
    let mut segments = Vec::new();
    for (idx, heading) in headings.iter().enumerate() {
        let end = headings.get(idx + 1).map_or(text.len(), |next| next.start);
        segments.push(DetectedChapterSegment {
            title: heading.title.clone(),
            original_text: text[heading.content_start..end].trim().to_string(),
        });
    }
    let segments = normalize_detected_chapter_segments(segments);
    if segments.is_empty() {
        return Err("自定义章节规则匹配结果均被识别为非正文提示，未生成章节。".to_string());
    }
    let chapters = segments
        .into_iter()
        .enumerate()
        .map(|(idx, segment)| Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title: segment.title,
            original_text: segment.original_text,
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        })
        .collect();
    Ok(SplitResult {
        chapters,
        detected_chapters: true,
    })
}

pub(crate) fn split_long_detected_chapters(
    novel_id: &str,
    chapters: Vec<Chapter>,
    enabled: bool,
) -> Vec<Chapter> {
    if !enabled {
        return chapters;
    }
    let mut rewritten = Vec::new();
    for chapter in chapters {
        let parts = split_long_chapter_text(&chapter.original_text);
        let split_count = parts.len();
        for (part_idx, part) in parts.into_iter().enumerate() {
            let title = if split_count > 1 {
                format!("{}（{}）", chapter.title, part_idx + 1)
            } else {
                chapter.title.clone()
            };
            rewritten.push(Chapter {
                id: Uuid::new_v4().to_string(),
                novel_id: novel_id.to_string(),
                index: rewritten.len() as i64 + 1,
                title,
                original_text: part,
                analysis_json: None,
                rewrite_text: None,
                rewrite_edited: false,
                single_rewrite_original_available: false,
                analysis_status: "pending".to_string(),
                rewrite_status: "pending".to_string(),
            });
        }
    }
    rewritten
}

fn split_long_chapter_text(text: &str) -> Vec<String> {
    let non_whitespace_count = text.chars().filter(|ch| !ch.is_whitespace()).count();
    if non_whitespace_count <= LONG_CHAPTER_SPLIT_LIMIT {
        return vec![text.to_string()];
    }
    let part_count = non_whitespace_count.div_ceil(LONG_CHAPTER_SPLIT_LIMIT);
    let mut boundaries = Vec::new();
    let mut last = 0usize;
    for part_index in 1..part_count {
        let target = non_whitespace_count * part_index / part_count;
        let ideal = byte_index_for_non_whitespace_target(text, target);
        let preferred = nearest_line_boundary(text, target, non_whitespace_count).unwrap_or(ideal);
        let boundary = if preferred > last { preferred } else { ideal };
        if boundary > last && boundary < text.len() {
            boundaries.push(boundary);
            last = boundary;
        }
    }

    let mut start = 0usize;
    let mut parts = Vec::new();
    for end in boundaries.into_iter().chain(std::iter::once(text.len())) {
        let part = text[start..end].trim().to_string();
        if !part.is_empty() {
            parts.push(part);
        }
        start = end;
    }
    if parts.is_empty() {
        vec![text.trim().to_string()]
    } else {
        parts
    }
}

fn byte_index_for_non_whitespace_target(text: &str, target: usize) -> usize {
    let mut seen = 0usize;
    for (idx, ch) in text.char_indices() {
        if !ch.is_whitespace() {
            seen += 1;
            if seen >= target {
                return idx + ch.len_utf8();
            }
        }
    }
    text.len()
}

fn nearest_line_boundary(text: &str, target: usize, total: usize) -> Option<usize> {
    let tolerance = LONG_CHAPTER_SPLIT_LIMIT / 5;
    let min_target = target.saturating_sub(tolerance).max(1);
    let max_target = (target + tolerance).min(total.saturating_sub(1));
    let mut best: Option<(usize, usize)> = None;
    let mut seen = 0usize;
    for (idx, ch) in text.char_indices() {
        if !ch.is_whitespace() {
            seen += 1;
        }
        if ch == '\n' && seen >= min_target && seen <= max_target {
            let distance = seen.abs_diff(target);
            if best
                .as_ref()
                .is_none_or(|(_, best_distance)| distance < *best_distance)
            {
                best = Some((idx + ch.len_utf8(), distance));
            }
        }
    }
    best.map(|(idx, _)| idx)
}

struct CustomHeadingMatch {
    start: usize,
    content_start: usize,
    title: String,
}

fn custom_rule_matches_heading(heading_re: &Regex, include_re: Option<&Regex>, line: &str) -> bool {
    heading_re.is_match(line) || include_re.is_some_and(|include| include.is_match(line))
}

fn custom_optional_regex(pattern: &str, label: &str) -> Result<Option<Regex>, String> {
    if pattern.is_empty() {
        return Ok(None);
    }
    if pattern.contains('\n')
        || pattern.contains('\r')
        || pattern.contains("\\n")
        || pattern.contains("\\r")
    {
        return Err(format!("{}必须按单行匹配，不能包含换行匹配。", label));
    }
    Regex::new(pattern)
        .map(Some)
        .map_err(|error| format!("{}不是有效正则：{}", label, error))
}

fn custom_chapter_heading_regex(rule: &ChapterRule) -> Result<Regex, String> {
    if rule.mode == "regex" {
        let pattern = rule.regex_pattern.trim();
        if pattern.is_empty() {
            return Err("正则表达式不能为空。".to_string());
        }
        if pattern.contains('\n')
            || pattern.contains('\r')
            || pattern.contains("\\n")
            || pattern.contains("\\r")
        {
            return Err("章节标题正则必须按单行匹配，不能包含换行匹配。".to_string());
        }
        return Regex::new(pattern).map_err(|error| format!("正则表达式无效：{}", error));
    }

    let prefix = simple_rule_token_expression(&rule.prefix, "前缀")?;
    let unit = simple_rule_token_expression(&rule.unit, "章节单位")?;
    let number = match rule.number_type.as_str() {
        "arabic" => r#"[0-9０-９]+"#,
        "chinese" => r#"[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+"#,
        _ => r#"[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+"#,
    };
    let start = if rule.line_start {
        r#"^[\s\u{feff}　]*"#
    } else {
        r#"[\s\u{feff}　]*"#
    };
    let pattern = format!(
        r#"{}{}[ \t　]*{}[ \t　]*{}[ \t　:：、.．\-—_·|]*[^\r\n]{{0,80}}[ \t　]*$"#,
        start, prefix, number, unit
    );
    Regex::new(&pattern).map_err(|error| format!("简易章节规则无效：{}", error))
}

fn simple_rule_token_expression(value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{}不能为空。", label));
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(format!("{}不能包含换行。", label));
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.chars().count() >= 3 {
        let inner = &trimmed[1..trimmed.len() - 1];
        if inner.trim().is_empty() {
            return Err(format!("{}字符组不能为空。", label));
        }
        let escaped = inner
            .chars()
            .map(|ch| regex::escape(&ch.to_string()))
            .collect::<Vec<_>>()
            .join("");
        Ok(format!("[{}]", escaped))
    } else {
        Ok(regex::escape(trimmed))
    }
}

struct DetectedChapterSegment {
    title: String,
    original_text: String,
}

#[derive(Clone, Copy)]
struct NumberedHeadingSignature {
    ordinal: u64,
    unit: char,
}

fn normalize_detected_chapter_segments(
    mut segments: Vec<DetectedChapterSegment>,
) -> Vec<DetectedChapterSegment> {
    for segment in &mut segments {
        segment.original_text = trim_boundary_author_notes(&segment.original_text);
    }

    let pseudo_titles = segments
        .iter()
        .map(|segment| is_extended_update_notice(&compact_heading_line(&segment.title)))
        .collect::<Vec<_>>();
    let signatures = segments
        .iter()
        .map(|segment| numbered_heading_signature(&segment.title))
        .collect::<Vec<_>>();
    let mut normalized: Vec<DetectedChapterSegment> = Vec::with_capacity(segments.len());
    for (idx, mut segment) in segments.into_iter().enumerate() {
        if !pseudo_titles[idx] {
            normalized.push(segment);
            continue;
        }

        let previous_formal = (0..idx).rev().find(|candidate| !pseudo_titles[*candidate]);
        let next_formal =
            ((idx + 1)..pseudo_titles.len()).find(|candidate| !pseudo_titles[*candidate]);
        let is_between_sequential_formal_headings =
            previous_formal
                .zip(next_formal)
                .is_some_and(|(previous, next)| {
                    signatures[previous]
                        .zip(signatures[next])
                        .is_some_and(|(previous, next)| {
                            headings_are_sequential_peers(previous, next)
                        })
                });

        if is_between_sequential_formal_headings {
            if !segment.original_text.is_empty() {
                if let Some(previous) = normalized.last_mut() {
                    append_chapter_body(&mut previous.original_text, &segment.original_text);
                }
            }
            continue;
        }

        if segment.original_text.is_empty()
            || is_obvious_droppable_author_note_text(&segment.original_text)
        {
            continue;
        }
        segment.original_text = trim_boundary_author_notes(&segment.original_text);
        normalized.push(segment);
    }
    normalized
}

fn headings_are_sequential_peers(
    previous: NumberedHeadingSignature,
    next: NumberedHeadingSignature,
) -> bool {
    previous.unit == next.unit && previous.unit != '更' && next.ordinal == previous.ordinal + 1
}

fn numbered_heading_signature(title: &str) -> Option<NumberedHeadingSignature> {
    static NUMBERED_RE: OnceLock<Regex> = OnceLock::new();
    let numbered_re = NUMBERED_RE.get_or_init(|| {
        Regex::new(
            r#"第([0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+)([章节回卷部集篇话幕节页季段册夜案场弹折更])"#,
        )
        .expect("valid numbered heading signature regex")
    });
    numbered_re
        .captures_iter(&compact_heading_line(title))
        .last()
        .and_then(|captures| {
            let ordinal = captures.get(1)?.as_str();
            let unit = captures.get(2)?.as_str().chars().next()?;
            let ordinal =
                parse_fullwidth_digits(ordinal).or_else(|| parse_chinese_ordinal(ordinal))?;
            Some(NumberedHeadingSignature { ordinal, unit })
        })
}

fn append_chapter_body(target: &mut String, body: &str) {
    if body.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(body);
}

fn trim_boundary_author_notes(text: &str) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let mut start = 0;
    let mut end = lines.len();
    while start < end && is_obvious_droppable_author_note_line(lines[start]) {
        start += 1;
    }
    while end > start && is_obvious_droppable_author_note_line(lines[end - 1]) {
        end -= 1;
    }
    if start == 0 && end == lines.len() {
        return text.to_string();
    }
    lines[start..end].join("\n").trim().to_string()
}

pub(crate) fn chapter_heading_matches(text: &str) -> Vec<regex::Match<'_>> {
    let heading_re = chapter_heading_regex();
    let matches = heading_re
        .find_iter(text)
        .filter(|mat| is_plausible_strict_heading_line(mat.as_str()))
        .collect::<Vec<_>>();
    if matches
        .iter()
        .any(|mat| is_numbered_strict_chapter_heading(mat.as_str()))
    {
        return matches;
    }
    let loose_heading_re = loose_numbered_chapter_heading_regex();
    let loose_matches = loose_heading_re
        .find_iter(text)
        .filter(|mat| is_plausible_loose_numbered_heading_line(mat.as_str()))
        .collect::<Vec<_>>();
    if loose_numbered_headings_are_plausible(text, &loose_matches) {
        let mut merged_matches = matches
            .into_iter()
            .filter(|mat| {
                !is_loose_container_heading(mat.as_str())
                    && !is_loose_metadata_heading(mat.as_str())
            })
            .chain(loose_matches)
            .collect::<Vec<_>>();
        merged_matches.sort_by_key(|mat| mat.start());
        merged_matches
            .into_iter()
            .fold(Vec::new(), |mut deduped, mat| {
                if deduped
                    .last()
                    .is_none_or(|last: &regex::Match<'_>| last.start() != mat.start())
                {
                    deduped.push(mat);
                }
                deduped
            })
    } else if !matches.is_empty() {
        matches
    } else {
        Vec::new()
    }
}

pub(crate) fn chapter_heading_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(
        r#"(?m)^[\s\u{feff}　]*(?:={2,6}[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][^=\r\n]{0,80}={2,6}|={2,6}[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[^=\r\n]{0,80}={2,6}|[【〔［「『《（(\[]?[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:卷|篇|部|章|回|幕|册|节)[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[上中下前后终外][ \t　]*(?:卷|篇|部|章|册)[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:Chapter|CHAPTER|chapter|Chap\.?|CH\.?|ch\.?|Section|SECTION|section|Part|PART|part|Episode|EPISODE|episode|No\.?|NO\.?|no\.?)[ \t　]*[0-9０-９IVXLCDMivxlcdm]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[【〔［「『《（(\[]?[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80})[\t 　]*\r?$"#,
    )
    .expect("valid chapter regex"))
}

pub(crate) fn is_plausible_strict_heading_line(line: &str) -> bool {
    let core = line
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || ch == '\u{feff}'
                || ch == '　'
                || ch == '='
                || matches!(
                    ch,
                    '【' | '】'
                        | '〔'
                        | '〕'
                        | '［'
                        | '］'
                        | '「'
                        | '」'
                        | '『'
                        | '』'
                        | '《'
                        | '》'
                        | '（'
                        | '）'
                        | '('
                        | ')'
                        | '['
                        | ']'
                )
        })
        .trim();
    let compact = core
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if starts_with_inline_round_phrase(&compact) {
        return false;
    }
    if strict_heading_is_obvious_update_notice(&compact) {
        return false;
    }
    if strict_numbered_heading_looks_like_body_sentence(&compact) {
        return false;
    }
    if [
        "章正文",
        "节正文",
        "回正文",
        "卷正文",
        "部正文",
        "集正文",
        "篇正文",
        "话正文",
        "幕正文",
        "页正文",
        "季正文",
        "段正文",
        "册正文",
        "夜正文",
        "案正文",
        "场正文",
        "弹正文",
        "折正文",
        "更正文",
    ]
    .iter()
    .any(|pattern| compact.contains(pattern))
    {
        return false;
    }
    [
        "序章正文",
        "楔子正文",
        "引子正文",
        "引言正文",
        "序言正文",
        "序幕正文",
        "前言正文",
        "终章正文",
        "尾声正文",
        "后记正文",
        "番外正文",
        "番外篇正文",
        "番外章正文",
        "特别篇正文",
        "外传正文",
        "插曲正文",
        "间章正文",
    ]
    .iter()
    .all(|pattern| !compact.starts_with(pattern))
        && !special_heading_content_looks_like_body(&compact)
}

pub(crate) fn strict_heading_is_obvious_update_notice(compact: &str) -> bool {
    static UPDATE_RE: OnceLock<Regex> = OnceLock::new();
    let update_re = UPDATE_RE.get_or_init(|| Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+更[！!。．.…~～]*$"#,
    )
    .expect("valid update notice heading regex"));
    if update_re.is_match(compact) {
        return true;
    }

    static NUMBERED_RE: OnceLock<Regex> = OnceLock::new();
    let numbered_re = NUMBERED_RE.get_or_init(|| Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[章节回集话幕节页季段夜案场弹折更](.+)$"#,
    )
    .expect("valid update prose heading regex"));
    let Some(rest) = numbered_re
        .captures(compact)
        .and_then(|captures| captures.get(1))
        .map(|mat| mat.as_str())
    else {
        return false;
    };
    let rest = rest.trim_matches(|ch| {
        matches!(
            ch,
            '：' | ':'
                | '、'
                | '-'
                | '—'
                | '_'
                | '·'
                | '|'
                | '.'
                | '．'
                | '，'
                | ','
                | '。'
                | '！'
                | '!'
                | '？'
                | '?'
                | '（'
                | '）'
                | '('
                | ')'
                | ' '
                | '　'
        )
    });
    contains_update_notice_language(rest)
}

fn is_extended_update_notice(compact: &str) -> bool {
    if strict_heading_is_obvious_update_notice(compact) || contains_update_notice_language(compact)
    {
        return true;
    }

    static UPDATE_PREFIX_RE: OnceLock<Regex> = OnceLock::new();
    let update_prefix_re = UPDATE_PREFIX_RE.get_or_init(|| {
        Regex::new(
            r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+更(.*)$"#,
        )
        .expect("valid extended update notice prefix regex")
    });
    let Some(rest) = update_prefix_re
        .captures(compact)
        .and_then(|captures| captures.get(1))
        .map(|mat| mat.as_str())
    else {
        return false;
    };
    let rest = rest.trim_matches(|ch| {
        matches!(
            ch,
            '：' | ':'
                | '、'
                | '-'
                | '—'
                | '_'
                | '·'
                | '|'
                | '.'
                | '．'
                | '，'
                | ','
                | '。'
                | '！'
                | '!'
                | '？'
                | '?'
                | '（'
                | '）'
                | '('
                | ')'
                | '~'
                | '～'
                | '…'
        )
    });
    if rest.is_empty() || rest.starts_with('到') {
        return true;
    }

    static FOLLOWUP_UPDATE_RE: OnceLock<Regex> = OnceLock::new();
    let followup_update_re = FOLLOWUP_UPDATE_RE.get_or_init(|| {
        Regex::new(
            r#"第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+(?:更|章)(?:也?快了|快了|马上|稍后|未完|$)"#,
        )
        .expect("valid follow-up update notice regex")
    });
    followup_update_re.is_match(rest)
        || contains_any_text(
            rest,
            &[
                "继续写",
                "继续码字",
                "继续更新",
                "还有一更",
                "还有两更",
                "还有三更",
                "还有第",
                "未完待续",
                "最快阅读",
                "搜搜",
                "求月票",
                "求推荐票",
                "求收藏",
                "求订阅",
            ],
        )
}

pub(crate) fn contains_update_notice_language(text: &str) -> bool {
    contains_any_text(
        text,
        &[
            "未完待续",
            "下一章也快了",
            "下章也快了",
            "第二章也快了",
            "下一更也快了",
            "下更也快了",
            "稍后还有一更",
            "稍后还有更新",
            "今天还有一更",
            "明天继续更新",
        ],
    )
}

fn contains_any_text(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(crate) fn is_obvious_droppable_author_note_line(line: &str) -> bool {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    if trimmed.is_empty() {
        return true;
    }
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if contains_any_text(
        &compact,
        &["完本感言", "完结感言", "卷末后记", "卷后记", "后记"],
    ) {
        return false;
    }
    if is_extended_update_notice(&compact) || contains_update_notice_language(&compact) {
        return true;
    }
    if compact.chars().all(|ch| {
        matches!(
            ch,
            '-' | '—' | '_' | '=' | '*' | '＊' | '~' | '～' | '·' | '.' | '。' | '…' | '━' | '─'
        )
    }) {
        return true;
    }
    if compact.chars().count() == 1
        && compact
            .chars()
            .next()
            .is_some_and(|ch| !is_cjk_char(ch) && !ch.is_ascii_alphanumeric())
    {
        return true;
    }
    if matches!(
        compact.as_str(),
        "作者的话" | "作者有话说" | "作者附言" | "题外话" | "PS" | "P.S." | "し"
    ) {
        return true;
    }
    let author_correction = contains_any_text(&compact, &["勘误", "更正", "修正"])
        && contains_any_text(
            &compact,
            &["作者", "年份", "时间", "日期", "前文", "上一章"],
        );
    let reader_interaction = contains_any_text(
        &compact,
        &[
            "求月票",
            "求推荐票",
            "求收藏",
            "求订阅",
            "求追读",
            "大家投票",
            "大家投",
            "投月票",
            "投推荐票",
            "感谢打赏",
            "谢谢打赏",
        ],
    );
    author_correction || reader_interaction
}

pub(crate) fn is_obvious_droppable_author_note_text(text: &str) -> bool {
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    !lines.is_empty()
        && lines
            .iter()
            .all(|line| is_obvious_droppable_author_note_line(line))
}

pub(crate) fn starts_with_inline_round_phrase(compact: &str) -> bool {
    static ROUND_RE: OnceLock<Regex> = OnceLock::new();
    let round_re = ROUND_RE.get_or_init(|| {
        Regex::new(
            r#"^(?:第)?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+回合"#,
        )
        .expect("valid round phrase regex")
    });
    if !round_re.is_match(compact) {
        return false;
    }
    compact
        .chars()
        .any(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'))
        || compact.contains("回合的")
}

pub(crate) fn special_heading_content_looks_like_body(compact: &str) -> bool {
    for keyword in [
        "作品相关",
        "上架感言",
        "完本感言",
        "番外篇",
        "番外章",
        "特别篇",
        "序章",
        "楔子",
        "引子",
        "引言",
        "序言",
        "序幕",
        "前言",
        "终章",
        "尾声",
        "后记",
        "番外",
        "外传",
        "插曲",
        "间章",
        "简介",
        "文案",
    ] {
        if let Some(rest) = compact.strip_prefix(keyword) {
            let rest = rest.trim_matches(|ch| {
                matches!(
                    ch,
                    '：' | ':'
                        | '、'
                        | '-'
                        | '—'
                        | '_'
                        | '·'
                        | '|'
                        | '。'
                        | '.'
                        | '．'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                )
            });
            if rest.is_empty() {
                return false;
            }
            return [
                "写", "中", "里", "是", "的", "时", "我", "也", "就", "到", "说", "提", "已经",
                "终于", "无", "不", "没", "没有", "大家", "今天", "明天", "这", "那", "继续",
                "感谢", "谢谢", "各位", "看到",
            ]
            .iter()
            .any(|prefix| rest.starts_with(prefix));
        }
    }
    false
}

pub(crate) fn strict_numbered_heading_looks_like_body_sentence(compact: &str) -> bool {
    static NUMBERED_RE: OnceLock<Regex> = OnceLock::new();
    let numbered_re = NUMBERED_RE.get_or_init(|| Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+([章节回集话幕节页季段夜案场弹折更])(.+)$"#,
    )
    .expect("valid strict numbered heading parser regex"));
    let Some(captures) = numbered_re.captures(compact) else {
        return false;
    };
    let unit = captures.get(1).map_or("", |mat| mat.as_str());
    let rest = captures
        .get(2)
        .map_or("", |mat| mat.as_str())
        .trim_matches(|ch| {
            matches!(
                ch,
                '：' | ':' | '、' | '-' | '—' | '_' | '·' | '|' | '.' | '．' | ' ' | '　'
            )
        });
    if rest.is_empty() {
        return false;
    }

    let rest_len = rest.chars().count();
    let has_sentence_punctuation = rest
        .chars()
        .any(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'));
    let prose_like_prefix = [
        "是", "的", "了", "在", "从", "到", "把", "被", "让", "和", "与", "又", "却", "就", "便",
        "都", "还", "也", "向", "将", "能", "会", "要", "问", "说", "看到", "发现", "遇到", "魔力",
    ]
    .iter()
    .any(|prefix| rest.starts_with(prefix));

    unit == "回"
        && ((has_sentence_punctuation && rest_len > 14) || (prose_like_prefix && rest_len > 18))
}

pub(crate) fn is_numbered_strict_chapter_heading(line: &str) -> bool {
    let compact = compact_heading_line(line);
    static NUMBERED_RE: OnceLock<Regex> = OnceLock::new();
    let numbered_re = NUMBERED_RE.get_or_init(|| Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[章节回集话幕节页季段夜案场弹折更]"#,
    )
    .expect("valid numbered strict chapter regex"));
    numbered_re.is_match(&compact)
        || compact.starts_with("Chapter")
        || compact.starts_with("CHAPTER")
        || compact.starts_with("chapter")
        || compact.starts_with("Chap")
        || compact.starts_with("CH.")
        || compact.starts_with("ch.")
        || compact.starts_with("Section")
        || compact.starts_with("SECTION")
        || compact.starts_with("section")
        || compact.starts_with("Part")
        || compact.starts_with("PART")
        || compact.starts_with("part")
        || compact.starts_with("Episode")
        || compact.starts_with("EPISODE")
        || compact.starts_with("episode")
        || compact.starts_with("No.")
        || compact.starts_with("NO.")
        || compact.starts_with("no.")
}

pub(crate) fn is_loose_container_heading(line: &str) -> bool {
    let compact = compact_heading_line(line);
    static CONTAINER_RE: OnceLock<Regex> = OnceLock::new();
    let container_re = CONTAINER_RE.get_or_init(|| Regex::new(
        r#"^(?:第?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[卷部]|[卷部][0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+|[上中下前后终外][卷部])"#,
    )
    .expect("valid loose container heading regex"));
    container_re.is_match(&compact)
}

pub(crate) fn is_loose_metadata_heading(line: &str) -> bool {
    matches!(
        compact_heading_line(line).as_str(),
        "文案" | "作品相关" | "上架感言" | "完本感言"
    )
}

pub(crate) fn compact_heading_line(line: &str) -> String {
    line.trim_matches(|ch: char| {
        ch.is_whitespace()
            || ch == '\u{feff}'
            || ch == '　'
            || ch == '='
            || matches!(
                ch,
                '【' | '】'
                    | '〔'
                    | '〕'
                    | '［'
                    | '］'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
                    | '《'
                    | '》'
                    | '（'
                    | '）'
                    | '('
                    | ')'
                    | '['
                    | ']'
            )
    })
    .chars()
    .filter(|ch| !ch.is_whitespace())
    .collect()
}

pub(crate) fn loose_numbered_chapter_heading_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(
        r#"(?m)^[ \t\u{feff}　]*(?:[（(]?[ \t　]*)?(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*(?:[）)][ \t　]*)?(?:[ \t　]+|[、.．:：\-—_·|][ \t　]*)[^\r\n]{1,60}[ \t　]*\r?$"#,
    )
    .expect("valid loose numbered chapter regex"))
}

pub(crate) fn loose_numbered_headings_are_plausible(
    text: &str,
    matches: &[regex::Match<'_>],
) -> bool {
    if matches.len() < 2 {
        return false;
    }
    let ordinals = matches
        .iter()
        .filter_map(|mat| parse_loose_numbered_heading_ordinal(mat.as_str()))
        .collect::<Vec<_>>();
    if ordinals.len() != matches.len() || ordinals.first().is_none_or(|value| *value > 3) {
        return false;
    }
    if !loose_numbered_ordinals_are_plausible(&ordinals) {
        return false;
    }
    loose_numbered_heading_bodies_are_plausible(text, matches)
}

pub(crate) fn loose_numbered_ordinals_are_plausible(ordinals: &[u64]) -> bool {
    let Some(first) = ordinals.first().copied() else {
        return false;
    };
    if first > 3 {
        return false;
    }

    let allowed_glitches = (ordinals.len() / 20).max(2);
    let allowed_resets = (ordinals.len() / 20).max(3);
    let mut glitches = 0usize;
    let mut resets = 0usize;
    let mut expected = first;
    for ordinal in ordinals {
        if *ordinal == expected {
            expected += 1;
        } else if *ordinal <= 3 && expected > 10 {
            resets += 1;
            if resets > allowed_resets {
                return false;
            }
            expected = *ordinal + 1;
        } else if *ordinal < expected {
            glitches += 1;
            if glitches > allowed_glitches {
                return false;
            }
        } else if *ordinal == expected + 1 {
            glitches += 1;
            if glitches > allowed_glitches {
                return false;
            }
            expected = *ordinal + 1;
        } else {
            return false;
        }
    }
    true
}

pub(crate) fn is_plausible_loose_numbered_heading_line(line: &str) -> bool {
    let Some(title) = loose_numbered_heading_title(line) else {
        return false;
    };
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 40 {
        return false;
    }
    if loose_numbered_heading_looks_like_date_marker(line, title) {
        return false;
    }
    if loose_numbered_title_looks_like_body_sentence(line, title) {
        return false;
    }
    if ["列表", "列表项", "选项", "步骤", "序号"]
        .iter()
        .any(|word| title.contains(word))
    {
        return false;
    }
    let meaningful = title
        .chars()
        .filter(|ch| ch.is_alphanumeric() || is_cjk_char(*ch))
        .count();
    if meaningful < 2 {
        return false;
    }
    let symbol_count = title
        .chars()
        .filter(|ch| {
            !ch.is_alphanumeric()
                && !is_cjk_char(*ch)
                && !ch.is_whitespace()
                && *ch != '\u{feff}'
                && *ch != '　'
        })
        .count();
    symbol_count * 2 <= title.chars().count()
}

pub(crate) fn loose_numbered_title_looks_like_body_sentence(line: &str, title: &str) -> bool {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let starts_with_chinese_ordinal = trimmed.chars().next().is_some_and(|ch| {
        "零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O".contains(ch)
    });
    if starts_with_chinese_ordinal
        && title
            .chars()
            .next()
            .is_some_and(|ch| "零〇一二两三四五六七八九十百千万".contains(ch))
        && title.contains('岁')
    {
        return true;
    }

    let punctuation_count = title
        .chars()
        .filter(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'))
        .count();
    punctuation_count >= 2 && title.chars().count() > 24
}

pub(crate) fn loose_numbered_heading_looks_like_date_marker(line: &str, title: &str) -> bool {
    let Some(ordinal) = parse_loose_numbered_heading_ordinal(line) else {
        return false;
    };
    if !(1900..=2099).contains(&ordinal) {
        return false;
    }

    let title = title.trim();
    static DATE_TAIL_RE: OnceLock<Regex> = OnceLock::new();
    let date_tail_re = DATE_TAIL_RE.get_or_init(|| {
        Regex::new(r#"^[0-9０-９]{1,2}(?:[.．/\-—年月日]|$)"#).expect("valid date tail regex")
    });
    date_tail_re.is_match(title)
}

pub(crate) fn loose_numbered_heading_bodies_are_plausible(
    text: &str,
    matches: &[regex::Match<'_>],
) -> bool {
    let body_lengths = matches
        .iter()
        .enumerate()
        .map(|(idx, mat)| {
            let start = mat.end();
            let end = matches.get(idx + 1).map_or(text.len(), |next| next.start());
            text[start..end]
                .trim()
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .count()
        })
        .collect::<Vec<_>>();
    if body_lengths.iter().any(|len| *len < 20) {
        return false;
    }
    let total = body_lengths.iter().sum::<usize>();
    total / body_lengths.len() >= 20
}

pub(crate) fn loose_numbered_heading_title(line: &str) -> Option<&str> {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let ordinal_re = loose_numbered_heading_ordinal_prefix_regex();
    let mat = ordinal_re.find(trimmed)?;
    Some(trimmed[mat.end()..].trim())
}

pub(crate) fn loose_numbered_heading_ordinal_prefix_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(
        r#"^[（(]?[ \t　]*(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*[）)]?[ \t　]*(?:[、.．:：\-—_·|]|[ \t　]+)"#,
    )
    .expect("valid loose numbered heading ordinal prefix regex"))
}

pub(crate) fn parse_loose_numbered_heading_ordinal(line: &str) -> Option<u64> {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    static ORDINAL_RE: OnceLock<Regex> = OnceLock::new();
    let ordinal_re = ORDINAL_RE.get_or_init(|| Regex::new(
        r#"^[（(]?[ \t　]*([0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})"#,
    )
    .expect("valid loose numbered heading ordinal parser regex"));
    let token = ordinal_re.captures(trimmed)?.get(1)?.as_str();
    parse_fullwidth_digits(token).or_else(|| parse_chinese_ordinal(token))
}

pub(crate) fn is_cjk_char(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9fff | 0xf900..=0xfaff)
}

pub(crate) fn parse_fullwidth_digits(token: &str) -> Option<u64> {
    let mut normalized = String::new();
    for ch in token.chars() {
        let digit = match ch {
            '0'..='9' => ch,
            '０' => '0',
            '１' => '1',
            '２' => '2',
            '３' => '3',
            '４' => '4',
            '５' => '5',
            '６' => '6',
            '７' => '7',
            '８' => '8',
            '９' => '9',
            _ => return None,
        };
        normalized.push(digit);
    }
    normalized.parse::<u64>().ok()
}

pub(crate) fn parse_chinese_ordinal(token: &str) -> Option<u64> {
    let mut total = 0;
    let mut section = 0;
    let mut number = 0;
    let mut seen = false;
    for ch in token.chars() {
        if let Some(value) = chinese_ordinal_digit(ch) {
            number = value;
            seen = true;
        } else if let Some(unit) = chinese_ordinal_unit(ch) {
            seen = true;
            if unit == 10_000 {
                section = (section + number.max(1)) * unit;
                total += section;
                section = 0;
            } else {
                section += number.max(1) * unit;
            }
            number = 0;
        } else {
            return None;
        }
    }
    if !seen {
        return None;
    }
    let value = total + section + number;
    (value > 0).then_some(value)
}

pub(crate) fn chinese_ordinal_digit(ch: char) -> Option<u64> {
    match ch {
        '零' | '〇' | 'O' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '贰' | '两' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

pub(crate) fn chinese_ordinal_unit(ch: char) -> Option<u64> {
    match ch {
        '十' | '拾' => Some(10),
        '百' | '佰' => Some(100),
        '千' | '仟' => Some(1000),
        '万' | '萬' => Some(10_000),
        _ => None,
    }
}

pub(crate) fn chunk_without_headings(novel_id: &str, text: &str) -> Vec<Chapter> {
    let chars = text.chars().collect::<Vec<_>>();
    let chunk_size = 100_000;
    chars
        .chunks(chunk_size)
        .enumerate()
        .map(|(idx, chunk)| Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title: format!("自动分段 {}", idx + 1),
            original_text: chunk.iter().collect::<String>().trim().to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        })
        .collect()
}
