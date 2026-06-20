use super::shard_context::split_chapters_for_parallelism;
use crate::domain::Chapter;
use crate::to_string;
use rusqlite::{params, Connection};

pub(crate) fn estimate_requests_for_chapters(
    chapters: &[Chapter],
    rewrite_parallelism: usize,
    review_enabled: bool,
) -> usize {
    if chapters.is_empty() {
        return 0;
    }
    let shard_count = split_chapters_for_parallelism(chapters, rewrite_parallelism).len();
    shard_count * if review_enabled { 7 } else { 2 }
}

fn estimate_wait_stages_for_chapters(chapters: &[Chapter], review_enabled: bool) -> usize {
    if chapters.is_empty() {
        0
    } else if review_enabled {
        7
    } else {
        2
    }
}

pub(crate) fn estimate_wait_seconds_for_chapters(
    chapters: &[Chapter],
    rewrite_parallelism: usize,
    review_enabled: bool,
    average_call_seconds: Option<f64>,
    average_input_chars: Option<usize>,
) -> Option<f64> {
    let average_call_seconds = average_call_seconds?;
    if chapters.is_empty() {
        return Some(0.0);
    }

    let shard_count = split_chapters_for_parallelism(chapters, rewrite_parallelism).len();
    let total_chars = chapters.iter().map(chapter_text_chars).sum::<usize>();
    let average_shard_chars = total_chars as f64 / shard_count as f64;
    let size_factor = average_input_chars
        .filter(|chars| *chars > 0)
        .map(|chars| {
            let relative_size = (average_shard_chars / chars as f64).clamp(0.05, 4.0);
            0.4 + relative_size * 0.6
        })
        .unwrap_or(1.0);

    Some(
        average_call_seconds
            * estimate_wait_stages_for_chapters(chapters, review_enabled) as f64
            * size_factor,
    )
}

pub(crate) fn chapter_text_chars(chapter: &Chapter) -> usize {
    chapter.title.chars().count() + chapter.original_text.chars().count()
}

#[derive(Default)]
pub(crate) struct RecentModelStats {
    pub(crate) success_calls: usize,
    pub(crate) failed_calls: usize,
    total_elapsed_seconds: f64,
    elapsed_samples: usize,
    total_input_chars: usize,
    input_samples: usize,
    total_output_chars: usize,
    output_samples: usize,
}

impl RecentModelStats {
    pub(crate) fn average_call_seconds(&self) -> Option<f64> {
        if self.elapsed_samples == 0 {
            None
        } else {
            Some(self.total_elapsed_seconds / self.elapsed_samples as f64)
        }
    }

    pub(crate) fn average_input_chars(&self) -> Option<usize> {
        self.total_input_chars.checked_div(self.input_samples)
    }

    pub(crate) fn average_output_chars(&self) -> Option<usize> {
        self.total_output_chars.checked_div(self.output_samples)
    }
}

