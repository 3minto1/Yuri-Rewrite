use crate::domain::{AppSettings, AppState, ChapterBatch, NovelSettings};
use crate::task_control::{auto_runs_are_only_paused, auto_runs_have_non_paused};
use crate::{load_chapters, load_novel_settings, normalize_name_list, to_string};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub(crate) fn get_app_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    let review_enabled = load_review_enabled(&conn)?;
    let review_profile_id = load_review_profile_id(&conn)?;
    let analysis_profile_id = load_analysis_profile_id(&conn)?;
    let selected_profile_id = load_selected_profile_id(&conn)?;
    let chapter_batch_size = load_chapter_batch_size(&conn)?;
    let rewrite_parallelism = load_rewrite_parallelism(&conn)?;
    let core_prompt = load_core_prompt(&conn)?;
    Ok(AppSettings {
        export_dir,
        core_prompt,
        review_enabled,
        review_profile_id,
        analysis_profile_id,
        selected_profile_id,
        chapter_batch_size,
        rewrite_parallelism,
    })
}

#[tauri::command]
pub(crate) fn save_app_settings(
    settings: AppSettings,
    state: State<AppState>,
) -> Result<AppSettings, String> {
    if state.active_tasks.any_active()? || auto_runs_have_non_paused(&state.auto_runs)? {
        return Err("任务运行中不能修改应用设置。".to_string());
    }
    let paused_auto_run = auto_runs_are_only_paused(&state.auto_runs)?;
    let mut conn = state.conn.lock().map_err(to_string)?;
    let current_batch_size = load_chapter_batch_size(&conn)?;
    let chapter_batch_size = normalize_chapter_batch_size(settings.chapter_batch_size);
    if paused_auto_run {
        let current = load_app_settings(&conn)?;
        if current.export_dir != settings.export_dir || current.core_prompt != settings.core_prompt
        {
            return Err(
                "一键任务暂停中只能修改并发、复检和模型选择，不能修改导出目录或全局核心设定。"
                    .to_string(),
            );
        }
        if chapter_batch_size != current_batch_size {
            return Err("一键任务暂停中不能修改每批次章节数，请先继续或终止任务。".to_string());
        }
    }
    let export_dir = settings
        .export_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(export_dir) = export_dir.as_deref() {
        fs::create_dir_all(export_dir).map_err(to_string)?;
    }
    let rewrite_parallelism =
        clamp_parallelism_for_batch_size(settings.rewrite_parallelism, chapter_batch_size);
    let normalized = AppSettings {
        export_dir,
        core_prompt: settings.core_prompt.trim().to_string(),
        review_enabled: settings.review_enabled,
        review_profile_id: normalize_review_profile_id(settings.review_profile_id.as_deref()),
        analysis_profile_id: normalize_analysis_profile_id(
            settings.analysis_profile_id.as_deref(),
        ),
        selected_profile_id: normalize_profile_id(settings.selected_profile_id.as_deref()),
        chapter_batch_size,
        rewrite_parallelism,
    };
    if chapter_batch_size != current_batch_size {
        rebuild_detected_chapter_batches_and_save(&mut conn, &state.data_dir, &normalized)?;
    } else {
        save_app_settings_values(&conn, &normalized)?;
    }
    Ok(normalized)
}

fn load_app_settings(conn: &Connection) -> Result<AppSettings, String> {
    let export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    Ok(AppSettings {
        export_dir,
        core_prompt: load_core_prompt(conn)?,
        review_enabled: load_review_enabled(conn)?,
        review_profile_id: load_review_profile_id(conn)?,
        analysis_profile_id: load_analysis_profile_id(conn)?,
        selected_profile_id: load_selected_profile_id(conn)?,
        chapter_batch_size: load_chapter_batch_size(conn)?,
        rewrite_parallelism: load_rewrite_parallelism(conn)?,
    })
}

