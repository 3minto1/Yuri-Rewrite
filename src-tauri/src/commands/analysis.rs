use crate::domain::{AppState, Job};
use crate::{
    analyze_batch_with_parallelism, create_job, ensure_name_mapping_asset, format_batch_label,
    load_chapters_for_batch, load_job, load_model_profile, load_rewrite_parallelism,
    mark_chapters_analysis_failed, read_stored_api_key, require_novel_settings,
    save_parsed_analyses, set_chapter_status, to_string, update_job,
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
    let (chapters, settings, rewrite_parallelism) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        (
            load_chapters_for_batch(&conn, &novel_id, &batch_id)?,
            settings,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有可分析的内容。".to_string());
    }
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, [&profile.id], "分析")?;
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "analysis", total)?;
    let batch_label = format_batch_label(&chapters);

    update_job(
        &state,
        &job.id,
        "running",
        0,
        &format!("正在批次分析 {}", batch_label),
    )?;
    for chapter in &chapters {
        set_chapter_status(&state, &chapter.id, "analysis_status", "running")?;
    }

    let parsed_analysis = match analyze_batch_with_parallelism(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &chapters,
        rewrite_parallelism,
    )
    .await
    {
        Ok(parsed) => parsed,
        Err(error) => {
            mark_chapters_analysis_failed(&state, &chapters)?;
            update_job(&state, &job.id, "failed", 0, &error)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }
    };

    save_parsed_analyses(&state, &novel_id, &chapters, parsed_analysis)?;
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
