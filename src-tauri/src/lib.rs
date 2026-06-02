use chrono::Utc;
use encoding_rs::{GBK, UTF_8};
use regex::Regex;
use reqwest::Client;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};
use uuid::Uuid;

const KEYRING_SERVICE: &str = "YuriRewrite";

struct AppState {
    conn: Mutex<Connection>,
    client: Client,
    data_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Novel {
    id: String,
    title: String,
    source_path: String,
    encoding: String,
    status: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Chapter {
    id: String,
    novel_id: String,
    index: i64,
    title: String,
    original_text: String,
    analysis_json: Option<String>,
    rewrite_text: Option<String>,
    analysis_status: String,
    rewrite_status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CanonAsset {
    novel_id: String,
    kind: String,
    content: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NovelDetail {
    novel: Novel,
    chapters: Vec<Chapter>,
    canon_assets: Vec<CanonAsset>,
    batches: Vec<ChapterBatch>,
    settings: Option<NovelSettings>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChapterBatch {
    id: String,
    novel_id: String,
    batch_index: i64,
    label: String,
    start_chapter: i64,
    end_chapter: i64,
    file_path: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NovelSettings {
    novel_id: String,
    protagonist_name: String,
    additional_feminize_names: String,
    bust: String,
    body_type: String,
    advanced_settings: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ModelProfile {
    id: String,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    temperature: f64,
    has_api_key: bool,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ModelProfileInput {
    id: Option<String>,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    temperature: f64,
    api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelTestResult {
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Job {
    id: String,
    novel_id: String,
    job_type: String,
    status: String,
    current_chapter: i64,
    total_chapters: i64,
    message: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AiLog {
    id: String,
    novel_id: Option<String>,
    profile_id: String,
    action: String,
    chapter_title: Option<String>,
    status: String,
    content: String,
    reasoning: Option<String>,
    raw_response: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppSettings {
    export_dir: Option<String>,
}

struct ModelOutput {
    text: String,
    reasoning: Option<String>,
    raw_response: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportResult {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CanonAssetInput {
    kind: String,
    content: String,
}

struct SplitResult {
    chapters: Vec<Chapter>,
    detected_chapters: bool,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            fs::create_dir_all(data_dir.join("exports"))?;
            let conn = Connection::open(data_dir.join("yuri-rewrite.sqlite3"))?;
            init_db(&conn)?;
            app.manage(AppState {
                conn: Mutex::new(conn),
                client: Client::new(),
                data_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_txt,
            list_novels,
            get_novel_detail,
            delete_novel,
            save_model_profile,
            delete_model_profile,
            list_model_profiles,
            test_model_profile,
            list_ai_logs,
            clear_ai_logs,
            get_app_settings,
            save_app_settings,
            get_novel_settings,
            save_novel_settings,
            list_chapter_batches,
            update_canon_assets,
            start_analysis,
            start_rewrite,
            get_job,
            export_novel
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Yuri Rewrite");
}

fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS novels (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source_path TEXT NOT NULL,
            encoding TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chapters (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            chapter_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            original_text TEXT NOT NULL,
            analysis_json TEXT,
            rewrite_text TEXT,
            analysis_status TEXT NOT NULL,
            rewrite_status TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS canon_assets (
            novel_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY(novel_id, kind)
        );

        CREATE TABLE IF NOT EXISTS model_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            provider TEXT NOT NULL,
            base_url TEXT NOT NULL,
            model TEXT NOT NULL,
            temperature REAL NOT NULL,
            api_key TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            current_chapter INTEGER NOT NULL,
            total_chapters INTEGER NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_logs (
            id TEXT PRIMARY KEY,
            novel_id TEXT,
            profile_id TEXT NOT NULL,
            action TEXT NOT NULL,
            chapter_title TEXT,
            status TEXT NOT NULL,
            content TEXT NOT NULL,
            reasoning TEXT,
            raw_response TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS novel_settings (
            novel_id TEXT PRIMARY KEY,
            protagonist_name TEXT NOT NULL,
            additional_feminize_names TEXT NOT NULL,
            bust TEXT NOT NULL,
            body_type TEXT NOT NULL,
            advanced_settings TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS chapter_batches (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            batch_index INTEGER NOT NULL,
            label TEXT NOT NULL,
            start_chapter INTEGER NOT NULL,
            end_chapter INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_chapters_novel ON chapters(novel_id, chapter_index);
        CREATE INDEX IF NOT EXISTS idx_jobs_novel ON jobs(novel_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_logs_created ON ai_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_logs_novel ON ai_logs(novel_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_chapter_batches_novel ON chapter_batches(novel_id, batch_index);
        "#,
    )?;
    ensure_column(conn, "model_profiles", "api_key", "TEXT")?;
    ensure_column(conn, "ai_logs", "reasoning", "TEXT")?;
    ensure_column(conn, "ai_logs", "raw_response", "TEXT")?;
    ensure_column(conn, "novel_settings", "advanced_settings", "TEXT NOT NULL DEFAULT ''")?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, column_type: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, column_type),
            [],
        )?;
    }
    Ok(())
}

#[tauri::command]
fn import_txt(file_path: String, state: State<AppState>) -> Result<Novel, String> {
    let bytes = fs::read(&file_path).map_err(to_string)?;
    let (text, encoding) = decode_text(&bytes);
    let source = Path::new(&file_path);
    let title = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("未命名小说")
        .to_string();
    let novel = Novel {
        id: Uuid::new_v4().to_string(),
        title,
        source_path: file_path,
        encoding,
        status: "imported".to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let split = split_chapters(&novel.id, &text);
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![novel.id, novel.title, novel.source_path, novel.encoding, novel.status, novel.created_at],
    )
    .map_err(to_string)?;

    for chapter in &split.chapters {
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
            params![chapter.id, chapter.novel_id, chapter.index, chapter.title, chapter.original_text],
        )
        .map_err(to_string)?;
    }

    create_chapter_batches(&conn, &state.data_dir, &novel.id, &split.chapters, split.detected_chapters)
        .map_err(to_string)?;
    seed_canon_assets(&conn, &novel.id).map_err(to_string)?;
    Ok(novel)
}

#[tauri::command]
fn list_novels(state: State<AppState>) -> Result<Vec<Novel>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare("SELECT id, title, source_path, encoding, status, created_at FROM novels ORDER BY created_at DESC")
        .map_err(to_string)?;
    let rows = stmt
        .query_map([], row_to_novel)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(rows)
}

#[tauri::command]
fn get_novel_detail(novel_id: String, state: State<AppState>) -> Result<NovelDetail, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let novel = conn
        .query_row(
            "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
            params![novel_id],
            row_to_novel,
        )
        .map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel.id)?;
    if !chapters.is_empty() && load_chapter_batches(&conn, &novel.id)?.is_empty() {
        create_chapter_batches(&conn, &state.data_dir, &novel.id, &chapters, true)?;
    }
    fill_empty_canon_assets_from_analysis(&conn, &novel.id).map_err(to_string)?;
    let canon_assets = load_canon_assets(&conn, &novel.id)?;
    let batches = load_chapter_batches(&conn, &novel.id)?;
    let settings = load_novel_settings(&conn, &novel.id)?;
    Ok(NovelDetail {
        novel,
        chapters,
        canon_assets,
        batches,
        settings,
    })
}

#[tauri::command]
fn delete_novel(novel_id: String, state: State<AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let batch_dir = state.data_dir.join("chapter_batches").join(&novel_id);
    if batch_dir.exists() {
        fs::remove_dir_all(&batch_dir).map_err(to_string)?;
    }
    conn.execute("DELETE FROM novel_settings WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM chapter_batches WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM chapters WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM canon_assets WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM jobs WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM ai_logs WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM novels WHERE id = ?1", params![novel_id])
        .map_err(to_string)?;
    Ok(())
}

#[tauri::command]
fn save_model_profile(input: ModelProfileInput, state: State<AppState>) -> Result<ModelProfile, String> {
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let updated_at = Utc::now().to_rfc3339();
    let api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "********")
        .map(str::to_string);
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(value) = &api_key {
        let _ = write_api_key(&id, value);
    }
    let profile = ModelProfile {
        id: id.clone(),
        name: input.name,
        provider: input.provider,
        base_url: input.base_url,
        model: input.model,
        temperature: input.temperature,
        has_api_key: api_key.is_some() || stored_api_key_exists(&conn, &id),
        updated_at,
    };

    conn.execute(
        r#"
        INSERT INTO model_profiles (id, name, provider, base_url, model, temperature, updated_at, api_key)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            base_url = excluded.base_url,
            model = excluded.model,
            temperature = excluded.temperature,
            updated_at = excluded.updated_at,
            api_key = COALESCE(excluded.api_key, model_profiles.api_key)
        "#,
        params![
            profile.id,
            profile.name,
            profile.provider,
            profile.base_url,
            profile.model,
            profile.temperature,
            profile.updated_at,
            api_key
        ],
    )
    .map_err(to_string)?;

    Ok(profile)
}

#[tauri::command]
fn delete_model_profile(profile_id: String, state: State<AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute("DELETE FROM model_profiles WHERE id = ?1", params![profile_id])
        .map_err(to_string)?;
    conn.execute("DELETE FROM ai_logs WHERE profile_id = ?1", params![profile_id])
        .map_err(to_string)?;
    let _ = delete_api_key(&profile_id);
    Ok(())
}

#[tauri::command]
fn list_model_profiles(state: State<AppState>) -> Result<Vec<ModelProfile>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider, base_url, model, temperature, updated_at, api_key FROM model_profiles ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let profiles = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let db_api_key: Option<String> = row.get(7)?;
        Ok(ModelProfile {
            has_api_key: read_api_key(&id).is_ok()
                || db_api_key.as_deref().is_some_and(|value| !value.is_empty()),
            id,
            name: row.get(1)?,
            provider: row.get(2)?,
            base_url: row.get(3)?,
            model: row.get(4)?,
            temperature: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })
    .map_err(to_string)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(to_string)?;
    Ok(profiles)
}

#[tauri::command]
fn list_ai_logs(novel_id: Option<String>, state: State<AppState>) -> Result<Vec<AiLog>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(novel_id) = novel_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, created_at FROM ai_logs WHERE novel_id = ?1 OR novel_id IS NULL ORDER BY created_at DESC LIMIT 80",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map(params![novel_id], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, created_at FROM ai_logs ORDER BY created_at DESC LIMIT 80",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map([], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    }
}

#[tauri::command]
fn clear_ai_logs(novel_id: Option<String>, state: State<AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(novel_id) = novel_id {
        conn.execute(
            "DELETE FROM ai_logs WHERE novel_id = ?1 OR novel_id IS NULL",
            params![novel_id],
        )
        .map_err(to_string)?;
    } else {
        conn.execute("DELETE FROM ai_logs", []).map_err(to_string)?;
    }
    Ok(())
}

#[tauri::command]
fn get_app_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    Ok(AppSettings { export_dir })
}

#[tauri::command]
fn save_app_settings(settings: AppSettings, state: State<AppState>) -> Result<AppSettings, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(export_dir) = settings.export_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        fs::create_dir_all(export_dir).map_err(to_string)?;
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('export_dir', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![export_dir],
        )
        .map_err(to_string)?;
        Ok(AppSettings {
            export_dir: Some(export_dir.to_string()),
        })
    } else {
        conn.execute("DELETE FROM app_settings WHERE key = 'export_dir'", [])
            .map_err(to_string)?;
        Ok(AppSettings { export_dir: None })
    }
}

#[tauri::command]
fn get_novel_settings(novel_id: String, state: State<AppState>) -> Result<Option<NovelSettings>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_novel_settings(&conn, &novel_id)
}

#[tauri::command]
fn save_novel_settings(
    novel_id: String,
    protagonist_name: String,
    additional_feminize_names: String,
    bust: String,
    body_type: String,
    advanced_settings: String,
    state: State<AppState>,
) -> Result<NovelSettings, String> {
    let protagonist_name = protagonist_name.trim();
    let additional_feminize_names = normalize_name_list(&additional_feminize_names);
    let bust = bust.trim();
    let body_type = body_type.trim();
    if protagonist_name.is_empty() {
        return Err("主角姓名为必填项。".to_string());
    }
    if !["平胸", "巨乳"].contains(&bust) {
        return Err("身材只能选择平胸或巨乳。".to_string());
    }
    if !["萝莉", "御姐", "少女"].contains(&body_type) {
        return Err("体型只能选择萝莉、御姐或少女。".to_string());
    }

    let settings = NovelSettings {
        novel_id: novel_id.clone(),
        protagonist_name: protagonist_name.to_string(),
        additional_feminize_names,
        bust: bust.to_string(),
        body_type: body_type.to_string(),
        advanced_settings: advanced_settings.trim().to_string(),
        updated_at: Utc::now().to_rfc3339(),
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        r#"
        INSERT INTO novel_settings (novel_id, protagonist_name, additional_feminize_names, bust, body_type, advanced_settings, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(novel_id) DO UPDATE SET
            protagonist_name = excluded.protagonist_name,
            additional_feminize_names = excluded.additional_feminize_names,
            bust = excluded.bust,
            body_type = excluded.body_type,
            advanced_settings = excluded.advanced_settings,
            updated_at = excluded.updated_at
        "#,
        params![
            settings.novel_id,
            settings.protagonist_name,
            settings.additional_feminize_names,
            settings.bust,
            settings.body_type,
            settings.advanced_settings,
            settings.updated_at
        ],
    )
    .map_err(to_string)?;
    Ok(settings)
}

#[tauri::command]
fn list_chapter_batches(novel_id: String, state: State<AppState>) -> Result<Vec<ChapterBatch>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_chapter_batches(&conn, &novel_id)
}

#[tauri::command]
async fn test_model_profile(profile_id: String, state: State<'_, AppState>) -> Result<ModelTestResult, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    match generate_text(
        &state.client,
        &profile,
        &api_key,
        "你是一个连接测试助手。只回复一句中文。",
        "请回复：连接成功。",
    )
    .await
    {
        Ok(output) => {
            append_ai_log(
                &state,
                None,
                &profile.id,
                "测试模型",
                None,
                "success",
                &output.text,
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            Ok(ModelTestResult {
                ok: true,
                message: output.text,
            })
        }
        Err(error) => {
            append_ai_log(&state, None, &profile.id, "测试模型", None, "error", &error, None, None)?;
            Ok(ModelTestResult {
                ok: false,
                message: error,
            })
        }
    }
}

#[tauri::command]
fn update_canon_assets(
    novel_id: String,
    assets: Vec<CanonAssetInput>,
    state: State<AppState>,
) -> Result<Vec<CanonAsset>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let updated_at = Utc::now().to_rfc3339();
    for asset in assets {
        conn.execute(
            r#"
            INSERT INTO canon_assets (novel_id, kind, content, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(novel_id, kind) DO UPDATE SET
                content = excluded.content,
                updated_at = excluded.updated_at
            "#,
            params![novel_id, asset.kind, asset.content, updated_at],
        )
        .map_err(to_string)?;
    }
    load_canon_assets(&conn, &novel_id)
}

#[tauri::command]
async fn start_analysis(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let chapters = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_chapters_for_batch(&conn, &novel_id, &batch_id)?
    };
    if chapters.is_empty() {
        return Err("当前批次没有可分析的内容。".to_string());
    }
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "analysis", total)?;

    for chapter in chapters {
        update_job(&state, &job.id, "running", chapter.index, &format!("正在分析 {}", chapter.title))?;
        set_chapter_status(&state, &chapter.id, "analysis_status", "running")?;
        let prompt = build_analysis_prompt(&chapter);
        match generate_text(
            &state.client,
            &profile,
            &api_key,
            "你是严谨的中文长篇小说结构分析助手。必须输出合法 JSON，不要输出 Markdown。",
            &prompt,
        )
        .await
        {
            Ok(output) => {
                let normalized = normalize_jsonish(&output.text);
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节分析",
                    Some(&chapter.title),
                    "success",
                    &normalized,
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let conn = state.conn.lock().map_err(to_string)?;
                conn.execute(
                    "UPDATE chapters SET analysis_json = ?1, analysis_status = 'completed' WHERE id = ?2",
                    params![normalized, chapter.id],
                )
                .map_err(to_string)?;
                merge_analysis_into_canon_assets(&conn, &novel_id).map_err(to_string)?;
            }
            Err(error) => {
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节分析",
                    Some(&chapter.title),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                set_chapter_status(&state, &chapter.id, "analysis_status", "failed")?;
                update_job(&state, &job.id, "failed", chapter.index, &error)?;
                job = get_job(job.id.clone(), state)?;
                return Ok(job);
            }
        }
    }

    update_job(&state, &job.id, "completed", total, "分析完成")?;
    get_job(job.id, state)
}

#[tauri::command]
async fn start_rewrite(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (chapters, canon_assets, settings) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        let chapters = load_chapters_for_batch(&conn, &novel_id, &batch_id)?
            .into_iter()
            .filter(|chapter| chapter.analysis_status == "completed")
            .collect::<Vec<_>>();
        (chapters, load_canon_assets(&conn, &novel_id)?, settings)
    };
    if chapters.is_empty() {
        return Err("当前批次没有已完成分析的内容，请先分析该批次。".to_string());
    }
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    let canon_text = canon_assets
        .iter()
        .map(|asset| format!("## {}\n{}", asset.kind, asset.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    for chapter in chapters {
        update_job(&state, &job.id, "running", chapter.index, &format!("正在改写 {}", chapter.title))?;
        set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
        let prompt = build_rewrite_prompt_with_settings(&chapter, &canon_text, &settings);
        match generate_text(
            &state.client,
            &profile,
            &api_key,
            "你是中文小说改写助手，任务是把男女主文本改写为自然的双女主百合文本。只输出改写后的正文。",
            &prompt,
        )
        .await
        {
            Ok(output) => {
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节改写",
                    Some(&chapter.title),
                    "success",
                    output.text.trim(),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let conn = state.conn.lock().map_err(to_string)?;
                conn.execute(
                    "UPDATE chapters SET rewrite_text = ?1, rewrite_status = 'completed' WHERE id = ?2",
                    params![output.text.trim(), chapter.id],
                )
                .map_err(to_string)?;
            }
            Err(error) => {
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节改写",
                    Some(&chapter.title),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                set_chapter_status(&state, &chapter.id, "rewrite_status", "failed")?;
                update_job(&state, &job.id, "failed", chapter.index, &error)?;
                job = get_job(job.id.clone(), state)?;
                return Ok(job);
            }
        }
    }

    update_job(&state, &job.id, "completed", total, "改写完成")?;
    get_job(job.id, state)
}

#[tauri::command]
fn get_job(job_id: String, state: State<AppState>) -> Result<Job, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
        params![job_id],
        row_to_job,
    )
    .map_err(to_string)
}

#[tauri::command]
fn export_novel(novel_id: String, format: String, state: State<AppState>) -> Result<ExportResult, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let novel = conn
        .query_row(
            "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
            params![novel_id],
            row_to_novel,
        )
        .map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel.id)?;
    let configured_export_dir = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty());
    let safe_title = sanitize_file_name(&novel.title);
    let extension = if format == "markdown" { "md" } else { "txt" };
    let output_dir = configured_export_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| state.data_dir.join("exports"));
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let output_path = output_dir.join(format!("{}-rewrite.{}", safe_title, extension));
    let mut body = String::new();
    for chapter in chapters {
        if format == "markdown" {
            body.push_str(&format!("# {}\n\n", chapter.title));
        } else {
            body.push_str(&format!("{}\n\n", chapter.title));
        }
        body.push_str(chapter.rewrite_text.as_deref().unwrap_or(&chapter.original_text));
        body.push_str("\n\n");
    }
    fs::write(&output_path, body).map_err(to_string)?;
    Ok(ExportResult {
        path: output_path.to_string_lossy().to_string(),
    })
}