fn save_app_settings_values(conn: &Connection, settings: &AppSettings) -> Result<(), String> {
    if let Some(export_dir) = settings.export_dir.as_deref() {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('export_dir', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![export_dir],
        )
        .map_err(to_string)?;
    } else {
        conn.execute("DELETE FROM app_settings WHERE key = 'export_dir'", [])
            .map_err(to_string)?;
    }
    save_review_enabled(conn, settings.review_enabled)?;
    save_chapter_batch_size(conn, settings.chapter_batch_size)?;
    save_rewrite_parallelism(conn, settings.rewrite_parallelism)?;
    save_review_profile_id(conn, settings.review_profile_id.as_deref())?;
    save_analysis_profile_id(conn, settings.analysis_profile_id.as_deref())?;
    save_selected_profile_id_value(conn, settings.selected_profile_id.as_deref())?;
    save_core_prompt(conn, &settings.core_prompt)
}

struct PreparedNovelBatches {
    prepared_dir: PathBuf,
    final_dir: PathBuf,
    backup_dir: PathBuf,
    batches: Vec<ChapterBatch>,
    had_existing_dir: bool,
}

fn rebuild_detected_chapter_batches_and_save(
    conn: &mut Connection,
    data_dir: &Path,
    settings: &AppSettings,
) -> Result<(), String> {
    let rebuild_root = data_dir
        .join("chapter-batches-rebuild")
        .join(Uuid::new_v4().to_string());
    let prepared_root = rebuild_root.join("prepared");
    let backup_root = rebuild_root.join("backup");
    fs::create_dir_all(&prepared_root).map_err(to_string)?;
    fs::create_dir_all(&backup_root).map_err(to_string)?;

    let result = (|| {
        let novel_ids = {
            let mut stmt = conn
                .prepare("SELECT id FROM novels WHERE detected_chapters = 1 ORDER BY created_at")
                .map_err(to_string)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(to_string)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(to_string)?;
            rows
        };
        let mut prepared = Vec::with_capacity(novel_ids.len());
        for novel_id in novel_ids {
            let chapters = load_chapters(conn, &novel_id)?;
            let prepared_dir = prepared_root.join(&novel_id);
            let final_dir = data_dir.join("chapter_batches").join(&novel_id);
            let backup_dir = backup_root.join(&novel_id);
            fs::create_dir_all(&prepared_dir).map_err(to_string)?;
            let batches = write_chapter_batch_files(
                &prepared_dir,
                &final_dir,
                &novel_id,
                &chapters,
                settings.chapter_batch_size,
            )?;
            prepared.push(PreparedNovelBatches {
                prepared_dir,
                final_dir: final_dir.clone(),
                backup_dir,
                batches,
                had_existing_dir: final_dir.exists(),
            });
        }

        fs::create_dir_all(data_dir.join("chapter_batches")).map_err(to_string)?;
        for (swapped, item) in prepared.iter().enumerate() {
            if item.had_existing_dir {
                if let Err(error) = fs::rename(&item.final_dir, &item.backup_dir) {
                    restore_swapped_batch_dirs(&prepared[..swapped]);
                    return Err(to_string(error));
                }
            }
            if let Err(error) = fs::rename(&item.prepared_dir, &item.final_dir) {
                if item.had_existing_dir {
                    let _ = fs::rename(&item.backup_dir, &item.final_dir);
                }
                restore_swapped_batch_dirs(&prepared[..swapped]);
                return Err(to_string(error));
            }
        }

        let database_result = (|| {
            let tx = conn.transaction().map_err(to_string)?;
            tx.execute(
                "DELETE FROM chapter_batches WHERE novel_id IN (
                    SELECT id FROM novels WHERE detected_chapters = 1
                )",
                [],
            )
            .map_err(to_string)?;
            for item in &prepared {
                for batch in &item.batches {
                    tx.execute(
                        "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            batch.id,
                            batch.novel_id,
                            batch.batch_index,
                            batch.label,
                            batch.start_chapter,
                            batch.end_chapter,
                            batch.file_path,
                            batch.created_at
                        ],
                    )
                    .map_err(to_string)?;
                }
            }
            save_app_settings_values(&tx, settings)?;
            tx.commit().map_err(to_string)
        })();
        if let Err(error) = database_result {
            restore_swapped_batch_dirs(&prepared);
            return Err(error);
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&rebuild_root);
    result
}

