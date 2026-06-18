use crate::domain::{AppState, Job};
use crate::services::analysis::analyze_and_save;
use crate::{
    chapter_has_source_body, create_job, ensure_name_mapping_asset, format_batch_label,
    load_chapters_for_batch, load_job, load_model_profile, load_rewrite_parallelism,
    mark_chapters_analysis_failed, mark_empty_source_chapters_skipped, read_stored_api_key,
    require_novel_settings, set_chapter_status, to_string, update_job,
};
use tauri::State;

#[tauri::command]
pub(crate) async fn start_analysis(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (all_chapters, settings, rewrite_parallelism) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        (
            load_chapters_for_batch(&conn, &novel_id, &batch_id)?,
            settings,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if all_chapters.is_empty() {
        return Err("当前批次没有可分析的内容。".to_string());
    }
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, [&profile.id], "分析")?;
    let total = all_chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "analysis", total)?;
    let batch_label = format_batch_label(&all_chapters);
    mark_empty_source_chapters_skipped(&state, &all_chapters)?;
    let chapters = all_chapters
        .into_iter()
        .filter(chapter_has_source_body)
        .collect::<Vec<_>>();

    update_job(
        &state,
        &job.id,
        "running",
        0,
        &format!("正在批次分析 {}", batch_label),
    )?;
    if chapters.is_empty() {
        ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;
        update_job(
            &state,
            &job.id,
            "completed",
            total,
            "当前批次仅包含空正文伪章节，已跳过模型调用",
        )?;
        return load_job(&state, &job.id);
    }
    for chapter in &chapters {
        set_chapter_status(&state, &chapter.id, "analysis_status", "running")?;
    }

    if let Err(error) = analyze_and_save(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &chapters,
        rewrite_parallelism,
    )
    .await
    {
        mark_chapters_analysis_failed(&state, &chapters)?;
        update_job(&state, &job.id, "failed", 0, &error)?;
        job = load_job(&state, &job.id)?;
        return Ok(job);
    }
    ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;

    update_job(
        &state,
        &job.id,
        "completed",
        total,
        "分析完成，姓名映射表已更新",
    )?;
    load_job(&state, &job.id)
}
