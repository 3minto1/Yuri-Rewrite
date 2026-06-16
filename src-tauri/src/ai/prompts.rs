use crate::domain::{CanonAsset, Chapter, NovelSettings, ParsedChapterRewrite};
use crate::truncate_text;
use std::collections::HashSet;

#[allow(dead_code)]
pub(crate) fn build_novel_settings_prompt(settings: &NovelSettings) -> String {
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "留空，由 AI 按姓名女性化规则生成".to_string()
    } else {
        settings.rewritten_protagonist_name.trim().to_string()
    };
    let additional = if settings.additional_feminize_names.trim().is_empty() {
        "无".to_string()
    } else {
        settings.additional_feminize_names.clone()
    };
    let additional = if settings.advanced_settings.trim().is_empty() {
        additional
    } else {
        format!(
            "{}\n\n高级设定：{}",
            additional,
            settings.advanced_settings.trim()
        )
    };
    format!(
        r#"小说基本设定：
- 主角原姓名：{}
- 主角改写后姓名：{}
- 其他需要女性化的人物姓名：{}
- 身材：{}
- 体型：{}

姓名女性化规则：
1. 如果“主角改写后姓名”不是留空，必须把主角统一改为该姓名，标题和正文都必须遵守，不得自行生成其他主角新名。
2. 如果“主角改写后姓名”留空，主角姓名必须女性化，不能保留明显男性化姓名；优先保留姓氏，名字部分用同音字或近音字替换为更女性化的字。
3. 示例：萧炎 -> 萧妍；李火旺 -> 李火婉。
4. 其他需要女性化的人物姓名只在文本中实际出现时处理，未出现则忽略。
5. 分析和改写必须维护一致的姓名映射，避免同一人物前后姓名不一致。"#,
        settings.protagonist_name, rewritten_name, additional, settings.bust, settings.body_type
    )
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

    let compacted = assets
        .iter()
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
            collect_mapping_keywords(&asset.content, &mut keywords);
        }
    }

    let selected = assets
        .iter()
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
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
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
    for value in settings
        .additional_feminize_names
        .split(['\n', ',', '，', ';', '；'])
    {
        insert_keyword(&mut keywords, value);
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

pub(crate) fn build_rewrite_settings_prompt(settings: &NovelSettings) -> String {
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "留空，由 AI 按姓名女性化规则生成".to_string()
    } else {
        settings.rewritten_protagonist_name.trim().to_string()
    };
    let forced_name_rule = if settings.rewritten_protagonist_name.trim().is_empty() {
        "当前未指定主角改写后姓名：AI 必须按同音或近音原则为主角生成女性化姓名，并在全批次保持一致。".to_string()
    } else {
        format!(
            "强制姓名规则：用户已指定主角改写后姓名为“{}”。改写标题、正文、称谓映射和后续复检时，主角姓名必须统一为“{}”；不得自行改成其他姓名，也不得保留主角原姓名“{}”。",
            settings.rewritten_protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
            settings.protagonist_name.trim()
        )
    };
    let additional_names = if settings.additional_feminize_names.trim().is_empty() {
        "无".to_string()
    } else {
        settings.additional_feminize_names.clone()
    };
    let advanced_settings = if settings.advanced_settings.trim().is_empty() {
        "无".to_string()
    } else {
        settings.advanced_settings.trim().to_string()
    };

    format!(
        r#"小说基本设定：
- 主角原姓名：{}
- 主角改写后姓名：{}
- 其他需要女性化的人物姓名：{}
- 身材：{}
- 体型：{}
- 改写模式：{}

{}

高级设定：
{}

姓名女性化规则：
1. {}
2. 正文必须检查主角姓名。章节标题原则上保留原标题和原编号；只有标题明确出现主角原名，或明确描述主角的男性身份、男性称谓、男性身体状态时，才需要改成女性化表达。普通意象、事件概括、其他角色描述和无法确认指向主角的男性词语都不需要为了女性化而修改。
3. 如果用户未指定主角改写后姓名，优先保留姓氏，名字部分用同音字或近音字替换为更女性化的字；如果用户已指定，则以用户指定姓名为最高优先级。
4. 示例：萧炎 -> 萧妍；李火旺 -> 李火婉。
5. 其他需要女性化的人物姓名只在文本中实际出现时处理，未出现则忽略。
6. 一致性资产中的“姓名映射表”优先级最高；凡是映射表中已有 `source -> target`，标题和正文都必须统一替换为 target，不得自行生成同一人物的其他女性化姓名。
7. 改写必须维护一致的姓名映射，避免同一人物前后姓名不一致；并发分片和后续批次也必须继续使用同一份映射表。

核心目标：
让没读过原文的读者阅读改写后的标题和正文时，看不出主角改写前曾是男性。凡是与主角有关的男性化姓名、代词、称谓、身份、身体特征、外貌气质、动作习惯、社会评价、亲密互动暗示，都必须改成自然的女性化表达；不能只删除男性化信息，也不能留下“男主”“少年郎”“公子”“他作为男人”等残留痕迹。

人物性别与代词一致性规则：
1. 只允许主角、用户填写的“其他需要女性化的人物姓名”、以及一致性资产“姓名映射表”中明确存在映射的人物进行性别转换。
2. 其他未指定人物必须保持原文性别、身份、称谓和人称代词：原文男性配角继续使用男性身份与“他/父亲/兄弟/少爷/公子”等符合原文的表达；原文女性配角继续使用女性身份与“她/母亲/姐妹/小姐”等符合原文的表达。
3. 不得因为百合改写目标而把所有重要配角、敌人、长辈、师父、兄弟、父亲或旁观者都改成女性；也不得在不同章节中让同一配角一会儿是男性、一会儿是女性。
4. 对性别不明或原文暂未明确的人物，应保持中性称呼或沿用原文称谓，等一致性资产或原文后续明确后再固定；不要凭空改成女性或男性。
5. 对原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，必须保留原文中的人称代词和称谓，不要为了女性化主角而强行改成“她”或改成其他性别表达。
6. 改写时必须参考一致性资产中的人物卡、人物关系、姓名映射表和原文上下文，确保每个人物的性别、代词、称谓、亲属关系和社会身份跨章节一致。

一致性硬性要求：
1. 人物外貌特征必须前后一致。发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态一旦由原文、设定或一致性资产确立，后续章节不得随意改变；例如上一章是金发，下一章不能无理由变成红发。
2. 如果原文没有明确外貌，不要每章随机发明互相矛盾的新特征；需要补充女性化描写时，应使用与已建立设定兼容的细节，并保持后续复用。
3. 人物关系和百合向情绪推进必须连续。暧昧、信任、依赖、吃醋、保护欲、亲密距离等变化要承接前文，不能上一章刚建立的关系下一章突然重置。
4. 称谓、代词、身份和旁人态度必须统一。主角已经女性化后，旁人对她的称呼、视线、互动距离、社会评价也要自然匹配女性身份，不能在不同章节反复摇摆。
5. 新增女性化细节必须服务当前剧情和人物状态，不得为了强调性别而制造与原文战力、性格、伏笔、剧情逻辑冲突的描写。"#,
        settings.protagonist_name,
        rewritten_name,
        additional_names,
        settings.bust,
        settings.body_type,
        rewrite_mode_label(&settings.rewrite_mode),
        rewrite_mode_prompt(&settings.rewrite_mode),
        advanced_settings,
        forced_name_rule
    )
}

