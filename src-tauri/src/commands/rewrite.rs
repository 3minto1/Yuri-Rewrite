use crate::domain::{AppState, Job};
use crate::{
    build_compact_canon_text, create_job, ensure_name_mapping_asset, format_batch_label,
    load_canon_assets, load_chapters_for_batch, load_core_prompt, load_job, load_model_profile,
    load_review_enabled, load_review_profile_for_run, load_review_profile_id,
    load_rewrite_parallelism, mark_chapters_rewrite_failed, read_stored_api_key,
    require_novel_settings, rewrite_batch_with_parallelism, save_parsed_rewrites,
    set_chapter_status, to_string, update_job,
};
use tauri::State;

#[tauri::command]
pub(crate) async fn start_rewrite(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (chapters, settings, core_prompt, review_enabled, review_profile_id, rewrite_parallelism) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        let chapters = load_chapters_for_batch(&conn, &novel_id, &batch_id)?
            .into_iter()
            .filter(|chapter| chapter.analysis_status == "completed")
            .collect::<Vec<_>>();
        (
            chapters,
            settings,
            load_core_prompt(&conn)?,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有已完成分析的内容，请先分析该批次。".to_string());
    }

    let (review_profile, review_api_key) = load_review_profile_for_run(
        &state,
        &profile,
        review_enabled,
        review_profile_id.as_deref(),
    )?;
    let mut active_profile_ids = vec![profile.id.as_str()];
    if let Some(review_profile) = review_profile.as_ref() {
        if review_profile.id != profile.id {
            active_profile_ids.push(review_profile.id.as_str());
        }
    }
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, active_profile_ids, "改写")?;
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, &novel_id)?
    };
    let canon_text = build_compact_canon_text(&canon_assets);
    let batch_label = format_batch_label(&chapters);

    for chapter in &chapters {
        set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
    }

    update_job(
        &state,
        &job.id,
        "running",
        0,
        &format!("正在批次改写 {}", batch_label),
    )?;
    let final_rewrite = match rewrite_batch_with_parallelism(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &chapters,
        &canon_text,
        &settings,
        &core_prompt,
        review_enabled,
        review_profile.as_ref(),
        review_api_key.as_deref(),
        rewrite_parallelism,
    )
    .await
    {
        Ok(rewrites) => rewrites,
        Err(error) => {
            mark_chapters_rewrite_failed(&state, &chapters)?;
            update_job(&state, &job.id, "failed", 0, &error)?;
            job = load_job(&state, &job.id)?;
            return Ok(job);
        }
    };

    save_parsed_rewrites(&state, final_rewrite)?;

    update_job(
        &state,
        &job.id,
        "completed",
        total,
        if review_enabled {
            "改写与复检完成"
        } else {
            "改写完成"
        },
    )?;
    load_job(&state, &job.id)
}
