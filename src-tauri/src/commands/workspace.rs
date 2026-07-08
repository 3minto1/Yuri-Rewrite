use crate::domain::{AppState, CanonAsset, CanonAssetInput, Chapter, ChapterBatch, JobEstimate};
use crate::task_control::{AutoRunControl, AutoRunProgressState};
use crate::{
    chapter_text_chars, estimate_requests_for_chapters, estimate_wait_seconds_for_chapters,
    load_canon_assets, load_chapter_batches, load_chapters, load_recent_model_stats,
    load_review_enabled, load_rewrite_parallelism, row_to_chapter, to_string,
};
use chrono::Utc;
use rusqlite::params;
use tauri::State;

fn chapter_edit_is_allowed(
    state: &State<'_, AppState>,
    conn: &rusqlite::Connection,
    chapter: &Chapter,
) -> Result<(), String> {
    let auto_run = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(&chapter.novel_id)
        .cloned();
    if let Some(control) = auto_run {
        let progress = state
            .auto_run_progress
            .lock()
            .map_err(to_string)?
            .get(&chapter.novel_id)
            .cloned();
        return chapter_edit_is_allowed_for_auto_run(conn, chapter, &control, progress.as_ref());
    }

    if let Some(job_type) = state
        .active_tasks
        .novel_active_job_type(&chapter.novel_id)?
    {
        return active_task_edit_is_allowed(&job_type);
    }
    Ok(())
}

fn active_task_edit_is_allowed(job_type: &str) -> Result<(), String> {
    if job_type == "分析" {
        Ok(())
    } else {
        Err("当前小说任务正在运行，不能编辑改写稿。".to_string())
    }
}

fn chapter_edit_is_allowed_for_auto_run(
    conn: &rusqlite::Connection,
    chapter: &Chapter,
    control: &AutoRunControl,
    _progress: Option<&AutoRunProgressState>,
) -> Result<(), String> {
    let batch_index = chapter_batch_index(conn, chapter)?;
    match control.status.as_str() {
        "paused" | "running" => {
            if batch_index > control.completed_batches {
                let message = if control.status == "paused" {
                    "暂停任务当前未完成批次及后续批次不能编辑。"
                } else {
                    "当前一键任务正在处理当前批次或后续批次，不能编辑改写稿。"
                };
                Err(message.to_string())
            } else {
                Ok(())
            }
        }
        _ => Err("当前一键任务正在处理或等待暂停，不能编辑改写稿。".to_string()),
    }
}

fn chapter_batch_index(conn: &rusqlite::Connection, chapter: &Chapter) -> Result<i64, String> {
    conn.query_row(
        "SELECT batch_index FROM chapter_batches WHERE novel_id = ?1 AND ?2 BETWEEN start_chapter AND end_chapter LIMIT 1",
        params![chapter.novel_id, chapter.index],
        |row| row.get::<_, i64>(0),
    )
    .map_err(to_string)
}

fn chapter_title_edit_is_allowed(
    state: &State<'_, AppState>,
    chapter: &Chapter,
) -> Result<(), String> {
    if state.active_tasks.novel_is_active(&chapter.novel_id)? {
        return Err("当前小说任务正在运行，不能修改章节名称。".to_string());
    }
    if state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .contains_key(&chapter.novel_id)
    {
        return Err("当前一键任务正在运行或暂停，不能修改章节名称。".to_string());
    }
    Ok(())
}

fn load_chapter_by_id(conn: &rusqlite::Connection, chapter_id: &str) -> Result<Chapter, String> {
    conn.query_row(
        "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text,
            rewrite_edited_at IS NOT NULL,
            EXISTS (SELECT 1 FROM chapter_rewrite_snapshots WHERE chapter_id = chapters.id),
            analysis_status, rewrite_status
         FROM chapters WHERE id = ?1",
        params![chapter_id],
        row_to_chapter,
    )
    .map_err(to_string)
}