pub(crate) fn load_recent_model_stats(
    conn: &Connection,
    profile_id: &str,
) -> Result<RecentModelStats, String> {
    let mut stmt = conn
        .prepare(
            "SELECT status, content FROM ai_logs WHERE profile_id = ?1 ORDER BY created_at DESC LIMIT 80",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![profile_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    let mut stats = RecentModelStats::default();
    for (status, content) in rows {
        if status == "success" {
            stats.success_calls += 1;
            if let Some(value) = extract_usize_after_label(&content, "输入字符数：") {
                stats.total_input_chars += value;
                stats.input_samples += 1;
            }
            if let Some(value) = extract_usize_after_label(&content, "输出字符数：") {
                stats.total_output_chars += value;
                stats.output_samples += 1;
            }
            if let Some(value) = extract_f64_after_label(&content, "AI 调用耗时：") {
                stats.total_elapsed_seconds += value;
                stats.elapsed_samples += 1;
            }
        } else if status == "error" {
            stats.failed_calls += 1;
        }
    }
    Ok(stats)
}

fn extract_usize_after_label(text: &str, label: &str) -> Option<usize> {
    extract_value_after_label(text, label)?
        .parse::<usize>()
        .ok()
}

fn extract_f64_after_label(text: &str, label: &str) -> Option<f64> {
    extract_value_after_label(text, label)?.parse::<f64>().ok()
}

fn extract_value_after_label(text: &str, label: &str) -> Option<String> {
    let rest = text.split_once(label)?.1.trim_start();
    let value = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use chrono::Utc;

    fn chapter(index: i64, body: &str) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: format!("第{index}章"),
            original_text: body.to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "completed".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    #[test]
    fn estimate_requests_include_analysis_rewrite_and_optional_review() {
        let chapters = (1..=30)
            .map(|index| chapter(index, "原文"))
            .collect::<Vec<_>>();

        assert_eq!(estimate_requests_for_chapters(&chapters, 6, false), 12);
        assert_eq!(estimate_requests_for_chapters(&chapters, 6, true), 42);
        assert_eq!(estimate_requests_for_chapters(&chapters[..3], 10, true), 21);
        assert_eq!(estimate_requests_for_chapters(&[], 6, true), 0);
    }

    #[test]
    fn estimate_wait_stages_follow_pipeline_not_shard_count() {
        let chapters = (1..=30)
            .map(|index| chapter(index, "原文"))
            .collect::<Vec<_>>();

        assert_eq!(split_chapters_for_parallelism(&chapters, 6).len(), 6);
        assert_eq!(estimate_wait_stages_for_chapters(&chapters, false), 2);
        assert_eq!(estimate_wait_stages_for_chapters(&chapters, true), 7);
        assert_eq!(estimate_wait_stages_for_chapters(&[], true), 0);
    }

    #[test]
    fn estimated_wait_accounts_for_larger_shards_at_lower_parallelism() {
        let body = "原".repeat(1_000);
        let chapters = (1..=50)
            .map(|index| chapter(index, &body))
            .collect::<Vec<_>>();

        let low_parallelism =
            estimate_wait_seconds_for_chapters(&chapters, 6, false, Some(60.0), Some(10_000))
                .expect("estimate low parallelism");
        let high_parallelism =
            estimate_wait_seconds_for_chapters(&chapters, 50, false, Some(60.0), Some(10_000))
                .expect("estimate high parallelism");

        assert!(low_parallelism > high_parallelism);
        assert_eq!(
            estimate_wait_seconds_for_chapters(&[], 6, false, Some(60.0), Some(10_000)),
            Some(0.0)
        );
    }

    #[test]
    fn estimated_wait_distinguishes_twenty_five_and_fifty_way_shards() {
        let body = "原".repeat(3_000);
        let chapters = (1..=100)
            .map(|index| chapter(index, &body))
            .collect::<Vec<_>>();

        let twenty_five =
            estimate_wait_seconds_for_chapters(&chapters, 25, false, Some(90.0), Some(27_000))
                .expect("estimate 25-way shards");
        let fifty =
            estimate_wait_seconds_for_chapters(&chapters, 50, false, Some(90.0), Some(27_000))
                .expect("estimate 50-way shards");

        assert!(twenty_five > fifty);
    }

    #[test]
    fn recent_model_stats_default_to_no_history() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        let stats = load_recent_model_stats(&conn, "missing-profile").expect("load stats");

        assert_eq!(stats.success_calls, 0);
        assert_eq!(stats.failed_calls, 0);
        assert_eq!(stats.average_call_seconds(), None);
        assert_eq!(stats.average_input_chars(), None);
        assert_eq!(stats.average_output_chars(), None);
    }

    #[test]
    fn recent_model_stats_parse_log_content() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, created_at) VALUES (?1, NULL, ?2, '测试', NULL, 'success', ?3, ?4)",
            params![
                "log-1",
                "profile-1",
                "调用统计：\n- 输入字符数：120\n- 输出字符数：30\n- AI 调用耗时：2.50 秒\n\n正文",
                Utc::now().to_rfc3339()
            ],
        )
        .expect("insert success log");
        conn.execute(
            "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, created_at) VALUES (?1, NULL, ?2, '测试', NULL, 'error', 'HTTP 401', ?3)",
            params!["log-2", "profile-1", Utc::now().to_rfc3339()],
        )
        .expect("insert error log");

        let stats = load_recent_model_stats(&conn, "profile-1").expect("load stats");

        assert_eq!(stats.success_calls, 1);
        assert_eq!(stats.failed_calls, 1);
        assert_eq!(stats.average_call_seconds(), Some(2.5));
        assert_eq!(stats.average_input_chars(), Some(120));
        assert_eq!(stats.average_output_chars(), Some(30));
    }
}
