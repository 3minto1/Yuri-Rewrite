use crate::domain::{
    AppState, Chapter, ChapterRule, ChapterRuleLongChapterPreviewItem, ChapterRulePreview,
    ChapterRulePreviewItem, StoredChapterRule,
};
use crate::services::chapter_batch_files::{
    cleanup_stale_batch_files_for_novel, discard_prepared_generations,
    prepare_chapter_batch_generation, replace_chapter_batch_rows,
};
use crate::{
    decode_text, load_chapter_batch_size, seed_canon_assets, split_chapters,
    split_chapters_with_custom_rule, split_long_detected_chapters, to_string,
    LONG_CHAPTER_SPLIT_LIMIT,
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::Path};
use tauri::State;

#[tauri::command]
pub(crate) fn get_chapter_rule(
    novel_id: String,
    state: State<AppState>,
) -> Result<Option<StoredChapterRule>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_chapter_rule(&conn, &novel_id)
}

#[tauri::command]
pub(crate) fn preview_chapter_rule(
    novel_id: String,
    rule: ChapterRule,
    split_long_chapters: Option<bool>,
    state: State<AppState>,
) -> Result<ChapterRulePreview, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters_with_custom_rule(&novel_id, &text, &normalize_chapter_rule(rule))?;
    let long_chapters = long_chapter_preview_items(&split.chapters, split.detected_chapters);
    let chapters = maybe_split_long_detected_chapters(
        &novel_id,
        split.chapters,
        split.detected_chapters,
        split_long_chapters,
    );
    Ok(preview_from_chapters(
        &chapters,
        long_chapters,
        "预览已生成，确认无误后可保存应用。",
    ))
}

#[tauri::command]
pub(crate) fn save_chapter_rule_and_split(
    novel_id: String,
    rule: ChapterRule,
    split_long_chapters: Option<bool>,
    state: State<AppState>,
) -> Result<StoredChapterRule, String> {
    let _active_task =
        state
            .active_tasks
            .acquire(&novel_id, std::iter::empty::<&str>(), "重新分章")?;
    ensure_can_split_novel(&state, &novel_id)?;
    let normalized = normalize_chapter_rule(rule);
    let mut conn = state.conn.lock().map_err(to_string)?;
    ensure_chapter_split_allowed(&conn, &novel_id)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters_with_custom_rule(&novel_id, &text, &normalized)?;
    let chapters = maybe_split_long_detected_chapters(
        &novel_id,
        split.chapters,
        split.detected_chapters,
        split_long_chapters,
    );
    if chapters.is_empty() {
        return Err("自定义章节规则未生成任何章节。".to_string());
    }
    rebuild_pending_novel_chapters(
        &mut conn,
        &state.data_dir,
        &novel_id,
        &chapters,
        split.detected_chapters,
        Some(&normalized),
        |conn| {
            ensure_can_split_novel(&state, &novel_id)?;
            ensure_chapter_split_allowed(conn, &novel_id)
        },
    )?
    .ok_or_else(|| "章节规则未能随重新分章一并保存。".to_string())
}

#[tauri::command]
pub(crate) fn split_novel_with_builtin_rule(
    novel_id: String,
    split_long_chapters: Option<bool>,
    state: State<AppState>,
) -> Result<(), String> {
    let _active_task =
        state
            .active_tasks
            .acquire(&novel_id, std::iter::empty::<&str>(), "重新分章")?;
    ensure_can_split_novel(&state, &novel_id)?;
    let mut conn = state.conn.lock().map_err(to_string)?;
    ensure_chapter_split_allowed(&conn, &novel_id)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters(&novel_id, &text);
    let chapters = maybe_split_long_detected_chapters(
        &novel_id,
        split.chapters,
        split.detected_chapters,
        split_long_chapters,
    );
    if chapters.is_empty() {
        return Err("内置章节识别未生成任何章节。".to_string());
    }
    rebuild_pending_novel_chapters(
        &mut conn,
        &state.data_dir,
        &novel_id,
        &chapters,
        split.detected_chapters,
        None,
        |conn| {
            ensure_can_split_novel(&state, &novel_id)?;
            ensure_chapter_split_allowed(conn, &novel_id)
        },
    )
    .map(|_| ())
}