#[tauri::command]
pub(crate) fn update_chapter_title(
    chapter_id: String,
    title: String,
    state: State<AppState>,
) -> Result<Chapter, String> {
    let normalized_title = title.trim();
    if normalized_title.is_empty() {
        return Err("章节名称不能为空。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let chapter = load_chapter_by_id(&conn, &chapter_id)?;
    chapter_title_edit_is_allowed(&state, &chapter)?;
    conn.execute(
        "UPDATE chapters SET title = ?1 WHERE id = ?2",
        params![normalized_title, chapter_id],
    )
    .map_err(to_string)?;
    load_chapter_by_id(&conn, &chapter_id)
}

#[tauri::command]
pub(crate) fn save_chapter_rewrite_edit(
    chapter_id: String,
    rewrite_text: String,
    state: State<AppState>,
) -> Result<Chapter, String> {
    if rewrite_text.trim().is_empty() {
        return Err("改写正文不能为空。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let chapter = load_chapter_by_id(&conn, &chapter_id)?;
    if chapter.rewrite_status != "completed"
        || chapter
            .rewrite_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err("当前章节尚无可编辑的已完成改写稿。".to_string());
    }
    chapter_edit_is_allowed(&state, &conn, &chapter)?;
    conn.execute(
        "UPDATE chapters SET ai_rewrite_text = COALESCE(ai_rewrite_text, rewrite_text), rewrite_text = ?1, rewrite_edited_at = ?2 WHERE id = ?3",
        params![rewrite_text, Utc::now().to_rfc3339(), chapter_id],
    )
    .map_err(to_string)?;
    load_chapter_by_id(&conn, &chapter_id)
}

#[tauri::command]
pub(crate) fn restore_chapter_rewrite_edit(
    chapter_id: String,
    state: State<AppState>,
) -> Result<Chapter, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let chapter = load_chapter_by_id(&conn, &chapter_id)?;
    if chapter.rewrite_status != "completed"
        || chapter
            .rewrite_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err("当前章节尚无可编辑的已完成改写稿。".to_string());
    }
    chapter_edit_is_allowed(&state, &conn, &chapter)?;
    let restored = conn
        .execute(
            "UPDATE chapters SET rewrite_text = ai_rewrite_text, rewrite_edited_at = NULL WHERE id = ?1 AND ai_rewrite_text IS NOT NULL AND trim(ai_rewrite_text) != ''",
            params![chapter_id],
        )
        .map_err(to_string)?;
    if restored == 0 {
        return Err("当前章节没有可恢复的 AI 改写稿。".to_string());
    }
    load_chapter_by_id(&conn, &chapter_id)
}

#[tauri::command]
pub(crate) fn restore_single_chapter_rewrite(
    chapter_id: String,
    state: State<AppState>,
) -> Result<Chapter, String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let chapter = load_chapter_by_id(&conn, &chapter_id)?;
    chapter_edit_is_allowed(&state, &conn, &chapter)?;
    restore_single_chapter_snapshot(&mut conn, &chapter_id)?;
    load_chapter_by_id(&conn, &chapter_id)
}

fn restore_single_chapter_snapshot(
    conn: &mut rusqlite::Connection,
    chapter_id: &str,
) -> Result<(), String> {
    let snapshot = conn
        .query_row(
            "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at
             FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
            params![chapter_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => "当前章节没有可恢复的初稿。".to_string(),
            other => to_string(other),
        })?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "UPDATE chapters
         SET title = ?1, rewrite_text = ?2, ai_rewrite_text = ?3,
             rewrite_edited_at = ?4, rewrite_status = 'completed'
         WHERE id = ?5",
        params![snapshot.0, snapshot.1, snapshot.2, snapshot.3, chapter_id],
    )
    .map_err(to_string)?;
    tx.execute(
        "DELETE FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
        params![chapter_id],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn list_chapter_batches(
    novel_id: String,
    state: State<AppState>,
) -> Result<Vec<ChapterBatch>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_chapter_batches(&conn, &novel_id)
}

