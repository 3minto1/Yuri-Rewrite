use crate::domain::{AppState, ChapterBatch, Job};
use crate::rate_limit::is_rate_limit_retry_exhausted;
use crate::task_control::AutoRunCleanup;
use crate::{
    analyze_chapters_for_auto, begin_auto_batch_progress, clear_auto_run, create_job,
    emit_job_progress, finish_stopped_auto_run, load_analysis_profile_for_run,
    load_analysis_profile_id, load_chapter_batches, load_chapters_for_batch, load_job,
    load_model_profile, load_review_enabled, load_review_profile_for_run, load_review_profile_id,
    pause_auto_run_after_model_format_error, pause_auto_run_after_network_error,
    pause_auto_run_after_rate_limit, pause_auto_run_after_temporary_gateway_error,
    prepare_auto_run, read_stored_api_key, register_auto_run_job, request_auto_run_stop,
    requested_auto_run_stop, require_novel_settings, rewrite_chapters_for_auto, row_to_novel,
    set_auto_run_completed, to_string, update_auto_run_checkpoint_phase, update_job,
    AUTO_RUN_PAUSED, AUTO_RUN_TERMINATED,
};
use crate::{
    is_recoverable_model_format_error, is_recoverable_network_error, is_temporary_gateway_error,
};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoverableAutoRunFailure {
    RateLimit,
    TemporaryGateway,
    Network,
    ModelFormat,
}

fn paused_recovery_job_type(
    state: &State<'_, AppState>,
    novel_id: &str,
) -> Result<Option<String>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let job_type = conn
        .query_row(
            "SELECT jobs.job_type
             FROM auto_run_checkpoints
             JOIN jobs ON jobs.id = auto_run_checkpoints.job_id
             WHERE auto_run_checkpoints.novel_id = ?1
               AND auto_run_checkpoints.status = 'paused'",
            params![novel_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(job_type)
}

fn paused_recovery_batch_index(
    state: &State<'_, AppState>,
    novel_id: &str,
) -> Result<Option<i64>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT batch_index
         FROM auto_run_checkpoints
         WHERE novel_id = ?1 AND status = 'paused'",
        params![novel_id],
        |row| row.get::<_, Option<i64>>(0),
    )
    .optional()
    .map(|value| value.flatten())
    .map_err(to_string)
}

#[allow(clippy::too_many_arguments)]
fn pause_auto_run_after_recoverable_failure(
    kind: RecoverableAutoRunFailure,
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    error: &str,
) -> Result<Job, String> {
    match kind {
        RecoverableAutoRunFailure::RateLimit => pause_auto_run_after_rate_limit(
            state,
            app,
            job,
            completed_batches,
            start_batch_index,
            error,
        ),
        RecoverableAutoRunFailure::TemporaryGateway => {
            pause_auto_run_after_temporary_gateway_error(
                state,
                app,
                job,
                completed_batches,
                start_batch_index,
                error,
            )
        }
        RecoverableAutoRunFailure::Network => pause_auto_run_after_network_error(
            state,
            app,
            job,
            completed_batches,
            start_batch_index,
            error,
        ),
        RecoverableAutoRunFailure::ModelFormat => pause_auto_run_after_model_format_error(
            state,
            app,
            job,
            completed_batches,
            start_batch_index,
            error,
        ),
    }
}

