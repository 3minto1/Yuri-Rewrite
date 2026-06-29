use crate::domain::{AppState, AutoRunRecovery, AutoRunRecoverySummary, Job};
use crate::{row_to_job, to_string};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use tauri::State;

const MAX_RECOVERY_PENDING_RANGES: usize = 5;

struct AutoRunCheckpointRow {
    novel_id: String,
    start_batch_index: i64,
    next_batch_index: i64,
    status: String,
    pause_reason: String,
    phase: Option<String>,
    batch_index: Option<i64>,
    profile_ids: Vec<String>,
    job_id: Option<String>,
}

#[tauri::command]
pub(crate) fn get_job(job_id: String, state: State<AppState>) -> Result<Job, String> {
    load_job(&state, &job_id)
}

#[tauri::command]
pub(crate) fn list_auto_run_recoveries(
    state: State<AppState>,
) -> Result<Vec<AutoRunRecovery>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT novel_id, start_batch_index, next_batch_index, status, pause_reason, phase, batch_index, profile_ids, job_id FROM auto_run_checkpoints ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let rows = stmt.query_map([], |row| {
        let profile_json: String = row.get(7)?;
        let profile_ids = serde_json::from_str(&profile_json).unwrap_or_default();
        Ok(AutoRunCheckpointRow {
            novel_id: row.get(0)?,
            start_batch_index: row.get(1)?,
            next_batch_index: row.get(2)?,
            status: row.get(3)?,
            pause_reason: row.get(4)?,
            phase: row.get(5)?,
            batch_index: row.get(6)?,
            profile_ids,
            job_id: row.get(8)?,
        })
    })
    .map_err(to_string)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(to_string)?;
    rows.into_iter()
        .map(|row| {
            let job = row
                .job_id
                .as_deref()
                .and_then(|id| {
                    conn.query_row(
                        "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
                        params![id],
                        row_to_job,
                    )
                    .ok()
                });
            let summary = build_auto_run_recovery_summary(
                &conn,
                &row.novel_id,
                row.phase.clone(),
                row.batch_index,
            )?;
            Ok(AutoRunRecovery {
                novel_id: row.novel_id,
                start_batch_index: row.start_batch_index,
                next_batch_index: row.next_batch_index,
                status: row.status,
                pause_reason: row.pause_reason,
                phase: row.phase,
                batch_index: row.batch_index,
                profile_ids: row.profile_ids,
                job,
                summary,
            })
        })
        .collect()
}

fn build_auto_run_recovery_summary(
    conn: &Connection,
    novel_id: &str,
    phase: Option<String>,
    batch_index: Option<i64>,
) -> Result<Option<AutoRunRecoverySummary>, String> {
    let Some(phase) = phase.filter(|value| matches!(value.as_str(), "analysis" | "rewrite")) else {
        return Ok(None);
    };
    let Some(batch_index) = batch_index else {
        return Ok(None);
    };
    let batch = match conn.query_row(
        "SELECT id, label, start_chapter, end_chapter
         FROM chapter_batches
         WHERE novel_id = ?1 AND batch_index = ?2",
        params![novel_id, batch_index],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        },
    ) {
        Ok(batch) => batch,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(to_string(error)),
    };
    let (_, batch_label, start_chapter, end_chapter) = batch;
    let chapters = load_batch_chapter_indexes(conn, novel_id, start_chapter, end_chapter)?;
    if chapters.is_empty() {
        return Ok(None);
    }
    let staged = load_staged_chapter_indexes(conn, novel_id, batch_index, &phase)?;
    let total_chapters = chapters.len();
    let staged_chapters = chapters
        .iter()
        .filter(|(_, chapter_id)| staged.contains(chapter_id.as_str()))
        .count();
    let pending_ranges_all = pending_chapter_ranges(&chapters, &staged);
    let pending_ranges_truncated = pending_ranges_all.len() > MAX_RECOVERY_PENDING_RANGES;
    let pending_ranges = pending_ranges_all
        .into_iter()
        .take(MAX_RECOVERY_PENDING_RANGES)
        .collect::<Vec<_>>();
    Ok(Some(AutoRunRecoverySummary {
        phase,
        batch_index,
        batch_label,
        total_chapters,
        staged_chapters,
        pending_chapters: total_chapters.saturating_sub(staged_chapters),
        pending_ranges,
        pending_ranges_truncated,
    }))
}

fn load_batch_chapter_indexes(
    conn: &Connection,
    novel_id: &str,
    start_chapter: i64,
    end_chapter: i64,
) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT chapter_index, id
             FROM chapters
             WHERE novel_id = ?1 AND chapter_index BETWEEN ?2 AND ?3
             ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let chapters = stmt
        .query_map(params![novel_id, start_chapter, end_chapter], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}

fn load_staged_chapter_indexes(
    conn: &Connection,
    novel_id: &str,
    batch_index: i64,
    phase: &str,
) -> Result<HashSet<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT chapter_id
             FROM auto_run_shard_outputs
             WHERE novel_id = ?1 AND batch_index = ?2 AND phase = ?3",
        )
        .map_err(to_string)?;
    let staged = stmt
        .query_map(params![novel_id, batch_index, phase], |row| row.get(0))
        .map_err(to_string)?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(to_string)?;
    Ok(staged)
}

fn pending_chapter_ranges(chapters: &[(i64, String)], staged: &HashSet<String>) -> Vec<String> {
    let mut ranges = Vec::new();
    let mut start: Option<i64> = None;
    let mut end: Option<i64> = None;
    for (chapter_index, chapter_id) in chapters {
        if staged.contains(chapter_id.as_str()) {
            if let (Some(range_start), Some(range_end)) = (start.take(), end.take()) {
                ranges.push(format_chapter_range(range_start, range_end));
            }
            continue;
        }
        if start.is_none() {
            start = Some(*chapter_index);
        }
        end = Some(*chapter_index);
    }
    if let (Some(range_start), Some(range_end)) = (start, end) {
        ranges.push(format_chapter_range(range_start, range_end));
    }
    ranges
}

fn format_chapter_range(start: i64, end: i64) -> String {
    if start == end {
        format!("第{start}章")
    } else {
        format!("第{start}-{end}章")
    }
}

pub(crate) fn load_job(state: &State<'_, AppState>, job_id: &str) -> Result<Job, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
        params![job_id],
        row_to_job,
    )
    .map_err(to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_chapter_ranges_skip_staged_gaps() {
        let chapters = (1..=10)
            .map(|index| (index, format!("chapter-{index}")))
            .collect::<Vec<_>>();
        let staged = [1, 2, 5, 6, 9]
            .into_iter()
            .map(|index| format!("chapter-{index}"))
            .collect::<HashSet<_>>();

        assert_eq!(
            pending_chapter_ranges(&chapters, &staged),
            vec!["第3-4章", "第7-8章", "第10章"]
        );
    }
}
