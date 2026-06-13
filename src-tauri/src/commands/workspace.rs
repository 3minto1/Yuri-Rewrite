use crate::domain::{AppState, CanonAsset, CanonAssetInput, ChapterBatch, JobEstimate};
use crate::{
    chapter_text_chars, estimate_requests_for_chapters, estimate_wait_stages_for_chapters,
    load_canon_assets, load_chapter_batches, load_chapters, load_chapters_for_batch,
    load_recent_model_stats, load_review_enabled, load_rewrite_parallelism, to_string,
};
use chrono::Utc;
use rusqlite::params;
use tauri::State;

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
    let selected_batch = batch_id
        .as_deref()
        .and_then(|id| load_chapters_for_batch(&conn, &novel_id, id).ok())
        .or_else(|| {
            batches
                .first()
                .and_then(|batch| load_chapters_for_batch(&conn, &novel_id, &batch.id).ok())
        })
        .unwrap_or_default();
    let current_batch_requests =
        estimate_requests_for_chapters(&selected_batch, parallelism, review_enabled);
    let current_batch_wait_stages =
        estimate_wait_stages_for_chapters(&selected_batch, review_enabled);
    let full_run_requests = batches
        .iter()
        .map(|batch| {
            load_chapters_for_batch(&conn, &novel_id, &batch.id)
                .map(|batch_chapters| {
                    estimate_requests_for_chapters(&batch_chapters, parallelism, review_enabled)
                })
                .unwrap_or(0)
        })
        .sum::<usize>();
    let full_run_wait_stages = batches
        .iter()
        .map(|batch| {
            load_chapters_for_batch(&conn, &novel_id, &batch.id)
                .map(|batch_chapters| {
                    estimate_wait_stages_for_chapters(&batch_chapters, review_enabled)
                })
                .unwrap_or(0)
        })
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
