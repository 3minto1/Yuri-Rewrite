use crate::domain::{AppState, CanonAsset, CanonAssetInput, Chapter, ChapterBatch, JobEstimate};
use crate::{
    chapter_text_chars, estimate_requests_for_chapters, estimate_wait_stages_for_chapters,
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
    if state.active_tasks.novel_is_active(&chapter.novel_id)? {
        return Err("当前小说任务正在运行，不能编辑改写稿。".to_string());
    }
    let paused = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(&chapter.novel_id)
        .cloned();
    if let Some(control) = paused {
        if control.status != "paused" {
            return Err("当前一键任务正在处理或等待暂停，不能编辑改写稿。".to_string());
        }
        let batch_index = conn
            .query_row(
                "SELECT batch_index FROM chapter_batches WHERE novel_id = ?1 AND ?2 BETWEEN start_chapter AND end_chapter LIMIT 1",
                params![chapter.novel_id, chapter.index],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_string)?;
        if batch_index > control.completed_batches {
            return Err("暂停任务当前未完成批次及后续批次不能编辑。".to_string());
        }
    }
    Ok(())
}

fn load_chapter_by_id(conn: &rusqlite::Connection, chapter_id: &str) -> Result<Chapter, String> {
    conn.query_row(
        "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, rewrite_edited_at IS NOT NULL, analysis_status, rewrite_status FROM chapters WHERE id = ?1",
        params![chapter_id],
        row_to_chapter,
    )
    .map_err(to_string)
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
        || chapter.rewrite_text.as_deref().is_none_or(str::is_empty)
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
    let current_batch_wait_stages =
        estimate_wait_stages_for_chapters(&selected_batch, review_enabled);
    let full_run_requests = chapters_by_batch
        .iter()
        .map(|batch_chapters| {
            estimate_requests_for_chapters(batch_chapters, parallelism, review_enabled)
        })
        .sum::<usize>();
    let full_run_wait_stages = chapters_by_batch
        .iter()
        .map(|batch_chapters| estimate_wait_stages_for_chapters(batch_chapters, review_enabled))
        .sum::<usize>();
    let stats = profile_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| load_recent_model_stats(&conn, id).ok())
        .unwrap_or_default();
    let average_call_seconds = stats.average_call_seconds();
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
        estimated_current_batch_seconds: average_call_seconds
            .map(|seconds| seconds * current_batch_wait_stages as f64),
        estimated_full_run_seconds: average_call_seconds
            .map(|seconds| seconds * full_run_wait_stages as f64),
        recent_success_calls: stats.success_calls,
        recent_failed_calls: stats.failed_calls,
        average_input_chars: stats.average_input_chars(),
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