fn maybe_split_long_detected_chapters(
    novel_id: &str,
    chapters: Vec<Chapter>,
    detected_chapters: bool,
    split_long_chapters: Option<bool>,
) -> Vec<Chapter> {
    split_long_detected_chapters(
        novel_id,
        chapters,
        detected_chapters && split_long_chapters.unwrap_or(false),
    )
}

fn load_chapter_rule(
    conn: &Connection,
    novel_id: &str,
) -> Result<Option<StoredChapterRule>, String> {
    conn.query_row(
        "SELECT novel_id, rule_json, updated_at FROM chapter_rules WHERE novel_id = ?1",
        params![novel_id],
        |row| {
            let novel_id = row.get::<_, String>(0)?;
            let rule_json = row.get::<_, String>(1)?;
            let updated_at = row.get::<_, String>(2)?;
            let rule = serde_json::from_str::<ChapterRule>(&rule_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    rule_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(StoredChapterRule {
                novel_id,
                rule,
                updated_at,
            })
        },
    )
    .optional()
    .map_err(to_string)
}

fn save_chapter_rule_value(
    conn: &Connection,
    novel_id: &str,
    rule: &ChapterRule,
) -> Result<StoredChapterRule, String> {
    let updated_at = Utc::now().to_rfc3339();
    let rule_json = serde_json::to_string(rule).map_err(to_string)?;
    conn.execute(
        "INSERT INTO chapter_rules (novel_id, rule_json, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(novel_id) DO UPDATE SET
             rule_json = excluded.rule_json,
             updated_at = excluded.updated_at",
        params![novel_id, rule_json, updated_at],
    )
    .map_err(to_string)?;
    Ok(StoredChapterRule {
        novel_id: novel_id.to_string(),
        rule: rule.clone(),
        updated_at,
    })
}

fn normalize_chapter_rule(mut rule: ChapterRule) -> ChapterRule {
    rule.mode = if rule.mode == "regex" {
        "regex".to_string()
    } else {
        "simple".to_string()
    };
    rule.prefix = rule.prefix.trim().to_string();
    rule.number_type = match rule.number_type.as_str() {
        "arabic" | "chinese" => rule.number_type,
        _ => "mixed".to_string(),
    };
    rule.unit = rule.unit.trim().to_string();
    rule.include_pattern = rule.include_pattern.trim().to_string();
    rule.extra_pattern = rule.extra_pattern.trim().to_string();
    rule.regex_pattern = rule.regex_pattern.trim().to_string();
    rule
}

fn preview_from_chapters(
    chapters: &[Chapter],
    long_chapters: Vec<ChapterRuleLongChapterPreviewItem>,
    message: &str,
) -> ChapterRulePreview {
    ChapterRulePreview {
        total_chapters: chapters.len(),
        chapters: chapters
            .iter()
            .map(|chapter| ChapterRulePreviewItem {
                index: chapter.index,
                title: chapter.title.clone(),
            })
            .collect(),
        long_chapters,
        can_apply: !chapters.is_empty(),
        message: message.to_string(),
    }
}

fn long_chapter_preview_items(
    chapters: &[Chapter],
    detected_chapters: bool,
) -> Vec<ChapterRuleLongChapterPreviewItem> {
    if !detected_chapters {
        return Vec::new();
    }
    chapters
        .iter()
        .filter_map(|chapter| {
            let char_count = chapter
                .original_text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .count();
            (char_count > LONG_CHAPTER_SPLIT_LIMIT).then(|| ChapterRuleLongChapterPreviewItem {
                index: chapter.index,
                title: chapter.title.clone(),
                char_count,
            })
        })
        .collect()
}

fn load_source_text(conn: &Connection, novel_id: &str) -> Result<String, String> {
    let source_path = conn
        .query_row(
            "SELECT source_path FROM novels WHERE id = ?1",
            params![novel_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?;
    let bytes = fs::read(&source_path)
        .map_err(|error| format!("无法读取原始 TXT 文件「{}」：{}", source_path, error))?;
    let (text, _) = decode_text(&bytes);
    Ok(text)
}

fn ensure_can_split_novel(state: &State<'_, AppState>, novel_id: &str) -> Result<(), String> {
    if state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .contains_key(novel_id)
    {
        return Err("当前小说存在一键任务检查点，请先继续或终止任务后再生成章节列表。".to_string());
    }
    Ok(())
}

fn ensure_chapter_split_allowed(conn: &Connection, novel_id: &str) -> Result<(), String> {
    let status = conn
        .query_row(
            "SELECT status FROM novels WHERE id = ?1",
            params![novel_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?;
    if status == "pending_split" {
        return Ok(());
    }
    if status != "imported" {
        return Err("当前小说状态不支持重新拆分。".to_string());
    }
    if chapter_processing_trace_exists(conn, novel_id)? || task_history_exists(conn, novel_id)? {
        return Err(
            "当前小说已开始分析或改写，不能重新拆分；如需修改章节规则，请重新导入小说。"
                .to_string(),
        );
    }
    if non_empty_canon_asset_exists(conn, novel_id)? {
        return Err(
            "当前小说已有手动一致性资产内容，不能重新拆分；如需修改章节规则，请重新导入小说。"
                .to_string(),
        );
    }
    Ok(())
}

fn chapter_processing_trace_exists(conn: &Connection, novel_id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM chapters
            WHERE novel_id = ?1
              AND (
                analysis_status != 'pending'
                OR rewrite_status != 'pending'
                OR COALESCE(trim(analysis_json), '') != ''
                OR COALESCE(trim(rewrite_text), '') != ''
                OR COALESCE(trim(ai_rewrite_text), '') != ''
                OR rewrite_edited_at IS NOT NULL
              )
        )",
        params![novel_id],
        |row| row.get(0),
    )
    .map_err(to_string)
}

fn task_history_exists(conn: &Connection, novel_id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM jobs
            WHERE novel_id = ?1
              AND job_type IN ('analysis', 'rewrite', 'auto', 'auto_batch')
        ) OR EXISTS(
            SELECT 1 FROM auto_run_checkpoints WHERE novel_id = ?1
        ) OR EXISTS(
            SELECT 1 FROM rewrite_ab_runs WHERE novel_id = ?1
        )",
        params![novel_id],
        |row| row.get(0),
    )
    .map_err(to_string)
}

fn non_empty_canon_asset_exists(conn: &Connection, novel_id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM canon_assets
            WHERE novel_id = ?1 AND trim(content) != ''
        )",
        params![novel_id],
        |row| row.get(0),
    )
    .map_err(to_string)
}

