use crate::domain::{Chapter, SplitResult};
use regex::Regex;
use uuid::Uuid;

pub(crate) fn split_chapters(novel_id: &str, text: &str) -> SplitResult {
    let matches = chapter_heading_matches(text);
    if matches.is_empty() {
        return SplitResult {
            chapters: chunk_without_headings(novel_id, text),
            detected_chapters: false,
        };
    }

    let mut chapters = Vec::new();
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
        chapters.push(Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title,
            original_text,
            analysis_json: None,
            rewrite_text: None,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        });
    }
    SplitResult {
        chapters,
        detected_chapters: true,
    }
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

pub(crate) fn chapter_heading_regex() -> Regex {
    Regex::new(
        r#"(?m)^[\s\u{feff}　]*(?:={2,6}[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][^=\r\n]{0,80}={2,6}|={2,6}[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[^=\r\n]{0,80}={2,6}|[【〔［「『《（(\[]?[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:卷|篇|部|章|回|幕|册|节)[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[上中下前后终外][ \t　]*(?:卷|篇|部|章|册)[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:Chapter|CHAPTER|chapter|Chap\.?|CH\.?|ch\.?|Section|SECTION|section|Part|PART|part|Episode|EPISODE|episode|No\.?|NO\.?|no\.?)[ \t　]*[0-9０-９IVXLCDMivxlcdm]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[【〔［「『《（(\[]?[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80})[\t 　]*\r?$"#,
    )
    .expect("valid chapter regex")
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

pub(crate) fn starts_with_inline_round_phrase(compact: &str) -> bool {
    let round_re = Regex::new(
        r#"^(?:第)?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+回合"#,
    )
    .expect("valid round phrase regex");
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
    let numbered_re = Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+([章节回集话幕节页季段夜案场弹折更])(.+)$"#,
    )
    .expect("valid strict numbered heading parser regex");
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
    let numbered_re = Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[章节回集话幕节页季段夜案场弹折更]"#,
    )
    .expect("valid numbered strict chapter regex");
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
    let container_re = Regex::new(
        r#"^(?:第?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[卷部]|[卷部][0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+|[上中下前后终外][卷部])"#,
    )
    .expect("valid loose container heading regex");
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

pub(crate) fn loose_numbered_chapter_heading_regex() -> Regex {
    Regex::new(
        r#"(?m)^[ \t\u{feff}　]*(?:[（(]?[ \t　]*)?(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*(?:[）)][ \t　]*)?(?:[ \t　]+|[、.．:：\-—_·|][ \t　]*)[^\r\n]{1,60}[ \t　]*\r?$"#,
    )
    .expect("valid loose numbered chapter regex")
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
    let date_tail_re =
        Regex::new(r#"^[0-9０-９]{1,2}(?:[.．/\-—年月日]|$)"#).expect("valid date tail regex");
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

pub(crate) fn loose_numbered_heading_ordinal_prefix_regex() -> Regex {
    Regex::new(
        r#"^[（(]?[ \t　]*(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*[）)]?[ \t　]*(?:[、.．:：\-—_·|]|[ \t　]+)"#,
    )
    .expect("valid loose numbered heading ordinal prefix regex")
}

pub(crate) fn parse_loose_numbered_heading_ordinal(line: &str) -> Option<u64> {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let ordinal_re = Regex::new(
        r#"^[（(]?[ \t　]*([0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})"#,
    )
    .expect("valid loose numbered heading ordinal parser regex");
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
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        })
        .collect()
}
