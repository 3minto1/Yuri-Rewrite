use super::rules::{render_rewrite_hard_rules, CLEANUP_RULE, DYNAMIC_CONTEXT_MARKER};
use crate::domain::{CanonAsset, Chapter, NovelSettings, ParsedChapterRewrite};
use crate::{additional_feminize_name_sources, relationship_targets_summary, truncate_text};
use std::collections::HashSet;

pub(crate) fn build_analysis_identity_context(settings: &NovelSettings) -> String {
    let alias_sources = additional_feminize_name_sources(&settings.protagonist_aliases);
    if alias_sources.is_empty() {
        return String::new();
    }
    format!(
        "已知原文人物身份提示（仅用于识别同一人物，不代表改写要求）：主角“{}”在原文中还可能以这些姓名或别名出现：{}。分析时应把这些称呼归属于同一人物，并记录原文实际使用方式；不得据此改变姓名、性别、关系或剧情。",
        settings.protagonist_name.trim(),
        alias_sources.join("、")
    )
}

fn aliases_or_none(settings: &NovelSettings) -> String {
    if settings.protagonist_aliases.trim().is_empty() {
        "无".to_string()
    } else {
        settings
            .protagonist_aliases
            .lines()
            .collect::<Vec<_>>()
            .join("、")
    }
}

pub(crate) fn format_batch_label(chapters: &[Chapter]) -> String {
    match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => format!("第{}章", first.index),
        (Some(first), Some(last)) => format!("第{}-{}章", first.index, last.index),
        _ => "空批次".to_string(),
    }
}

pub(crate) fn build_compact_canon_text(assets: &[CanonAsset]) -> String {
    if assets.is_empty() {
        return "无".to_string();
    }

    let compacted = sorted_canon_assets(assets)
        .into_iter()
        .filter_map(|asset| {
            let content = compact_canon_asset_content(&asset.kind, &asset.content);
            if content.trim().is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", asset.kind, content))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if compacted.trim().is_empty() {
        "无".to_string()
    } else {
        compacted
    }
}

pub(crate) fn build_relevant_canon_text(
    assets: &[CanonAsset],
    chapters: &[Chapter],
    settings: &NovelSettings,
) -> String {
    if assets.is_empty() {
        return "无".to_string();
    }

    let mut keywords = relevant_canon_keywords(chapters, settings);
    for asset in assets {
        if asset.kind == "姓名映射表" {
            collect_mapping_keywords(
                &sanitize_name_mapping_for_prompt(&asset.content),
                &mut keywords,
            );
        }
    }

    let selected = sorted_canon_assets(assets)
        .into_iter()
        .filter_map(|asset| {
            let content = select_relevant_canon_content(asset, &keywords, settings);
            if content.trim().is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", asset.kind, content))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    if selected.trim().is_empty() {
        build_compact_canon_text(assets)
    } else {
        selected
    }
}

fn sorted_canon_assets(assets: &[CanonAsset]) -> Vec<&CanonAsset> {
    let mut sorted = assets.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        canon_asset_sort_key(&left.kind).cmp(&canon_asset_sort_key(&right.kind))
    });
    sorted
}

fn canon_asset_sort_key(kind: &str) -> (usize, &str) {
    let rank = match kind {
        "姓名映射表" => 0,
        "AI分析汇总" => 1,
        "人物卡" => 2,
        "人物关系" => 3,
        "地点" => 4,
        "术语表" => 5,
        "伏笔" => 6,
        _ => 100,
    };
    (rank, kind)
}

pub(crate) fn build_relevant_canon_text_from_text(
    canon_text: &str,
    chapters: &[Chapter],
    settings: &NovelSettings,
) -> String {
    if canon_text.trim().is_empty() || canon_text.trim() == "无" {
        return "无".to_string();
    }
    let assets = parse_compact_canon_assets(canon_text);
    if assets.is_empty() {
        truncate_text(canon_text, 8_000)
    } else {
        build_relevant_canon_text(&assets, chapters, settings)
    }
}

pub(crate) fn compact_canon_asset_content(kind: &str, content: &str) -> String {
    let sanitized = sanitize_canon_asset_content(kind, content);
    let normalized = sanitized.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    let deduped = lines.join("\n");
    let max_chars = canon_asset_char_limit(kind);
    if deduped.chars().count() <= max_chars {
        return deduped;
    }

    let head_limit = max_chars / 2;
    let tail_limit = max_chars.saturating_sub(head_limit);
    format!(
        "{}\n\n[一致性资产已压缩：省略中间重复或历史内容]\n\n{}",
        take_chars(&deduped, head_limit),
        take_last_chars(&deduped, tail_limit)
    )
}