fn decode_text(bytes: &[u8]) -> (String, String) {
    let (utf8, _, had_errors) = UTF_8.decode(bytes);
    if !had_errors {
        return (utf8.into_owned(), "utf-8".to_string());
    }
    let (gbk, _, _) = GBK.decode(bytes);
    (gbk.into_owned(), "gbk".to_string())
}

fn split_chapters(novel_id: &str, text: &str) -> SplitResult {
    let heading_re = chapter_heading_regex();
    let matches = heading_re.find_iter(text).collect::<Vec<_>>();
    if matches.is_empty() {
        return SplitResult {
            chapters: chunk_without_headings(novel_id, text),
            detected_chapters: false,
        };
    }

    let mut chapters = Vec::new();
    for (idx, mat) in matches.iter().enumerate() {
        let start = mat.start();
        let content_start = mat.end();
        let end = matches.get(idx + 1).map_or(text.len(), |next| next.start());
        let title = text[start..content_start].trim();
        let title = if title.is_empty() {
            format!("第{}章", idx + 1)
        } else {
            title.to_string()
        };
        let original_text = text[content_start..end].trim().to_string();
        chapters.push(Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title,
            original_text,
            analysis_json: None,
            rewrite_text: None,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        });
    }
    SplitResult {
        chapters,
        detected_chapters: true,
    }
}

