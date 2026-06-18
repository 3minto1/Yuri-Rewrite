use crate::domain::{Chapter, ParsedChapterAnalysis, ParsedChapterRewrite};
use crate::{
    analysis_chapter_end_marker, analysis_chapter_start_marker, chapter_end_marker,
    chapter_heading_regex, chapter_start_marker, is_plausible_strict_heading_line,
    parse_jsonish_value, to_string,
};
use regex::Regex;

pub(crate) fn parse_batch_analysis_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let json_error = match parse_batch_analysis_json_output(output, expected_chapters) {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error,
    };

    if output.contains("YURI_ANALYSIS_CHAPTER_START") {
        return parse_batch_analysis_marker_output(output, expected_chapters).map_err(|marker_error| {
            format!(
                "AI 分析输出既不是合法批次 JSON，也不是有效章节边界格式。JSON 解析错误：{}；边界格式解析错误：{}",
                json_error, marker_error
            )
        });
    }

    Err(json_error)
}

pub(crate) fn parse_batch_analysis_json_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let value = parse_jsonish_value(output)
        .map_err(|error| format!("AI 分析输出不是合法 JSON：{}", error))?;
    if let Ok(batch_json) = extract_batch_level_analysis_json(&value) {
        return Ok(vec![ParsedChapterAnalysis {
            id: expected_chapters
                .first()
                .ok_or_else(|| "缺少待分析章节。".to_string())?
                .id
                .clone(),
            json: batch_json,
        }]);
    }

    let items = match &value {
        serde_json::Value::Object(map) => map
            .get("chapters")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "AI 分析 JSON 缺少 chapters 数组。".to_string())?,
        serde_json::Value::Array(items) => items,
        _ => return Err("AI 分析 JSON 必须是对象或数组。".to_string()),
    };

    if items.len() != expected_chapters.len() {
        return Err(format!(
            "AI 分析 JSON 章节数量不匹配：期望 {} 章，实际 {} 章。",
            expected_chapters.len(),
            items.len()
        ));
    }

    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (item, chapter) in items.iter().zip(expected_chapters.iter()) {
        let item_object = item
            .as_object()
            .ok_or_else(|| format!("章节 {} 的分析项必须是 JSON 对象。", chapter.index))?;
        if let Some(index) = item_object
            .get("index")
            .or_else(|| item_object.get("chapter_index"))
            .and_then(serde_json::Value::as_i64)
        {
            if index != chapter.index {
                return Err(format!(
                    "AI 分析 JSON 章节顺序不匹配：期望第 {} 章，实际第 {} 章。",
                    chapter.index, index
                ));
            }
        }
        if let Some(id) = item_object
            .get("id")
            .or_else(|| item_object.get("chapter_id"))
            .and_then(serde_json::Value::as_str)
        {
            if id != chapter.id {
                return Err(format!(
                    "AI 分析 JSON 章节 id 不匹配：期望 {}，实际 {}。",
                    chapter.id, id
                ));
            }
        }

        let analysis_value = item_object.get("analysis").unwrap_or(item);
        let mut analysis = analysis_value
            .as_object()
            .ok_or_else(|| format!("章节 {} 的 analysis 必须是 JSON 对象。", chapter.index))?
            .clone();
        analysis.remove("id");
        analysis.remove("chapter_id");
        analysis.remove("index");
        analysis.remove("chapter_index");
        analysis.remove("title");
        analysis.remove("chapter_title");
        if analysis.is_empty() {
            return Err(format!("章节 {} 的分析 JSON 为空。", chapter.index));
        }
        let json = serde_json::to_string_pretty(&serde_json::Value::Object(analysis))
            .map_err(to_string)?;
        parsed.push(ParsedChapterAnalysis {
            id: chapter.id.clone(),
            json,
        });
    }

    Ok(parsed)
}

pub(crate) fn extract_batch_level_analysis_json(
    value: &serde_json::Value,
) -> Result<String, String> {
    let candidate = value
        .get("batch_assets")
        .or_else(|| value.get("consistency_assets"))
        .or_else(|| value.get("assets"))
        .or_else(|| value.get("analysis"))
        .unwrap_or(value);
    let object = candidate
        .as_object()
        .ok_or_else(|| "批次级分析 JSON 必须是对象。".to_string())?;
    if object.contains_key("chapters") {
        return Err("检测到逐章 chapters 输出。".to_string());
    }
    let useful_fields = [
        "outline",
        "characters",
        "relationships",
        "locations",
        "foreshadowing",
        "terms",
        "names",
    ];
    if !useful_fields
        .iter()
        .any(|field| object.contains_key(*field))
    {
        return Err("批次级分析 JSON 缺少一致性资产字段。".to_string());
    }
    serde_json::to_string_pretty(candidate).map_err(to_string)
}

