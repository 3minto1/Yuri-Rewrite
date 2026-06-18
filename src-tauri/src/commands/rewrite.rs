use crate::domain::{AppState, Job};
use crate::services::rewrite::{rewrite_and_save, RewriteRunContext};
use crate::{
    build_relevant_canon_text, chapter_has_source_body, create_job, ensure_name_mapping_asset,
    format_batch_label, load_canon_assets, load_chapters_for_batch, load_core_prompt, load_job,
    load_model_profile, load_review_enabled, load_review_profile_for_run, load_review_profile_id,
    load_rewrite_parallelism, mark_chapters_rewrite_failed, mark_empty_source_chapters_skipped,
    read_stored_api_key, require_novel_settings, set_chapter_status, to_string, update_job,
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
    let (
        all_chapters,
        settings,
        core_prompt,
        review_enabled,
        review_profile_id,
        rewrite_parallelism,
    ) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        (
            load_chapters_for_batch(&conn, &novel_id, &batch_id)?,
            settings,
            load_core_prompt(&conn)?,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if all_chapters.is_empty() {
        return Err("当前批次没有可改写的内容。".to_string());
    }
    let has_unanalyzed_source = all_chapters
        .iter()
        .any(|chapter| chapter_has_source_body(chapter) && chapter.analysis_status != "completed");
    let chapters = all_chapters
        .iter()
        .filter(|chapter| {
            chapter_has_source_body(chapter) && chapter.analysis_status == "completed"
        })
        .cloned()
        .collect::<Vec<_>>();
    if chapters.is_empty() && has_unanalyzed_source {
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
    let total = all_chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    mark_empty_source_chapters_skipped(&state, &all_chapters)?;
    if chapters.is_empty() {
        update_job(
            &state,
            &job.id,
            "completed",
            total,
            "当前批次仅包含空正文伪章节，已清除旧占位改写并跳过模型调用",
        )?;
        return load_job(&state, &job.id);
    }
    ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, &novel_id)?
    };
    let canon_text = build_relevant_canon_text(&canon_assets, &chapters, &settings);
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
    if let Err(error) = rewrite_and_save(
        &state,
        RewriteRunContext {
            novel_id: &novel_id,
            profile: &profile,
            api_key: &api_key,
            chapters: &chapters,
            canon_text: &canon_text,
            settings: &settings,
            core_prompt: &core_prompt,
            review_enabled,
            review_profile: review_profile.as_ref(),
            review_api_key: review_api_key.as_deref(),
            parallelism: rewrite_parallelism,
        },
    )
    .await
    {
        mark_chapters_rewrite_failed(&state, &chapters)?;
        update_job(&state, &job.id, "failed", 0, &error)?;
        job = load_job(&state, &job.id)?;
        return Ok(job);
    }

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