fn chapter_heading_regex() -> Regex {
    Regex::new(
        r#"(?m)^[\s\u{feff}　]*(?:[【〔［「『《（(\[]?\s*(?:正文\s*)?第\s*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+\s*[章节回卷部集篇话幕节页季段册]\s*[】〕］」』》）)\]]?[\s:：、.．\-—_·|]*.{0,80}|(?:卷|篇|部|章|回|幕|册|话|节|季|段)\s*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[\s:：、.．\-—_·|]*.{0,80}|[上中下前后终外]\s*(?:卷|篇|部|章|册)[\s:：、.．\-—_·|]*.{0,80}|(?:Chapter|CHAPTER|chapter|Chap\.?|CH\.?|ch\.?|Section|SECTION|section|Part|PART|part|Episode|EPISODE|episode|No\.?|NO\.?|no\.?)\s*[0-9０-９IVXLCDMivxlcdm]+[\s:：、.．\-—_·|]*.{0,80}|[【〔［「『《（(\[]?\s*(?:序章|楔子|引子|前言|正文|终章|尾声|后记|番外(?:篇|章)?|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)\s*[】〕］」』》）)\]]?[\s:：、.．\-—_·|]*.{0,80}|[0-9０-９]{1,5}\.?\s*|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12}\s*|(?:第?\s*)?[0-9０-９]{1,5}\s*[、.．:：\-—_·|]\s*.{1,80}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,8}\s*[、.．:：\-—_·|]\s*.{1,80}|[（(]?[0-9０-９]{1,5}[）)]\s*.{0,80}|[【〔［「『《（(\[].{1,40}[】〕］」』》）)\]]|={2,6}.{1,60}={2,6})[\s　]*$"#,
    )
    .expect("valid chapter regex")
}