pub(crate) fn rewrite_mode_label(mode: &str) -> &'static str {
    match mode {
        "creative" => "创意模式",
        _ => "严谨模式",
    }
}

pub(crate) fn rewrite_mode_prompt(mode: &str) -> &'static str {
    match mode {
        "creative" => {
            r#"改写模式规则：当前为创意模式，此规则优先级高于普通的“中度再创作”约束。
1. 必须让读者在每章都能明确感知主角已经从男性变为女性，而不是只替换姓名和代词。
2. 在不改变主线、关键事件、章节顺序和核心逻辑的前提下，主动补充女性化细节：女性外貌、身形仪态、神态反应、衣着/发丝/气息等可感知细节，以及旁人看待女性主角时的称谓、距离感、保护欲、亲密互动或误会。
3. 原文涉及男性身体、男性身份、男性社会称呼、男性动作习惯、男性气质展示时，必须改写为与设定身材和体型一致的女性表达；不能只删除这些内容。
4. 主角与周围人物互动时，应自然体现她作为女性后的关系变化，例如语气、肢体距离、旁人态度、暧昧张力、同性亲密感和百合向情绪推进。
5. 每章至少在关键场景中增加或强化 2-4 处女性化感知点；战斗、修炼、对话、日常和情感场景都要优先寻找可自然植入的位置。
6. 新增内容必须贴合原剧情和原文风格，不要写成与当前情节无关的堆砌描写，不得破坏已有伏笔、战力逻辑和人物动机。"#
        }
        _ => {
            "改写模式规则：当前为严谨模式。AI 必须更加忠于原文，不做过大改动，不对主角添加过多额外女性化描写；但必要的女性化描写不能减少，原文本身已有的男性化描写在改写后必须自然转换为女性化描写。"
        }
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
pub(crate) fn build_analysis_prompt_with_settings(
    chapter: &Chapter,
    settings: &NovelSettings,
) -> String {
    format!(
        r#"请分析以下章节，并输出 JSON：
{{
  "outline": "本章大纲",
  "characters": ["角色与设定变化"],
  "relationships": ["人物关系变化"],
  "locations": ["地点"],
  "foreshadowing": ["伏笔或回收"],
  "name_feminization_map": ["原姓名 -> 女性化姓名，未出现的人物不要写入"],
  "rewrite_notes": ["后续百合改写必须注意的性别、称谓、动作、外貌、关系细节"]
}}

{}

章节标题：{}

章节正文：
{}"#,
        build_rewrite_settings_prompt(settings),
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
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

pub(crate) fn build_batch_rewrite_prompt_with_context(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"{}

改写要求：
1. 将原本男女性别叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序、战力逻辑、人物动机和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 正文必须完成改写。章节标题原则上保留原标题和原编号；只有标题明确出现主角原名，或明确描述主角的男性身份、男性称谓、男性身体状态时，才同步女性化。不要仅因创意模式、普通男性词语、标题意象或标题编号与 marker index 不同而修改标题。
4. 清除所有原男性主角痕迹，包括姓名、代词、身体描述、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示；所有相关内容都要自然转换为女性主角表达。
5. 主角姓名和指定 NPC 姓名必须严格使用一致性资产中的“姓名映射表”。没有映射时才按同音或近音原则女性化，优先保留姓氏；例如萧炎改为萧妍，李火旺改为李火婉。
6. 按基本设定中的身材和体型调整外貌、动作和互动细节，不要出现与设定冲突的描写。
7. 只有主角、用户指定的额外女性化人物、以及姓名映射表中明确存在映射的人物可以性别转换；其他配角、敌人、长辈、师父、兄弟、父亲、旁观者必须保持原文性别、身份、称谓和人称代词，不得跨章节忽男忽女。
8. 对未指定性转的人物，原文男性继续使用男性代词/称谓，原文女性继续使用女性代词/称谓，性别不明者保持原文称谓或中性表达，等原文或一致性资产明确后再固定。
9. 对原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，保留原文中的人称代词和称谓，不要强行女性化或男性化。
10. 人物外貌特征必须前后一致。发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态一旦由原文、设定或一致性资产确立，后续章节不得随意改变；例如上一章是金发，下一章不能无理由变成红发。
11. 如果原文没有明确外貌，不要每章随机发明互相矛盾的新特征；需要补充女性化描写时，应使用与已建立设定兼容的细节，并保持后续复用。
12. 百合向关系推进必须承接前文。暧昧、信任、依赖、吃醋、保护欲、亲密距离、旁人态度和称谓变化要符合当前剧情阶段，不能突然重置或跳跃。
13. 女性化细节应覆盖正文中与主角有关的视线、评价、互动距离和社会称呼；标题仅按第 3 条的明确条件修改。新增内容必须服务当前剧情，不得破坏原文战力、伏笔、人物性格和逻辑。
14. 输入可能是完整批次，也可能是并发分片；必须一次性改写当前输入中实际出现的全部章节，不要逐章分开回答。
15. 每章必须以输入中对应的 `<<<YURI_REWRITE_CHAPTER_START ...>>>` 开始标记开头，并以对应的 `<<<YURI_REWRITE_CHAPTER_END ...>>>` 结束标记结尾；marker 中的 index 和 id 必须逐字复制，不得省略、改写或自行生成。
16. 只输出当前输入章节的边界标记、改写后标题和正文，不要解释、不要 Markdown 包裹，不要输出当前输入之外的章节。

{}

并发分片上下文：
{}

一致性资产：
{}

当前输入章节：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_settings_prompt(settings),
        shard_context,
        canon_text,
        build_batch_chapter_text(chapters, false)
    )
}

#[allow(dead_code)]
pub(crate) fn build_batch_review_prompt_with_settings(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> String {
    build_batch_review_prompt_with_context(chapters, rewrites, settings, "")
}

pub(crate) fn build_batch_review_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请复检并自动修正以下批次改写稿。

重点检查：
1. 主角姓名是否已按规则女性化，且全批次一致。
2. 章节标题原则上必须保留原标题和原编号。只有标题明确出现主角原名，或明确描述主角的男性身份、男性称谓、男性身体状态时才检查并修正；普通意象、事件概括、其他角色描述和无法确认指向主角的男性词语都不是标题问题。
3. 其他指定姓名只在出现时女性化，且前后一致。
4. 人称代词、称谓、身体描写、外貌气质、社会称呼、动作习惯和互动细节是否仍残留男性主角痕迹。
5. 身材、体型和高级设定是否被遵守。
6. 如果当前为创意模式，检查每章关键场景是否有足够清晰的女性化感知点；若只是替换姓名/代词，应主动补充贴合原剧情的女性外貌、神态、互动距离、称谓变化、百合向情绪张力等细节。
7. 改写后的标题和正文是否能让没读过原文的读者看不出主角原本是男性。
8. 人物外貌特征是否前后一致：发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态不能在不同章节无理由变化。
9. 百合向关系推进是否承接前文：暧昧、信任、依赖、吃醋、保护欲、亲密距离、称谓和旁人态度不能突然重置或跳跃。
10. 女性化补充是否贴合剧情和一致性资产，不能为了强调性别而破坏原文战力、伏笔、人物性格和逻辑。
11. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲、旁观者是否被误改性别；同一人物在不同章节中的他/她、先生/小姐、父亲/母亲、兄弟/姐妹、少爷/小姐等代词和称谓是否前后一致。
12. 原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，改写稿保留原文人称代词和称谓时应视为合格，不要当作主角男性残留或未女性化问题。
13. 章节内部和章节之间是否有逻辑不通、缺句、重复、边界错乱。
14. marker 中的 index 是内部顺序标识，不是原标题中的章节编号。序章、楔子、番外等会使二者不同；不得据此修改标题或判定章节矛盾。

输出要求：
1. 如果发现问题，直接在正文中修正。
2. 如果没有问题，原样输出改写稿。
3. 每章必须以输入中对应的 `<<<YURI_REWRITE_CHAPTER_START ...>>>` 开始标记开头，并以对应的 `<<<YURI_REWRITE_CHAPTER_END ...>>>` 结束标记结尾；marker 中的 index 和 id 必须逐字复制，不得省略、改写或自行生成。
4. 只输出当前输入章节的边界标记、修正后标题和正文，不要解释、不要 Markdown 包裹，不要输出当前输入之外的章节。

{}

并发分片上下文：
{}

待复检改写稿：
{}"#,
        build_rewrite_settings_prompt(settings),
        shard_context,
        build_batch_rewrite_text(chapters, rewrites)
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

改写要求：
1. 将原本男女主叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 正文必须改写。章节标题原则上保留原标题和原编号；只有标题明确出现主角原名，或明确描述主角的男性身份、男性称谓、男性身体状态时才同步女性化。
4. 清除所有原男主痕迹，包括姓名、代词、身体描写、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
5. 主角姓名必须按同音或近音原则女性化，例如萧炎改为萧妍，李火旺改为李火婉；其他指定姓名只在本章出现时女性化。
6. 按基本设定中的身材和体型调整外貌、动作和互动细节，不要出现与设定冲突的描写。
7. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲和旁观者必须保持原文性别、代词、称谓和身份一致，不得因为百合改写目标被误改成女性或跨章节忽男忽女。
8. 原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，必须保留原文人称代词和称谓，不要强行改成女性或男性。
9. 保持中文网文可读性，只输出改写后的标题和正文，不要解释。

{}

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_settings_prompt(settings),
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
    let (start_index, end_index) = match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) => (first.index, last.index),
        _ => (0, 0),
    };
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请只基于原文分析以下整个批次，并输出一个合法 JSON 对象。

输出结构必须是：
{{
  "batch": {{
    "start_index": {},
    "end_index": {},
    "chapter_count": {}
  }},
  "outline": ["本批次原文主线、关键事件和状态变化，按时间顺序概括"],
  "characters": ["本批次出现的重要人物、别名、原文性别线索、原文人称代词、身份、称谓、外貌、性格、动机、能力或状态变化"],
  "relationships": ["本批次人物关系与关系变化"],
  "locations": ["本批次地点、场景和空间关系"],
  "foreshadowing": ["本批次伏笔、悬念、回收或关键信息"],
  "terms": ["本批次术语、组织、物品、功法、系统规则等"],
  "names": ["本批次出现的人名、称谓、别名、指代对象、对应人物的原文性别或性别不明状态"]
}}

要求：
1. 输入可能是完整批次，也可能是并发分片；必须一次性分析当前输入中实际出现的全部章节。
2. 只输出一份当前输入级一致性资产，不要按章节逐章输出，不要输出 `chapters` 数组。
3. 不要补充原文没有的信息，不要改变原文人物、姓名、关系或剧情。
4. 必须尽量记录人物的原文性别线索、代词、称谓和亲属身份；无法确定时写“性别不明”，不要猜测。
5. 不要提出任何后续处理方向。
6. JSON 字符串内部如果需要换行，必须写成 `\n`，不要在字符串里输出真实换行或其他控制字符。
7. 只输出 JSON，不要解释、不要 Markdown。

并发分片上下文：
{}

当前输入章节：
{}"#,
        start_index,
        end_index,
        chapters.len(),
        shard_context,
        build_batch_analysis_chapter_text(chapters)
    )
}