fn restore_swapped_batch_dirs(items: &[PreparedNovelBatches]) {
    for item in items.iter().rev() {
        if item.final_dir.exists() {
            let _ = fs::remove_dir_all(&item.final_dir);
        }
        if item.had_existing_dir && item.backup_dir.exists() {
            let _ = fs::rename(&item.backup_dir, &item.final_dir);
        }
    }
}

fn write_chapter_batch_files(
    output_dir: &Path,
    final_dir: &Path,
    novel_id: &str,
    chapters: &[crate::domain::Chapter],
    batch_size: usize,
) -> Result<Vec<ChapterBatch>, String> {
    let now = Utc::now().to_rfc3339();
    chapters
        .chunks(batch_size)
        .enumerate()
        .map(|(idx, chunk)| {
            let first = chunk.first().ok_or_else(|| "批次内容为空。".to_string())?;
            let last = chunk.last().ok_or_else(|| "批次内容为空。".to_string())?;
            let batch_index = (idx + 1) as i64;
            let file_name = format!("batch-{batch_index:03}.txt");
            let body = chunk
                .iter()
                .map(|chapter| format!("{}\n\n{}", chapter.title, chapter.original_text))
                .collect::<Vec<_>>()
                .join("\n\n");
            fs::write(output_dir.join(&file_name), body).map_err(to_string)?;
            Ok(ChapterBatch {
                id: Uuid::new_v4().to_string(),
                novel_id: novel_id.to_string(),
                batch_index,
                label: format!("{}-{}章", first.index, last.index),
                start_chapter: first.index,
                end_chapter: last.index,
                file_path: final_dir.join(file_name).to_string_lossy().to_string(),
                created_at: now.clone(),
            })
        })
        .collect()
}

pub(crate) fn load_core_prompt(conn: &Connection) -> Result<String, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'core_prompt'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(value.unwrap_or_default())
}

pub(crate) fn save_core_prompt(conn: &Connection, value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        conn.execute("DELETE FROM app_settings WHERE key = 'core_prompt'", [])
            .map_err(to_string)?;
    } else {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('core_prompt', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![value],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

pub(crate) fn load_review_enabled(conn: &Connection) -> Result<bool, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'review_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(matches!(
        value.as_deref().map(str::trim),
        Some("true") | Some("1") | Some("yes")
    ))
}

pub(crate) fn save_review_enabled(conn: &Connection, enabled: bool) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('review_enabled', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    )
    .map_err(to_string)?;
    Ok(())
}

pub(crate) fn normalize_review_profile_id(value: Option<&str>) -> Option<String> {
    normalize_profile_id(value)
}

pub(crate) fn normalize_analysis_profile_id(value: Option<&str>) -> Option<String> {
    normalize_profile_id(value)
}

fn normalize_profile_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn load_review_profile_id(conn: &Connection) -> Result<Option<String>, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'review_profile_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(normalize_review_profile_id(value.as_deref()))
}

