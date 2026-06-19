use crate::domain::{AppState, Chapter, Job};
use crate::services::rewrite::{rewrite_and_save, RewriteRunContext};
use crate::{
    build_relevant_canon_text, chapter_has_source_body, create_job, ensure_name_mapping_asset,
    format_batch_label, load_canon_assets, load_chapter_batches, load_chapters,
    load_chapters_for_batch, load_core_prompt, load_job, load_model_profile, load_review_enabled,
    load_review_profile_for_run, load_review_profile_id, load_rewrite_parallelism,
    mark_chapters_rewrite_failed, mark_empty_source_chapters_skipped, read_stored_api_key,
    require_novel_settings, rewrite_batch_with_parallelism, set_chapter_status, to_string,
    update_job,
};
use chrono::Utc;
use rusqlite::params;
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
            checkpoint_batch_index: None,
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

#[tauri::command]
pub(crate) async fn rewrite_single_chapter(
    novel_id: String,
    profile_id: String,
    chapter_id: String,
    instructions: String,
    state: State<'_, AppState>,
) -> Result<Chapter, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (
        all_chapters,
        chapter,
        settings,
        core_prompt,
        review_enabled,
        review_profile_id,
        rewrite_parallelism,
    ) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        let chapter = load_chapters(&conn, &novel_id)?
            .into_iter()
            .find(|chapter| chapter.id == chapter_id)
            .ok_or_else(|| "未找到要重新改写的章节。".to_string())?;
        let batch = load_chapter_batches(&conn, &novel_id)?
            .into_iter()
            .find(|batch| {
                chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter
            })
            .ok_or_else(|| "未找到该章节所属批次。".to_string())?;
        (
            load_chapters_for_batch(&conn, &novel_id, &batch.id)?,
            chapter,
            settings,
            load_core_prompt(&conn)?,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if !chapter_has_source_body(&chapter) {
        return Err("当前章节没有可改写的正文。".to_string());
    }
    if chapter.analysis_status != "completed" {
        return Err("当前章节尚未完成分析，不能单独重新改写。".to_string());
    }
    if chapter.rewrite_status != "completed"
        || chapter
            .rewrite_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err("当前章节尚未完成改写。".to_string());
    }
    if chapter.single_rewrite_original_available {
        return Err("当前章节已有可恢复的初稿，请先恢复初稿后再重新改写。".to_string());
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
        .acquire(&novel_id, active_profile_ids, "重新改写本章")?;

    ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, &novel_id)?
    };
    let target = vec![chapter.clone()];
    let canon_text = build_relevant_canon_text(&canon_assets, &target, &settings);
    let custom_instructions = instructions.trim();
    let single_chapter_core_prompt = if custom_instructions.is_empty() {
        core_prompt
    } else if core_prompt.trim().is_empty() {
        format!(
            "【本次单章重写补充要求】\n{}\n以上要求仅适用于当前目标章节；不得改写相邻只读章节，不得破坏既有姓名映射、人物关系和剧情连续性。",
            custom_instructions
        )
    } else {
        format!(
            "{}\n\n【本次单章重写补充要求】\n{}\n以上要求仅适用于当前目标章节；不得改写相邻只读章节，不得破坏既有姓名映射、人物关系和剧情连续性。",
            core_prompt.trim(),
            custom_instructions
        )
    };

    set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
    let result = rewrite_batch_with_parallelism(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &all_chapters,
        &target,
        &canon_text,
        &settings,
        &single_chapter_core_prompt,
        review_enabled,
        review_profile.as_ref(),
        review_api_key.as_deref(),
        rewrite_parallelism,
        None,
    )
    .await;
    let rewrites = match result {
        Ok(rewrites) => rewrites,
        Err(error) => {
            set_chapter_status(&state, &chapter.id, "rewrite_status", "completed")?;
            return Err(error);
        }
    };
    if let Err(error) = save_single_chapter_rewrite(&state, &chapter, rewrites) {
        set_chapter_status(&state, &chapter.id, "rewrite_status", "completed")?;
        return Err(error);
    }
    let conn = state.conn.lock().map_err(to_string)?;
    load_chapters(&conn, &novel_id)?
        .into_iter()
        .find(|item| item.id == chapter.id)
        .ok_or_else(|| "重新改写已完成，但刷新章节失败。".to_string())
}