#[tauri::command]
pub(crate) async fn start_analyze_rewrite_batch(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (batch, chapters, review_enabled, review_profile_id, analysis_profile_id) = {
        let conn = state.conn.lock().map_err(to_string)?;
        require_novel_settings(&conn, &novel_id)?;
        let batch = load_chapter_batches(&conn, &novel_id)?
            .into_iter()
            .find(|batch| batch.id == batch_id)
            .ok_or_else(|| "未找到当前批次。".to_string())?;
        let chapters = load_chapters_for_batch(&conn, &novel_id, &batch.id)?;
        (
            batch,
            chapters,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_analysis_profile_id(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有可处理的内容。".to_string());
    }
    if let Some(job_type) = paused_recovery_job_type(&state, &novel_id)? {
        if job_type != "auto_batch" {
            return Err("当前有未完成的全文一键分析改写任务，请先继续或终止该任务。".to_string());
        }
        if paused_recovery_batch_index(&state, &novel_id)? != Some(batch.batch_index) {
            return Err("当前有未完成的其他批次一键任务，请先继续或终止该任务。".to_string());
        }
    }

    let (review_profile, _review_api_key) = load_review_profile_for_run(
        &state,
        &profile,
        review_enabled,
        review_profile_id.as_deref(),
    )?;
    let (analysis_profile, analysis_api_key) =
        load_analysis_profile_for_run(&state, &profile, analysis_profile_id.as_deref())?;
    let mut active_profile_ids = vec![profile.id.as_str()];
    if analysis_profile.id != profile.id {
        active_profile_ids.push(analysis_profile.id.as_str());
    }
    if let Some(review_profile) = review_profile.as_ref() {
        if !active_profile_ids.contains(&review_profile.id.as_str()) {
            active_profile_ids.push(review_profile.id.as_str());
        }
    }
    let profile_ids = active_profile_ids
        .iter()
        .map(|profile_id| (*profile_id).to_string())
        .collect::<HashSet<_>>();
    let _active_task = state.active_tasks.acquire(
        &novel_id,
        active_profile_ids.iter().copied(),
        "一键分析改写当前批次",
    )?;
    let current_start_batch_index = batch.batch_index.saturating_sub(1);
    let (resume_from, start_batch_index) =
        prepare_auto_run(&state, &novel_id, profile_ids, current_start_batch_index)?;
    if start_batch_index != current_start_batch_index || resume_from != current_start_batch_index {
        return Err("当前有未完成的一键分析改写任务，请先继续或终止该任务。".to_string());
    }
    let _auto_run_cleanup = AutoRunCleanup::new(
        &state.auto_runs,
        &state.auto_run_progress,
        &state.conn,
        &novel_id,
    );
    let mut job = create_job(&state, &novel_id, "auto_batch", 1)?;
    register_auto_run_job(
        &state,
        &novel_id,
        &job.id,
        current_start_batch_index,
        current_start_batch_index,
    )?;
    let start_message = format!("准备分析并改写当前批次：{}", batch.label);
    update_job(&state, &job.id, "running", 0, &start_message)?;
    emit_job_progress(&app, &job, "running", 0, &start_message);

    update_auto_run_checkpoint_phase(&state, &novel_id, "analysis", batch.batch_index)?;
    begin_auto_batch_progress(&state, &novel_id, "analysis", 1, 1, &batch.label)?;
    if let Err(error) = analyze_chapters_for_auto(
        &state,
        &novel_id,
        &analysis_profile,
        &analysis_api_key,
        &chapters,
        Some(batch.batch_index),
    )
    .await
    {
        if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
            return finish_stopped_auto_run(
                &state,
                &app,
                job,
                current_start_batch_index,
                current_start_batch_index,
                &error,
            );
        }
        if let Some(kind) = classify_recoverable_auto_run_failure(&error) {
            return pause_auto_run_after_recoverable_failure(
                kind,
                &state,
                &app,
                job,
                current_start_batch_index,
                current_start_batch_index,
                &error,
            );
        }
        update_job(&state, &job.id, "failed", 0, &error)?;
        emit_job_progress(&app, &job, "failed", 0, &error);
        clear_auto_run(&state, &novel_id)?;
        return load_job(&state, &job.id);
    }
    if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
        return finish_stopped_auto_run(
            &state,
            &app,
            job,
            current_start_batch_index,
            current_start_batch_index,
            &status,
        );
    }

    update_auto_run_checkpoint_phase(&state, &novel_id, "rewrite", batch.batch_index)?;
    begin_auto_batch_progress(&state, &novel_id, "rewrite", 1, 1, &batch.label)?;
    if let Err(error) = rewrite_chapters_for_auto(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &batch.id,
        Some(batch.batch_index),
    )
    .await
    {
        if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
            return finish_stopped_auto_run(
                &state,
                &app,
                job,
                current_start_batch_index,
                current_start_batch_index,
                &error,
            );
        }
        if let Some(kind) = classify_recoverable_auto_run_failure(&error) {
            return pause_auto_run_after_recoverable_failure(
                kind,
                &state,
                &app,
                job,
                current_start_batch_index,
                current_start_batch_index,
                &error,
            );
        }
        update_job(&state, &job.id, "failed", 0, &error)?;
        emit_job_progress(&app, &job, "failed", 0, &error);
        clear_auto_run(&state, &novel_id)?;
        return load_job(&state, &job.id);
    }

    let completed_message = format!("当前批次分析与改写完成：{}", batch.label);
    update_job(&state, &job.id, "completed", 1, &completed_message)?;
    emit_job_progress(&app, &job, "completed", 1, &completed_message);
    clear_auto_run(&state, &novel_id)?;
    job = load_job(&state, &job.id)?;
    Ok(job)
}