#[allow(dead_code)]
pub(crate) fn build_analysis_prompt(chapter: &Chapter) -> String {
    format!(
        r#"请只基于原文分析以下章节，并输出合法 JSON：
{{
  "outline": "本章原文大纲",
  "characters": ["原文人物、别名、原文性别线索、原文人称代词、身份、称谓、外貌、性格、动机、能力或状态变化"],
  "relationships": ["原文人物关系与关系变化"],
  "locations": ["原文地点、场景和空间关系"],
  "foreshadowing": ["原文伏笔、悬念、回收或关键信息"],
  "terms": ["原文术语、组织、物品、功法、系统规则等"],
  "names": ["原文出现的人名、称谓、别名、指代对象、对应人物的原文性别或性别不明状态"]
}}

要求：
1. 只提取和维护原文一致性资产。
2. 不要提出任何后续处理方向。
3. 不要补充原文没有的信息，不要改变原文人物、姓名、关系或剧情。
4. 必须尽量记录人物的原文性别线索、代词、称谓和亲属身份；无法确定时写“性别不明”，不要猜测。
5. 只输出 JSON，不要 Markdown。

章节标题：{}

章节正文：
{}"#,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
pub(crate) fn build_analysis_prompt_legacy(chapter: &Chapter) -> String {
    format!(
        r#"请分析以下章节，并输出 JSON：
{{
  "outline": "本章大纲",
  "characters": ["角色与设定变化"],
  "relationships": ["人物关系变化"],
  "locations": ["地点"],
  "foreshadowing": ["伏笔或回收"],
  "rewrite_notes": ["后续百合改写必须注意的性别、称谓、动作、外貌、关系细节"]
}}

章节标题：{}

章节正文：
{}"#,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
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
6. 原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，必须保留原文人称代词和称谓，不要强行改成女性或男性。
7. 保持中文网文可读性，只输出改写后的标题和正文，不要解释。

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
