use crate::domain::{AppState, Chapter, ExportResult};
use crate::{load_chapters, row_to_novel, sanitize_file_name, to_string};
use rusqlite::{params, Connection};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;

#[tauri::command]
pub(crate) fn export_novel(
    novel_id: String,
    format: String,
    state: State<AppState>,
) -> Result<ExportResult, String> {
    if format != "txt" {
        return Err("当前仅支持导出 TXT。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let novel = conn
        .query_row(
            "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
            params![novel_id],
            row_to_novel,
        )
        .map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel.id)?;
    let body = build_rewritten_export_body(&chapters)?;
    let configured_export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    let safe_title = sanitize_file_name(&novel.title);
    let extension = "txt";
    let output_dir = configured_export_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| state.data_dir.join("exports"));
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let output_path = output_dir.join(format!("{}-rewrite.{}", safe_title, extension));
    fs::write(&output_path, body).map_err(to_string)?;
    Ok(ExportResult {
        path: output_path.to_string_lossy().to_string(),
    })
}

pub(crate) fn build_rewritten_export_body(chapters: &[Chapter]) -> Result<String, String> {
    let rewritten_chapters = chapters
        .iter()
        .filter(|chapter| {
            chapter.rewrite_status == "completed"
                && chapter
                    .rewrite_text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
        })
        .collect::<Vec<_>>();
    if rewritten_chapters.is_empty() {
        return Err("没有已完成改写的章节可导出。".to_string());
    }

    let mut body = String::new();
    for chapter in rewritten_chapters {
        body.push_str(&format!("{}\n\n", chapter.title));
        body.push_str(chapter.rewrite_text.as_deref().unwrap_or_default().trim());
        body.push_str("\n\n");
    }
    Ok(body)
}

pub(crate) fn resolve_rewrite_export_dir(
    conn: &Connection,
    data_dir: &Path,
) -> Result<PathBuf, String> {
    let configured_export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    Ok(configured_export_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("exports")))
}

pub(crate) fn chinese_batch_label(index: i64) -> String {
    format!("第{}批", chinese_number(index))
}

pub(crate) fn chinese_number(value: i64) -> String {
    if value <= 0 {
        return value.to_string();
    }
    if value <= 10 {
        return chinese_digit(value).to_string();
    }
    if value < 20 {
        return format!(
            "十{}",
            if value % 10 == 0 {
                ""
            } else {
                chinese_digit(value % 10)
            }
        );
    }
    if value < 100 {
        let ten = value / 10;
        let one = value % 10;
        return format!(
            "{}十{}",
            chinese_digit(ten),
            if one == 0 { "" } else { chinese_digit(one) }
        );
    }
    value.to_string()
}

pub(crate) fn chinese_digit(value: i64) -> &'static str {
    match value {
        1 => "一",
        2 => "二",
        3 => "三",
        4 => "四",
        5 => "五",
        6 => "六",
        7 => "七",
        8 => "八",
        9 => "九",
        10 => "十",
        _ => "",
    }
}
