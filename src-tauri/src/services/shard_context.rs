use crate::commands::settings::normalize_rewrite_parallelism;
use crate::domain::{AppState, Chapter};
use crate::{to_string, truncate_text, truncate_text_tail};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use tauri::State;

pub(crate) fn split_chapters_for_parallelism(
    chapters: &[Chapter],
    rewrite_parallelism: usize,
) -> Vec<Vec<Chapter>> {
    if chapters.is_empty() {
        return Vec::new();
    }
    let parallelism = normalize_rewrite_parallelism(rewrite_parallelism).min(chapters.len());
    if parallelism <= 1 {
        return vec![chapters.to_vec()];
    }
    let base_size = chapters.len() / parallelism;
    let remainder = chapters.len() % parallelism;
    let mut shards = Vec::with_capacity(parallelism);
    let mut start = 0usize;
    for shard_index in 0..parallelism {
        let shard_size = base_size + usize::from(shard_index < remainder);
        let end = start + shard_size;
        shards.push(chapters[start..end].to_vec());
        start = end;
    }
    shards
}

#[derive(Debug, Clone)]
pub(crate) struct ChapterShardWork {
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) previous: Option<Chapter>,
    pub(crate) next: Option<Chapter>,
}

pub(crate) fn build_contiguous_shard_work(
    all_chapters: &[Chapter],
    target_chapters: &[Chapter],
    rewrite_parallelism: usize,
) -> Vec<ChapterShardWork> {
    if target_chapters.is_empty() {
        return Vec::new();
    }
    let target_ids = target_chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect::<HashSet<_>>();
    if target_ids.len() == all_chapters.len()
        && all_chapters
            .iter()
            .all(|chapter| target_ids.contains(chapter.id.as_str()))
    {
        return split_chapters_for_parallelism(all_chapters, rewrite_parallelism)
            .into_iter()
            .map(|chapters| ChapterShardWork {
                chapters,
                previous: None,
                next: None,
            })
            .collect();
    }

    let parallelism = normalize_rewrite_parallelism(rewrite_parallelism).max(1);
    let target_shard_size = all_chapters.len().div_ceil(parallelism).max(1);
    let mut work = Vec::new();
    let mut position = 0usize;
    while position < all_chapters.len() {
        if !target_ids.contains(all_chapters[position].id.as_str()) {
            position += 1;
            continue;
        }
        let run_start = position;
        while position < all_chapters.len()
            && target_ids.contains(all_chapters[position].id.as_str())
        {
            position += 1;
        }
        let run_end = position;
        let mut chunk_start = run_start;
        while chunk_start < run_end {
            let chunk_end = (chunk_start + target_shard_size).min(run_end);
            work.push(ChapterShardWork {
                chapters: all_chapters[chunk_start..chunk_end].to_vec(),
                previous: chunk_start
                    .checked_sub(1)
                    .and_then(|index| all_chapters.get(index))
                    .cloned(),
                next: all_chapters.get(chunk_end).cloned(),
            });
            chunk_start = chunk_end;
        }
    }
    work
}

#[derive(Debug, Clone)]
pub(crate) struct StagedChapterOutput {
    pub(crate) title: Option<String>,
    pub(crate) content: Option<String>,
}

pub(crate) fn load_staged_outputs(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    phase: &str,
) -> Result<HashMap<String, StagedChapterOutput>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT chapter_id, title, content FROM auto_run_shard_outputs
             WHERE novel_id = ?1 AND batch_index = ?2 AND phase = ?3",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![novel_id, batch_index, phase], |row| {
            Ok((
                row.get::<_, String>(0)?,
                StagedChapterOutput {
                    title: row.get(1)?,
                    content: row.get(2)?,
                },
            ))
        })
        .map_err(to_string)?
        .collect::<Result<HashMap<_, _>, _>>()
        .map_err(to_string)?;
    Ok(rows)
}

fn format_readonly_neighbor(
    label: &str,
    chapter: Option<&Chapter>,
    staged: &HashMap<String, StagedChapterOutput>,
    phase: &str,
    use_tail: bool,
) -> Option<String> {
    let chapter = chapter?;
    let staged_output = staged.get(&chapter.id);
    let title = staged_output
        .and_then(|output| output.title.as_deref())
        .unwrap_or(&chapter.title);
    let completed_context = staged_output
        .and_then(|output| output.content.as_deref())
        .filter(|content| !content.trim().is_empty());
    let summarize = |content: &str| {
        if use_tail {
            truncate_text_tail(content.trim(), 600)
        } else {
            truncate_text(content.trim(), 600)
        }
    };
    let context = match (phase, completed_context) {
        ("rewrite", Some(content)) => {
            format!("已完成改写摘要：{}", summarize(content))
        }
        ("analysis", Some(content)) => {
            format!("已完成分析摘要：{}", summarize(content))
        }
        _ => format!("原文摘要：{}", summarize(&chapter.original_text)),
    };
    Some(format!(
        "{}：内部索引 {} · 标题：{}\n{}",
        label, chapter.index, title, context
    ))
}