fn classify_recoverable_auto_run_failure(error: &str) -> Option<RecoverableAutoRunFailure> {
    if is_rate_limit_retry_exhausted(error) {
        Some(RecoverableAutoRunFailure::RateLimit)
    } else if is_temporary_gateway_error(error) {
        Some(RecoverableAutoRunFailure::TemporaryGateway)
    } else if is_recoverable_network_error(error) {
        Some(RecoverableAutoRunFailure::Network)
    } else if is_recoverable_model_format_error(error) {
        Some(RecoverableAutoRunFailure::ModelFormat)
    } else {
        None
    }
}

#[tauri::command]
pub(crate) async fn start_analyze_rewrite_all(
    novel_id: String,
    profile_id: String,
    start_batch_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (_novel, batches, review_enabled, review_profile_id, analysis_profile_id) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let novel = conn
            .query_row(
                "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
                params![novel_id],
                row_to_novel,
            )
            .map_err(to_string)?;
        require_novel_settings(&conn, &novel.id)?;
        (
            novel,
            load_chapter_batches(&conn, &novel_id)?,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_analysis_profile_id(&conn)?,
        )
    };
    if batches.is_empty() {
        return Err("当前小说没有可处理的批次。".to_string());
    }
    if paused_recovery_job_type(&state, &novel_id)?.as_deref() == Some("auto_batch") {
        return Err("当前有未完成的当前批次一键任务，请先继续或终止该任务。".to_string());
    }
    let requested_start_batch_index =
        resolve_auto_run_start_batch_index(&batches, start_batch_id.as_deref())?;

    let (review_profile, _review_api_key) = load_review_profile_for_run(
        &state,
        &profile,
        review_enabled,
        review_profile_id.as_deref(),
    )?;
    let (analysis_profile, analysis_api_key) =
        load_analysis_profile_for_run(&state, &profile, analysis_profile_id.as_deref())?;
    let mut active_profile_ids = vec![profile.id.as_str()];
    if analysis_profile.id != profile.id {
        active_profile_ids.push(analysis_profile.id.as_str());
    }
    if let Some(review_profile) = review_profile.as_ref() {
        if !active_profile_ids.contains(&review_profile.id.as_str()) {
            active_profile_ids.push(review_profile.id.as_str());
        }
    }
    let auto_profile_ids = active_profile_ids
        .iter()
        .map(|profile_id| (*profile_id).to_string())
        .collect::<HashSet<_>>();
    let _active_task = state.active_tasks.acquire(
        &novel_id,
        active_profile_ids.iter().copied(),
        "一键分析改写",
    )?;
    let (resume_from, start_batch_index) = prepare_auto_run(
        &state,
        &novel_id,
        auto_profile_ids,
        requested_start_batch_index,
    )?;
    let _auto_run_cleanup = AutoRunCleanup::new(
        &state.auto_runs,
        &state.auto_run_progress,
        &state.conn,
        &novel_id,
    );
    let range_total = batches.len() as i64 - start_batch_index;
    let mut job = create_job(&state, &novel_id, "auto", range_total)?;
    register_auto_run_job(&state, &novel_id, &job.id, resume_from, start_batch_index)?;
    let completed_in_range = resume_from.saturating_sub(start_batch_index);
    let start_message = if resume_from > start_batch_index {
        format!(
            "继续一键分析改写，将处理第 {} 批的未完成分片",
            resume_from + 1
        )
    } else if start_batch_index > 0 {
        format!("准备从第 {} 批开始一键分析改写", start_batch_index + 1)
    } else {
        "准备开始一键分析改写".to_string()
    };
    update_job(
        &state,
        &job.id,
        "running",
        completed_in_range,
        &start_message,
    )?;
    emit_job_progress(&app, &job, "running", completed_in_range, &start_message);
    for (idx, batch) in batches.iter().enumerate() {
        let current = (idx + 1) as i64;
        if current <= resume_from {
            continue;
        }
        let completed = idx as i64;
        let completed_in_range = completed.saturating_sub(start_batch_index);
        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(
                &state,
                &app,
                job,
                completed,
                start_batch_index,
                &status,
            );
        }
        let analysis_message = format!("正在分析第 {} 批", current);
        update_auto_run_checkpoint_phase(&state, &novel_id, "analysis", current)?;
        begin_auto_batch_progress(
            &state,
            &novel_id,
            "analysis",
            current,
            batches.len() as i64,
            &batch.label,
        )?;
        update_job(
            &state,
            &job.id,
            "running",
            completed_in_range,
            &analysis_message,
        )?;
        emit_job_progress(&app, &job, "running", completed_in_range, &analysis_message);
        let chapters = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_chapters_for_batch(&conn, &novel_id, &batch.id)?
        };
        if chapters.is_empty() {
            continue;
        }
        if let Err(error) = analyze_chapters_for_auto(
            &state,
            &novel_id,
            &analysis_profile,
            &analysis_api_key,
            &chapters,
            Some(current),
        )
        .await
        {
            if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
                return finish_stopped_auto_run(
                    &state,
                    &app,
                    job,
                    completed,
                    start_batch_index,
                    &error,
                );
            }
            if let Some(kind) = classify_recoverable_auto_run_failure(&error) {
                return match kind {
                    RecoverableAutoRunFailure::RateLimit => pause_auto_run_after_rate_limit(
                        &state,
                        &app,
                        job,
                        completed,
                        start_batch_index,
                        &error,
                    ),
                    RecoverableAutoRunFailure::TemporaryGateway => {
                        pause_auto_run_after_temporary_gateway_error(
                            &state,
                            &app,
                            job,
                            completed,
                            start_batch_index,
                            &error,
                        )
                    }
                    RecoverableAutoRunFailure::Network => pause_auto_run_after_network_error(
                        &state,
                        &app,
                        job,
                        completed,
                        start_batch_index,
                        &error,
                    ),
                    RecoverableAutoRunFailure::ModelFormat => {
                        pause_auto_run_after_model_format_error(
                            &state,
                            &app,
                            job,
                            completed,
                            start_batch_index,
                            &error,
                        )
                    }
                };
            }
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }

        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(
                &state,
                &app,
                job,
                completed,
                start_batch_index,
                &status,
            );
        }
        let rewrite_message = format!("正在改写第 {} 批", current);
        update_auto_run_checkpoint_phase(&state, &novel_id, "rewrite", current)?;
        begin_auto_batch_progress(
            &state,
            &novel_id,
            "rewrite",
            current,
            batches.len() as i64,
            &batch.label,
        )?;
        update_job(
            &state,
            &job.id,
            "running",
            completed_in_range,
            &rewrite_message,
        )?;
        emit_job_progress(&app, &job, "running", completed_in_range, &rewrite_message);
        if let Err(error) = rewrite_chapters_for_auto(
            &state,
            &novel_id,
            &profile,
            &api_key,
            &batch.id,
            Some(current),
        )
        .await
        {
            if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
                return finish_stopped_auto_run(
                    &state,
                    &app,
                    job,
                    completed,
                    start_batch_index,
                    &error,
                );
            }
            if let Some(kind) = classify_recoverable_auto_run_failure(&error) {
                return match kind {
                    RecoverableAutoRunFailure::RateLimit => pause_auto_run_after_rate_limit(
                        &state,
                        &app,
                        job,
                        completed,
                        start_batch_index,
                        &error,
                    ),
                    RecoverableAutoRunFailure::TemporaryGateway => {
                        pause_auto_run_after_temporary_gateway_error(
                            &state,
                            &app,
                            job,
                            completed,
                            start_batch_index,
                            &error,
                        )
                    }
                    RecoverableAutoRunFailure::Network => pause_auto_run_after_network_error(
                        &state,
                        &app,
                        job,
                        completed,
                        start_batch_index,
                        &error,
                    ),
                    RecoverableAutoRunFailure::ModelFormat => {
                        pause_auto_run_after_model_format_error(
                            &state,
                            &app,
                            job,
                            completed,
                            start_batch_index,
                            &error,
                        )
                    }
                };
            }
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }

        let completed_range_batches = current.saturating_sub(start_batch_index);
        let completed_message = format!("已完成第 {} 批改写", current);
        update_job(
            &state,
            &job.id,
            "running",
            completed_range_batches,
            &completed_message,
        )?;
        set_auto_run_completed(&state, &novel_id, current)?;
        emit_job_progress(
            &app,
            &job,
            "running",
            completed_range_batches,
            &completed_message,
        );
    }

    let finished_message = "一键分析改写完成，可在对比页面手动导出 TXT";
    update_job(
        &state,
        &job.id,
        "completed",
        range_total,
        finished_message,
    )?;
    emit_job_progress(
        &app,
        &job,
        "completed",
        range_total,
        finished_message,
    );
    clear_auto_run(&state, &novel_id)?;
    load_job(&state, &job.id)
}