pub(crate) fn save_review_profile_id(
    conn: &Connection,
    profile_id: Option<&str>,
) -> Result<(), String> {
    if let Some(profile_id) = normalize_review_profile_id(profile_id) {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('review_profile_id', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![profile_id],
        )
        .map_err(to_string)?;
    } else {
        conn.execute(
            "DELETE FROM app_settings WHERE key = 'review_profile_id'",
            [],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

pub(crate) fn load_analysis_profile_id(conn: &Connection) -> Result<Option<String>, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'analysis_profile_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(normalize_analysis_profile_id(value.as_deref()))
}

pub(crate) fn save_analysis_profile_id(
    conn: &Connection,
    profile_id: Option<&str>,
) -> Result<(), String> {
    if let Some(profile_id) = normalize_analysis_profile_id(profile_id) {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('analysis_profile_id', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![profile_id],
        )
        .map_err(to_string)?;
    } else {
        conn.execute(
            "DELETE FROM app_settings WHERE key = 'analysis_profile_id'",
            [],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

pub(crate) fn load_selected_profile_id(conn: &Connection) -> Result<Option<String>, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'selected_profile_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(normalize_profile_id(value.as_deref()))
}

fn save_selected_profile_id_value(
    conn: &Connection,
    profile_id: Option<&str>,
) -> Result<(), String> {
    if let Some(profile_id) = normalize_profile_id(profile_id) {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('selected_profile_id', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![profile_id],
        )
        .map_err(to_string)?;
    } else {
        conn.execute(
            "DELETE FROM app_settings WHERE key = 'selected_profile_id'",
            [],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn save_selected_profile_id(
    profile_id: Option<String>,
    state: State<AppState>,
) -> Result<AppSettings, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    save_selected_profile_id_value(&conn, profile_id.as_deref())?;
    drop(conn);
    get_app_settings(state)
}

pub(crate) fn default_rewrite_parallelism() -> usize {
    6
}

pub(crate) fn normalize_rewrite_parallelism(value: usize) -> usize {
    match value {
        1 | 3 | 6 | 10 | 25 | 50 => value,
        _ => default_rewrite_parallelism(),
    }
}

pub(crate) fn default_chapter_batch_size() -> usize {
    30
}

pub(crate) fn normalize_chapter_batch_size(value: usize) -> usize {
    match value {
        30 | 50 | 100 => value,
        _ => default_chapter_batch_size(),
    }
}

pub(crate) fn clamp_parallelism_for_batch_size(value: usize, batch_size: usize) -> usize {
    let parallelism = normalize_rewrite_parallelism(value);
    let max_parallelism = match normalize_chapter_batch_size(batch_size) {
        100 => 50,
        50 => 25,
        _ => 10,
    };
    if parallelism > max_parallelism {
        10
    } else {
        parallelism
    }
}

pub(crate) fn load_chapter_batch_size(conn: &Connection) -> Result<usize, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'chapter_batch_size'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    Ok(value
        .as_deref()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(normalize_chapter_batch_size)
        .unwrap_or_else(default_chapter_batch_size))
}

pub(crate) fn save_chapter_batch_size(conn: &Connection, value: usize) -> Result<(), String> {
    let normalized = normalize_chapter_batch_size(value);
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('chapter_batch_size', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![normalized.to_string()],
    )
    .map_err(to_string)?;
    Ok(())
}

pub(crate) fn load_rewrite_parallelism(conn: &Connection) -> Result<usize, String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'rewrite_parallelism'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(to_string)?;
    let parallelism = value
        .as_deref()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(normalize_rewrite_parallelism)
        .unwrap_or_else(default_rewrite_parallelism);
    Ok(clamp_parallelism_for_batch_size(
        parallelism,
        load_chapter_batch_size(conn)?,
    ))
}

pub(crate) fn save_rewrite_parallelism(conn: &Connection, value: usize) -> Result<(), String> {
    let normalized = clamp_parallelism_for_batch_size(value, load_chapter_batch_size(conn)?);
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('rewrite_parallelism', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![normalized.to_string()],
    )
    .map_err(to_string)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_novel_settings(
    novel_id: String,
    state: State<AppState>,
) -> Result<Option<NovelSettings>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_novel_settings(&conn, &novel_id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_novel_settings(
    novel_id: String,
    protagonist_name: String,
    rewritten_protagonist_name: String,
    additional_feminize_names: String,
    bust: String,
    body_type: String,
    rewrite_mode: String,
    advanced_settings: String,
    state: State<AppState>,
) -> Result<NovelSettings, String> {
    if state.active_tasks.novel_is_active(&novel_id)?
        || state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .contains_key(&novel_id)
    {
        return Err("当前小说任务运行中，不能修改小说设定。".to_string());
    }
    let protagonist_name = protagonist_name.trim();
    let rewritten_protagonist_name = rewritten_protagonist_name.trim();
    let additional_feminize_names = normalize_name_list(&additional_feminize_names);
    let bust = bust.trim();
    let body_type = body_type.trim();
    let rewrite_mode = rewrite_mode.trim();
    if protagonist_name.is_empty() {
        return Err("主角姓名为必填项。".to_string());
    }
    if !["平胸", "巨乳"].contains(&bust) {
        return Err("身材只能选择平胸或巨乳。".to_string());
    }
    if !["萝莉", "御姐", "少女"].contains(&body_type) {
        return Err("体型只能选择萝莉、御姐或少女。".to_string());
    }

    if !["strict", "creative"].contains(&rewrite_mode) {
        return Err("改写模式只能选择严谨模式或创意模式。".to_string());
    }

    let settings = NovelSettings {
        novel_id: novel_id.clone(),
        protagonist_name: protagonist_name.to_string(),
        rewritten_protagonist_name: rewritten_protagonist_name.to_string(),
        additional_feminize_names,
        bust: bust.to_string(),
        body_type: body_type.to_string(),
        rewrite_mode: rewrite_mode.to_string(),
        advanced_settings: advanced_settings.trim().to_string(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        r#"
        INSERT INTO novel_settings (novel_id, protagonist_name, rewritten_protagonist_name, additional_feminize_names, bust, body_type, rewrite_mode, advanced_settings, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(novel_id) DO UPDATE SET
            protagonist_name = excluded.protagonist_name,
            rewritten_protagonist_name = excluded.rewritten_protagonist_name,
            additional_feminize_names = excluded.additional_feminize_names,
            bust = excluded.bust,
            body_type = excluded.body_type,
            rewrite_mode = excluded.rewrite_mode,
            advanced_settings = excluded.advanced_settings,
            updated_at = excluded.updated_at
        "#,
        params![
            settings.novel_id,
            settings.protagonist_name,
            settings.rewritten_protagonist_name,
            settings.additional_feminize_names,
            settings.bust,
            settings.body_type,
            settings.rewrite_mode,
            settings.advanced_settings,
            settings.updated_at
        ],
    )
    .map_err(to_string)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn test_settings(batch_size: usize, parallelism: usize) -> AppSettings {
        AppSettings {
            export_dir: None,
            core_prompt: String::new(),
            review_enabled: false,
            review_profile_id: None,
            analysis_profile_id: None,
            selected_profile_id: None,
            chapter_batch_size: batch_size,
            rewrite_parallelism: parallelism,
        }
    }

    #[test]
    fn analysis_profile_defaults_to_current_model_and_can_be_saved() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        assert_eq!(
            load_analysis_profile_id(&conn).expect("load default analysis profile"),
            None
        );
        save_analysis_profile_id(&conn, Some(" analysis-profile "))
            .expect("save analysis profile");
        assert_eq!(
            load_analysis_profile_id(&conn).expect("load analysis profile"),
            Some("analysis-profile".to_string())
        );
        save_analysis_profile_id(&conn, None).expect("clear analysis profile");
        assert_eq!(
            load_analysis_profile_id(&conn).expect("load cleared analysis profile"),
            None
        );
    }

    fn seed_detected_novel(conn: &Connection, data_dir: &Path) {
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at) VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 1, 'now')",
            [],
        )
        .expect("insert novel");
        for index in 1..=61 {
            conn.execute(
                "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, ai_rewrite_text, rewrite_edited_at, analysis_status, rewrite_status) VALUES (?1, 'novel-1', ?2, ?3, ?4, ?5, ?6, ?6, ?7, 'completed', 'completed')",
                params![
                    format!("chapter-{index}"),
                    index,
                    format!("第{index}章"),
                    format!("原文{index}"),
                    format!("{{\"index\":{index}}}"),
                    format!("改写{index}"),
                    if index == 2 { Some("now") } else { None }
                ],
            )
            .expect("insert chapter");
        }
        let old_dir = data_dir.join("chapter_batches").join("novel-1");
        fs::create_dir_all(&old_dir).expect("create old batch dir");
        let old_file = old_dir.join("batch-001.txt");
        fs::write(&old_file, "旧批次").expect("write old batch");
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at) VALUES ('old-batch', 'novel-1', 1, '1-30章', 1, 30, ?1, 'now')",
            params![old_file.to_string_lossy().to_string()],
        )
        .expect("insert old batch");
    }

    fn temp_data_dir() -> PathBuf {
        std::env::temp_dir().join(format!("yuri-rewrite-settings-{}", Uuid::new_v4()))
    }

    #[test]
    fn rebuilds_detected_batches_without_changing_chapter_data() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        let data_dir = temp_data_dir();
        seed_detected_novel(&conn, &data_dir);

        let before: (String, String, String, Option<String>, String, String) = conn
            .query_row(
                "SELECT title, original_text, analysis_json, rewrite_edited_at, analysis_status, rewrite_status FROM chapters WHERE id = 'chapter-2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .expect("load chapter before rebuild");

        rebuild_detected_chapter_batches_and_save(&mut conn, &data_dir, &test_settings(50, 25))
            .expect("rebuild batches");

        let batch_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chapter_batches WHERE novel_id = 'novel-1'",
                [],
                |row| row.get(0),
            )
            .expect("count batches");
        assert_eq!(batch_count, 2);
        let ranges = conn
            .prepare(
                "SELECT start_chapter, end_chapter FROM chapter_batches WHERE novel_id = 'novel-1' ORDER BY batch_index",
            )
            .expect("prepare ranges")
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .expect("query ranges")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect ranges");
        assert_eq!(ranges, vec![(1, 50), (51, 61)]);
        let after: (String, String, String, Option<String>, String, String) = conn
            .query_row(
                "SELECT title, original_text, analysis_json, rewrite_edited_at, analysis_status, rewrite_status FROM chapters WHERE id = 'chapter-2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .expect("load chapter after rebuild");
        assert_eq!(after, before);
        assert!(data_dir
            .join("chapter_batches")
            .join("novel-1")
            .join("batch-002.txt")
            .exists());
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn restores_old_batch_files_and_rows_when_database_rebuild_fails() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        let data_dir = temp_data_dir();
        seed_detected_novel(&conn, &data_dir);
        conn.execute_batch(
            "CREATE TRIGGER prevent_batch_delete BEFORE DELETE ON chapter_batches BEGIN SELECT RAISE(FAIL, 'stop'); END;",
        )
        .expect("create failure trigger");

        let error = rebuild_detected_chapter_batches_and_save(
            &mut conn,
            &data_dir,
            &test_settings(100, 50),
        )
        .expect_err("database failure should abort rebuild");
        assert!(error.contains("stop"));
        let batch_id: String = conn
            .query_row("SELECT id FROM chapter_batches", [], |row| row.get(0))
            .expect("old row remains");
        assert_eq!(batch_id, "old-batch");
        let old_contents = fs::read_to_string(
            data_dir
                .join("chapter_batches")
                .join("novel-1")
                .join("batch-001.txt"),
        )
        .expect("old file restored");
        assert_eq!(old_contents, "旧批次");
        let _ = fs::remove_dir_all(data_dir);
    }
}
