use crate::domain::{AppState, Chapter, ChapterBatch, Job};
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
use rusqlite::{params, Connection};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

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
    let export_suffix = auto_run_export_suffix(&batches, start_batch_index);
    let cumulative_export_path = auto_run_export_path(&output_dir, &novel.title, &export_suffix);
    remove_legacy_auto_batch_exports(&output_dir, &novel.title, &batches)?;

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

        let export_chapters = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_auto_run_export_chapters(
                &conn,
                &novel_id,
                &batches,
                start_batch_index,
                idx as i64,
            )?
        };
        if let Err(error) =
            write_auto_run_cumulative_export(&app, &cumulative_export_path, &export_chapters).await
        {
            update_job(&state, &job.id, "failed", completed_in_range, &error)?;
            emit_job_progress(&app, &job, "failed", completed_in_range, &error);
            clear_auto_run(&state, &novel_id)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }
        let exported_message = format!(
            "已更新合并导出至第 {} 批：{}",
            current,
            cumulative_export_path.to_string_lossy()
        );
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

    update_job(
        &state,
        &job.id,
        "completed",
        range_total,
        &format!(
            "一键分析改写完成，已输出：{}",
            cumulative_export_path.to_string_lossy()
        ),
    )?;
    emit_job_progress(
        &app,
        &job,
        "completed",
        range_total,
        &format!(
            "一键分析改写完成，已输出：{}",
            cumulative_export_path.to_string_lossy()
        ),
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

fn auto_run_export_suffix(batches: &[ChapterBatch], start_batch_index: i64) -> String {
    if start_batch_index == 0 {
        "全文".to_string()
    } else {
        format!(
            "{}起",
            chinese_batch_label(batches[start_batch_index as usize].batch_index)
        )
    }
}

fn auto_run_export_path(output_dir: &Path, novel_title: &str, export_suffix: &str) -> PathBuf {
    output_dir.join(format!(
        "{}_{}.txt",
        sanitize_file_name(novel_title),
        export_suffix
    ))
}

fn legacy_auto_batch_export_path(
    output_dir: &Path,
    novel_title: &str,
    batch: &ChapterBatch,
) -> PathBuf {
    output_dir.join(format!(
        "{}_{}.txt",
        sanitize_file_name(novel_title),
        chinese_batch_label(batch.batch_index)
    ))
}

fn remove_legacy_auto_batch_exports(
    output_dir: &Path,
    novel_title: &str,
    batches: &[ChapterBatch],
) -> Result<(), String> {
    for batch in batches {
        let path = legacy_auto_batch_export_path(output_dir, novel_title, batch);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "无法清理旧批次导出文件：{}。请关闭正在占用该 TXT 的阅读器或编辑器后重试。系统错误：{}",
                    path.to_string_lossy(),
                    to_string(error)
                )
            })?;
        }
    }
    Ok(())
}

fn load_auto_run_export_chapters(
    conn: &Connection,
    novel_id: &str,
    batches: &[ChapterBatch],
    start_batch_index: i64,
    end_batch_index: i64,
) -> Result<Vec<Chapter>, String> {
    let mut chapters = Vec::new();
    for batch in &batches[start_batch_index as usize..=end_batch_index as usize] {
        chapters.extend(load_chapters_for_batch(conn, novel_id, &batch.id)?);
    }
    Ok(chapters)
}

async fn write_auto_run_cumulative_export(
    app: &AppHandle,
    path: &Path,
    chapters: &[Chapter],
) -> Result<(), String> {
    let body = build_rewritten_export_body(chapters)?;
    loop {
        match fs::write(path, &body) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let error_message = to_string(error);
                let retry = prompt_user_to_close_locked_export(app, path, &error_message).await?;
                if !retry {
                    return Err(format!(
                        "用户取消更新合并导出文件：{}。系统错误：{}",
                        path.to_string_lossy(),
                        error_message
                    ));
                }
            }
        }
    }
}

fn locked_export_dialog_message(path: &Path, error_message: &str) -> String {
    format!(
        "无法更新累计 TXT：{}\n\n该文件可能正在被阅读器或编辑器打开，导致程序暂时无法写入。\n\n请手动关闭正在打开这个 TXT 的阅读器/编辑器窗口，确认文件已不再被占用后，再点击“已关闭，继续更新”。\n\n程序不会尝试关闭任何外部程序。\n\n系统错误：{}",
        path.to_string_lossy(),
        error_message
    )
}

async fn prompt_user_to_close_locked_export(
    app: &AppHandle,
    path: &Path,
    error_message: &str,
) -> Result<bool, String> {
    let message = locked_export_dialog_message(path, error_message);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .message(message)
            .title("累计 TXT 被占用")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "已关闭，继续更新".to_string(),
                "取消任务".to_string(),
            ))
            .blocking_show()
    })
    .await
    .map_err(to_string)
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

    #[test]
    fn auto_run_uses_one_cumulative_export_path_for_a_range() {
        let batches = vec![sample_batch(1), sample_batch(2), sample_batch(3)];
        let output_dir = PathBuf::from("C:/exports");

        let first_path = auto_run_export_path(
            &output_dir,
            "测试小说",
            &auto_run_export_suffix(&batches, 0),
        );
        let second_path = auto_run_export_path(
            &output_dir,
            "测试小说",
            &auto_run_export_suffix(&batches, 0),
        );
        let range_path = auto_run_export_path(
            &output_dir,
            "测试小说",
            &auto_run_export_suffix(&batches, 1),
        );

        assert_eq!(first_path, second_path);
        assert!(first_path.ends_with("测试小说_全文.txt"));
        assert!(range_path.ends_with("测试小说_第二批起.txt"));
        assert_ne!(
            first_path,
            legacy_auto_batch_export_path(&output_dir, "测试小说", &batches[0])
        );
    }

    #[test]
    fn locked_export_dialog_message_asks_for_manual_close_only() {
        let message = locked_export_dialog_message(
            Path::new("C:/exports/测试小说_全文.txt"),
            "另一个程序正在使用此文件。",
        );

        assert!(message.contains("请手动关闭"));
        assert!(message.contains("已关闭，继续更新"));
        assert!(message.contains("程序不会尝试关闭任何外部程序"));
        assert!(message.contains("C:/exports/测试小说_全文.txt"));
    }
}