fn rebuild_pending_novel_chapters<F>(
    conn: &mut Connection,
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    detected_chapters: bool,
    chapter_rule: Option<&ChapterRule>,
    before_commit: F,
) -> Result<Option<StoredChapterRule>, String>
where
    F: FnOnce(&Connection) -> Result<(), String>,
{
    let batch_size = load_chapter_batch_size(conn)?;
    let mut prepared = prepare_chapter_batch_generation(
        data_dir,
        novel_id,
        chapters,
        detected_chapters,
        batch_size,
    )?;
    if let Err(error) = before_commit(conn) {
        return Err(discard_prepared_generations(
            std::slice::from_mut(&mut prepared),
            error,
        ));
    }
    let database_result = (|| {
        let tx = conn.transaction().map_err(to_string)?;
        tx.execute(
            "DELETE FROM chapters WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        replace_chapter_batch_rows(&tx, &prepared)?;
        tx.execute(
            "DELETE FROM canon_assets WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        for chapter in chapters {
            tx.execute(
                "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
                params![
                    chapter.id,
                    chapter.novel_id,
                    chapter.index,
                    chapter.title,
                    chapter.original_text
                ],
            )
            .map_err(to_string)?;
        }
        seed_canon_assets(&tx, novel_id).map_err(to_string)?;
        tx.execute(
            "UPDATE novels SET status = 'imported', detected_chapters = ?1 WHERE id = ?2",
            params![detected_chapters, novel_id],
        )
        .map_err(to_string)?;
        let stored_rule = chapter_rule
            .map(|rule| save_chapter_rule_value(&tx, novel_id, rule))
            .transpose()?;
        tx.commit().map_err(to_string)?;
        Ok(stored_rule)
    })();
    let stored_rule = match database_result {
        Ok(stored_rule) => stored_rule,
        Err(error) => {
            return Err(discard_prepared_generations(
                std::slice::from_mut(&mut prepared),
                error,
            ));
        }
    };
    prepared.retain_after_commit();
    let _ = cleanup_stale_batch_files_for_novel(conn, data_dir, novel_id);
    Ok(stored_rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn test_rule() -> ChapterRule {
        ChapterRule {
            mode: "simple".to_string(),
            line_start: true,
            prefix: "第".to_string(),
            number_type: "mixed".to_string(),
            unit: "章".to_string(),
            include_pattern: r#"^\s*(序言|序章|序卷|序[1-9]|序曲|楔子|引子|引言|序幕|前言|终章|最终章|尾声|后记|卷末后记|完本感言|番外|番外篇|番外章|特别篇|外传|插曲|间章)"#
                .to_string(),
            extra_pattern: "未完待续|作者的话|求月票|求推荐票|第二更|第三更".to_string(),
            regex_pattern: "".to_string(),
        }
    }

    #[test]
    fn custom_rule_preview_returns_all_titles_without_writing_database() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        let dir = std::env::temp_dir().join(format!("yuri-rule-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("novel.txt");
        fs::write(&path, "第一章 开始\n正文\n第二章 继续\n正文").expect("write source");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', ?1, 'UTF-8', 'pending_split', 1, 'now')",
            params![path.to_string_lossy().to_string()],
        )
        .expect("insert novel");

        let text = load_source_text(&conn, "novel-1").expect("source text");
        let split = split_chapters_with_custom_rule("novel-1", &text, &test_rule())
            .expect("split with custom rule");
        let preview = preview_from_chapters(
            &split.chapters,
            long_chapter_preview_items(&split.chapters, split.detected_chapters),
            "ok",
        );

        assert_eq!(preview.total_chapters, 2);
        assert_eq!(preview.chapters[0].title, "第一章 开始");
        assert!(preview.long_chapters.is_empty());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapters", [], |row| row.get(0))
            .expect("count chapters");
        assert_eq!(count, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    fn pending_test_chapter(title: &str, body: &str) -> Chapter {
        Chapter {
            id: uuid::Uuid::new_v4().to_string(),
            novel_id: "novel-1".to_string(),
            index: 1,
            title: title.to_string(),
            original_text: body.to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    fn compact_text(value: &str) -> String {
        value.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    #[test]
    fn long_chapter_split_option_controls_detected_chapter_splitting() {
        let body = "字".repeat(5_001);
        let disabled = split_long_detected_chapters(
            "novel-1",
            vec![pending_test_chapter("第一章 长章", &body)],
            false,
        );
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].title, "第一章 长章");

        let enabled = split_long_detected_chapters(
            "novel-1",
            vec![pending_test_chapter("第一章 长章", &body)],
            true,
        );
        assert_eq!(enabled.len(), 2);
        assert_eq!(enabled[0].index, 1);
        assert_eq!(enabled[1].index, 2);
        assert_eq!(enabled[0].title, "第一章 长章（1）");
        assert_eq!(enabled[1].title, "第一章 长章（2）");
        assert_eq!(
            compact_text(
                &enabled
                    .iter()
                    .map(|chapter| chapter.original_text.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            ),
            compact_text(&body)
        );
    }

    #[test]
    fn long_chapter_split_handles_ten_thousand_plus_chars_and_paragraph_boundaries() {
        let body = "字".repeat(10_001);
        let enabled = split_long_detected_chapters(
            "novel-1",
            vec![pending_test_chapter("第一章 万字章", &body)],
            true,
        );
        assert_eq!(enabled.len(), 3);
        assert_eq!(
            enabled
                .iter()
                .map(|chapter| chapter.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "第一章 万字章（1）",
                "第一章 万字章（2）",
                "第一章 万字章（3）"
            ]
        );
        assert_eq!(
            compact_text(
                &enabled
                    .iter()
                    .map(|chapter| chapter.original_text.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            ),
            compact_text(&body)
        );

        let paragraph_body = format!("{}\n\n{}", "甲".repeat(4_000), "乙".repeat(4_001));
        let paragraph_split = split_long_detected_chapters(
            "novel-1",
            vec![pending_test_chapter("第二章 段落", &paragraph_body)],
            true,
        );
        assert_eq!(paragraph_split.len(), 2);
        assert!(paragraph_split[0].original_text.ends_with('甲'));
        assert!(paragraph_split[1].original_text.starts_with('乙'));
    }

    #[test]
    fn long_chapter_split_is_skipped_for_undetected_chunks() {
        let body = "字".repeat(10_001);
        let source_chapters = vec![pending_test_chapter("自动分段 1", &body)];
        let long_chapters = long_chapter_preview_items(&source_chapters, false);
        let chapters =
            maybe_split_long_detected_chapters("novel-1", source_chapters, false, Some(true));
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "自动分段 1");
        assert!(long_chapters.is_empty());
    }

    #[test]
    fn custom_rule_preview_reports_long_chapters_above_limit_only() {
        let chapters = vec![
            pending_test_chapter("第一章 临界", &"字".repeat(5_000)),
            {
                let mut chapter = pending_test_chapter(
                    "第二章 超长",
                    &format!("{}\n\n{}", "字".repeat(2_500), "字".repeat(2_501)),
                );
                chapter.index = 2;
                chapter
            },
        ];
        let long_chapters = long_chapter_preview_items(&chapters, true);

        assert_eq!(long_chapters.len(), 1);
        assert_eq!(long_chapters[0].index, 2);
        assert_eq!(long_chapters[0].title, "第二章 超长");
        assert_eq!(long_chapters[0].char_count, 5_001);
    }

    #[test]
    fn custom_rule_preview_can_show_split_long_chapter_titles() {
        let text = format!("第一章 开始\n{}\n第二章 继续\n短正文", "字".repeat(5_001));
        let split = split_chapters_with_custom_rule("novel-1", &text, &test_rule())
            .expect("split with custom rule");
        let long_chapters = long_chapter_preview_items(&split.chapters, split.detected_chapters);
        let chapters = maybe_split_long_detected_chapters(
            "novel-1",
            split.chapters,
            split.detected_chapters,
            Some(true),
        );
        let preview = preview_from_chapters(&chapters, long_chapters, "ok");

        assert_eq!(preview.total_chapters, 3);
        assert_eq!(preview.long_chapters.len(), 1);
        assert_eq!(preview.long_chapters[0].title, "第一章 开始");
        assert_eq!(preview.long_chapters[0].char_count, 5_001);
        assert_eq!(preview.chapters[0].title, "第一章 开始（1）");
        assert_eq!(preview.chapters[1].title, "第一章 开始（2）");
        assert_eq!(preview.chapters[2].title, "第二章 继续");
    }

    #[test]
    fn custom_rule_uses_include_pattern_and_exclude_pattern() {
        let text = "序言\n开头\n第一章 正文\n内容\n第二更\n作者更新提示\n第二章 继续\n内容";
        let split = split_chapters_with_custom_rule("novel-1", text, &test_rule())
            .expect("split with include and exclude rule");

        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["序言", "第一章 正文", "第二章 继续"]);
    }

    #[test]
    fn imported_unprocessed_novel_can_be_resplit_and_rebuilds_empty_assets() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        let dir = std::env::temp_dir().join(format!("yuri-rule-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("novel.txt");
        fs::write(&path, "第一章 开始\n正文\n第二章 继续\n正文").expect("write source");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', ?1, 'UTF-8', 'imported', 1, 'now')",
            params![path.to_string_lossy().to_string()],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status)
             VALUES ('old-chapter', 'novel-1', 1, '旧章节', '旧正文', 'pending', 'pending')",
            [],
        )
        .expect("insert old chapter");
        seed_canon_assets(&conn, "novel-1").expect("seed assets");

        ensure_chapter_split_allowed(&conn, "novel-1")
            .expect("unprocessed imported novel can split");
        let text = load_source_text(&conn, "novel-1").expect("source text");
        let split = split_chapters_with_custom_rule("novel-1", &text, &test_rule())
            .expect("split with custom rule");
        let mut mutable_conn = conn;
        rebuild_pending_novel_chapters(
            &mut mutable_conn,
            &dir,
            "novel-1",
            &split.chapters,
            split.detected_chapters,
            None,
            |_| Ok(()),
        )
        .expect("rebuild chapters");

        let old_count: i64 = mutable_conn
            .query_row(
                "SELECT COUNT(*) FROM chapters WHERE id = 'old-chapter'",
                [],
                |row| row.get(0),
            )
            .expect("count old chapter");
        assert_eq!(old_count, 0);
        let chapter_count: i64 = mutable_conn
            .query_row(
                "SELECT COUNT(*) FROM chapters WHERE novel_id = 'novel-1'",
                [],
                |row| row.get(0),
            )
            .expect("count chapters");
        assert_eq!(chapter_count, 2);
        let batch_count: i64 = mutable_conn
            .query_row(
                "SELECT COUNT(*) FROM chapter_batches WHERE novel_id = 'novel-1'",
                [],
                |row| row.get(0),
            )
            .expect("count batches");
        assert_eq!(batch_count, 1);
        let non_empty_asset_count: i64 = mutable_conn
            .query_row(
                "SELECT COUNT(*) FROM canon_assets WHERE novel_id = 'novel-1' AND trim(content) != ''",
                [],
                |row| row.get(0),
            )
            .expect("count non-empty assets");
        assert_eq!(non_empty_asset_count, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rule_save_failure_keeps_old_chapters_rows_and_batch_files() {
        let mut conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        let dir = std::env::temp_dir().join(format!("yuri-rule-atomic-{}", uuid::Uuid::new_v4()));
        let batch_dir = dir.join("chapter_batches").join("novel-1");
        fs::create_dir_all(&batch_dir).expect("create old batch directory");
        let old_batch = batch_dir.join("batch-001.txt");
        fs::write(&old_batch, "旧批次").expect("write old batch");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status)
             VALUES ('old-chapter', 'novel-1', 1, '旧章节', '旧正文', 'pending', 'pending')",
            [],
        )
        .expect("insert old chapter");
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at)
             VALUES ('old-batch', 'novel-1', 1, '1-1章', 1, 1, ?1, 'now')",
            params![old_batch.to_string_lossy().to_string()],
        )
        .expect("insert old batch row");
        conn.execute_batch(
            "CREATE TRIGGER reject_rule_save BEFORE INSERT ON chapter_rules
             BEGIN SELECT RAISE(FAIL, 'rule save stopped'); END;",
        )
        .expect("create failure trigger");

        let new_chapters = vec![Chapter {
            id: "new-chapter".to_string(),
            novel_id: "novel-1".to_string(),
            index: 1,
            title: "第一章".to_string(),
            original_text: "新正文".to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        }];
        let error = rebuild_pending_novel_chapters(
            &mut conn,
            &dir,
            "novel-1",
            &new_chapters,
            true,
            Some(&test_rule()),
            |_| Ok(()),
        )
        .expect_err("rule failure should roll back the complete rebuild");
        assert!(error.contains("rule save stopped"));
        let chapter_id: String = conn
            .query_row("SELECT id FROM chapters", [], |row| row.get(0))
            .expect("load old chapter");
        assert_eq!(chapter_id, "old-chapter");
        let batch_id: String = conn
            .query_row("SELECT id FROM chapter_batches", [], |row| row.get(0))
            .expect("load old batch");
        assert_eq!(batch_id, "old-batch");
        assert_eq!(
            fs::read_to_string(&old_batch).expect("read old batch"),
            "旧批次"
        );
        let generation_count = fs::read_dir(&batch_dir)
            .expect("list batch directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("generation-")
            })
            .count();
        assert_eq!(generation_count, 0);
        let rule_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapter_rules", [], |row| row.get(0))
            .expect("count rules");
        assert_eq!(rule_count, 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pending_split_novel_can_still_apply_chapter_rule() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'pending_split', 1, 'now')",
            [],
        )
        .expect("insert novel");
        ensure_chapter_split_allowed(&conn, "novel-1").expect("pending split can apply");
    }

    #[test]
    fn imported_novel_with_processing_trace_cannot_be_resplit() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, analysis_status, rewrite_status)
             VALUES ('chapter-1', 'novel-1', 1, '第一章', '正文', '{\"summary\":\"已分析\"}', '', 'completed', 'pending')",
            [],
        )
        .expect("insert chapter");
        let error = ensure_chapter_split_allowed(&conn, "novel-1")
            .expect_err("processed chapter should reject");
        assert!(error.contains("已开始分析或改写"));
    }

    #[test]
    fn imported_novel_with_rewrite_text_cannot_be_resplit_even_when_status_pending() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, rewrite_text, analysis_status, rewrite_status)
             VALUES ('chapter-1', 'novel-1', 1, '第一章', '正文', '改写稿', 'pending', 'pending')",
            [],
        )
        .expect("insert chapter");
        let error =
            ensure_chapter_split_allowed(&conn, "novel-1").expect_err("rewrite text should reject");
        assert!(error.contains("已开始分析或改写"));
    }

    #[test]
    fn imported_novel_with_task_history_cannot_be_resplit() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO jobs (id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at)
             VALUES ('job-1', 'novel-1', 'analysis', 'completed', 1, 1, '完成', 'now', 'now')",
            [],
        )
        .expect("insert job");
        let error =
            ensure_chapter_split_allowed(&conn, "novel-1").expect_err("job history should reject");
        assert!(error.contains("已开始分析或改写"));
    }

    #[test]
    fn imported_novel_with_ab_experiment_cannot_be_resplit() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO rewrite_ab_runs (
                id, novel_id, batch_id, batch_label, batch_fingerprint,
                input_snapshot_json, status, review_enabled, created_at, updated_at
             ) VALUES (
                'run-1', 'novel-1', 'batch-1', '1-1章', 'fingerprint',
                '{}', 'ready', 0, 'now', 'now'
             )",
            [],
        )
        .expect("insert A/B experiment");
        let error = ensure_chapter_split_allowed(&conn, "novel-1")
            .expect_err("A/B experiment should reject resplit");
        assert!(error.contains("已开始分析或改写"));
    }

    #[test]
    fn imported_novel_with_auto_checkpoint_cannot_be_resplit() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (novel_id, start_batch_index, next_batch_index, status, profile_ids, created_at, updated_at)
             VALUES ('novel-1', 0, 0, 'paused', '[]', 'now', 'now')",
            [],
        )
        .expect("insert checkpoint");
        let error =
            ensure_chapter_split_allowed(&conn, "novel-1").expect_err("checkpoint should reject");
        assert!(error.contains("已开始分析或改写"));
    }

    #[test]
    fn imported_novel_with_non_empty_canon_asset_cannot_be_resplit() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO canon_assets (novel_id, kind, content, updated_at)
             VALUES ('novel-1', '人物卡', '手动设定', 'now')",
            [],
        )
        .expect("insert asset");
        let error = ensure_chapter_split_allowed(&conn, "novel-1")
            .expect_err("manual canon asset should reject");
        assert!(error.contains("手动一致性资产"));
    }
}