fn sanitize_canon_asset_content(kind: &str, content: &str) -> String {
    match kind {
        "姓名映射表" => sanitize_name_mapping_for_prompt(content),
        "AI分析汇总" => sanitize_analysis_summary_for_prompt(content),
        "术语表" => sanitize_terms_for_prompt(content),
        _ => content.to_string(),
    }
}

fn sanitize_name_mapping_for_prompt(content: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(content) else {
        return content.to_string();
    };
    if let Some(object) = value.as_object_mut() {
        let legacy_sources = object
            .get("legacy_unmanaged_sources")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect::<HashSet<_>>();
        if let Some(names) = object
            .get_mut("names")
            .and_then(serde_json::Value::as_array_mut)
        {
            names.retain(|entry| {
                entry
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|source| !legacy_sources.contains(source))
            });
        }
        let remove_protagonist = object
            .get("protagonist")
            .and_then(|entry| entry.get("source"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|source| legacy_sources.contains(source));
        if remove_protagonist {
            object.insert("protagonist".to_string(), serde_json::Value::Null);
        }
        object.remove("version");
        object.remove("managed_sources");
        object.remove("legacy_unmanaged_sources");
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| content.to_string())
}

fn sanitize_analysis_summary_for_prompt(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = Vec::new();
    let mut header: Option<String> = None;
    let mut body = Vec::new();
    let flush = |header: &mut Option<String>, body: &mut Vec<String>, output: &mut Vec<String>| {
        let Some(header) = header.take() else {
            return;
        };
        let joined = body.join("\n");
        body.clear();
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&joined) {
            if let Some(object) = value.as_object_mut() {
                object.remove("name_feminization_map");
                object.remove("rewrite_notes");
            }
            output.push(header);
            output
                .push(serde_json::to_string_pretty(&value).unwrap_or_else(|_| joined.to_string()));
        } else {
            output.push(header);
            output.push(joined);
        }
    };
    for line in normalized.lines() {
        if line.trim_start().starts_with("## ") {
            flush(&mut header, &mut body, &mut output);
            header = Some(line.trim().to_string());
        } else if header.is_some() {
            body.push(line.to_string());
        } else {
            output.push(line.to_string());
        }
    }
    flush(&mut header, &mut body, &mut output);
    output
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_terms_for_prompt(content: &str) -> String {
    let mut output = Vec::new();
    let mut skip_deprecated = false;
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    for line in normalized.lines() {
        let trimmed = line.trim();
        if matches!(trimmed, "姓名女性化映射：" | "改写注意事项：") {
            skip_deprecated = true;
            continue;
        }
        if trimmed.starts_with("## ") || matches!(trimmed, "原文术语：" | "原文姓名与称谓：")
        {
            skip_deprecated = false;
        }
        if !skip_deprecated {
            output.push(line);
        }
    }
    output.join("\n")
}

fn parse_compact_canon_assets(canon_text: &str) -> Vec<CanonAsset> {
    let mut assets = Vec::new();
    let mut current_kind: Option<String> = None;
    let mut current_lines = Vec::new();
    let flush =
        |kind: &mut Option<String>, lines: &mut Vec<String>, assets: &mut Vec<CanonAsset>| {
            if let Some(kind) = kind.take() {
                let content = lines.join("\n");
                if !content.trim().is_empty() {
                    assets.push(CanonAsset {
                        novel_id: String::new(),
                        kind,
                        content,
                        updated_at: String::new(),
                    });
                }
            }
            lines.clear();
        };

    for line in canon_text.lines() {
        if let Some(kind) = line.trim().strip_prefix("## ") {
            flush(&mut current_kind, &mut current_lines, &mut assets);
            current_kind = Some(kind.trim().to_string());
        } else {
            current_lines.push(line.to_string());
        }
    }
    flush(&mut current_kind, &mut current_lines, &mut assets);
    assets
}

fn relevant_canon_keywords(chapters: &[Chapter], settings: &NovelSettings) -> HashSet<String> {
    let mut keywords = HashSet::new();
    for value in [
        settings.protagonist_name.as_str(),
        settings.rewritten_protagonist_name.as_str(),
    ] {
        insert_keyword(&mut keywords, value);
    }
    for value in additional_feminize_name_sources(&settings.protagonist_aliases) {
        insert_keyword(&mut keywords, &value);
    }
    for value in additional_feminize_name_sources(&settings.additional_feminize_names) {
        insert_keyword(&mut keywords, &value);
    }
    for chapter in chapters {
        collect_text_keywords(&chapter.title, &mut keywords);
        collect_text_keywords(&chapter.original_text, &mut keywords);
        if let Some(rewrite_text) = chapter.rewrite_text.as_deref() {
            collect_text_keywords(rewrite_text, &mut keywords);
        }
    }
    keywords
}

