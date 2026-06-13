use crate::domain::{AppState, Job};
use crate::task_control::AutoRunCleanup;
use crate::{
    analyze_chapters_for_auto, build_rewritten_export_body, chinese_batch_label, clear_auto_run,
    create_job, emit_job_progress, finish_stopped_auto_run, load_chapter_batches, load_chapters,
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
    let resume_from = prepare_auto_run(&state, &novel_id, auto_profile_ids)?;
    let _auto_run_cleanup = AutoRunCleanup::new(&state.auto_runs, &novel_id);
    let mut job = create_job(&state, &novel_id, "auto", batches.len() as i64)?;
    register_auto_run_job(&state, &novel_id, &job.id, resume_from)?;
    let start_message = if resume_from > 0 {
        format!("继续一键分析改写，将从第 {} 批重新开始", resume_from + 1)
    } else {
        "准备开始一键分析改写".to_string()
    };
    update_job(&state, &job.id, "running", resume_from, &start_message)?;
    emit_job_progress(&app, &job, "running", resume_from, &start_message);
    for (idx, batch) in batches.iter().enumerate() {
        let current = (idx + 1) as i64;
        if current <= resume_from {
            continue;
        }
        let completed = idx as i64;
        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(&state, &app, job, completed, &status);
        }
        let analysis_message = format!("正在分析第 {} 批", current);
        update_job(&state, &job.id, "running", completed, &analysis_message)?;
        emit_job_progress(&app, &job, "running", completed, &analysis_message);
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
                return finish_stopped_auto_run(&state, &app, job, completed, &error);
            }
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }

        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(&state, &app, job, completed, &status);
        }
        let rewrite_message = format!("正在改写第 {} 批", current);
        update_job(&state, &job.id, "running", completed, &rewrite_message)?;
        emit_job_progress(&app, &job, "running", completed, &rewrite_message);
        if let Err(error) =
            rewrite_chapters_for_auto(&state, &novel_id, &profile, &api_key, &batch.id).await
        {
            if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
                return finish_stopped_auto_run(&state, &app, job, completed, &error);
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
        update_job(&state, &job.id, "running", current, &exported_message)?;
        set_auto_run_completed(&state, &novel_id, current)?;
        emit_job_progress(&app, &job, "running", current, &exported_message);
    }

    let all_chapters = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_chapters(&conn, &novel_id)?
    };
    let full_body = build_rewritten_export_body(&all_chapters)?;
    let full_path = output_dir.join(format!("{}_全文.txt", sanitize_file_name(&novel.title)));
    fs::write(&full_path, full_body).map_err(to_string)?;

    update_job(
        &state,
        &job.id,
        "completed",
        batches.len() as i64,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    )?;
    emit_job_progress(
        &app,
        &job,
        "completed",
        batches.len() as i64,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    );
    clear_auto_run(&state, &novel_id)?;
    load_job(&state, &job.id)
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