fn chunk_without_headings(novel_id: &str, text: &str) -> Vec<Chapter> {
    let chars = text.chars().collect::<Vec<_>>();
    let chunk_size = 100_000;
    chars
        .chunks(chunk_size)
        .enumerate()
        .map(|(idx, chunk)| Chapter {
            id: Uuid::new_v4().to_string(),
            novel_id: novel_id.to_string(),
            index: (idx + 1) as i64,
            title: format!("自动分段 {}", idx + 1),
            original_text: chunk.iter().collect::<String>().trim().to_string(),
            analysis_json: None,
            rewrite_text: None,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        })
        .collect()
}

fn seed_canon_assets(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    for kind in ["人物卡", "人物关系", "地点", "伏笔", "术语表"] {
        conn.execute(
            "INSERT OR IGNORE INTO canon_assets (novel_id, kind, content, updated_at) VALUES (?1, ?2, '', ?3)",
            params![novel_id, kind, now],
        )?;
    }
    Ok(())
}

fn create_chapter_batches(
    conn: &Connection,
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    detected_chapters: bool,
) -> Result<(), String> {
    let batch_size = if detected_chapters { 30 } else { 1 };
    let batch_dir = data_dir.join("chapter_batches").join(novel_id);
    fs::create_dir_all(&batch_dir).map_err(to_string)?;
    let now = Utc::now().to_rfc3339();

    for (idx, chunk) in chapters.chunks(batch_size).enumerate() {
        let first = chunk
            .first()
            .ok_or_else(|| "批次内容为空。".to_string())?;
        let last = chunk
            .last()
            .ok_or_else(|| "批次内容为空。".to_string())?;
        let batch_index = (idx + 1) as i64;
        let label = if detected_chapters {
            format!("{}-{}章", first.index, last.index)
        } else {
            format!("第{}批（约10万字）", batch_index)
        };
        let file_path = batch_dir.join(format!("batch-{batch_index:03}.txt"));
        let body = chunk
            .iter()
            .map(|chapter| format!("{}\n\n{}", chapter.title, chapter.original_text))
            .collect::<Vec<_>>()
            .join("\n\n");
        fs::write(&file_path, body).map_err(to_string)?;
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                novel_id,
                batch_index,
                label,
                first.index,
                last.index,
                file_path.to_string_lossy().to_string(),
                now
            ],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

fn load_chapters(conn: &Connection, novel_id: &str) -> Result<Vec<Chapter>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, analysis_status, rewrite_status FROM chapters WHERE novel_id = ?1 ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let chapters = stmt.query_map(params![novel_id], row_to_chapter)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}