fn collect_mapping_keywords(content: &str, keywords: &mut HashSet<String>) {
    for line in content.lines() {
        let trimmed = line.trim();
        for separator in ["->", "=>", "→", "：", ":", "\"source\"", "\"target\""] {
            if trimmed.contains(separator) {
                collect_text_keywords(trimmed, keywords);
                break;
            }
        }
    }
}

fn collect_text_keywords(text: &str, keywords: &mut HashSet<String>) {
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            current.push(ch);
        } else {
            insert_keyword(keywords, &current);
            current.clear();
        }
    }
    insert_keyword(keywords, &current);
}

fn insert_keyword(keywords: &mut HashSet<String>, value: &str) {
    let trimmed = value.trim();
    let len = trimmed.chars().count();
    if (2..=20).contains(&len) {
        keywords.insert(trimmed.to_string());
    }
}

fn select_relevant_canon_content(
    asset: &CanonAsset,
    keywords: &HashSet<String>,
    settings: &NovelSettings,
) -> String {
    let kind = asset.kind.as_str();
    if matches!(kind, "姓名映射表" | "AI分析汇总") {
        return compact_canon_asset_content(kind, &asset.content);
    }

    let normalized = asset.content.replace("\r\n", "\n").replace('\r', "\n");
    let has_section_headers = normalized
        .lines()
        .any(|line| line.trim_start().starts_with("## "));
    let sections = split_canon_sections(&normalized);
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for section in sections {
        if is_core_canon_section(&section, settings) || section_matches_keywords(&section, keywords)
        {
            let compact = compact_canon_asset_content(kind, &section);
            let key = normalize_for_dedup(&compact);
            if !compact.trim().is_empty() && seen.insert(key) {
                selected.push(compact);
            }
        }
    }

    if selected.is_empty() && !has_section_headers {
        compact_canon_asset_content(kind, &asset.content)
    } else if selected.is_empty() {
        String::new()
    } else {
        compact_canon_asset_content(kind, &selected.join("\n\n"))
    }
}

fn split_canon_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with("## ") && !current.is_empty() {
            sections.push(current.join("\n"));
            current.clear();
        }
        current.push(line.to_string());
    }
    if !current.is_empty() {
        sections.push(current.join("\n"));
    }
    if sections.is_empty() && !content.trim().is_empty() {
        sections.push(content.trim().to_string());
    }
    sections
}

fn is_core_canon_section(section: &str, settings: &NovelSettings) -> bool {
    let protagonist = settings.protagonist_name.trim();
    let rewritten = settings.rewritten_protagonist_name.trim();
    (!protagonist.is_empty() && section.contains(protagonist))
        || (!rewritten.is_empty() && section.contains(rewritten))
        || settings
            .protagonist_aliases
            .lines()
            .any(|alias| !alias.trim().is_empty() && section.contains(alias.trim()))
}

fn section_matches_keywords(section: &str, keywords: &HashSet<String>) -> bool {
    keywords
        .iter()
        .filter(|keyword| keyword.chars().count() >= 2)
        .any(|keyword| section.contains(keyword))
}

fn normalize_for_dedup(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | '.' | '；' | ';'))
        .collect()
}

pub(crate) fn canon_asset_char_limit(kind: &str) -> usize {
    match kind {
        "姓名映射表" => 12_000,
        "AI分析汇总" => 4_000,
        "人物卡" | "人物关系" => 6_000,
        "伏笔" | "术语表" => 5_000,
        "地点" => 3_000,
        _ => 3_000,
    }
}

pub(crate) fn take_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

pub(crate) fn take_last_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