pub(crate) fn parse_batch_analysis_marker_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let mut cursor = output.replace("\r\n", "\n").replace('\r', "\n");
    let mut parsed = Vec::with_capacity(expected_chapters.len());

    for chapter in expected_chapters {
        let start_marker = analysis_chapter_start_marker(chapter);
        let end_marker = analysis_chapter_end_marker(chapter);
        let start_pos = cursor
            .find(&start_marker)
            .ok_or_else(|| format!("AI 输出缺少章节分析开始标记：{}", start_marker))?;
        if !cursor[..start_pos].trim().is_empty() {
            return Err(format!(
                "AI 输出在章节 {} 分析开始标记前包含多余内容。",
                chapter.index
            ));
        }
        let after_start = cursor[start_pos + start_marker.len()..].to_string();
        let end_pos = after_start
            .find(&end_marker)
            .ok_or_else(|| format!("AI 输出缺少章节分析结束标记：{}", end_marker))?;
        let block = after_start[..end_pos].trim();
        if block.trim().is_empty() {
            return Err(format!("章节 {} 的分析 JSON 为空。", chapter.index));
        }
        let value = parse_jsonish_value(block)
            .map_err(|error| format!("章节 {} 的分析 JSON 无效：{}", chapter.index, error))?;
        let normalized = serde_json::to_string_pretty(&value).map_err(to_string)?;
        parsed.push(ParsedChapterAnalysis {
            id: chapter.id.clone(),
            json: normalized,
        });
        cursor = after_start[end_pos + end_marker.len()..].to_string();
    }

    if !cursor.trim().is_empty() {
        return Err("AI 输出在最后一个章节分析结束标记后包含多余内容。".to_string());
    }
    Ok(parsed)
}

pub(crate) fn parse_batch_rewrite_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let marker_error = parse_batch_rewrite_marker_output(&normalized, expected_chapters).err();
    if marker_error.is_none() {
        return parse_batch_rewrite_marker_output(&normalized, expected_chapters);
    }
    if marker_error
        .as_deref()
        .is_some_and(|error| error.contains("章节顺序不匹配"))
    {
        return Err(marker_error.unwrap());
    }

    match parse_markerless_rewrite_output(&normalized, expected_chapters) {
        Ok(parsed) => Ok(parsed),
        Err(fallback_error) => Err(match marker_error {
            Some(error) => format!("{}；兜底解析也失败：{}", error, fallback_error),
            None => fallback_error,
        }),
    }
}

pub(crate) fn parse_batch_rewrite_marker_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut cursor = output.to_string();
    let mut parsed = Vec::with_capacity(expected_chapters.len());

    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start_marker = chapter_start_marker(chapter);
        let end_marker = chapter_end_marker(chapter);
        let (start_pos, start_len) = find_rewrite_marker(&cursor, chapter, "START")
            .ok_or_else(|| format!("AI 输出缺少章节开始标记：{}", start_marker))?;
        let before_start = cursor[..start_pos].trim();
        if !before_start.is_empty() && !before_start.contains("YURI_REWRITE_CHAPTER_START") {
            return Err(format!(
                "AI 输出在章节 {} 开始标记前包含多余内容。",
                chapter.index
            ));
        }
        if contains_expected_rewrite_start_marker(before_start, &expected_chapters[idx + 1..]) {
            return Err(format!(
                "AI 输出章节顺序不匹配：在章节 {} 前出现了当前分片内的后续章节标记。",
                chapter.index
            ));
        }
        let after_start = cursor[start_pos + start_len..].to_string();
        let (block, next_cursor) =
            if let Some((end_pos, end_len)) = find_rewrite_marker(&after_start, chapter, "END") {
                (
                    after_start[..end_pos].to_string(),
                    after_start[end_pos + end_len..].to_string(),
                )
            } else if let Some(next_chapter) = expected_chapters.get(idx + 1) {
                let next_start_marker = chapter_start_marker(next_chapter);
                let (next_pos, _) = find_rewrite_marker(&after_start, next_chapter, "START")
                    .ok_or_else(|| {
                        format!(
                            "AI 输出缺少章节结束标记：{}，且无法定位下一章开始标记：{}",
                            end_marker, next_start_marker
                        )
                    })?;
                (
                    after_start[..next_pos].to_string(),
                    after_start[next_pos..].to_string(),
                )
            } else if !after_start.trim().is_empty() {
                (after_start, String::new())
            } else {
                return Err(format!("AI 输出缺少章节结束标记：{}", end_marker));
            };
        let (title, text) = clean_rewrite_block(&block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
        cursor = next_cursor;
    }

    let trailing = cursor.trim();
    if !trailing.is_empty() && !trailing.contains("YURI_REWRITE_CHAPTER_START") {
        return Err("AI 输出在最后一个章节结束标记后包含多余内容。".to_string());
    }
    Ok(parsed)
}

