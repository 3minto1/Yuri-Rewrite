use crate::domain::{
    AppState, Chapter, ChapterRule, ChapterRulePreview, ChapterRulePreviewItem, StoredChapterRule,
};
use crate::{
    create_chapter_batches, decode_text, load_chapter_batch_size, seed_canon_assets,
    split_chapters, split_chapters_with_custom_rule, to_string,
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
    state: State<AppState>,
) -> Result<ChapterRulePreview, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters_with_custom_rule(&novel_id, &text, &normalize_chapter_rule(rule))?;
    Ok(preview_from_chapters(
        &split.chapters,
        "预览已生成，确认无误后可保存应用。",
    ))
}

#[tauri::command]
pub(crate) fn save_chapter_rule_and_split(
    novel_id: String,
    rule: ChapterRule,
    state: State<AppState>,
) -> Result<StoredChapterRule, String> {
    ensure_can_split_novel(&state, &novel_id)?;
    let normalized = normalize_chapter_rule(rule);
    let mut conn = state.conn.lock().map_err(to_string)?;
    ensure_pending_split(&conn, &novel_id)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters_with_custom_rule(&novel_id, &text, &normalized)?;
    if split.chapters.is_empty() {
        return Err("自定义章节规则未生成任何章节。".to_string());
    }
    rebuild_pending_novel_chapters(
        &mut conn,
        &state.data_dir,
        &novel_id,
        &split.chapters,
        split.detected_chapters,
    )?;
    save_chapter_rule_value(&conn, &novel_id, &normalized)
}

#[tauri::command]
pub(crate) fn split_novel_with_builtin_rule(
    novel_id: String,
    state: State<AppState>,
) -> Result<(), String> {
    ensure_can_split_novel(&state, &novel_id)?;
    let mut conn = state.conn.lock().map_err(to_string)?;
    ensure_pending_split(&conn, &novel_id)?;
    let text = load_source_text(&conn, &novel_id)?;
    let split = split_chapters(&novel_id, &text);
    if split.chapters.is_empty() {
        return Err("内置章节识别未生成任何章节。".to_string());
    }
    rebuild_pending_novel_chapters(
        &mut conn,
        &state.data_dir,
        &novel_id,
        &split.chapters,
        split.detected_chapters,
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

fn preview_from_chapters(chapters: &[Chapter], message: &str) -> ChapterRulePreview {
    ChapterRulePreview {
        total_chapters: chapters.len(),
        chapters: chapters
            .iter()
            .map(|chapter| ChapterRulePreviewItem {
                index: chapter.index,
                title: chapter.title.clone(),
            })
            .collect(),
        can_apply: !chapters.is_empty(),
        message: message.to_string(),
    }
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
    if state.active_tasks.novel_is_active(novel_id)? {
        return Err("当前小说任务正在运行，不能生成章节列表。".to_string());
    }
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

fn ensure_pending_split(conn: &Connection, novel_id: &str) -> Result<(), String> {
    let status = conn
        .query_row(
            "SELECT status FROM novels WHERE id = ?1",
            params![novel_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?;
    if status != "pending_split" {
        return Err("当前小说已经生成章节，本轮不支持重新拆分。".to_string());
    }
    Ok(())
}

fn rebuild_pending_novel_chapters(
    conn: &mut Connection,
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    detected_chapters: bool,
) -> Result<(), String> {
    let batch_dir = data_dir.join("chapter_batches").join(novel_id);
    if batch_dir.exists() {
        fs::remove_dir_all(&batch_dir).map_err(to_string)?;
    }
    let batch_size = load_chapter_batch_size(conn)?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "DELETE FROM chapters WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    tx.execute(
        "DELETE FROM chapter_batches WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
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
    create_chapter_batches(
        &tx,
        data_dir,
        novel_id,
        chapters,
        detected_chapters,
        batch_size,
    )?;
    seed_canon_assets(&tx, novel_id).map_err(to_string)?;
    tx.execute(
        "UPDATE novels SET status = 'imported', detected_chapters = ?1 WHERE id = ?2",
        params![detected_chapters, novel_id],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)
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
        let preview = preview_from_chapters(&split.chapters, "ok");

        assert_eq!(preview.total_chapters, 2);
        assert_eq!(preview.chapters[0].title, "第一章 开始");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chapters", [], |row| row.get(0))
            .expect("count chapters");
        assert_eq!(count, 0);
        let _ = fs::remove_dir_all(&dir);
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
    fn saving_rule_and_splitting_rejects_imported_novel() {
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES ('novel-1', '测试', '', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        let error = ensure_pending_split(&conn, "novel-1").expect_err("should reject imported");
        assert!(error.contains("已经生成章节"));
    }
}