pub(crate) fn build_readonly_adjacent_context(
    work: &ChapterShardWork,
    staged: &HashMap<String, StagedChapterOutput>,
    phase: &str,
) -> String {
    let context = [
        format_readonly_neighbor("前一相邻章节", work.previous.as_ref(), staged, phase, true),
        format_readonly_neighbor("后一相邻章节", work.next.as_ref(), staged, phase, false),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if context.is_empty() {
        "无相邻章节。".to_string()
    } else {
        context.join("\n\n")
    }
}

pub(crate) fn format_shard_label(
    batch_label: &str,
    shard_index: usize,
    shard_total: usize,
    chapters: &[Chapter],
) -> String {
    if shard_total <= 1 {
        return batch_label.to_string();
    }
    match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => {
            format!(
                "{} · 分片 {}/{} · 第{}章",
                batch_label,
                shard_index + 1,
                shard_total,
                first.index
            )
        }
        (Some(first), Some(last)) => format!(
            "{} · 分片 {}/{} · 第{}-{}章",
            batch_label,
            shard_index + 1,
            shard_total,
            first.index,
            last.index
        ),
        _ => format!("{} · 分片 {}/{}", batch_label, shard_index + 1, shard_total),
    }
}

pub(crate) fn format_shard_context(
    shard_index: usize,
    shard_total: usize,
    rewrite_parallelism: usize,
    batch_label: &str,
    chapters: &[Chapter],
) -> String {
    format_shard_context_with_neighbors(
        shard_index,
        shard_total,
        rewrite_parallelism,
        batch_label,
        chapters,
        "无相邻章节。",
    )
}