fn load_chapters_for_batch(conn: &Connection, novel_id: &str, batch_id: &str) -> Result<Vec<Chapter>, String> {
    let batch = conn
        .query_row(
            "SELECT id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at FROM chapter_batches WHERE id = ?1 AND novel_id = ?2",
            params![batch_id, novel_id],
            row_to_chapter_batch,
        )
        .map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, analysis_status, rewrite_status FROM chapters WHERE novel_id = ?1 AND chapter_index BETWEEN ?2 AND ?3 ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let chapters = stmt
        .query_map(params![novel_id, batch.start_chapter, batch.end_chapter], row_to_chapter)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}

fn load_chapter_batches(conn: &Connection, novel_id: &str) -> Result<Vec<ChapterBatch>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at FROM chapter_batches WHERE novel_id = ?1 ORDER BY batch_index",
        )
        .map_err(to_string)?;
    let batches = stmt
        .query_map(params![novel_id], row_to_chapter_batch)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(batches)
}

fn load_novel_settings(conn: &Connection, novel_id: &str) -> Result<Option<NovelSettings>, String> {
    let result = conn.query_row(
        "SELECT novel_id, protagonist_name, additional_feminize_names, bust, body_type, advanced_settings, updated_at FROM novel_settings WHERE novel_id = ?1",
        params![novel_id],
        row_to_novel_settings,
    );
    match result {
        Ok(settings) => Ok(Some(settings)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(to_string(error)),
    }
}

fn require_novel_settings(conn: &Connection, novel_id: &str) -> Result<NovelSettings, String> {
    let settings = load_novel_settings(conn, novel_id)?
        .ok_or_else(|| "请先填写设定".to_string())?;
    if settings.protagonist_name.trim().is_empty()
        || settings.bust.trim().is_empty()
        || settings.body_type.trim().is_empty()
    {
        return Err("请先填写设定".to_string());
    }
    Ok(settings)
}

fn load_canon_assets(conn: &Connection, novel_id: &str) -> Result<Vec<CanonAsset>, String> {
    let mut stmt = conn
        .prepare("SELECT novel_id, kind, content, updated_at FROM canon_assets WHERE novel_id = ?1 ORDER BY kind")
        .map_err(to_string)?;
    let assets = stmt.query_map(params![novel_id], |row| {
        Ok(CanonAsset {
            novel_id: row.get(0)?,
            kind: row.get(1)?,
            content: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })
    .map_err(to_string)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(to_string)?;
    Ok(assets)
}

fn load_model_profile(state: &State<'_, AppState>, profile_id: &str) -> Result<ModelProfile, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, name, provider, base_url, model, temperature, updated_at, api_key FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| {
            let id: String = row.get(0)?;
            let db_api_key: Option<String> = row.get(7)?;
            Ok(ModelProfile {
                has_api_key: read_api_key(&id).is_ok()
                    || db_api_key.as_deref().is_some_and(|value| !value.is_empty()),
                id,
                name: row.get(1)?,
                provider: row.get(2)?,
                base_url: row.get(3)?,
                model: row.get(4)?,
                temperature: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .map_err(to_string)
}

async fn generate_text(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<ModelOutput, String> {
    if profile.provider.to_lowercase().contains("gemini") {
        generate_gemini(client, profile, api_key, system, user).await
    } else {
        generate_openai_compatible(client, profile, api_key, system, user).await
    }
}

async fn generate_openai_compatible(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<ModelOutput, String> {
    let base = profile.base_url.trim().trim_end_matches('/');
    let model = normalize_model_name(base, &profile.model);
    let endpoint = if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{}/chat/completions", base)
    };
    let payload = json!({
        "model": model,
        "temperature": profile.temperature,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let response = client
        .post(endpoint)
        .bearer_auth(api_key.trim())
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?;
    let (value, raw_response) = response_json_or_error(response).await?;
    let text = value["choices"][0]["message"]["content"]
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("模型响应缺少 choices[0].message.content: {}", value))?;
    let reasoning = value["choices"][0]["message"]["reasoning_content"]
        .as_str()
        .or_else(|| value["choices"][0]["message"]["reasoning"].as_str())
        .map(str::to_string);
    Ok(ModelOutput {
        text,
        reasoning,
        raw_response,
    })
}

async fn generate_gemini(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<ModelOutput, String> {
    let base = if profile.base_url.trim().is_empty() {
        "https://generativelanguage.googleapis.com/v1beta".to_string()
    } else {
        profile.base_url.trim().trim_end_matches('/').to_string()
    };
    let endpoint = format!("{}/models/{}:generateContent?key={}", base, profile.model.trim(), api_key.trim());
    let payload = json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {"text": format!("{}\n\n{}", system, user)}
                ]
            }
        ],
        "generationConfig": {
            "temperature": profile.temperature
        }
    });
    let response = client
        .post(endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?;
    let (value, raw_response) = response_json_or_error(response).await?;
    let text = value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("Gemini 响应缺少正文: {}", value))?;
    let reasoning = value["candidates"][0]["content"]["parts"]
        .as_array()
        .and_then(|parts| {
            let thoughts = parts
                .iter()
                .filter(|part| part["thought"].as_bool().unwrap_or(false))
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>();
            if thoughts.is_empty() {
                None
            } else {
                Some(thoughts.join("\n\n"))
            }
        });
    Ok(ModelOutput {
        text,
        reasoning,
        raw_response,
    })
}

async fn response_json_or_error(response: reqwest::Response) -> Result<(serde_json::Value, String), String> {
    let status = response.status();
    let body = response.text().await.map_err(to_string)?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, compact_error_body(&body)));
    }
    let value = serde_json::from_str(&body)
        .map_err(|error| format!("模型响应不是合法 JSON: {}；原始响应：{}", error, body))?;
    Ok((value, body))
}

