use crate::domain::{AppState, Job};
use crate::task_control::AutoRunCleanup;
use crate::{
    analyze_chapters_for_auto, build_rewritten_export_body, chinese_batch_label, clear_auto_run,
    create_job, emit_job_progress, finish_stopped_auto_run, load_chapter_batches,
    load_chapters_for_batch, load_job, load_model_profile, load_review_enabled,
    load_review_profile_for_run, load_review_profile_id, prepare_auto_run, read_stored_api_key,
    register_auto_run_job, request_auto_run_stop, requested_auto_run_stop, require_novel_settings,
    resolve_rewrite_export_dir, rewrite_chapters_for_auto, row_to_novel, sanitize_file_name,
    set_auto_run_completed, to_string, update_job, AUTO_RUN_PAUSED, AUTO_RUN_TERMINATED,
};
use rusqlite::params;
use std::{collections::HashSet, fs};
use tauri::{AppHandle, State};

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
    let (novel, batches, review_enabled, review_profile_id) = {
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
        )
    };
    if batches.is_empty() {
        return Err("当前小说没有可处理的批次。".to_string());
    }
    let requested_start_batch_index =
        resolve_auto_run_start_batch_index(&batches, start_batch_id.as_deref())?;

    let (review_profile, _review_api_key) = load_review_profile_for_run(
        &state,
        &profile,
        review_enabled,
        review_profile_id.as_deref(),
    )?;
    let output_dir = {
        let conn = state.conn.lock().map_err(to_string)?;
        resolve_rewrite_export_dir(&conn, &state.data_dir)?
    };
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let mut active_profile_ids = vec![profile.id.as_str()];
    if let Some(review_profile) = review_profile.as_ref() {
        if review_profile.id != profile.id {
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
    let _auto_run_cleanup = AutoRunCleanup::new(&state.auto_runs, &novel_id);
    let range_total = batches.len() as i64 - start_batch_index;
    let mut job = create_job(&state, &novel_id, "auto", range_total)?;
    register_auto_run_job(&state, &novel_id, &job.id, resume_from, start_batch_index)?;
    let completed_in_range = resume_from.saturating_sub(start_batch_index);
    let start_message = if resume_from > start_batch_index {
        format!("继续一键分析改写，将从第 {} 批重新开始", resume_from + 1)
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
        if let Err(error) =
            analyze_chapters_for_auto(&state, &novel_id, &profile, &api_key, &chapters).await
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
        update_job(
            &state,
            &job.id,
            "running",
            completed_in_range,
            &rewrite_message,
        )?;
        emit_job_progress(&app, &job, "running", completed_in_range, &rewrite_message);
        if let Err(error) =
            rewrite_chapters_for_auto(&state, &novel_id, &profile, &api_key, &batch.id).await
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
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }

        let rewritten_batch = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_chapters_for_batch(&conn, &novel_id, &batch.id)?
        };
        let body = build_rewritten_export_body(&rewritten_batch)?;
        let batch_path = output_dir.join(format!(
            "{}_{}.txt",
            sanitize_file_name(&novel.title),
            chinese_batch_label(batch.batch_index)
        ));
        fs::write(&batch_path, body).map_err(to_string)?;
        let exported_message = format!("已输出第 {} 批：{}", current, batch_path.to_string_lossy());
        let completed_range_batches = current.saturating_sub(start_batch_index);
        update_job(
            &state,
            &job.id,
            "running",
            completed_range_batches,
            &exported_message,
        )?;
        set_auto_run_completed(&state, &novel_id, current)?;
        emit_job_progress(
            &app,
            &job,
            "running",
            completed_range_batches,
            &exported_message,
        );
    }

    let export_batches = &batches[start_batch_index as usize..];
    let mut export_chapters = Vec::new();
    for batch in export_batches {
        let conn = state.conn.lock().map_err(to_string)?;
        export_chapters.extend(load_chapters_for_batch(&conn, &novel_id, &batch.id)?);
    }
    let full_body = build_rewritten_export_body(&export_chapters)?;
    let export_suffix = auto_run_export_suffix(&batches, start_batch_index);
    let full_path = output_dir.join(format!(
        "{}_{}.txt",
        sanitize_file_name(&novel.title),
        export_suffix
    ));
    fs::write(&full_path, full_body).map_err(to_string)?;

    update_job(
        &state,
        &job.id,
        "completed",
        range_total,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    )?;
    emit_job_progress(
        &app,
        &job,
        "completed",
        range_total,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    );
    clear_auto_run(&state, &novel_id)?;
    load_job(&state, &job.id)
}

fn resolve_auto_run_start_batch_index(
    batches: &[crate::domain::ChapterBatch],
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

fn auto_run_export_suffix(
    batches: &[crate::domain::ChapterBatch],
    start_batch_index: i64,
) -> String {
    if start_batch_index == 0 {
        "全文".to_string()
    } else {
        format!(
            "{}起",
            chinese_batch_label(batches[start_batch_index as usize].batch_index)
        )
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
    fn resolves_optional_start_batch_and_range_export_name() {
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
        assert_eq!(auto_run_export_suffix(&batches, 0), "全文");
        assert_eq!(auto_run_export_suffix(&batches, 1), "第二批起");
    }
}