pub(crate) fn format_shard_context_with_neighbors(
    _shard_index: usize,
    _shard_total: usize,
    _rewrite_parallelism: usize,
    _batch_label: &str,
    chapters: &[Chapter],
    readonly_adjacent_context: &str,
) -> String {
    let chapter_list = chapters
        .iter()
        .map(|chapter| format!("第{}章", chapter.index))
        .collect::<Vec<_>>()
        .join("、");
    let chapter_range = match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => format!("第{}章", first.index),
        (Some(first), Some(last)) => format!("第{}-{}章", first.index, last.index),
        _ => "空分片".to_string(),
    };
    format!(
        "本次目标只包含{}：{}。只能处理和输出这些目标章节，严禁输出输入外的任何章节、标题、正文或章节边界标记。所有请求共享同一份小说设定、一致性资产、姓名女性化规则和章节边界规则；不得因为只看到当前输入就改变人物设定或重置关系进展。\n\n相邻章节只读上下文（仅用于判断人物、场景、称谓和剧情连续性；不得分析为本次结果，不得输出、改写或覆盖）：\n{}",
        chapter_range,
        chapter_list,
        readonly_adjacent_context.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(index: i64) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: format!("第{index}章"),
            original_text: "原文正文".to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "completed".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    #[test]
    fn rewrite_parallelism_splits_batch_into_contiguous_shards() {
        let chapters = (1..=30).map(chapter).collect::<Vec<_>>();

        let six = split_chapters_for_parallelism(&chapters, 6);
        assert_eq!(six.len(), 6);
        assert!(six.iter().all(|shard| shard.len() == 5));
        assert_eq!(six[0][0].index, 1);
        assert_eq!(six[5][4].index, 30);

        let three = split_chapters_for_parallelism(&chapters, 3);
        assert_eq!(three.len(), 3);
        assert!(three.iter().all(|shard| shard.len() == 10));

        let ten = split_chapters_for_parallelism(&chapters, 10);
        assert_eq!(ten.len(), 10);
        assert!(ten.iter().all(|shard| shard.len() == 3));

        let single = split_chapters_for_parallelism(&chapters, 1);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].len(), 30);

        let uneven = split_chapters_for_parallelism(&chapters[..24], 10);
        assert_eq!(uneven.len(), 10);
        assert_eq!(
            uneven.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![3, 3, 3, 3, 2, 2, 2, 2, 2, 2]
        );
        assert_eq!(uneven[0][0].index, 1);
        assert_eq!(uneven[9][1].index, 24);
    }

    #[test]
    fn resumed_shards_remain_contiguous_after_parallelism_changes() {
        let chapters = (1..=100).map(chapter).collect::<Vec<_>>();
        let staged = chapters
            .iter()
            .filter(|chapter| matches!(chapter.index % 5, 0 | 1))
            .map(|chapter| chapter.id.clone())
            .collect::<HashSet<_>>();
        let pending = chapters
            .iter()
            .filter(|chapter| !staged.contains(&chapter.id))
            .cloned()
            .collect::<Vec<_>>();

        let twenty_five = build_contiguous_shard_work(&chapters, &pending, 25);
        let one = build_contiguous_shard_work(&chapters, &pending, 1);

        assert_eq!(staged.len(), 40);
        assert_eq!(pending.len(), 60);
        assert_eq!(twenty_five.len(), 20);
        assert_eq!(one.len(), 20);
        for work in twenty_five.iter().chain(one.iter()) {
            assert!(work
                .chapters
                .windows(2)
                .all(|pair| pair[1].index == pair[0].index + 1));
        }
        assert_eq!(
            twenty_five
                .iter()
                .flat_map(|work| work.chapters.iter())
                .count(),
            pending.len()
        );
        assert_eq!(
            one.iter()
                .flat_map(|work| work.chapters.iter())
                .map(|chapter| chapter.id.as_str())
                .collect::<HashSet<_>>(),
            pending
                .iter()
                .map(|chapter| chapter.id.as_str())
                .collect::<HashSet<_>>()
        );
    }

    #[test]
    fn resumed_pending_chapters_do_not_cross_staged_gaps() {
        let chapters = (1..=10).map(chapter).collect::<Vec<_>>();
        let staged = [1, 2, 5, 6, 9]
            .into_iter()
            .map(|index| format!("chapter-{index}"))
            .collect::<HashSet<_>>();
        let pending = chapters
            .iter()
            .filter(|chapter| !staged.contains(&chapter.id))
            .cloned()
            .collect::<Vec<_>>();

        let work = build_contiguous_shard_work(&chapters, &pending, 1);
        let ranges = work
            .iter()
            .map(|work| {
                work.chapters
                    .iter()
                    .map(|chapter| chapter.index)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(ranges, vec![vec![3, 4], vec![7, 8], vec![10]]);
    }

    #[test]
    fn resumed_pending_runs_split_only_inside_each_run() {
        let chapters = (1..=12).map(chapter).collect::<Vec<_>>();
        let staged = [1, 6, 7, 12]
            .into_iter()
            .map(|index| format!("chapter-{index}"))
            .collect::<HashSet<_>>();
        let pending = chapters
            .iter()
            .filter(|chapter| !staged.contains(&chapter.id))
            .cloned()
            .collect::<Vec<_>>();

        let work = build_contiguous_shard_work(&chapters, &pending, 3);
        let ranges = work
            .iter()
            .map(|work| {
                work.chapters
                    .iter()
                    .map(|chapter| chapter.index)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        assert_eq!(ranges, vec![vec![2, 3, 4, 5], vec![8, 9, 10, 11]]);
        assert!(ranges.iter().all(|range| range.windows(2).all(|pair| pair[1] == pair[0] + 1)));
    }

    #[test]
    fn resumed_shards_include_completed_neighbors_as_read_only_context() {
        let chapters = (1..=5).map(chapter).collect::<Vec<_>>();
        let target = vec![chapters[2].clone()];
        let work = build_contiguous_shard_work(&chapters, &target, 1);
        let staged = HashMap::from([
            (
                chapters[1].id.clone(),
                StagedChapterOutput {
                    title: Some("第二章改写".to_string()),
                    content: Some("前文已完成改写".to_string()),
                },
            ),
            (
                chapters[3].id.clone(),
                StagedChapterOutput {
                    title: None,
                    content: Some("后文已完成改写".to_string()),
                },
            ),
        ]);

        let context = build_readonly_adjacent_context(&work[0], &staged, "rewrite");

        assert!(context.contains("前一相邻章节"));
        assert!(context.contains("第二章改写"));
        assert!(context.contains("前文已完成改写"));
        assert!(context.contains("后一相邻章节"));
        assert!(context.contains("后文已完成改写"));
        assert!(!context.contains("YURI_REWRITE_CHAPTER_START"));
    }

    #[test]
    fn shard_context_limits_model_to_current_shard_chapters() {
        let chapters = (25..=27).map(chapter).collect::<Vec<_>>();

        let context = format_shard_context(8, 10, 10, "第1-30章", &chapters);

        assert!(!context.contains("分片 9/10"));
        assert!(!context.contains("并发请求数"));
        assert!(context.contains("第25-27章"));
        assert!(context.contains("第25章、第26章、第27章"));
        assert!(context.contains("严禁输出输入外的任何章节"));
    }
}
