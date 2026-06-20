use crate::domain::{AppState, Chapter, Job};
use crate::services::rewrite::{rewrite_and_save, RewriteRunContext};
use crate::{
    append_ai_log, build_relevant_canon_text, build_single_chapter_rewrite_from_draft_prompt,
    chapter_has_source_body, create_job, ensure_name_mapping_asset, format_batch_label,
    format_model_log_content, generate_text, load_canon_assets, load_chapter_batches,
    load_chapters, load_chapters_for_batch, load_core_prompt, load_job, load_model_profile,
    load_review_enabled, load_review_profile_for_run, load_review_profile_id,
    load_rewrite_parallelism, mark_chapters_rewrite_failed, mark_empty_source_chapters_skipped,
    parse_rewrite_model_output, read_stored_api_key, require_novel_settings,
    restore_orphaned_rewrite_status_for_chapter, rewrite_batch_with_parallelism,
    set_chapter_status, to_string, truncate_text, truncate_text_tail, update_job,
    SYSTEM_REWRITE_EXPERT,
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
    source_mode: Option<String>,
    state: State<'_, AppState>,
) -> Result<Chapter, String> {
    let source_mode = source_mode.as_deref().unwrap_or("original");
    if !matches!(source_mode, "original" | "rewrite") {
        return Err("单章重写来源只能选择原文或改写稿。".to_string());
    }
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (
        all_chapters,
        mut chapter,
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
    let (review_profile, review_api_key) = if source_mode == "original" {
        load_review_profile_for_run(
            &state,
            &profile,
            review_enabled,
            review_profile_id.as_deref(),
        )?
    } else {
        (None, None)
    };
    let mut active_profile_ids = vec![profile.id.as_str()];
    if let Some(review_profile) = review_profile.as_ref() {
        if review_profile.id != profile.id {
            active_profile_ids.push(review_profile.id.as_str());
        }
    }
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, active_profile_ids, "重新改写本章")?;
    if chapter.rewrite_status == "running" {
        let conn = state.conn.lock().map_err(to_string)?;
        if restore_orphaned_rewrite_status_for_chapter(&conn, &chapter.id).map_err(to_string)? {
            chapter.rewrite_status = "completed".to_string();
        }
    }
    if chapter.rewrite_status != "completed"
        || chapter
            .rewrite_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err("当前章节尚未完成改写。".to_string());
    }
    let cancellation = state.single_rewrite_tasks.register(&novel_id)?;
    set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
    let operation = async {
        ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;
        let canon_assets = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_canon_assets(&conn, &novel_id)?
        };
        let target = vec![chapter.clone()];
        let canon_text = build_relevant_canon_text(&canon_assets, &target, &settings);
        let custom_instructions = instructions.trim();
        let single_chapter_core_prompt = if custom_instructions.is_empty() {
            core_prompt.clone()
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

        if source_mode == "rewrite" {
            let adjacent_context = build_single_chapter_adjacent_context(&all_chapters, &chapter);
            let prompt = build_single_chapter_rewrite_from_draft_prompt(
                &chapter,
                &canon_text,
                &settings,
                &core_prompt,
                &adjacent_context,
                custom_instructions,
            );
            match generate_text(
                &state.client,
                Some(state.rate_limits.clone()),
                &profile,
                &api_key,
                SYSTEM_REWRITE_EXPERT,
                &prompt,
                false,
            )
            .await
            {
                Ok(output) => {
                    append_ai_log(
                        &state,
                        Some(&novel_id),
                        &profile.id,
                        "单章基于改写稿重写",
                        Some(&chapter.title),
                        "success",
                        &format_model_log_content(&output, &profile, None),
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    parse_rewrite_model_output(&output, &target)
                }
                Err(error) => Err(error),
            }
        } else {
            rewrite_batch_with_parallelism(
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
            .await
        }
    };
    let result = tokio::select! {
        result = operation => result,
        _ = cancellation.cancelled() => Err("单章重写已终止。".to_string()),
    };
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

#[tauri::command]
pub(crate) fn terminate_single_chapter_rewrite(
    novel_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state.single_rewrite_tasks.cancel(&novel_id)? {
        Ok(())
    } else {
        Err("当前小说没有正在运行的单章重写任务。".to_string())
    }
}

fn build_single_chapter_adjacent_context(chapters: &[Chapter], target: &Chapter) -> String {
    let Some(position) = chapters.iter().position(|chapter| chapter.id == target.id) else {
        return "无相邻章节。".to_string();
    };
    let format_neighbor = |label: &str, chapter: &Chapter, use_tail: bool| {
        let (source, text) = chapter
            .rewrite_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .map(|text| ("已完成改写稿", text))
            .unwrap_or(("原文", chapter.original_text.as_str()));
        let summary = if use_tail {
            truncate_text_tail(text.trim(), 600)
        } else {
            truncate_text(text.trim(), 600)
        };
        format!(
            "{}：内部索引 {} · 标题：{}\n{}摘要：{}",
            label, chapter.index, chapter.title, source, summary
        )
    };
    let mut context = Vec::new();
    if let Some(previous) = position
        .checked_sub(1)
        .and_then(|index| chapters.get(index))
    {
        context.push(format_neighbor("前一相邻章节", previous, true));
    }
    if let Some(next) = chapters.get(position + 1) {
        context.push(format_neighbor("后一相邻章节", next, false));
    }
    if context.is_empty() {
        "无相邻章节。".to_string()
    } else {
        context.join("\n\n")
    }
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
    tx.execute(
        "INSERT OR IGNORE INTO chapter_rewrite_snapshots (
                chapter_id, title, rewrite_text, ai_rewrite_text, rewrite_edited_at, created_at
             )
             SELECT id, title, rewrite_text, ai_rewrite_text, rewrite_edited_at, ?1
             FROM chapters
             WHERE id = ?2
               AND rewrite_text IS NOT NULL
               AND trim(rewrite_text) != ''
            ",
        params![Utc::now().to_rfc3339(), original_id],
    )
    .map_err(to_string)?;
    let snapshot_exists = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chapter_rewrite_snapshots WHERE chapter_id = ?1)",
            params![original_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(to_string)?;
    if !snapshot_exists {
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

    fn chapter(
        index: i64,
        title: &str,
        original_text: &str,
        rewrite_text: Option<&str>,
    ) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: title.to_string(),
            original_text: original_text.to_string(),
            analysis_json: None,
            rewrite_text: rewrite_text.map(str::to_string),
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "completed".to_string(),
            rewrite_status: "completed".to_string(),
        }
    }

    #[test]
    fn draft_rewrite_context_prefers_neighbor_rewrites_and_falls_back_to_original() {
        let chapters = vec![
            chapter(1, "第一章", "前章原文", Some("前章改写稿")),
            chapter(2, "第二章", "目标原文", Some("目标改写稿")),
            chapter(3, "第三章", "后章原文", None),
        ];
        let context = build_single_chapter_adjacent_context(&chapters, &chapters[1]);
        assert!(context.contains("前一相邻章节"));
        assert!(context.contains("已完成改写稿摘要：前章改写稿"));
        assert!(context.contains("后一相邻章节"));
        assert!(context.contains("原文摘要：后章原文"));
    }

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

        let second_rewrite = ParsedChapterRewrite {
            id: "chapter-1".to_string(),
            index: 1,
            title: "第二次标题".to_string(),
            text: "第二次重新改写稿".to_string(),
        };
        persist_single_chapter_rewrite(&mut conn, "chapter-1", &second_rewrite)
            .expect("persist second rewrite");
        let preserved_snapshot: (String, String) = conn
            .query_row(
                "SELECT title, rewrite_text FROM chapter_rewrite_snapshots
                 WHERE chapter_id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load preserved snapshot");
        assert_eq!(preserved_snapshot.0, "初始标题");
        assert_eq!(preserved_snapshot.1, "人工修改过的初稿");
        let second_current: (String, String) = conn
            .query_row(
                "SELECT title, rewrite_text FROM chapters WHERE id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load second rewrite");
        assert_eq!(second_current.0, "第二次标题");
        assert_eq!(second_current.1, "第二次重新改写稿");
    }
}