pub(crate) fn build_compact_rewrite_rule_pack(settings: &NovelSettings) -> String {
    let mut setting_lines = vec![
        format!("- 主角原姓名：{}", settings.protagonist_name.trim()),
        format!(
            "- 主角改写后姓名：{}",
            if settings.rewritten_protagonist_name.trim().is_empty() {
                "未指定，按姓名映射表或同音/近音规则生成并保持一致"
            } else {
                settings.rewritten_protagonist_name.trim()
            }
        ),
        format!(
            "- 身材/体型/模式：{} / {} / {}",
            settings.bust.trim(),
            settings.body_type.trim(),
            rewrite_mode_label(&settings.rewrite_mode)
        ),
    ];

    if !settings.protagonist_aliases.trim().is_empty() {
        setting_lines.push(format!(
            "- 主角原文别名/指定别名映射：{}；这些别名与主角是同一人物，按姓名映射同步女性化；`原别名 -> 改写后别名` 必须逐字使用 target。",
            aliases_or_none(settings)
        ));
    }
    if !settings.additional_feminize_names.trim().is_empty() {
        setting_lines.push(format!(
            "- 其他指定女性化人物/姓名映射：{}；`原姓名 -> 改写后姓名` 必须逐字使用 target，只填原名时才由 AI 女性化。",
            settings.additional_feminize_names.trim()
        ));
    }
    let relationship_targets = relationship_targets_summary(&settings.relationship_targets);
    if relationship_targets != "无" {
        setting_lines.push(format!(
            "- 重点百合互动对象：{}；只维护关系连续性和百合互动，不得改变未指定角色性别、原文主线逻辑或章节边界。",
            relationship_targets
        ));
    }
    if !settings.advanced_settings.trim().is_empty() {
        setting_lines.push(format!("- 高级设定：{}", settings.advanced_settings.trim()));
    }

    let mode_rule = match settings.rewrite_mode.as_str() {
        "creative" => "模式规则：创意模式，可在不破坏主线、战力、伏笔和人物动机时强化女性外貌、身形仪态、旁人态度、同性亲密感和百合互动；每章在关键场景自然增加或强化 2-4 处女性化感知点，不能堆砌。",
        _ => "模式规则：严谨模式，忠于原文，不做过大改动；但原文男主相关男性化内容必须自然转换，必要女性化描写不能减少。",
    };

    format!(
        r#"【改写规则包】
{}

硬性规则：
{}
8. {}
9. {}"#,
        setting_lines.join("\n"),
        render_rewrite_hard_rules(),
        CLEANUP_RULE,
        mode_rule
    )
}

pub(crate) fn rewrite_mode_label(mode: &str) -> &'static str {
    match mode {
        "creative" => "创意模式",
        _ => "严谨模式",
    }
}

pub(crate) fn analysis_chapter_start_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_ANALYSIS_CHAPTER_START index={} id={}>>>",
        chapter.index, chapter.id
    )
}

pub(crate) fn analysis_chapter_end_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_ANALYSIS_CHAPTER_END index={} id={}>>>",
        chapter.index, chapter.id
    )
}

pub(crate) fn chapter_start_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_REWRITE_CHAPTER_START index={} id={}>>>",
        chapter.index, chapter.id
    )
}

pub(crate) fn chapter_end_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_REWRITE_CHAPTER_END index={} id={}>>>",
        chapter.index, chapter.id
    )
}

/// High-confidence ad / author-note lines that are safe to remove from the
/// rewrite input before generation. Deliberately narrower than the full
/// droppable-line classifier: blank lines and soft update notices stay, so
/// story text can never be pre-stripped by mistake.
pub(crate) fn is_high_confidence_ad_line(line: &str) -> bool {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '\u{3000}');
    if trimmed.is_empty() {
        return false;
    }
    let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
    if matches!(
        compact.as_str(),
        "作者的话" | "作者有话说" | "作者附言" | "题外话" | "PS" | "P.S."
    ) {
        return true;
    }
    if [
        "求月票",
        "求推荐票",
        "求收藏",
        "求订阅",
        "求追读",
        "求点击",
        "求鲜花",
        "求评价票",
        "大家投票",
        "投月票",
        "投推荐票",
        "感谢打赏",
        "谢谢打赏",
        "打赏加更",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
    {
        return true;
    }
    compact.contains("http://") || compact.contains("https://") || compact.contains("www.")
}

pub(crate) fn strip_high_confidence_ad_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !is_high_confidence_ad_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Chapter input for the rewrite prompt: obvious ad lines are removed here so
/// the model never sees them and cannot forget (or refuse) to remove them.
pub(crate) fn build_batch_rewrite_input_text(chapters: &[Chapter]) -> String {
    chapters
        .iter()
        .map(|chapter| {
            let text = strip_high_confidence_ad_lines(&chapter.original_text);
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                chapter_start_marker(chapter),
                chapter.title,
                text.trim(),
                chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn explicit_name_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (source, target) = line.split_once("->")?;
            let source = source.trim();
            let target = target.trim();
            if source.is_empty() || target.is_empty() || source == target {
                return None;
            }
            Some((source.to_string(), target.to_string()))
        })
        .collect()
}