fn save_single_chapter_rewrite(
    state: &State<'_, AppState>,
    original: &Chapter,
    rewrites: Vec<crate::ParsedChapterRewrite>,
) -> Result<(), String> {
    if rewrites.len() != 1 {
        return Err("单章重新改写返回了异常的章节数量，未覆盖现有改写稿。".to_string());
    }
    let rewrite = rewrites
        .into_iter()
        .next()
        .expect("single rewrite count checked");
    if rewrite.id != original.id {
        return Err("单章重新改写结果无法匹配当前章节，未覆盖现有改写稿。".to_string());
    }

    let mut conn = state.conn.lock().map_err(to_string)?;
    persist_single_chapter_rewrite(&mut conn, &original.id, &rewrite)
}

fn persist_single_chapter_rewrite(
    conn: &mut rusqlite::Connection,
    original_id: &str,
    rewrite: &crate::ParsedChapterRewrite,
) -> Result<(), String> {
    let tx = conn.transaction().map_err(to_string)?;
    let snapshot_inserted = tx
        .execute(
            "INSERT INTO chapter_rewrite_snapshots (
                chapter_id, title, rewrite_text, ai_rewrite_text, rewrite_edited_at, created_at
             )
             SELECT id, title, rewrite_text, ai_rewrite_text, rewrite_edited_at, ?1
             FROM chapters
             WHERE id = ?2
               AND rewrite_text IS NOT NULL
               AND trim(rewrite_text) != ''
               AND NOT EXISTS (
                   SELECT 1 FROM chapter_rewrite_snapshots WHERE chapter_id = chapters.id
               )",
            params![Utc::now().to_rfc3339(), original_id],
        )
        .map_err(to_string)?;
    if snapshot_inserted != 1 {
        return Err("保存当前章节初稿失败，未覆盖现有改写稿。".to_string());
    }
    tx.execute(
        "UPDATE chapters
         SET title = ?1, rewrite_text = ?2, ai_rewrite_text = ?2,
             rewrite_edited_at = NULL, rewrite_status = 'completed'
         WHERE id = ?3",
        params![rewrite.title.trim(), rewrite.text.trim(), rewrite.id],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use crate::ParsedChapterRewrite;
    use rusqlite::Connection;

    #[test]
    fn single_chapter_rewrite_saves_initial_draft_before_overwrite() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (
                id, title, source_path, encoding, status, detected_chapters, created_at
             ) VALUES ('novel-1', '测试', '', 'utf-8', 'ready', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text, analysis_json,
                rewrite_text, ai_rewrite_text, rewrite_edited_at, analysis_status, rewrite_status
             ) VALUES (
                'chapter-1', 'novel-1', 1, '初始标题', '原文', NULL,
                '人工修改过的初稿', '最初 AI 稿', 'edited-at', 'completed', 'completed'
             )",
            [],
        )
        .expect("insert chapter");
        let rewrite = ParsedChapterRewrite {
            id: "chapter-1".to_string(),
            index: 1,
            title: "新标题".to_string(),
            text: "重新改写稿".to_string(),
        };

        persist_single_chapter_rewrite(&mut conn, "chapter-1", &rewrite)
            .expect("persist rewrite");

        let snapshot: (String, String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at
                 FROM chapter_rewrite_snapshots WHERE chapter_id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load snapshot");
        assert_eq!(snapshot.0, "初始标题");
        assert_eq!(snapshot.1, "人工修改过的初稿");
        assert_eq!(snapshot.2.as_deref(), Some("最初 AI 稿"));
        assert_eq!(snapshot.3.as_deref(), Some("edited-at"));
        let current: (String, String, String, Option<String>) = conn
            .query_row(
                "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at
                 FROM chapters WHERE id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load rewritten chapter");
        assert_eq!(current.0, "新标题");
        assert_eq!(current.1, "重新改写稿");
        assert_eq!(current.2, "重新改写稿");
        assert!(current.3.is_none());
    }
}