pub(crate) fn find_rewrite_marker(
    text: &str,
    chapter: &Chapter,
    kind: &str,
) -> Option<(usize, usize)> {
    let exact = if kind == "START" {
        chapter_start_marker(chapter)
    } else {
        chapter_end_marker(chapter)
    };
    if let Some(pos) = text.find(&exact) {
        return Some((pos, exact.len()));
    }

    let pattern = format!(
        r#"<<<\s*YURI_REWRITE_CHAPTER_{}\s+index\s*=\s*{}(?:\s+id\s*=\s*[^>\s]+)?\s*>>>"#,
        kind, chapter.index
    );
    let regex = Regex::new(&pattern).ok()?;
    regex
        .find(text)
        .map(|mat| (mat.start(), mat.end() - mat.start()))
}

pub(crate) fn contains_expected_rewrite_start_marker(text: &str, chapters: &[Chapter]) -> bool {
    chapters
        .iter()
        .any(|chapter| find_rewrite_marker(text, chapter, "START").is_some())
}

pub(crate) fn parse_markerless_rewrite_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let normalized = strip_rewrite_marker_lines(output);
    if normalized.trim().is_empty() {
        return Err("AI 输出为空，无法兜底解析。".to_string());
    }

    if let Ok(parsed) = parse_markerless_by_title_labels(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if let Ok(parsed) = parse_markerless_by_expected_titles(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if let Ok(parsed) = parse_markerless_by_heading_regex(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if expected_chapters.len() == 1 {
        let (title, text) = clean_rewrite_block(&normalized, &expected_chapters[0].title);
        if !text.trim().is_empty() {
            return Ok(vec![ParsedChapterRewrite {
                id: expected_chapters[0].id.clone(),
                index: expected_chapters[0].index,
                title,
                text,
            }]);
        }
    }

    Err("无法从无 marker 输出中稳定拆回当前分片章节。".to_string())
}

pub(crate) fn strip_rewrite_marker_lines(output: &str) -> String {
    output
        .trim()
        .trim_start_matches("```text")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("<<<YURI_REWRITE_CHAPTER_START")
                && !trimmed.starts_with("<<<YURI_REWRITE_CHAPTER_END")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn parse_markerless_by_title_labels(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let lines = output.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("标题：") || trimmed.starts_with("标题:") {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if starts.len() != expected_chapters.len() {
        return Err("标题行数量与分片章节数量不匹配。".to_string());
    }

    parse_markerless_line_blocks(&lines, &starts, expected_chapters)
}

pub(crate) fn parse_markerless_by_heading_regex(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let lines = output.lines().collect::<Vec<_>>();
    let heading_re = chapter_heading_regex();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if !matches!(trimmed, "正文" | "正文：" | "正文:")
                && heading_re.is_match(trimmed)
                && is_plausible_strict_heading_line(trimmed)
            {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if starts.len() != expected_chapters.len() {
        return Err("章节标题数量与分片章节数量不匹配。".to_string());
    }

    parse_markerless_line_blocks(&lines, &starts, expected_chapters)
}

pub(crate) fn parse_markerless_line_blocks(
    lines: &[&str],
    starts: &[usize],
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start = starts[idx];
        let end = starts.get(idx + 1).copied().unwrap_or(lines.len());
        let block = lines[start..end].join("\n");
        let (title, text) = clean_rewrite_block(&block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的兜底改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
    }
    Ok(parsed)
}

pub(crate) fn parse_markerless_by_expected_titles(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut positions = Vec::with_capacity(expected_chapters.len());
    let mut search_from = 0usize;
    for chapter in expected_chapters {
        let title = chapter.title.trim();
        if title.is_empty() {
            return Err("章节标题为空，无法按标题兜底解析。".to_string());
        }
        let relative = output[search_from..]
            .find(title)
            .ok_or_else(|| format!("兜底解析找不到章节标题：{}", title))?;
        let pos = search_from + relative;
        positions.push(pos);
        search_from = pos + title.len();
    }

    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start = positions[idx];
        let end = positions.get(idx + 1).copied().unwrap_or(output.len());
        let block = output[start..end].trim();
        let (title, text) = clean_rewrite_block(block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的兜底改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
    }
    Ok(parsed)
}

pub(crate) fn clean_rewrite_block(block: &str, fallback_title: &str) -> (String, String) {
    let mut lines = block.trim().lines().collect::<Vec<_>>();
    let mut title = fallback_title.trim().to_string();
    if lines.first().is_some_and(|line| {
        line.trim_start().starts_with("标题：") || line.trim_start().starts_with("标题:")
    }) {
        let title_line = lines.remove(0).trim().to_string();
        let parsed_title = title_line
            .strip_prefix("标题：")
            .or_else(|| title_line.strip_prefix("标题:"))
            .unwrap_or("")
            .trim();
        if !parsed_title.is_empty() {
            title = parsed_title.to_string();
        }
    }
    if lines
        .first()
        .is_some_and(|line| line.trim() == fallback_title.trim())
    {
        lines.remove(0);
    }
    if lines
        .first()
        .is_some_and(|line| matches!(line.trim(), "正文：" | "正文:" | "正文"))
    {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| {
        matches!(
            line.trim(),
            "标题：" | "标题:" | "标题" | "正文：" | "正文:" | "正文"
        )
    }) {
        lines.pop();
    }
    (title, lines.join("\n").trim().to_string())
}