fn character_baseline_lines(settings: &NovelSettings, covered: &mut HashSet<String>) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let protagonist = settings.protagonist_name.trim();
    let rewritten = settings.rewritten_protagonist_name.trim();
    if !protagonist.is_empty() {
        if rewritten.is_empty() {
            lines.push(format!(
                "- {protagonist}：主角，必须彻底女性化（改写名按姓名映射或同音/近音规则生成并保持一致）"
            ));
        } else {
            lines.push(format!(
                "- {protagonist} -> {rewritten}：主角，必须彻底女性化并逐字使用改写名"
            ));
        }
        covered.insert(protagonist.to_string());
        if !rewritten.is_empty() {
            covered.insert(rewritten.to_string());
        }
    }
    for alias in additional_feminize_name_sources(&settings.protagonist_aliases) {
        let alias = alias.trim();
        if !alias.is_empty() {
            lines.push(format!(
                "- {alias}：主角原文别名/指定别名，与主角是同一人物，按姓名映射同步女性化"
            ));
            covered.insert(alias.to_string());
        }
    }
    for (source, target) in explicit_name_pairs(&settings.additional_feminize_names) {
        lines.push(format!(
            "- {source} -> {target}：姓名映射，必须逐字使用 {target} 并彻底女性化"
        ));
        covered.insert(source);
        covered.insert(target);
    }
    lines
}

const ROSTER_KEEP_REST_LINE: &str = "- 其余人物：一律保持原文性别、身份、称谓与代词（详见人物卡）；性别不明者保持中性；非人生物保留原文代词；群体代词按成员构成判断";