fn resolve_auto_run_start_batch_index(
    batches: &[ChapterBatch],
    start_batch_id: Option<&str>,
) -> Result<i64, String> {
    match start_batch_id {
        Some(batch_id) => batches
            .iter()
            .position(|batch| batch.id == batch_id)
            .map(|index| index as i64)
            .ok_or_else(|| "选中的起始批次不存在，请刷新小说后重试。".to_string()),
        None => Ok(0),
    }
}

#[tauri::command]
pub(crate) fn pause_analyze_rewrite_all(
    novel_id: String,
    state: State<AppState>,
) -> Result<Job, String> {
    request_auto_run_stop(&state, &novel_id, "pause_requested")
}

#[tauri::command]
pub(crate) fn terminate_analyze_rewrite_all(
    novel_id: String,
    state: State<AppState>,
) -> Result<Job, String> {
    request_auto_run_stop(&state, &novel_id, "terminate_requested")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChapterBatch;

    fn sample_batch(index: i64) -> ChapterBatch {
        ChapterBatch {
            id: format!("batch-{index}"),
            novel_id: "novel-1".to_string(),
            batch_index: index,
            label: format!("第{index}批"),
            start_chapter: (index - 1) * 30 + 1,
            end_chapter: index * 30,
            file_path: format!("batch-{index}.txt"),
            created_at: "now".to_string(),
        }
    }

    #[test]
    fn resolves_optional_start_batch() {
        let batches = vec![sample_batch(1), sample_batch(2), sample_batch(3)];

        assert_eq!(
            resolve_auto_run_start_batch_index(&batches, None).expect("full run"),
            0
        );
        assert_eq!(
            resolve_auto_run_start_batch_index(&batches, Some("batch-2")).expect("range run"),
            1
        );
        assert!(resolve_auto_run_start_batch_index(&batches, Some("missing")).is_err());
    }

    #[test]
    fn gateway_failures_pause_auto_run_before_format_recovery() {
        for status in [502, 503, 504, 524] {
            let error = format!("分析输出格式修复重试调用失败：HTTP {status}: proxy error");
            assert_eq!(
                classify_recoverable_auto_run_failure(&error),
                Some(RecoverableAutoRunFailure::TemporaryGateway)
            );
        }
        assert_eq!(
            classify_recoverable_auto_run_failure(
                "分析输出格式修复重试调用失败：HTTP 500: provider error"
            ),
            Some(RecoverableAutoRunFailure::ModelFormat)
        );
        assert_eq!(
            classify_recoverable_auto_run_failure(
                "第221-230章 · 分片 9/10 · 第229章：自动细分到单章后仍无法解析：AI 输出缺少章节开始标记；兜底解析也失败：AI 输出为空，无法兜底解析。"
            ),
            Some(RecoverableAutoRunFailure::ModelFormat)
        );
        assert_eq!(
            classify_recoverable_auto_run_failure("HTTP 401: unauthorized"),
            None
        );
    }
}