fn compact_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "响应体为空".to_string();
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

fn normalize_model_name(base_url: &str, model: &str) -> String {
    let trimmed = model.trim();
    if base_url.to_ascii_lowercase().contains("api.deepseek.com") {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_string()
    }
}

fn build_novel_settings_prompt(settings: &NovelSettings) -> String {
    let additional = if settings.additional_feminize_names.trim().is_empty() {
        "无".to_string()
    } else {
        settings.additional_feminize_names.clone()
    };
    let additional = if settings.advanced_settings.trim().is_empty() {
        additional
    } else {
        format!("{}\n\n高级设定：{}", additional, settings.advanced_settings.trim())
    };
    format!(
        r#"小说基本设定：
- 主角原姓名：{}
- 其他需要女性化的人物姓名：{}
- 身材：{}
- 体型：{}

姓名女性化规则：
1. 主角姓名必须女性化，不能保留明显男性化姓名。
2. 优先保留姓氏，名字部分用同音字或近音字替换为更女性化的字。
3. 示例：萧炎 -> 萧妍；李火旺 -> 李火婉。
4. 其他需要女性化的人物姓名只在文本中实际出现时处理，未出现则忽略。
5. 分析和改写必须维护一致的姓名映射，避免同一人物前后姓名不一致。"#,
        settings.protagonist_name,
        additional,
        settings.bust,
        settings.body_type
    )
}