/// Deterministic gender baseline for the chapter prompt, built from the
/// protagonist and the user-configured name mappings. Conversion targets are
/// listed explicitly so pronoun handling becomes a lookup instead of reasoning.
/// Full gender baseline: the settings anchors above, plus every structured
/// 人物卡 entry whose name appears in the input chapters. Non-target
/// characters become explicit "keep original gender" rows, so pronoun
/// handling for the whole cast is a lookup instead of reasoning.
pub(crate) fn build_full_character_roster(
    canon_text: &str,
    chapters: &[Chapter],
    settings: &NovelSettings,
) -> String {
    let mut covered = HashSet::new();
    let mut lines = character_baseline_lines(settings, &mut covered);
    let chapter_text = chapters
        .iter()
        .map(|chapter| chapter.original_text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for asset in parse_compact_canon_assets(canon_text) {
        if asset.kind == "姓名映射表" {
            for (source, target) in explicit_name_pairs(&asset.content) {
                if covered.insert(target.clone()) {
                    lines.push(format!(
                        "- {source} -> {target}：姓名映射，必须逐字使用 {target} 并彻底女性化"
                    ));
                    covered.insert(source);
                }
            }
        }
    }

    for asset in parse_compact_canon_assets(canon_text) {
        if asset.kind != "人物卡" {
            continue;
        }
        for line in asset.content.lines() {
            let line = line.trim();
            if line.is_empty() || !(line.contains('|') || line.contains('｜')) {
                // Legacy prose character cards have no structured fields; skip
                // them instead of guessing (backward compatible).
                continue;
            }
            let parts: Vec<&str> = line.split(['|', '｜']).map(str::trim).collect();
            let Some(name) = parts
                .first()
                .copied()
                .filter(|name| !name.is_empty() && name.chars().count() <= 12)
            else {
                continue;
            };
            if covered.contains(name) || !chapter_text.contains(name) {
                continue;
            }
            let gender = parts.iter().skip(1).find_map(|part| {
                let part = part.trim();
                let part = part.strip_prefix("性别").map(str::trim).unwrap_or(part);
                let part = part.trim_start_matches([':', '：']).trim();
                match part {
                    "男" => Some("男"),
                    "女" => Some("女"),
                    "未知" => Some("未知"),
                    "非人" => Some("非人"),
                    _ => None,
                }
            });
            let Some(gender) = gender else {
                continue;
            };
            let keep = match gender {
                "男" => "保持原文男性，不得女性化",
                "女" => "保持原文女性",
                "未知" => "性别未确认，保持中性称呼",
                _ => "非人生物，保持原文代词和称谓",
            };
            covered.insert(name.to_string());
            lines.push(format!("- {name}：{keep}（人物卡）"));
        }
    }

    if lines.is_empty() {
        return String::new();
    }
    lines.push(ROSTER_KEEP_REST_LINE.to_string());
    format!(
        "【人物性别基准表】所有代词、称谓和性别表达必须按此表执行：\n{}",
        lines.join("\n")
    )
}

pub(crate) fn build_batch_chapter_text(chapters: &[Chapter], use_rewrite_text: bool) -> String {
    chapters
        .iter()
        .map(|chapter| {
            let text = if use_rewrite_text {
                chapter
                    .rewrite_text
                    .as_deref()
                    .unwrap_or(&chapter.original_text)
            } else {
                &chapter.original_text
            };
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                chapter_start_marker(chapter),
                chapter.title,
                text.trim(),
                chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn build_batch_analysis_chapter_text(chapters: &[Chapter]) -> String {
    chapters
        .iter()
        .map(|chapter| {
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                analysis_chapter_start_marker(chapter),
                chapter.title,
                chapter.original_text.trim(),
                analysis_chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn build_batch_rewrite_text(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
) -> String {
    chapters
        .iter()
        .zip(rewrites.iter())
        .map(|(chapter, rewrite)| {
            debug_assert_eq!(chapter.id, rewrite.id);
            debug_assert_eq!(chapter.index, rewrite.index);
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                chapter_start_marker(chapter),
                rewrite.title,
                rewrite.text.trim(),
                chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[allow(dead_code)]
pub(crate) fn build_batch_rewrite_prompt_with_settings(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
) -> String {
    build_batch_rewrite_prompt_with_context(chapters, canon_text, settings, "", "")
}

pub(crate) fn build_core_prompt_section(core_prompt: &str) -> String {
    let core_prompt = core_prompt.trim();
    if core_prompt.is_empty() {
        return "最高优先级核心设定：无。".to_string();
    }
    format!(
        "最高优先级核心设定：\n以下内容是用户设置的全局写作规则，优先级高于本次改写中的其他风格、描写、节奏和表达要求。必须在不破坏章节边界、姓名映射、角色性别规则、原文主线和逻辑的前提下，始终按这些文风、描写方式、语气、节奏和其他全局要求改写。\n{}",
        core_prompt
    )
}

pub(crate) fn rewrite_marker_format_guard(scope: &str) -> String {
    format!(
        r#"【输出格式硬性要求】
- 只输出{scope}的改写结果，不要输出解释、总结、Markdown、代码块、审查意见或额外章节。
- 每章必须完整复制输入中的 START marker 和 END marker。
- 不得修改 marker 中的 index 和 id，不得省略 START/END，不得自行生成新 marker。
- 每章输出结构必须是：
<<<YURI_REWRITE_CHAPTER_START index=原样复制 id=原样复制>>>
标题：改写后标题
正文：
改写后正文
<<<YURI_REWRITE_CHAPTER_END index=原样复制 id=原样复制>>>"#
    )
}

pub(crate) fn rewrite_marker_final_reminder(scope: &str) -> String {
    format!(
        "再次确认：只输出{scope}的结果；每章 START/END marker 必须逐字复制；不要输出任何解释、Markdown、额外章节或缺失章节。"
    )
}

pub(crate) fn build_rewrite_priority_prompt() -> &'static str {
    r#"【规则优先级】
当规则之间出现冲突时，必须按以下顺序判断：
1. 章节 START/END marker、输出范围和非空正文格式要求最高，任何情况下不得破坏。
2. 用户填写的最高优先级核心设定优先于普通文风和润色要求，但不得破坏 marker、姓名映射、未指定角色性别保持和原文关键逻辑。
3. 用户指定的主角改写姓名、一致性资产中的“姓名映射表”和已建立姓名映射优先于模型自行生成的新名字。
4. 未指定角色必须保持原文性别、身份、称谓和群体代词判断；不得为了百合化或创意模式把配角、长辈、父亲、兄弟、敌人、旁观者或性别不明非人生物强行女性化。
5. 原文主线、章节顺序、因果、战力、伏笔和人物动机优先于新增女性化细节。
6. 创意模式只允许在不违反以上规则时强化女性化感知和百合向互动。
7. 文风润色、错别字修正、广告/乱码清理优先级最低，只能做保守处理，不得删除或改写剧情正文和设定信息。"#
}

pub(crate) fn cleanup_text_rule() -> &'static str {
    CLEANUP_RULE
}

pub(crate) struct BatchRewritePromptParts {
    pub(crate) static_prefix: String,
    pub(crate) dynamic_suffix: String,
}

pub(crate) fn build_batch_rewrite_prompt_parts(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
) -> BatchRewritePromptParts {
    let shard_context = prompt_context_or_none(shard_context);
    let static_prefix = format!(
        r#"{}

{}

{}

{}

{}

改写要求：
1. 将原本男女性别叙事自然改写为双女主百合叙事；当前输入中的所有章节必须一次性完整改写。
2. 采用中度再创作：保留主线、冲突、章节顺序、战力逻辑、人物动机和关键伏笔；只在【改写规则包】允许范围内调整互动、细节、称谓、外貌和关系推进。
3. 严格遵守【规则优先级】、【改写规则包】、【人物性别基准表】和一致性资产；如有冲突，按规则优先级处理。
4. 每章必须复制输入中对应的 START/END marker，index 和 id 逐字保留；只输出当前输入章节的 marker、标题和正文，不要解释、Markdown 或额外章节。"#,
        rewrite_marker_format_guard("当前输入章节"),
        build_rewrite_priority_prompt(),
        build_core_prompt_section(core_prompt),
        build_compact_rewrite_rule_pack(settings),
        build_full_character_roster(canon_text, chapters, settings),
    );
    let dynamic_suffix = format!(
        r#"一致性资产：
{}

处理范围约束：
{}

当前输入章节：
{}

{}"#,
        canon_text,
        shard_context,
        build_batch_rewrite_input_text(chapters),
        rewrite_marker_final_reminder("当前输入章节")
    );
    BatchRewritePromptParts {
        static_prefix,
        dynamic_suffix,
    }
}

pub(crate) fn build_batch_rewrite_prompt_with_context(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
) -> String {
    let parts = build_batch_rewrite_prompt_parts(chapters, canon_text, settings, core_prompt, shard_context);
    format!(
        "{}\n\n{}\n\n{}",
        parts.static_prefix, DYNAMIC_CONTEXT_MARKER, parts.dynamic_suffix
    )
}

pub(crate) fn build_single_chapter_rewrite_from_draft_prompt(
    chapter: &Chapter,
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    adjacent_context: &str,
    instructions: &str,
) -> String {
    let instructions = if instructions.trim().is_empty() {
        "无额外要求；在保持当前改写稿整体写法和内容的基础上进行必要优化。"
    } else {
        instructions.trim()
    };
    let roster = prompt_context_or_none(&build_full_character_roster(
        canon_text,
        std::slice::from_ref(chapter),
        settings,
    ));
    format!(
        r#"{}

{}

{}

{}

{}

任务说明：
1. 当前改写稿是本次修改的主要底稿。必须在当前改写稿基础上依照用户要求修改，不能抛弃现稿、退回原文重新生成，也不能无理由重写整体结构和文风。
2. 原文仅用于核对事实、人物、事件顺序、伏笔和剧情逻辑；原文与当前改写稿表达不同时，除非当前改写稿明显违背事实或用户明确要求，否则应保留当前改写稿的处理。
3. 默认保留当前改写标题；只有用户明确要求修改标题，或标题与正文事实明显冲突时才可调整。
4. 相邻章节是只读上下文，只用于判断人物、场景、称谓和剧情连续性，不得输出、改写或覆盖。
5. {}
6. 只输出当前章节的一组完整 marker、标题和非空正文，不要解释、不要 Markdown。

<<<YURI_REWRITE_DYNAMIC_CONTEXT>>>

本次用户要求：
{}

相关一致性资产：
{}

相邻章节只读上下文：
{}

参考原文：
标题：{}
正文：
{}

当前改写稿（主要底稿）：
{}

{}"#,
        rewrite_marker_format_guard("当前章节"),
        build_rewrite_priority_prompt(),
        build_core_prompt_section(core_prompt),
        build_compact_rewrite_rule_pack(settings),
        roster,
        cleanup_text_rule(),
        instructions,
        canon_text,
        prompt_context_or_none(adjacent_context),
        chapter.title,
        truncate_text(&chapter.original_text, 30_000),
        build_batch_chapter_text(std::slice::from_ref(chapter), true),
        rewrite_marker_final_reminder("当前章节")
    )
}

#[allow(dead_code)]
pub(crate) fn build_rewrite_prompt_with_settings(
    chapter: &Chapter,
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
) -> String {
    format!(
        r#"{}

{}

{}

改写要求：
1. 将原本男女性别叙事自然改写为双女主百合叙事；正文必须完整改写。
2. 采用中度再创作：保留主线、冲突、章节顺序、战力逻辑、人物动机和关键伏笔；只在【改写规则包】允许范围内调整互动、细节、称谓、外貌和关系推进。
3. {}
4. 严格遵守【规则优先级】、【改写规则包】和一致性资产；只输出改写后的标题和正文，不要解释。

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_priority_prompt(),
        build_compact_rewrite_rule_pack(settings),
        cleanup_text_rule(),
        canon_text,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
pub(crate) fn build_batch_analysis_prompt(chapters: &[Chapter]) -> String {
    build_batch_analysis_prompt_with_context(chapters, "")
}

pub(crate) fn build_batch_analysis_prompt_with_context(
    chapters: &[Chapter],
    shard_context: &str,
) -> String {
    build_batch_analysis_prompt_with_identity(chapters, shard_context, "")
}

pub(crate) fn build_batch_analysis_prompt_with_identity(
    chapters: &[Chapter],
    shard_context: &str,
    identity_context: &str,
) -> String {
    let (start_index, end_index) = match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) => (first.index, last.index),
        _ => (0, 0),
    };
    let shard_context = prompt_context_or_none(shard_context);
    let count = chapters.len();
    format!(
        r#"请只基于原文分析以下整个批次，并输出一个合法 JSON 对象。

输出结构必须是：
{{
  "batch": {{
    "start_index": "以实际输入章节为准",
    "end_index": "以实际输入章节为准",
    "chapter_count": "以实际输入章节为准"
  }},
  "outline": ["本批次原文主线、关键事件和状态变化，按时间顺序概括"],
  "characters": [
    "每个重要人物一条，格式必须为：姓名｜性别:男/女/未知/非人｜身份与称谓:原文称谓和亲属身份｜外貌与特征:原文外貌、性格、动机、能力或状态变化｜代词:他/她/它｜其他:原文别名"
  ],
  "relationships": ["本批次人物关系与关系变化"],
  "locations": ["本批次地点、场景和空间关系"],
  "foreshadowing": ["本批次伏笔、悬念、回收或关键信息"],
  "terms": ["本批次术语、组织、物品、功法、系统规则等"],
  "names": ["仅记录暂无法确定归属的称呼与指代（代词、绰号、敬称等）；凡能确定对应人物的性别，一律记录在 characters 中"]
}}

要求：
1. 输入可能是完整批次，也可能是并发分片；必须一次性分析当前输入中实际出现的全部章节。
2. 只输出一份当前输入级一致性资产，不要按章节逐章输出，不要输出 `chapters` 数组。
3. 不要补充原文没有的信息，不要改变原文人物、姓名、关系或剧情。
4. characters 每条必须严格使用上述字段格式，字段之间用全角或半角竖线分隔；性别只能取 男/女/未知/非人，必须依据原文性别线索、代词和称谓判断，无法确定时写 未知，不要猜测。
5. 不要提出任何后续处理方向。
6. JSON 字符串内部如果需要换行，必须写成 `\n`，不要在字符串里输出真实换行或其他控制字符。
7. 只输出 JSON，不要解释、不要 Markdown。

人物身份提示：
{}

<<<YURI_REWRITE_DYNAMIC_CONTEXT>>>

当前输入范围：第 {start_index} - {end_index} 章，共 {count} 章；batch 三个字段必须按此填写。

处理范围约束：
{}

当前输入章节：
{}"#,
        prompt_context_or_none(identity_context),
        shard_context,
        build_batch_analysis_chapter_text(chapters)
    )
}

pub(crate) fn prompt_context_or_none(context: &str) -> String {
    let context = context.trim();
    if context.is_empty() {
        "无".to_string()
    } else {
        context.to_string()
    }
}

#[allow(dead_code)]
pub(crate) fn build_rewrite_prompt(chapter: &Chapter, canon_text: &str) -> String {
    format!(
        r#"改写要求：
1. 将原本男女主叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 正文必须改写。章节标题原则上保留原标题和原编号；只有标题明确出现主角原名，或明确描述主角的男性身份、男性称谓、男性身体状态时才同步女性化。
4. 清除所有原男主痕迹，包括姓名、代词、身体描写、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
5. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲和旁观者必须保持原文性别、代词、称谓和身份一致，不得因为百合改写目标被误改成女性或跨章节忽男忽女。
6. 主角与男性角色共同被复数指代，或群体中含任一未指定性转的男性成员时，必须使用“他们”或准确的群体称呼，不能改成“她们”；只有确认全员女性时才使用“她们”，成员性别不明时保留原文“他们”或中性称呼。
7. 原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，必须保留原文人称代词和称谓，不要强行改成女性或男性。
8. 保持中文网文可读性，只输出改写后的标题和正文，不要解释。

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        canon_text,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}