#[tauri::command]
pub(crate) fn estimate_job_cost(
    novel_id: String,
    batch_id: Option<String>,
    profile_id: Option<String>,
    state: State<AppState>,
) -> Result<JobEstimate, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel_id)?;
    let batches = load_chapter_batches(&conn, &novel_id)?;
    let parallelism = load_rewrite_parallelism(&conn)?;
    let review_enabled = load_review_enabled(&conn)?;
    let chapters_by_batch = batches
        .iter()
        .map(|batch| {
            chapters
                .iter()
                .filter(|chapter| {
                    chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let selected_batch_index = batch_id
        .as_deref()
        .and_then(|id| batches.iter().position(|batch| batch.id == id))
        .unwrap_or(0);
    let selected_batch = chapters_by_batch
        .get(selected_batch_index)
        .cloned()
        .unwrap_or_default();
    let current_batch_requests =
        estimate_requests_for_chapters(&selected_batch, parallelism, review_enabled);
    let full_run_requests = chapters_by_batch
        .iter()
        .map(|batch_chapters| {
            estimate_requests_for_chapters(batch_chapters, parallelism, review_enabled)
        })
        .sum::<usize>();
    let stats = profile_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| load_recent_model_stats(&conn, id).ok())
        .unwrap_or_default();
    let average_call_seconds = stats.average_call_seconds();
    let average_input_chars = stats.average_input_chars();
    Ok(JobEstimate {
        novel_chapters: chapters.len(),
        novel_chars: chapters.iter().map(chapter_text_chars).sum(),
        novel_batches: batches.len(),
        selected_batch_chapters: selected_batch.len(),
        selected_batch_chars: selected_batch.iter().map(chapter_text_chars).sum(),
        parallelism,
        review_enabled,
        current_batch_requests,
        full_run_requests,
        average_call_seconds,
        estimated_current_batch_seconds: estimate_wait_seconds_for_chapters(
            &selected_batch,
            parallelism,
            review_enabled,
            average_call_seconds,
            average_input_chars,
        ),
        estimated_full_run_seconds: average_call_seconds.map(|_| {
            chapters_by_batch
                .iter()
                .filter_map(|batch_chapters| {
                    estimate_wait_seconds_for_chapters(
                        batch_chapters,
                        parallelism,
                        review_enabled,
                        average_call_seconds,
                        average_input_chars,
                    )
                })
                .sum()
        }),
        recent_success_calls: stats.success_calls,
        recent_failed_calls: stats.failed_calls,
        average_input_chars,
        average_output_chars: stats.average_output_chars(),
    })
}

#[tauri::command]
pub(crate) fn update_canon_assets(
    novel_id: String,
    assets: Vec<CanonAssetInput>,
    state: State<AppState>,
) -> Result<Vec<CanonAsset>, String> {
    if state.active_tasks.novel_is_active(&novel_id)?
        || state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .contains_key(&novel_id)
    {
        return Err("当前小说任务运行中，不能修改一致性资产。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let updated_at = Utc::now().to_rfc3339();
    for asset in assets {
        conn.execute(
            r#"
            INSERT INTO canon_assets (novel_id, kind, content, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(novel_id, kind) DO UPDATE SET
                content = excluded.content,
                updated_at = excluded.updated_at
            "#,
            params![novel_id, asset.kind, asset.content, updated_at],
        )
        .map_err(to_string)?;
    }
    load_canon_assets(&conn, &novel_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use rusqlite::Connection;
    use std::collections::HashSet;

    fn seed_batches(conn: &Connection) {
        init_db(conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (
                id, title, source_path, encoding, status, detected_chapters, created_at
             ) VALUES ('novel-1', '测试', '', 'utf-8', 'ready', 20, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapter_batches (
                id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at
             ) VALUES ('batch-1', 'novel-1', 1, '第一批', 1, 10, '', 'now')",
            [],
        )
        .expect("insert batch 1");
        conn.execute(
            "INSERT INTO chapter_batches (
                id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at
             ) VALUES ('batch-2', 'novel-1', 2, '第二批', 11, 20, '', 'now')",
            [],
        )
        .expect("insert batch 2");
    }

    fn chapter(index: i64) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: format!("第{index}章"),
            original_text: "原文".to_string(),
            analysis_json: None,
            rewrite_text: Some("改写".to_string()),
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "completed".to_string(),
            rewrite_status: "completed".to_string(),
        }
    }

    fn auto_control(status: &str, completed_batches: i64) -> AutoRunControl {
        AutoRunControl {
            status: status.to_string(),
            start_batch_index: 0,
            completed_batches,
            job_id: Some("job-1".to_string()),
            profile_ids: HashSet::new(),
            recoverable: true,
        }
    }

    fn auto_progress(phase: &str) -> AutoRunProgressState {
        AutoRunProgressState {
            phase: Some(phase.to_string()),
            ..AutoRunProgressState::default()
        }
    }

    #[test]
    fn active_analysis_task_allows_rewrite_edit_but_rewrite_task_blocks_it() {
        assert!(active_task_edit_is_allowed("分析").is_ok());
        assert!(active_task_edit_is_allowed("改写").is_err());
        assert!(active_task_edit_is_allowed("重新改写本章").is_err());
    }

    #[test]
    fn running_auto_allows_completed_prior_batches_across_phases() {
        let conn = Connection::open_in_memory().expect("open database");
        seed_batches(&conn);
        let control = auto_control("running", 1);
        let progress = auto_progress("analysis");

        chapter_edit_is_allowed_for_auto_run(&conn, &chapter(5), &control, Some(&progress))
            .expect("completed batch can be edited");
        assert!(chapter_edit_is_allowed_for_auto_run(
            &conn,
            &chapter(15),
            &control,
            Some(&progress)
        )
        .is_err());
        let rewrite_progress = auto_progress("rewrite");

        chapter_edit_is_allowed_for_auto_run(&conn, &chapter(5), &control, Some(&rewrite_progress))
            .expect("completed batch can be edited while later batch rewrites");
        chapter_edit_is_allowed_for_auto_run(&conn, &chapter(5), &control, None)
            .expect("completed batch does not depend on progress phase");
    }

    #[test]
    fn paused_auto_keeps_completed_batch_edit_rule() {
        let conn = Connection::open_in_memory().expect("open database");
        seed_batches(&conn);
        let control = auto_control("paused", 1);

        chapter_edit_is_allowed_for_auto_run(&conn, &chapter(5), &control, None)
            .expect("paused completed batch can be edited");
        assert!(chapter_edit_is_allowed_for_auto_run(&conn, &chapter(15), &control, None).is_err());
    }

    #[test]
    fn restoring_single_chapter_snapshot_restores_exact_initial_state() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (
                id, title, source_path, encoding, status, detected_chapters, created_at
             ) VALUES ('novel-1', '测试', '', 'utf-8', 'ready', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text, analysis_json,
                rewrite_text, ai_rewrite_text, rewrite_edited_at, analysis_status, rewrite_status
             ) VALUES (
                'chapter-1', 'novel-1', 1, '新标题', '原文', NULL,
                '重新改写稿', '重新改写稿', NULL, 'completed', 'completed'
             )",
            [],
        )
        .expect("insert chapter");
        conn.execute(
            "INSERT INTO chapter_rewrite_snapshots (
                chapter_id, title, rewrite_text, ai_rewrite_text, rewrite_edited_at, created_at
             ) VALUES (
                'chapter-1', '初始标题', '人工修改过的初稿', '最初 AI 稿', 'edited-at', 'now'
             )",
            [],
        )
        .expect("insert snapshot");

        restore_single_chapter_snapshot(&mut conn, "chapter-1").expect("restore snapshot");

        let restored: (String, String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at
                 FROM chapters WHERE id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load restored chapter");
        assert_eq!(restored.0, "初始标题");
        assert_eq!(restored.1, "人工修改过的初稿");
        assert_eq!(restored.2.as_deref(), Some("最初 AI 稿"));
        assert_eq!(restored.3.as_deref(), Some("edited-at"));
        let snapshot_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chapter_rewrite_snapshots WHERE chapter_id = 'chapter-1'",
                [],
                |row| row.get(0),
            )
            .expect("count snapshots");
        assert_eq!(snapshot_count, 0);
    }
}