#[allow(dead_code)]
fn build_analysis_prompt_with_settings(chapter: &Chapter, settings: &NovelSettings) -> String {
    format!(
        r#"请分析以下章节，并输出 JSON：
{{
  "outline": "本章大纲",
  "characters": ["角色与设定变化"],
  "relationships": ["人物关系变化"],
  "locations": ["地点"],
  "foreshadowing": ["伏笔或回收"],
  "name_feminization_map": ["原姓名 -> 女性化姓名，未出现的人物不要写入"],
  "rewrite_notes": ["后续百合改写必须注意的性别、称谓、动作、外貌、关系细节"]
}}

{}

章节标题：{}

章节正文：
{}"#,
        build_novel_settings_prompt(settings),
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

fn build_rewrite_prompt_with_settings(chapter: &Chapter, canon_text: &str, settings: &NovelSettings) -> String {
    format!(
        r#"改写要求：
1. 将原本男女主叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 清除所有原男主痕迹，包括代词、身体描写、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
4. 主角姓名必须按同音或近音原则女性化，例如萧炎改为萧妍，李火旺改为李火婉；其他指定姓名只在本章出现时女性化。
5. 按基本设定中的身材和体型调整外貌、动作和互动细节，不要出现与设定冲突的描写。
6. 保持中文网文可读性，只输出改写后的正文，不要解释。

{}

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        build_novel_settings_prompt(settings),
        canon_text,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
fn build_analysis_prompt(chapter: &Chapter) -> String {
    format!(
        r#"请分析以下章节，并输出 JSON：
{{
  "outline": "本章大纲",
  "characters": ["角色与设定变化"],
  "relationships": ["人物关系变化"],
  "locations": ["地点"],
  "foreshadowing": ["伏笔或回收"],
  "rewrite_notes": ["后续百合改写必须注意的性别、称谓、动作、外貌、关系细节"]
}}

章节标题：{}

章节正文：
{}"#,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
fn build_rewrite_prompt(chapter: &Chapter, canon_text: &str) -> String {
    format!(
        r#"改写要求：
1. 将原本男女主叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 清除所有原男主痕迹，包括代词、身体描写、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
4. 保持中文网文可读性，只输出改写后的正文，不要解释。

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        canon_text,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

fn merge_analysis_into_canon_assets(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let rows = stmt
        .query_map(params![novel_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let analyses = rows
        .iter()
        .map(|(title, analysis_json)| format!("## {}\n{}", title, analysis_json))
        .collect::<Vec<_>>()
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    upsert_canon_asset(conn, novel_id, "AI分析汇总", &analyses, &now)?;
    upsert_canon_asset(conn, novel_id, "人物卡", &collect_analysis_field(&rows, "characters"), &now)?;
    upsert_canon_asset(conn, novel_id, "人物关系", &collect_analysis_field(&rows, "relationships"), &now)?;
    upsert_canon_asset(conn, novel_id, "地点", &collect_analysis_field(&rows, "locations"), &now)?;
    upsert_canon_asset(conn, novel_id, "伏笔", &collect_analysis_field(&rows, "foreshadowing"), &now)?;
    upsert_canon_asset(conn, novel_id, "术语表", &collect_analysis_terms(&rows), &now)?;
    Ok(())
}

fn upsert_canon_asset(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
    content: &str,
    updated_at: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(novel_id, kind) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
        "#,
        params![novel_id, kind, content, updated_at],
    )?;
    Ok(())
}

fn collect_analysis_field(rows: &[(String, String)], field: &str) -> String {
    rows.iter()
        .filter_map(|(title, analysis_json)| {
            let value = serde_json::from_str::<serde_json::Value>(analysis_json).ok()?;
            let text = json_field_to_text(value.get(field)?);
            if text.trim().is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", title, text))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_analysis_terms(rows: &[(String, String)]) -> String {
    rows.iter()
        .filter_map(|(title, analysis_json)| {
            let value = serde_json::from_str::<serde_json::Value>(analysis_json).ok()?;
            let mut sections = Vec::new();
            if let Some(text) = value
                .get("name_feminization_map")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("姓名女性化映射：\n{}", text));
            }
            if let Some(text) = value
                .get("rewrite_notes")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("改写注意事项：\n{}", text));
            }
            if sections.is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", title, sections.join("\n\n")))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn json_field_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(json_field_to_text)
            .filter(|text| !text.trim().is_empty())
            .map(|text| format!("- {}", text))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(_) => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        _ => value.to_string(),
    }
}

fn fill_empty_canon_assets_from_analysis(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let rows = stmt
        .query_map(params![novel_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(());
    }

    let analyses = rows
        .iter()
        .map(|(title, analysis_json)| format!("## {}\n{}", title, analysis_json))
        .collect::<Vec<_>>()
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    upsert_empty_canon_asset(conn, novel_id, "AI分析汇总", &analyses, &now)?;
    upsert_empty_canon_asset(conn, novel_id, "人物卡", &collect_analysis_field(&rows, "characters"), &now)?;
    upsert_empty_canon_asset(conn, novel_id, "人物关系", &collect_analysis_field(&rows, "relationships"), &now)?;
    upsert_empty_canon_asset(conn, novel_id, "地点", &collect_analysis_field(&rows, "locations"), &now)?;
    upsert_empty_canon_asset(conn, novel_id, "伏笔", &collect_analysis_field(&rows, "foreshadowing"), &now)?;
    upsert_empty_canon_asset(conn, novel_id, "术语表", &collect_analysis_terms(&rows), &now)?;
    Ok(())
}

fn upsert_empty_canon_asset(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
    content: &str,
    updated_at: &str,
) -> rusqlite::Result<()> {
    if content.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(novel_id, kind) DO UPDATE SET
            content = CASE
                WHEN trim(canon_assets.content) = '' THEN excluded.content
                ELSE canon_assets.content
            END,
            updated_at = CASE
                WHEN trim(canon_assets.content) = '' THEN excluded.updated_at
                ELSE canon_assets.updated_at
            END
        "#,
        params![novel_id, kind, content, updated_at],
    )?;
    Ok(())
}

#[allow(dead_code)]
fn merge_analysis_into_canon(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let analyses = stmt
        .query_map(params![novel_id], |row| {
            Ok(format!("## {}\n{}", row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, 'AI分析汇总', ?2, ?3)
        ON CONFLICT(novel_id, kind) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
        "#,
        params![novel_id, analyses, now],
    )?;
    Ok(())
}

fn create_job(state: &State<'_, AppState>, novel_id: &str, job_type: &str, total: i64) -> Result<Job, String> {
    let now = Utc::now().to_rfc3339();
    let job = Job {
        id: Uuid::new_v4().to_string(),
        novel_id: novel_id.to_string(),
        job_type: job_type.to_string(),
        status: "running".to_string(),
        current_chapter: 0,
        total_chapters: total,
        message: "任务已开始".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO jobs (id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![job.id, job.novel_id, job.job_type, job.status, job.current_chapter, job.total_chapters, job.message, job.created_at, job.updated_at],
    )
    .map_err(to_string)?;
    Ok(job)
}

fn update_job(
    state: &State<'_, AppState>,
    job_id: &str,
    status: &str,
    current_chapter: i64,
    message: &str,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "UPDATE jobs SET status = ?1, current_chapter = ?2, message = ?3, updated_at = ?4 WHERE id = ?5",
        params![status, current_chapter, message, Utc::now().to_rfc3339(), job_id],
    )
    .map_err(to_string)?;
    Ok(())
}

fn set_chapter_status(
    state: &State<'_, AppState>,
    chapter_id: &str,
    column: &str,
    status: &str,
) -> Result<(), String> {
    let sql = match column {
        "analysis_status" => "UPDATE chapters SET analysis_status = ?1 WHERE id = ?2",
        "rewrite_status" => "UPDATE chapters SET rewrite_status = ?1 WHERE id = ?2",
        _ => return Err("invalid chapter status column".to_string()),
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(sql, params![status, chapter_id])
        .map_err(to_string)?;
    Ok(())
}

fn read_api_key(profile_id: &str) -> Result<String, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, profile_id).map_err(to_string)?;
    entry.get_password().map_err(to_string)
}

fn write_api_key(profile_id: &str, api_key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, profile_id).map_err(to_string)?;
    entry.set_password(api_key).map_err(to_string)
}

fn delete_api_key(profile_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, profile_id).map_err(to_string)?;
    entry.delete_credential().map_err(to_string)
}

fn read_stored_api_key(state: &State<'_, AppState>, profile_id: &str) -> Result<String, String> {
    if let Ok(api_key) = read_api_key(profile_id) {
        if !api_key.trim().is_empty() {
            return Ok(api_key);
        }
    }
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT api_key FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .map_err(to_string)?
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "未保存 API Key，请填写 API Key 后点击保存。".to_string())
}

fn stored_api_key_exists(conn: &Connection, profile_id: &str) -> bool {
    if read_api_key(profile_id).is_ok() {
        return true;
    }
    conn.query_row(
        "SELECT api_key FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .as_deref()
    .is_some_and(|value| !value.trim().is_empty())
}

fn row_to_novel(row: &rusqlite::Row<'_>) -> rusqlite::Result<Novel> {
    Ok(Novel {
        id: row.get(0)?,
        title: row.get(1)?,
        source_path: row.get(2)?,
        encoding: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn row_to_chapter(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chapter> {
    Ok(Chapter {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        index: row.get(2)?,
        title: row.get(3)?,
        original_text: row.get(4)?,
        analysis_json: row.get(5)?,
        rewrite_text: row.get(6)?,
        analysis_status: row.get(7)?,
        rewrite_status: row.get(8)?,
    })
}

fn row_to_chapter_batch(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChapterBatch> {
    Ok(ChapterBatch {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        batch_index: row.get(2)?,
        label: row.get(3)?,
        start_chapter: row.get(4)?,
        end_chapter: row.get(5)?,
        file_path: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_to_novel_settings(row: &rusqlite::Row<'_>) -> rusqlite::Result<NovelSettings> {
    Ok(NovelSettings {
        novel_id: row.get(0)?,
        protagonist_name: row.get(1)?,
        additional_feminize_names: row.get(2)?,
        bust: row.get(3)?,
        body_type: row.get(4)?,
        advanced_settings: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        job_type: row.get(2)?,
        status: row.get(3)?,
        current_chapter: row.get(4)?,
        total_chapters: row.get(5)?,
        message: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_ai_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiLog> {
    Ok(AiLog {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        profile_id: row.get(2)?,
        action: row.get(3)?,
        chapter_title: row.get(4)?,
        status: row.get(5)?,
        content: row.get(6)?,
        reasoning: row.get(7)?,
        raw_response: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn append_ai_log(
    state: &State<'_, AppState>,
    novel_id: Option<&str>,
    profile_id: &str,
    action: &str,
    chapter_title: Option<&str>,
    status: &str,
    content: &str,
    reasoning: Option<&str>,
    raw_response: Option<&str>,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            Uuid::new_v4().to_string(),
            novel_id,
            profile_id,
            action,
            chapter_title,
            status,
            truncate_text(content, 12_000),
            reasoning.map(|value| truncate_text(value, 12_000)),
            raw_response.map(|value| truncate_text(value, 24_000)),
            Utc::now().to_rfc3339()
        ],
    )
    .map_err(to_string)?;
    Ok(())
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        value.push_str("\n\n[由于上下文限制，本章后续内容已截断。]");
    }
    value
}

fn normalize_jsonish(text: &str) -> String {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

fn normalize_name_list(input: &str) -> String {
    input
        .split(|ch| matches!(ch, '\n' | '\r' | ',' | '，' | '、' | ';' | '；'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_file_name(input: &str) -> String {
    let cleaned = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>();
    if cleaned.trim().is_empty() {
        "novel".to_string()
    } else {
        cleaned
    }
}

fn to_string<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_heading_regex_covers_common_toc_rules() {
        let heading_re = chapter_heading_regex();
        for title in [
            "第1章 限落的天才",
            "正文 第三章：客人",
            "第一话 新的开始",
            "卷五 开源盛典",
            "上卷 山雨",
            "Chapter 1 MyGrandmaIsNB",
            "Section 12",
            "Part 3 - After",
            "Episode 4",
            "No. 5",
            "12",
            "一百七十",
            "1、这就是标题",
            "二十四、我瞎编的标题",
            "（11）我奶常山赵子龙",
            "【特别篇】",
            "=== 起 ===",
            "番外篇 她们后来",
        ] {
            assert!(heading_re.is_match(title), "expected heading match: {title}");
        }
    }
}
