use crate::domain::{AppSettings, AppState, NovelSettings};
use crate::{load_novel_settings, normalize_name_list, to_string};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use tauri::State;

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
    let selected_profile_id = load_selected_profile_id(&conn)?;
    let rewrite_parallelism = load_rewrite_parallelism(&conn)?;
    let core_prompt = load_core_prompt(&conn)?;
    Ok(AppSettings {
        export_dir,
        core_prompt,
        review_enabled,
        review_profile_id,
        selected_profile_id,
        rewrite_parallelism,
    })
}

#[tauri::command]
pub(crate) fn save_app_settings(
    settings: AppSettings,
    state: State<AppState>,
) -> Result<AppSettings, String> {
    if state.active_tasks.any_active()? || !state.auto_runs.lock().map_err(to_string)?.is_empty() {
        return Err("任务运行中不能修改应用设置。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let rewrite_parallelism = normalize_rewrite_parallelism(settings.rewrite_parallelism);
    if let Some(export_dir) = settings
        .export_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        fs::create_dir_all(export_dir).map_err(to_string)?;
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('export_dir', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![export_dir],
        )
        .map_err(to_string)?;
        save_review_enabled(&conn, settings.review_enabled)?;
        save_rewrite_parallelism(&conn, rewrite_parallelism)?;
        save_review_profile_id(&conn, settings.review_profile_id.as_deref())?;
        save_selected_profile_id_value(&conn, settings.selected_profile_id.as_deref())?;
        save_core_prompt(&conn, &settings.core_prompt)?;
        Ok(AppSettings {
            export_dir: Some(export_dir.to_string()),
            core_prompt: settings.core_prompt.trim().to_string(),
            review_enabled: settings.review_enabled,
            review_profile_id: normalize_review_profile_id(settings.review_profile_id.as_deref()),
            selected_profile_id: normalize_profile_id(settings.selected_profile_id.as_deref()),
            rewrite_parallelism,
        })
    } else {
        conn.execute("DELETE FROM app_settings WHERE key = 'export_dir'", [])
            .map_err(to_string)?;
        save_review_enabled(&conn, settings.review_enabled)?;
        save_rewrite_parallelism(&conn, rewrite_parallelism)?;
        save_review_profile_id(&conn, settings.review_profile_id.as_deref())?;
        save_selected_profile_id_value(&conn, settings.selected_profile_id.as_deref())?;
        save_core_prompt(&conn, &settings.core_prompt)?;
        Ok(AppSettings {
            export_dir: None,
            core_prompt: settings.core_prompt.trim().to_string(),
            review_enabled: settings.review_enabled,
            review_profile_id: normalize_review_profile_id(settings.review_profile_id.as_deref()),
            selected_profile_id: normalize_profile_id(settings.selected_profile_id.as_deref()),
            rewrite_parallelism,
        })
    }
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
        1 | 3 | 6 | 10 => value,
        _ => default_rewrite_parallelism(),
    }
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
    Ok(value
        .as_deref()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(normalize_rewrite_parallelism)
        .unwrap_or_else(default_rewrite_parallelism))
}

pub(crate) fn save_rewrite_parallelism(conn: &Connection, value: usize) -> Result<(), String> {
    let normalized = normalize_rewrite_parallelism(value);
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
