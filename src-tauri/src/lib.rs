use chrono::Utc;
use encoding_rs::{GBK, UTF_8};
use regex::Regex;
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

const KEYRING_SERVICE: &str = "YuriRewrite";
const GITHUB_REPOSITORY_URL: &str = "https://github.com/3minto1/Yuri-Rewrite";
const GITHUB_LATEST_RELEASE_URL: &str = "https://github.com/3minto1/Yuri-Rewrite/releases/latest";
const AUTO_RUN_PAUSED: &str = "__YURI_AUTO_RUN_PAUSED__";
const AUTO_RUN_TERMINATED: &str = "__YURI_AUTO_RUN_TERMINATED__";

struct AppState {
    conn: Mutex<Connection>,
    client: Client,
    data_dir: PathBuf,
    app_dir: PathBuf,
    auto_runs: Mutex<HashMap<String, AutoRunControl>>,
}

#[derive(Debug, Clone)]
struct AutoRunControl {
    status: String,
    completed_batches: i64,
    job_id: Option<String>,
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
    rewritten_protagonist_name: String,
    additional_feminize_names: String,
    bust: String,
    body_type: String,
    rewrite_mode: String,
    advanced_settings: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NameMappingEntry {
    source: String,
    target: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NameMappingAsset {
    version: i64,
    protagonist: Option<NameMappingEntry>,
    names: Vec<NameMappingEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ModelProfile {
    id: String,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    temperature: f64,
    thinking_mode: String,
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
    thinking_mode: Option<String>,
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

#[derive(Debug, Serialize, Clone)]
struct JobProgress {
    id: String,
    novel_id: String,
    job_type: String,
    status: String,
    current_chapter: i64,
    total_chapters: i64,
    message: String,
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
    #[serde(default)]
    core_prompt: String,
    #[serde(default)]
    review_enabled: bool,
    #[serde(default)]
    review_profile_id: Option<String>,
    #[serde(default = "default_rewrite_parallelism")]
    rewrite_parallelism: usize,
}

struct ModelOutput {
    text: String,
    reasoning: Option<String>,
    raw_response: String,
    input_chars: usize,
    output_chars: usize,
    elapsed_ms: u128,
    retried_without_thinking: bool,
}

#[derive(Debug, Serialize)]
struct JobEstimate {
    novel_chapters: usize,
    novel_chars: usize,
    novel_batches: usize,
    selected_batch_chapters: usize,
    selected_batch_chars: usize,
    parallelism: usize,
    review_enabled: bool,
    current_batch_requests: usize,
    full_run_requests: usize,
    average_call_seconds: Option<f64>,
    estimated_current_batch_seconds: Option<f64>,
    estimated_full_run_seconds: Option<f64>,
    recent_success_calls: usize,
    recent_failed_calls: usize,
    average_input_chars: Option<usize>,
    average_output_chars: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ModelDiagnosis {
    status: String,
    recommended_thinking_mode: Option<String>,
    checks: Vec<ModelDiagnosisCheck>,
}

#[derive(Debug, Serialize)]
struct ModelDiagnosisCheck {
    name: String,
    status: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportResult {
    path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateCheckResult {
    current_version: String,
    latest_version: String,
    latest_tag: String,
    is_latest: bool,
    release_url: String,
    asset_name: String,
    asset_download_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateDownloadResult {
    path: String,
    version: String,
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

#[derive(Debug, Clone)]
struct ParsedChapterRewrite {
    id: String,
    index: i64,
    title: String,
    text: String,
}

#[derive(Debug, Clone)]
struct ParsedChapterAnalysis {
    id: String,
    json: String,
}

#[derive(Debug, Clone)]
struct ReviewDecision {
    approved: bool,
    issues: Vec<String>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let app_dir = env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| data_dir.clone());
            fs::create_dir_all(&data_dir)?;
            fs::create_dir_all(data_dir.join("exports"))?;
            let conn = Connection::open(data_dir.join("yuri-rewrite.sqlite3"))?;
            init_db(&conn)?;
            app.manage(AppState {
                conn: Mutex::new(conn),
                client: Client::new(),
                data_dir,
                app_dir,
                auto_runs: Mutex::new(HashMap::new()),
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
            diagnose_model_profile,
            estimate_job_cost,
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
            start_analyze_rewrite_all,
            pause_analyze_rewrite_all,
            terminate_analyze_rewrite_all,
            get_job,
            export_novel,
            open_github_url,
            check_for_updates,
            download_latest_update
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
            thinking_mode TEXT NOT NULL DEFAULT 'auto',
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
            rewritten_protagonist_name TEXT NOT NULL DEFAULT '',
            additional_feminize_names TEXT NOT NULL,
            bust TEXT NOT NULL,
            body_type TEXT NOT NULL,
            rewrite_mode TEXT NOT NULL DEFAULT 'strict',
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
    ensure_column(
        conn,
        "model_profiles",
        "thinking_mode",
        "TEXT NOT NULL DEFAULT 'auto'",
    )?;
    ensure_column(conn, "ai_logs", "reasoning", "TEXT")?;
    ensure_column(conn, "ai_logs", "raw_response", "TEXT")?;
    ensure_column(
        conn,
        "novel_settings",
        "rewritten_protagonist_name",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "novel_settings",
        "advanced_settings",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        conn,
        "novel_settings",
        "rewrite_mode",
        "TEXT NOT NULL DEFAULT 'strict'",
    )?;
    migrate_api_keys_to_keyring(conn)?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!(
                "ALTER TABLE {} ADD COLUMN {} {}",
                table, column, column_type
            ),
            [],
        )?;
    }
    Ok(())
}

fn migrate_api_keys_to_keyring(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, api_key FROM model_profiles WHERE api_key IS NOT NULL AND trim(api_key) != ''",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    for (profile_id, api_key) in rows {
        let _ = write_api_key(&profile_id, &api_key);
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
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![novel.id, novel.title, novel.source_path, novel.encoding, novel.status, novel.created_at],
    )
    .map_err(to_string)?;

    for chapter in &split.chapters {
        tx.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
            params![chapter.id, chapter.novel_id, chapter.index, chapter.title, chapter.original_text],
        )
        .map_err(to_string)?;
    }

    create_chapter_batches(
        &tx,
        &state.data_dir,
        &novel.id,
        &split.chapters,
        split.detected_chapters,
    )
    .map_err(to_string)?;
    seed_canon_assets(&tx, &novel.id).map_err(to_string)?;
    tx.commit().map_err(to_string)?;
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
    let mut conn = state.conn.lock().map_err(to_string)?;
    let batch_dir = state.data_dir.join("chapter_batches").join(&novel_id);
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "DELETE FROM novel_settings WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    tx.execute(
        "DELETE FROM chapter_batches WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    tx.execute(
        "DELETE FROM chapters WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    tx.execute(
        "DELETE FROM canon_assets WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    tx.execute("DELETE FROM jobs WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    tx.execute("DELETE FROM ai_logs WHERE novel_id = ?1", params![novel_id])
        .map_err(to_string)?;
    tx.execute("DELETE FROM novels WHERE id = ?1", params![novel_id])
        .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    if batch_dir.exists() {
        fs::remove_dir_all(&batch_dir).map_err(to_string)?;
    }
    Ok(())
}

#[tauri::command]
fn save_model_profile(
    input: ModelProfileInput,
    state: State<AppState>,
) -> Result<ModelProfile, String> {
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let updated_at = Utc::now().to_rfc3339();
    let api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "********")
        .map(str::to_string);
    let mut db_api_key_fallback = None;
    if let Some(value) = &api_key {
        let _ = write_api_key(&id, value);
        db_api_key_fallback = Some(value.clone());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let thinking_mode = normalize_thinking_mode(input.thinking_mode.as_deref())?;
    let profile = ModelProfile {
        id: id.clone(),
        name: input.name,
        provider: input.provider,
        base_url: input.base_url,
        model: input.model,
        temperature: input.temperature,
        thinking_mode,
        has_api_key: api_key.is_some() || stored_api_key_exists(&conn, &id),
        updated_at,
    };

    conn.execute(
        r#"
        INSERT INTO model_profiles (id, name, provider, base_url, model, temperature, thinking_mode, updated_at, api_key)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            base_url = excluded.base_url,
            model = excluded.model,
            temperature = excluded.temperature,
            thinking_mode = excluded.thinking_mode,
            updated_at = excluded.updated_at,
            api_key = CASE
                WHEN ?9 IS NOT NULL THEN excluded.api_key
                WHEN ?10 IS NOT NULL THEN NULL
                ELSE model_profiles.api_key
            END
        "#,
        params![
            profile.id,
            profile.name,
            profile.provider,
            profile.base_url,
            profile.model,
            profile.temperature,
            profile.thinking_mode,
            profile.updated_at,
            db_api_key_fallback,
            api_key
        ],
    )
    .map_err(to_string)?;

    Ok(profile)
}

#[tauri::command]
fn delete_model_profile(profile_id: String, state: State<AppState>) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "DELETE FROM model_profiles WHERE id = ?1",
        params![profile_id],
    )
    .map_err(to_string)?;
    conn.execute(
        "DELETE FROM ai_logs WHERE profile_id = ?1",
        params![profile_id],
    )
    .map_err(to_string)?;
    let _ = delete_api_key(&profile_id);
    Ok(())
}

#[tauri::command]
fn list_model_profiles(state: State<AppState>) -> Result<Vec<ModelProfile>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider, base_url, model, temperature, thinking_mode, updated_at, api_key FROM model_profiles ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let profiles = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let db_api_key: Option<String> = row.get(8)?;
            Ok(ModelProfile {
                has_api_key: read_api_key(&id).is_ok()
                    || db_api_key.as_deref().is_some_and(|value| !value.is_empty()),
                id,
                name: row.get(1)?,
                provider: row.get(2)?,
                base_url: row.get(3)?,
                model: row.get(4)?,
                temperature: row.get(5)?,
                thinking_mode: row.get(6)?,
                updated_at: row.get(7)?,
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
    let review_enabled = load_review_enabled(&conn)?;
    let review_profile_id = load_review_profile_id(&conn)?;
    let rewrite_parallelism = load_rewrite_parallelism(&conn)?;
    let core_prompt = load_core_prompt(&conn)?;
    Ok(AppSettings {
        export_dir,
        core_prompt,
        review_enabled,
        review_profile_id,
        rewrite_parallelism,
    })
}

#[tauri::command]
fn save_app_settings(settings: AppSettings, state: State<AppState>) -> Result<AppSettings, String> {
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
        save_core_prompt(&conn, &settings.core_prompt)?;
        Ok(AppSettings {
            export_dir: Some(export_dir.to_string()),
            core_prompt: settings.core_prompt.trim().to_string(),
            review_enabled: settings.review_enabled,
            review_profile_id: normalize_review_profile_id(settings.review_profile_id.as_deref()),
            rewrite_parallelism,
        })
    } else {
        conn.execute("DELETE FROM app_settings WHERE key = 'export_dir'", [])
            .map_err(to_string)?;
        save_review_enabled(&conn, settings.review_enabled)?;
        save_rewrite_parallelism(&conn, rewrite_parallelism)?;
        save_review_profile_id(&conn, settings.review_profile_id.as_deref())?;
        save_core_prompt(&conn, &settings.core_prompt)?;
        Ok(AppSettings {
            export_dir: None,
            core_prompt: settings.core_prompt.trim().to_string(),
            review_enabled: settings.review_enabled,
            review_profile_id: normalize_review_profile_id(settings.review_profile_id.as_deref()),
            rewrite_parallelism,
        })
    }
}

fn load_core_prompt(conn: &Connection) -> Result<String, String> {
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

fn save_core_prompt(conn: &Connection, value: &str) -> Result<(), String> {
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

fn load_review_enabled(conn: &Connection) -> Result<bool, String> {
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

fn save_review_enabled(conn: &Connection, enabled: bool) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('review_enabled', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![if enabled { "true" } else { "false" }],
    )
    .map_err(to_string)?;
    Ok(())
}

fn normalize_review_profile_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn load_review_profile_id(conn: &Connection) -> Result<Option<String>, String> {
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

fn save_review_profile_id(conn: &Connection, profile_id: Option<&str>) -> Result<(), String> {
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

fn default_rewrite_parallelism() -> usize {
    6
}

fn normalize_rewrite_parallelism(value: usize) -> usize {
    match value {
        1 | 3 | 6 | 10 => value,
        _ => default_rewrite_parallelism(),
    }
}

fn load_rewrite_parallelism(conn: &Connection) -> Result<usize, String> {
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

fn save_rewrite_parallelism(conn: &Connection, value: usize) -> Result<(), String> {
    let normalized = normalize_rewrite_parallelism(value);
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('rewrite_parallelism', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![normalized.to_string()],
    )
    .map_err(to_string)?;
    Ok(())
}

#[tauri::command]
fn get_novel_settings(
    novel_id: String,
    state: State<AppState>,
) -> Result<Option<NovelSettings>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_novel_settings(&conn, &novel_id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn save_novel_settings(
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

#[tauri::command]
fn list_chapter_batches(
    novel_id: String,
    state: State<AppState>,
) -> Result<Vec<ChapterBatch>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_chapter_batches(&conn, &novel_id)
}

#[tauri::command]
fn estimate_job_cost(
    novel_id: String,
    batch_id: Option<String>,
    profile_id: Option<String>,
    state: State<AppState>,
) -> Result<JobEstimate, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel_id)?;
    let batches = load_chapter_batches(&conn, &novel_id)?;
    let parallelism = load_rewrite_parallelism(&conn)?;
    let review_enabled = load_review_enabled(&conn)?;
    let selected_batch = batch_id
        .as_deref()
        .and_then(|id| load_chapters_for_batch(&conn, &novel_id, id).ok())
        .or_else(|| {
            batches
                .first()
                .and_then(|batch| load_chapters_for_batch(&conn, &novel_id, &batch.id).ok())
        })
        .unwrap_or_default();
    let current_batch_requests =
        estimate_requests_for_chapters(&selected_batch, parallelism, review_enabled);
    let current_batch_wait_stages =
        estimate_wait_stages_for_chapters(&selected_batch, review_enabled);
    let full_run_requests = batches
        .iter()
        .map(|batch| {
            load_chapters_for_batch(&conn, &novel_id, &batch.id)
                .map(|batch_chapters| {
                    estimate_requests_for_chapters(&batch_chapters, parallelism, review_enabled)
                })
                .unwrap_or(0)
        })
        .sum::<usize>();
    let full_run_wait_stages = batches
        .iter()
        .map(|batch| {
            load_chapters_for_batch(&conn, &novel_id, &batch.id)
                .map(|batch_chapters| {
                    estimate_wait_stages_for_chapters(&batch_chapters, review_enabled)
                })
                .unwrap_or(0)
        })
        .sum::<usize>();
    let stats = profile_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| load_recent_model_stats(&conn, id).ok())
        .unwrap_or_default();
    let average_call_seconds = stats.average_call_seconds();
    Ok(JobEstimate {
        novel_chapters: chapters.len(),
        novel_chars: chapters.iter().map(chapter_text_chars).sum(),
        novel_batches: batches.len(),
        selected_batch_chapters: selected_batch.len(),
        selected_batch_chars: selected_batch.iter().map(chapter_text_chars).sum(),
        parallelism,
        review_enabled,
        current_batch_requests,
        full_run_requests,
        average_call_seconds,
        estimated_current_batch_seconds: average_call_seconds
            .map(|seconds| seconds * current_batch_wait_stages as f64),
        estimated_full_run_seconds: average_call_seconds
            .map(|seconds| seconds * full_run_wait_stages as f64),
        recent_success_calls: stats.success_calls,
        recent_failed_calls: stats.failed_calls,
        average_input_chars: stats.average_input_chars(),
        average_output_chars: stats.average_output_chars(),
    })
}

#[tauri::command]
async fn test_model_profile(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<ModelTestResult, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    match generate_text(
        &state.client,
        &profile,
        &api_key,
        "你是一个连接测试助手。只回复一句中文。",
        "请回复：连接成功。",
        false,
    )
    .await
    {
        Ok(output) => {
            let log_content = format_model_log_content(&output, &profile, None);
            append_ai_log(
                &state,
                None,
                &profile.id,
                "测试模型",
                None,
                "success",
                &log_content,
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            Ok(ModelTestResult {
                ok: true,
                message: output.text,
            })
        }
        Err(error) => {
            append_ai_log(
                &state,
                None,
                &profile.id,
                "测试模型",
                None,
                "error",
                &error,
                None,
                None,
            )?;
            Ok(ModelTestResult {
                ok: false,
                message: error,
            })
        }
    }
}

#[tauri::command]
async fn diagnose_model_profile(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<ModelDiagnosis, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let mut checks = Vec::new();
    let api_key = match read_stored_api_key(&state, &profile.id) {
        Ok(api_key) => {
            checks.push(diagnosis_check(
                "API Key",
                "ok",
                "已找到本地保存的 API Key。",
            ));
            api_key
        }
        Err(error) => {
            checks.push(diagnosis_check(
                "API Key",
                "failed",
                &format!("无法读取 API Key：{}", error),
            ));
            let diagnosis = build_model_diagnosis(checks, Some("auto"));
            append_diagnosis_log(&state, &profile.id, &diagnosis)?;
            return Ok(diagnosis);
        }
    };

    let mut recommended_thinking_mode = None;
    let chat_output = generate_text(
        &state.client,
        &profile,
        &api_key,
        "你是一个模型诊断助手。只回复指定内容。",
        "请只回复：连接成功。",
        false,
    )
    .await;
    match chat_output {
        Ok(output) => {
            checks.push(diagnosis_check(
                "普通响应",
                "ok",
                &format!("模型已返回正文：{}", compact_log_line(&output.text, 80)),
            ));
            if profile.thinking_mode == "auto" {
                checks.push(diagnosis_check(
                    "思考模式",
                    "ok",
                    "当前为自动模式，不额外注入 thinking 参数。",
                ));
            } else if output.retried_without_thinking {
                recommended_thinking_mode = Some("auto".to_string());
                checks.push(diagnosis_check(
                    "思考模式",
                    "warning",
                    "当前服务商不接受所选 thinking 参数，已移除参数后重试成功；建议改为自动。",
                ));
            } else {
                checks.push(diagnosis_check(
                    "思考模式",
                    "ok",
                    "当前 thinking 设置在普通响应测试中可用。",
                ));
            }
        }
        Err(error) => {
            if profile.thinking_mode != "auto" {
                recommended_thinking_mode = Some("auto".to_string());
            }
            checks.push(diagnosis_check(
                "普通响应",
                "failed",
                &format!("模型调用失败：{}", error),
            ));
            checks.push(diagnosis_check(
                "思考模式",
                if profile.thinking_mode == "auto" {
                    "warning"
                } else {
                    "failed"
                },
                if profile.thinking_mode == "auto" {
                    "普通响应失败，无法确认 thinking 兼容性。"
                } else {
                    "普通响应失败，建议先切回自动模式排除 thinking 参数兼容问题。"
                },
            ));
            let diagnosis = build_model_diagnosis(checks, recommended_thinking_mode.as_deref());
            append_diagnosis_log(&state, &profile.id, &diagnosis)?;
            return Ok(diagnosis);
        }
    }

    let json_output = generate_text(
        &state.client,
        &profile,
        &api_key,
        "你是一个 JSON 诊断助手。必须只输出合法 JSON，不要 Markdown。",
        r#"请只输出 {"ok": true}。"#,
        true,
    )
    .await;
    match json_output {
        Ok(output) => match parse_jsonish_value(&output.text) {
            Ok(value) if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) => {
                checks.push(diagnosis_check(
                    "JSON 输出",
                    "ok",
                    "模型可以返回可解析 JSON。",
                ));
            }
            Ok(_) => checks.push(diagnosis_check(
                "JSON 输出",
                "warning",
                "模型返回了 JSON，但内容不符合诊断约定；分析仍可能需要重试。",
            )),
            Err(error) => checks.push(diagnosis_check(
                "JSON 输出",
                "warning",
                &format!("模型响应不是稳定 JSON：{}", error),
            )),
        },
        Err(error) => checks.push(diagnosis_check(
            "JSON 输出",
            "warning",
            &format!("JSON 诊断调用失败：{}", error),
        )),
    }

    let diagnosis = build_model_diagnosis(checks, recommended_thinking_mode.as_deref());
    append_diagnosis_log(&state, &profile.id, &diagnosis)?;
    Ok(diagnosis)
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
    let (chapters, settings, rewrite_parallelism) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        (
            load_chapters_for_batch(&conn, &novel_id, &batch_id)?,
            settings,
            load_rewrite_parallelism(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有可分析的内容。".to_string());
    }
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "analysis", total)?;
    let batch_label = format_batch_label(&chapters);

    update_job(
        &state,
        &job.id,
        "running",
        0,
        &format!("正在批次分析 {}", batch_label),
    )?;
    for chapter in &chapters {
        set_chapter_status(&state, &chapter.id, "analysis_status", "running")?;
    }

    let parsed_analysis = match analyze_batch_with_parallelism(
        &state,
        &novel_id,
        &profile,
        &api_key,
        &chapters,
        rewrite_parallelism,
    )
    .await
    {
        Ok(parsed) => parsed,
        Err(error) => {
            mark_chapters_analysis_failed(&state, &chapters)?;
            update_job(&state, &job.id, "failed", 0, &error)?;
            job = get_job(job.id.clone(), state)?;
            return Ok(job);
        }
    };

    save_parsed_analyses(&state, &novel_id, &chapters, parsed_analysis)?;
    ensure_name_mapping_asset(&state, &novel_id, &profile, &api_key, &settings).await?;

    update_job(
        &state,
        &job.id,
        "completed",
        total,
        "分析完成，姓名映射表已更新",
    )?;
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

    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    let (review_profile, review_api_key) = load_review_profile_for_run(
        &state,
        &profile,
        review_enabled,
        review_profile_id.as_deref(),
    )?;
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
            job = get_job(job.id.clone(), state)?;
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
    get_job(job.id, state)
}

#[tauri::command]
async fn start_analyze_rewrite_all(
    novel_id: String,
    profile_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let resume_from = prepare_auto_run(&state, &novel_id)?;
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (novel, batches) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let novel = conn
            .query_row(
                "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
                params![novel_id],
                row_to_novel,
            )
            .map_err(to_string)?;
        require_novel_settings(&conn, &novel.id)?;
        (novel, load_chapter_batches(&conn, &novel_id)?)
    };
    if batches.is_empty() {
        return Err("当前小说没有可处理的批次。".to_string());
    }

    let mut job = create_job(&state, &novel_id, "auto", batches.len() as i64)?;
    register_auto_run_job(&state, &novel_id, &job.id, resume_from)?;
    let start_message = if resume_from > 0 {
        format!("继续一键分析改写，将从第 {} 批重新开始", resume_from + 1)
    } else {
        "准备开始一键分析改写".to_string()
    };
    update_job(&state, &job.id, "running", resume_from, &start_message)?;
    emit_job_progress(&app, &job, "running", resume_from, &start_message);
    let output_dir = {
        let conn = state.conn.lock().map_err(to_string)?;
        resolve_rewrite_export_dir(&conn, &state.data_dir)?
    };
    fs::create_dir_all(&output_dir).map_err(to_string)?;

    for (idx, batch) in batches.iter().enumerate() {
        let current = (idx + 1) as i64;
        if current <= resume_from {
            continue;
        }
        let completed = idx as i64;
        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(&state, &app, job, completed, &status);
        }
        let analysis_message = format!("正在分析第 {} 批", current);
        update_job(&state, &job.id, "running", completed, &analysis_message)?;
        emit_job_progress(&app, &job, "running", completed, &analysis_message);
        let chapters = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_chapters_for_batch(&conn, &novel_id, &batch.id)?
        };
        if chapters.is_empty() {
            continue;
        }
        if let Err(error) =
            analyze_chapters_for_auto(&state, &novel_id, &profile, &api_key, &chapters).await
        {
            if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
                return finish_stopped_auto_run(&state, &app, job, completed, &error);
            }
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = get_job(job.id.clone(), state)?;
            return Ok(job);
        }

        if let Some(status) = requested_auto_run_stop(&state, &novel_id)? {
            return finish_stopped_auto_run(&state, &app, job, completed, &status);
        }
        let rewrite_message = format!("正在改写第 {} 批", current);
        update_job(&state, &job.id, "running", completed, &rewrite_message)?;
        emit_job_progress(&app, &job, "running", completed, &rewrite_message);
        if let Err(error) =
            rewrite_chapters_for_auto(&state, &novel_id, &profile, &api_key, &batch.id).await
        {
            if error == AUTO_RUN_PAUSED || error == AUTO_RUN_TERMINATED {
                return finish_stopped_auto_run(&state, &app, job, completed, &error);
            }
            update_job(&state, &job.id, "failed", completed, &error)?;
            emit_job_progress(&app, &job, "failed", completed, &error);
            clear_auto_run(&state, &novel_id)?;
            job = get_job(job.id.clone(), state)?;
            return Ok(job);
        }

        let rewritten_batch = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_chapters_for_batch(&conn, &novel_id, &batch.id)?
        };
        let body = build_rewritten_export_body(&rewritten_batch)?;
        let batch_path = output_dir.join(format!(
            "{}_{}.txt",
            sanitize_file_name(&novel.title),
            chinese_batch_label(batch.batch_index)
        ));
        fs::write(&batch_path, body).map_err(to_string)?;
        let exported_message = format!("已输出第 {} 批：{}", current, batch_path.to_string_lossy());
        update_job(&state, &job.id, "running", current, &exported_message)?;
        set_auto_run_completed(&state, &novel_id, current)?;
        emit_job_progress(&app, &job, "running", current, &exported_message);
    }

    let all_chapters = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_chapters(&conn, &novel_id)?
    };
    let full_body = build_rewritten_export_body(&all_chapters)?;
    let full_path = output_dir.join(format!("{}_全文.txt", sanitize_file_name(&novel.title)));
    fs::write(&full_path, full_body).map_err(to_string)?;

    update_job(
        &state,
        &job.id,
        "completed",
        batches.len() as i64,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    )?;
    emit_job_progress(
        &app,
        &job,
        "completed",
        batches.len() as i64,
        &format!("一键分析改写完成，已输出：{}", full_path.to_string_lossy()),
    );
    clear_auto_run(&state, &novel_id)?;
    get_job(job.id, state)
}

#[tauri::command]
fn pause_analyze_rewrite_all(novel_id: String, state: State<AppState>) -> Result<Job, String> {
    request_auto_run_stop(&state, &novel_id, "pause_requested")
}

#[tauri::command]
fn terminate_analyze_rewrite_all(novel_id: String, state: State<AppState>) -> Result<Job, String> {
    request_auto_run_stop(&state, &novel_id, "terminate_requested")
}

async fn analyze_chapters_for_auto(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
) -> Result<(), String> {
    for chapter in chapters {
        set_chapter_status(state, &chapter.id, "analysis_status", "running")?;
    }

    let rewrite_parallelism = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_rewrite_parallelism(&conn)?
    };
    let parsed_analysis = analyze_batch_with_parallelism(
        state,
        novel_id,
        profile,
        api_key,
        chapters,
        rewrite_parallelism,
    )
    .await
    .inspect_err(|error| {
        if error != AUTO_RUN_PAUSED && error != AUTO_RUN_TERMINATED {
            let _ = mark_chapters_analysis_failed(state, chapters);
        }
    })?;

    save_parsed_analyses(state, novel_id, chapters, parsed_analysis)?;
    ensure_name_mapping_asset_if_settings_available(state, novel_id, profile, api_key).await?;
    Ok(())
}

async fn analyze_batch_with_parallelism(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    rewrite_parallelism: usize,
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let shards = split_chapters_for_parallelism(chapters, rewrite_parallelism);
    let shard_total = shards.len();
    let batch_label = format_batch_label(chapters);
    let mut tasks = tokio::task::JoinSet::new();

    for (idx, shard) in shards.into_iter().enumerate() {
        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let context =
            format_shard_context(idx, shard_total, rewrite_parallelism, &batch_label, &shard);
        let prompt = build_batch_analysis_prompt_with_context(&shard, &context);
        let client = state.client.clone();
        let profile_for_task = profile.clone();
        let api_key = api_key.to_string();
        let shard_for_task = shard.clone();
        tasks.spawn(async move {
            let output = generate_text(
                &client,
                &profile_for_task,
                &api_key,
                "你是严谨的中文长篇小说结构分析助手。必须输出合法 JSON，不要输出 Markdown。",
                &prompt,
                true,
            )
            .await;
            (idx, shard_label, context, shard_for_task, output)
        });
    }

    let mut parsed_by_shard = Vec::new();
    while let Some(result) = next_auto_join(&mut tasks, state, novel_id).await? {
        let (idx, shard_label, context, shard, output) = result;
        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次分析",
                    Some(&shard_label),
                    "success",
                    &format_model_log_content(&output, profile, None),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let parsed = match parse_batch_analysis_output(&output.text, &shard) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次分析解析",
                            Some(&shard_label),
                            "error",
                            &error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;
                        match retry_analysis_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &shard,
                            &context,
                            &shard_label,
                            &error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(parsed) => parsed,
                            Err(retry_error) => {
                                return Err(format!("{}：{}", shard_label, retry_error));
                            }
                        }
                    }
                };
                parsed_by_shard.push((idx, parsed));
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次分析",
                    Some(&shard_label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("{}：{}", shard_label, error));
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn retry_analysis_shard_after_parse_error(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    shard_context: &str,
    shard_label: &str,
    parse_error: &str,
    bad_output: &str,
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let retry_context = format!(
        "{}\n\n修复重试：上一次分析输出无法解析，错误：{}。请重新分析当前分片，只输出当前分片级一致性资产 JSON 对象。不要输出 Markdown、解释、空内容或 chapters 数组；JSON 字符串内换行必须写成 \\n。",
        shard_context.trim(),
        parse_error
    );
    let base_prompt = build_batch_analysis_prompt_with_context(shard, retry_context.trim());
    let prompt = format!(
        "{}\n\n上一次无法解析的输出如下，仅供你避开格式错误，不要照抄：\n{}",
        base_prompt,
        truncate_text(bad_output, 12_000)
    );
    let output = generate_text(
        &state.client,
        profile,
        api_key,
        "你是严谨的中文长篇小说结构分析格式修复助手。必须只输出一个合法 JSON 对象，不要输出 Markdown、解释或空内容。",
        &prompt,
        true,
    )
    .await;

    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次分析重试",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, None),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_batch_analysis_output(&output.text, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次分析重试解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    Err(format!(
                        "分析输出解析失败后已自动重试，但重试输出仍无法解析：{}",
                        error
                    ))
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次分析重试",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("分析输出解析失败后自动重试也失败：{}", error))
        }
    }
}

fn save_parsed_analyses(
    state: &State<'_, AppState>,
    novel_id: &str,
    chapters: &[Chapter],
    analyses: Vec<ParsedChapterAnalysis>,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    for chapter in chapters {
        tx.execute(
            "UPDATE chapters SET analysis_json = NULL, analysis_status = 'completed' WHERE id = ?1",
            params![chapter.id],
        )
        .map_err(to_string)?;
    }
    for analysis in analyses {
        tx.execute(
            "UPDATE chapters SET analysis_json = ?1 WHERE id = ?2",
            params![analysis.json, analysis.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    merge_analysis_into_canon_assets(&conn, novel_id).map_err(to_string)?;
    Ok(())
}

async fn ensure_name_mapping_asset(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    settings: &NovelSettings,
) -> Result<(), String> {
    let existing_content = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_asset_content(&conn, novel_id, "姓名映射表")?
    };
    let mut mappings = parse_name_mapping_entries(existing_content.as_deref().unwrap_or(""));
    let required_names = required_feminized_name_sources(settings);
    if required_names.is_empty() {
        return Ok(());
    }

    if !settings.rewritten_protagonist_name.trim().is_empty() {
        upsert_name_mapping_entry(
            &mut mappings,
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
        );
    }

    let missing_sources = required_names
        .iter()
        .filter(|source| {
            !mappings
                .iter()
                .any(|entry| entry.source == **source && !entry.target.trim().is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();

    if !missing_sources.is_empty() {
        match generate_name_mapping_entries(
            state,
            novel_id,
            profile,
            api_key,
            settings,
            &missing_sources,
        )
        .await
        {
            Ok(generated) => {
                for entry in generated {
                    upsert_name_mapping_entry(&mut mappings, &entry.source, &entry.target);
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "姓名映射生成",
                    Some("姓名映射表"),
                    "error",
                    &format!("AI 姓名映射生成失败，已使用本地兜底规则：{}", error),
                    None,
                    None,
                )?;
            }
        }
    }

    for source in required_names {
        if !mappings
            .iter()
            .any(|entry| entry.source == source && !entry.target.trim().is_empty())
        {
            let target = fallback_feminized_name(&source);
            upsert_name_mapping_entry(&mut mappings, &source, &target);
        }
    }

    let content = build_name_mapping_asset_content(settings, mappings)?;
    let conn = state.conn.lock().map_err(to_string)?;
    upsert_canon_asset(
        &conn,
        novel_id,
        "姓名映射表",
        &content,
        &Utc::now().to_rfc3339(),
    )
    .map_err(to_string)?;
    Ok(())
}

async fn ensure_name_mapping_asset_if_settings_available(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
) -> Result<bool, String> {
    let settings = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_novel_settings(&conn, novel_id)?
    };
    let Some(settings) = settings else {
        return Ok(false);
    };
    if settings.protagonist_name.trim().is_empty()
        || settings.bust.trim().is_empty()
        || settings.body_type.trim().is_empty()
        || settings.rewrite_mode.trim().is_empty()
    {
        return Ok(false);
    }
    ensure_name_mapping_asset(state, novel_id, profile, api_key, &settings).await?;
    Ok(true)
}

async fn generate_name_mapping_entries(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    settings: &NovelSettings,
    sources: &[String],
) -> Result<Vec<NameMappingEntry>, String> {
    let prompt = build_name_mapping_prompt(settings, sources);
    let output = generate_text(
        &state.client,
        profile,
        api_key,
        "你是中文小说姓名女性化映射助手。必须只输出合法 JSON，不要输出 Markdown 或解释。",
        &prompt,
        true,
    )
    .await?;
    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "姓名映射生成",
        Some("姓名映射表"),
        "success",
        &format_model_log_content(&output, profile, None),
        output.reasoning.as_deref(),
        Some(&output.raw_response),
    )?;
    parse_generated_name_mapping_entries(&output.text, sources)
}

fn build_name_mapping_prompt(settings: &NovelSettings, sources: &[String]) -> String {
    let forced = if settings.rewritten_protagonist_name.trim().is_empty() {
        "无".to_string()
    } else {
        format!(
            "{} -> {}",
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim()
        )
    };
    format!(
        r#"请为以下中文小说人物姓名生成固定的女性化姓名映射。

输出 JSON 结构必须是：
{{
  "names": [
    {{ "source": "原姓名", "target": "女性化姓名" }}
  ]
}}

要求：
1. 每个输入姓名都必须输出一条映射。
2. target 必须是中文姓名，不能为空，不能与 source 完全相同。
3. 优先保留姓氏，名字部分使用同音或近音的女性化字。
4. 若存在强制映射，必须逐字使用强制 target。
5. 只输出 JSON，不要解释、不要 Markdown。

强制映射：
{}

待生成姓名：
{}"#,
        forced,
        sources.join("\n")
    )
}

fn parse_generated_name_mapping_entries(
    output: &str,
    expected_sources: &[String],
) -> Result<Vec<NameMappingEntry>, String> {
    let value = parse_jsonish_value(output)?;
    let items = value
        .get("names")
        .or_else(|| value.get("mappings"))
        .or_else(|| value.get("name_mapping"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "姓名映射 JSON 缺少 names 数组。".to_string())?;
    let expected = expected_sources
        .iter()
        .map(|source| source.trim().to_string())
        .collect::<HashSet<_>>();
    let mut parsed = Vec::new();
    for item in items {
        let source = item
            .get("source")
            .or_else(|| item.get("original"))
            .or_else(|| item.get("from"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let target = item
            .get("target")
            .or_else(|| item.get("rewritten"))
            .or_else(|| item.get("to"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if source.is_empty() || target.is_empty() || source == target || !expected.contains(source)
        {
            continue;
        }
        parsed.push(NameMappingEntry {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
    if parsed.is_empty() {
        return Err("姓名映射 JSON 中没有可用映射。".to_string());
    }
    Ok(parsed)
}

fn required_feminized_name_sources(settings: &NovelSettings) -> Vec<String> {
    let mut names = Vec::new();
    push_unique_name(&mut names, settings.protagonist_name.trim());
    for name in settings.additional_feminize_names.lines() {
        push_unique_name(&mut names, name.trim());
    }
    names
}

fn push_unique_name(names: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !names.iter().any(|existing| existing == name) {
        names.push(name.to_string());
    }
}

fn parse_name_mapping_entries(content: &str) -> Vec<NameMappingEntry> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    let Ok(value) = parse_jsonish_value(content) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let Some(protagonist) = value
        .get("protagonist")
        .and_then(serde_json::Value::as_object)
    {
        let source = protagonist
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let target = protagonist
            .get("target")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if !source.is_empty() && !target.is_empty() {
            entries.push(NameMappingEntry {
                source: source.to_string(),
                target: target.to_string(),
            });
        }
    }
    if let Some(items) = value.get("names").and_then(serde_json::Value::as_array) {
        for item in items {
            let source = item
                .get("source")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            let target = item
                .get("target")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            if !source.is_empty() && !target.is_empty() {
                upsert_name_mapping_entry(&mut entries, source, target);
            }
        }
    }
    entries
}

fn build_name_mapping_asset_content(
    settings: &NovelSettings,
    mut mappings: Vec<NameMappingEntry>,
) -> Result<String, String> {
    if !settings.rewritten_protagonist_name.trim().is_empty() {
        upsert_name_mapping_entry(
            &mut mappings,
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
        );
    }
    let protagonist = mappings
        .iter()
        .find(|entry| entry.source == settings.protagonist_name.trim())
        .cloned();
    mappings.sort_by(|left, right| left.source.cmp(&right.source));
    mappings.dedup_by(|left, right| left.source == right.source);
    let asset = NameMappingAsset {
        version: 1,
        protagonist,
        names: mappings,
    };
    serde_json::to_string_pretty(&asset).map_err(to_string)
}

fn upsert_name_mapping_entry(entries: &mut Vec<NameMappingEntry>, source: &str, target: &str) {
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return;
    }
    if let Some(entry) = entries.iter_mut().find(|entry| entry.source == source) {
        entry.target = target.to_string();
    } else {
        entries.push(NameMappingEntry {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
}

fn fallback_feminized_name(source: &str) -> String {
    let mut chars = source.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return "妍".to_string();
    }
    if chars.len() == 1 {
        return feminized_char(chars[0]).unwrap_or('妍').to_string();
    }
    let mut changed = false;
    for ch in chars.iter_mut().skip(1) {
        if let Some(next) = feminized_char(*ch) {
            *ch = next;
            changed = true;
        }
    }
    if !changed || chars.iter().collect::<String>() == source {
        if let Some(last) = chars.last_mut() {
            *last = '妍';
        }
    }
    chars.iter().collect()
}

fn feminized_char(ch: char) -> Option<char> {
    match ch {
        '炎' | '岩' | '言' | '焱' | '彦' => Some('妍'),
        '旺' | '望' | '王' => Some('婉'),
        '磊' | '雷' => Some('蕾'),
        '强' => Some('蔷'),
        '刚' | '钢' => Some('婉'),
        '伟' | '威' => Some('薇'),
        '勇' => Some('咏'),
        '龙' => Some('珑'),
        '虎' => Some('琥'),
        '峰' | '锋' => Some('枫'),
        '阳' => Some('漾'),
        '明' => Some('茗'),
        '杰' => Some('洁'),
        '豪' | '昊' => Some('皓'),
        '宇' => Some('羽'),
        '轩' => Some('萱'),
        '飞' => Some('霏'),
        '凡' => Some('樊'),
        '尘' => Some('晨'),
        '三' => Some('姗'),
        _ => None,
    }
}

async fn rewrite_chapters_for_auto(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    batch_id: &str,
) -> Result<(), String> {
    let (chapters, settings, core_prompt, review_enabled, review_profile_id, rewrite_parallelism) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, novel_id)?;
        let chapters = load_chapters_for_batch(&conn, novel_id, batch_id)?
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
        return Err("当前批次没有已完成分析的内容。".to_string());
    }

    let (review_profile, review_api_key) =
        load_review_profile_for_run(state, profile, review_enabled, review_profile_id.as_deref())?;
    ensure_name_mapping_asset(state, novel_id, profile, api_key, &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, novel_id)?
    };
    let canon_text = build_compact_canon_text(&canon_assets);
    for chapter in &chapters {
        set_chapter_status(state, &chapter.id, "rewrite_status", "running")?;
    }

    let final_rewrite = rewrite_batch_with_parallelism(
        state,
        novel_id,
        profile,
        api_key,
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
    .inspect_err(|error| {
        if error != AUTO_RUN_PAUSED && error != AUTO_RUN_TERMINATED {
            let _ = mark_chapters_rewrite_failed(state, &chapters);
        }
    })?;

    save_parsed_rewrites(state, final_rewrite)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn rewrite_batch_with_parallelism(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    review_enabled: bool,
    review_profile: Option<&ModelProfile>,
    review_api_key: Option<&str>,
    rewrite_parallelism: usize,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let parsed_rewrite = generate_rewrite_shards(
        state,
        novel_id,
        profile,
        api_key,
        chapters,
        canon_text,
        settings,
        core_prompt,
        review_enabled,
        rewrite_parallelism,
    )
    .await?;

    if !review_enabled {
        return Ok(parsed_rewrite);
    }

    generate_review_shards(
        state,
        novel_id,
        profile,
        api_key,
        review_profile.unwrap_or(profile),
        review_api_key.unwrap_or(api_key),
        chapters,
        &parsed_rewrite,
        canon_text,
        settings,
        core_prompt,
        rewrite_parallelism,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn generate_rewrite_shards(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    review_enabled: bool,
    rewrite_parallelism: usize,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let shards = split_chapters_for_parallelism(chapters, rewrite_parallelism);
    let shard_total = shards.len();
    let batch_label = format_batch_label(chapters);
    let mut tasks = tokio::task::JoinSet::new();

    for (idx, shard) in shards.into_iter().enumerate() {
        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let context =
            format_shard_context(idx, shard_total, rewrite_parallelism, &batch_label, &shard);
        let prompt = build_batch_rewrite_prompt_with_context(
            &shard,
            canon_text,
            settings,
            core_prompt,
            &context,
        );
        let client = state.client.clone();
        let profile_for_task = profile.clone();
        let api_key = api_key.to_string();
        let shard_for_task = shard.clone();
        tasks.spawn(async move {
            let output = generate_text(
                &client,
                &profile_for_task,
                &api_key,
                "你是中文小说改写助手，任务是把男女性别叙事自然改写为双女主百合文本。必须逐字保留输入中的章节边界标记，只输出当前输入章节的边界标记、标题和正文，不要输出输入外章节。",
                &prompt,
                false,
            )
            .await;
            (idx, shard_label, context, shard_for_task, output)
        });
    }

    let mut parsed_by_shard = Vec::new();
    while let Some(result) = next_auto_join(&mut tasks, state, novel_id).await? {
        let (idx, shard_label, context, shard, output) = result;
        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写",
                    Some(&shard_label),
                    "success",
                    &format_model_log_content(&output, profile, Some(review_enabled)),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let parsed = match parse_batch_rewrite_output(&output.text, &shard) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次改写解析",
                            Some(&shard_label),
                            "error",
                            &error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;
                        match retry_rewrite_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &shard,
                            canon_text,
                            settings,
                            core_prompt,
                            &context,
                            &shard_label,
                            review_enabled,
                            &error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(parsed) => parsed,
                            Err(retry_error) => {
                                match recover_rewrite_shard_by_subdivision(
                                    state,
                                    novel_id,
                                    profile,
                                    api_key,
                                    &shard,
                                    canon_text,
                                    settings,
                                    core_prompt,
                                    &shard_label,
                                    review_enabled,
                                    &retry_error,
                                )
                                .await
                                {
                                    Ok(parsed) => parsed,
                                    Err(recovery_error) => {
                                        return Err(format!("{}：{}", shard_label, recovery_error));
                                    }
                                }
                            }
                        }
                    }
                };
                parsed_by_shard.push((idx, parsed));
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写",
                    Some(&shard_label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("{}：{}", shard_label, error));
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn recover_rewrite_shard_by_subdivision(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_label: &str,
    review_enabled: bool,
    original_error: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let Some((left, right)) = split_chapters_for_rewrite_recovery(shard) else {
        return Err(original_error.to_string());
    };

    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "批次改写自动细分",
        Some(shard_label),
        "running",
        &format!(
            "原分片解析重试后仍失败，开始自动细分为更小分片重写。原错误：{}",
            original_error
        ),
        None,
        None,
    )?;

    let mut pending = std::collections::VecDeque::from([
        (format!("{} · 自动细分 1", shard_label), left),
        (format!("{} · 自动细分 2", shard_label), right),
    ]);
    let mut parsed = Vec::new();

    while let Some((label, subshard)) = pending.pop_front() {
        if let Some(status) = requested_auto_run_stop(state, novel_id)? {
            return Err(status);
        }

        let batch_label = format_batch_label(&subshard);
        let context = format!(
            "{}\n\n自动细分重试：较大的改写分片无法稳定解析，当前只处理这个更小分片。必须完整输出当前分片内的全部章节，不要输出原大分片中的其他章节。",
            format_shard_context(0, 1, 1, &batch_label, &subshard)
        );
        let prompt = build_batch_rewrite_prompt_with_context(
            &subshard,
            canon_text,
            settings,
            core_prompt,
            &context,
        );
        let output = generate_text(
            &state.client,
            profile,
            api_key,
            "你是中文小说改写助手。当前是失败分片的自动细分重写，只输出当前输入章节的边界标记、标题和正文；每章必须保留原样章节开始和结束标记。",
            &prompt,
            false,
        )
        .await;

        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写自动细分",
                    Some(&label),
                    "success",
                    &format_model_log_content(&output, profile, Some(review_enabled)),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;

                match parse_batch_rewrite_output(&output.text, &subshard) {
                    Ok(mut subparsed) => parsed.append(&mut subparsed),
                    Err(parse_error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次改写自动细分解析",
                            Some(&label),
                            "error",
                            &parse_error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;

                        match retry_rewrite_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &subshard,
                            canon_text,
                            settings,
                            core_prompt,
                            &context,
                            &label,
                            review_enabled,
                            &parse_error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(mut retried) => parsed.append(&mut retried),
                            Err(retry_error) => {
                                if let Some((left, right)) =
                                    split_chapters_for_rewrite_recovery(&subshard)
                                {
                                    pending.push_front((format!("{} · 继续细分 2", label), right));
                                    pending.push_front((format!("{} · 继续细分 1", label), left));
                                } else {
                                    return Err(format!(
                                        "自动细分到单章后仍无法解析：{}",
                                        retry_error
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写自动细分",
                    Some(&label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("自动细分改写调用失败：{}", error));
            }
        }
    }

    parsed.sort_by_key(|rewrite| rewrite.index);
    if parsed.len() == shard.len() {
        Ok(parsed)
    } else {
        Err(format!(
            "自动细分后章节数量不匹配：期望 {} 章，得到 {} 章。",
            shard.len(),
            parsed.len()
        ))
    }
}

fn split_chapters_for_rewrite_recovery(
    chapters: &[Chapter],
) -> Option<(Vec<Chapter>, Vec<Chapter>)> {
    if chapters.len() <= 1 {
        return None;
    }
    let mid = chapters.len().div_ceil(2);
    Some((chapters[..mid].to_vec(), chapters[mid..].to_vec()))
}

#[allow(clippy::too_many_arguments)]
async fn generate_review_shards(
    state: &State<'_, AppState>,
    novel_id: &str,
    rewrite_profile: &ModelProfile,
    rewrite_api_key: &str,
    review_profile: &ModelProfile,
    review_api_key: &str,
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    rewrite_parallelism: usize,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let chapter_shards = split_chapters_for_parallelism(chapters, rewrite_parallelism);
    let shard_total = chapter_shards.len();
    let batch_label = format_batch_label(chapters);
    let mut tasks = tokio::task::JoinSet::new();
    let mut rewrite_offset = 0usize;

    for (idx, shard) in chapter_shards.into_iter().enumerate() {
        let count = shard.len();
        let rewrite_shard = rewrites
            .get(rewrite_offset..rewrite_offset + count)
            .ok_or_else(|| "复检分片与改写结果数量不匹配。".to_string())?
            .to_vec();
        rewrite_offset += count;

        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let context =
            format_shard_context(idx, shard_total, rewrite_parallelism, &batch_label, &shard);
        let client = state.client.clone();
        let rewrite_profile_for_task = rewrite_profile.clone();
        let review_profile_for_task = review_profile.clone();
        let rewrite_api_key = rewrite_api_key.to_string();
        let review_api_key = review_api_key.to_string();
        let shard_for_task = shard.clone();
        let rewrite_shard_for_task = rewrite_shard.clone();
        let canon_text = canon_text.to_string();
        let settings = settings.clone();
        let core_prompt = core_prompt.to_string();
        tasks.spawn(async move {
            let prompt = build_batch_review_decision_prompt_with_context(
                &shard_for_task,
                &rewrite_shard_for_task,
                &settings,
                &core_prompt,
                &context,
            );
            let output = generate_text(
                &client,
                &review_profile_for_task,
                &review_api_key,
                "你是中文小说改写审查专家。只负责判断改写稿是否合格，并列出必须打回的问题；不要直接改写正文。必须输出合法 JSON。",
                &prompt,
                true,
            )
            .await;
            (
                idx,
                shard_label,
                context,
                shard_for_task,
                rewrite_shard_for_task,
                canon_text,
                settings,
                rewrite_profile_for_task,
                rewrite_api_key,
                review_profile_for_task,
                review_api_key,
                core_prompt,
                output,
            )
        });
    }

    let mut parsed_by_shard = Vec::new();
    while let Some(result) = next_auto_join(&mut tasks, state, novel_id).await? {
        let (
            idx,
            shard_label,
            context,
            shard,
            rewrite_shard,
            canon_text,
            settings,
            rewrite_profile,
            rewrite_api_key,
            review_profile,
            review_api_key,
            core_prompt,
            output,
        ) = result;
        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &review_profile.id,
                    "批次审查决策",
                    Some(&shard_label),
                    "success",
                    &format_model_log_content(&output, &review_profile, Some(true)),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let decision = match parse_review_decision_output(&output.text) {
                    Ok(decision) => decision,
                    Err(error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &review_profile.id,
                            "批次审查决策解析",
                            Some(&shard_label),
                            "error",
                            &error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;
                        return Err(format!("{}：审查决策无法解析：{}", shard_label, error));
                    }
                };
                if decision.approved {
                    parsed_by_shard.push((idx, rewrite_shard));
                    continue;
                }

                append_ai_log(
                    state,
                    Some(novel_id),
                    &review_profile.id,
                    "批次审查打回",
                    Some(&shard_label),
                    "warning",
                    &decision.issues.join("\n"),
                    None,
                    None,
                )?;
                let revised = revise_rewrite_shard_after_review(
                    state,
                    novel_id,
                    &rewrite_profile,
                    &rewrite_api_key,
                    &shard,
                    &rewrite_shard,
                    &canon_text,
                    &settings,
                    &core_prompt,
                    &context,
                    &shard_label,
                    &decision,
                )
                .await?;
                let final_decision = review_revised_shard(
                    state,
                    novel_id,
                    &review_profile,
                    &review_api_key,
                    &shard,
                    &revised,
                    &settings,
                    &core_prompt,
                    &context,
                    &shard_label,
                )
                .await?;
                if final_decision.approved {
                    parsed_by_shard.push((idx, revised));
                } else {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &review_profile.id,
                        "批次审查二次打回",
                        Some(&shard_label),
                        "warning",
                        &final_decision.issues.join("\n"),
                        None,
                        None,
                    )?;
                    let second_revised = revise_rewrite_shard_after_review(
                        state,
                        novel_id,
                        &rewrite_profile,
                        &rewrite_api_key,
                        &shard,
                        &revised,
                        &canon_text,
                        &settings,
                        &core_prompt,
                        &context,
                        &shard_label,
                        &final_decision,
                    )
                    .await?;
                    let third_decision = review_revised_shard(
                        state,
                        novel_id,
                        &review_profile,
                        &review_api_key,
                        &shard,
                        &second_revised,
                        &settings,
                        &core_prompt,
                        &context,
                        &format!("{} · 第三次审查", shard_label),
                    )
                    .await?;
                    if third_decision.approved {
                        parsed_by_shard.push((idx, second_revised));
                    } else {
                        let warning_log_result = append_review_warning_file(
                            state,
                            novel_id,
                            &shard_label,
                            &final_decision,
                            &third_decision,
                        );
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &review_profile.id,
                            "批次审查三次未通过",
                            Some(&shard_label),
                            "warning",
                            &format!(
                                "第三次审查仍未通过，已保存第二次重写稿并继续处理后续分片。\n警告日志：{}\n\n第二次审查问题：\n{}\n\n第三次审查问题：\n{}",
                                warning_log_result,
                                final_decision.issues.join("\n"),
                                third_decision.issues.join("\n")
                            ),
                            None,
                            None,
                        )?;
                        parsed_by_shard.push((idx, second_revised));
                    }
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &review_profile.id,
                    "批次审查决策",
                    Some(&shard_label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("{}：{}", shard_label, error));
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

fn build_batch_review_decision_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请以“审查专家”身份判断改写稿是否合格。你只做判定和列问题，不直接改写正文。

{}

审查目标：
1. 改写稿是否保持原文事件顺序、因果、战力、伏笔、人物动机和章节内逻辑。
2. 主角是否已按设置完成女性化，标题和正文都不能残留主角男性姓名、男性身份、男性代词、男性称谓、男性身体特征或男性社会角色。
3. 用户指定要女性化的其他姓名是否在出现处完成女性化，并保持前后一致。
4. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲、旁观者必须保持原文性别、身份、称谓和人称代词，不能因为百合改写目标被误改。
5. 身材、体型、外貌、发色、瞳色、年龄感、能力状态、标志性服饰和伤痕是否前后一致。
6. 百合向关系推进是否承接前文，不能突然重置、跳跃或破坏原文人物性格。
7. 改写是否自然合理，不能只机械替换姓名/代词，也不能为了强调女性化而破坏剧情逻辑。
8. 改写稿是否遵守最高优先级核心设定中的文风、描写方式、节奏、语气和其他全局写作要求。
9. 章节边界、标题、正文是否完整，没有缺句、重复、串章、空正文或额外章节。

判定规则：
- 只有不存在必须修改的问题时，approved 才能为 true。
- 如果存在任何主角男性残留、指定角色未女性化、未指定角色被误改性别、逻辑断裂、章节边界错误、正文缺失或明显不自然，approved 必须为 false。
- 问题必须具体到章节、角色、原文逻辑或需要修改的称谓/特征，方便写作专家重写。

输出要求：
只输出合法 JSON，不要 Markdown，不要解释。格式：
{{
  "approved": false,
  "summary": "一句话总体判断",
  "issues": [
    {{
      "chapter_index": 1,
      "severity": "blocking",
      "problem": "具体问题",
      "required_fix": "必须如何修改"
    }}
  ]
}}

如果完全合格：
{{
  "approved": true,
  "summary": "合格",
  "issues": []
}}

{}

并发分片上下文：
{}

原文章节：
{}

待审查改写稿：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_settings_prompt(settings),
        shard_context,
        build_batch_chapter_text(chapters, false),
        build_batch_rewrite_text(chapters, rewrites)
    )
}

fn build_batch_revision_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    decision: &ReviewDecision,
) -> String {
    let issue_text = if decision.issues.is_empty() {
        "审查专家未给出具体问题，但判定不通过；请全面复查主角女性化、指定角色女性化、未指定角色性别保持、称谓、逻辑和章节边界。".to_string()
    } else {
        decision
            .issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {}", idx + 1, issue))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let base_prompt = build_batch_rewrite_prompt_with_context(
        chapters,
        canon_text,
        settings,
        core_prompt,
        shard_context,
    );
    format!(
        r#"{}

审查专家已打回上一版改写稿。请你作为原改写专家，根据下面的问题清单重新输出当前分片的完整改写结果。

必须遵守：
1. 不要只局部补丁，必须重新输出当前分片所有章节的完整标题和正文。
2. 保留原章节顺序和所有 `<<<YURI_REWRITE_CHAPTER_START ...>>>` / `<<<YURI_REWRITE_CHAPTER_END ...>>>` marker，marker 的 index 和 id 必须逐字复制。
3. 逐条修复审查问题，同时继续遵守姓名映射、女性化要求、未指定角色性别保持、外貌一致性和原文逻辑。
4. 只输出当前分片章节，不要解释、不要 Markdown、不要输出审查意见。

审查打回问题：
{}

上一版改写稿：
{}"#,
        base_prompt,
        issue_text,
        build_batch_rewrite_text(chapters, rewrites)
    )
}

fn parse_review_decision_output(output: &str) -> Result<ReviewDecision, String> {
    let value = parse_jsonish_value(output)?;
    let approved = value
        .get("approved")
        .or_else(|| value.get("pass"))
        .or_else(|| value.get("passed"))
        .and_then(|value| {
            value.as_bool().or_else(|| {
                value.as_str().map(|text| {
                    matches!(
                        text.trim().to_ascii_lowercase().as_str(),
                        "true" | "yes" | "pass" | "passed" | "approved" | "ok"
                    )
                })
            })
        });
    let mut issues = Vec::new();
    if let Some(summary) = value.get("summary").and_then(serde_json::Value::as_str) {
        if !summary.trim().is_empty() && !matches!(summary.trim(), "合格" | "通过") {
            issues.push(format!("总体判断：{}", summary.trim()));
        }
    }
    if let Some(array) = value.get("issues").and_then(serde_json::Value::as_array) {
        for item in array {
            if let Some(text) = item.as_str() {
                if !text.trim().is_empty() {
                    issues.push(text.trim().to_string());
                }
            } else if item.is_object() {
                let chapter = item
                    .get("chapter_index")
                    .and_then(serde_json::Value::as_i64)
                    .map(|value| format!("第{}章", value))
                    .unwrap_or_else(|| "未指定章节".to_string());
                let problem = item
                    .get("problem")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let required_fix = item
                    .get("required_fix")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let severity = item
                    .get("severity")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("blocking");
                let text = format!(
                    "{} [{}] {} {}",
                    chapter,
                    severity,
                    problem.trim(),
                    required_fix.trim()
                )
                .trim()
                .to_string();
                if !text.is_empty() {
                    issues.push(text);
                }
            }
        }
    }
    let approved = approved.unwrap_or(issues.is_empty());
    Ok(ReviewDecision { approved, issues })
}

#[allow(clippy::too_many_arguments)]
async fn revise_rewrite_shard_after_review(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    decision: &ReviewDecision,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let prompt = build_batch_revision_prompt_with_context(
        shard,
        rewrites,
        canon_text,
        settings,
        core_prompt,
        shard_context,
        decision,
    );
    let output = generate_text(
        &state.client,
        profile,
        api_key,
        "你是中文小说改写专家。审查专家已打回上一版改写稿，你必须按问题清单重新改写当前分片，并严格保留章节边界标记。",
        &prompt,
        false,
    )
    .await;
    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(true)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_batch_rewrite_output(&output.text, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次打回重写解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    Err(format!("审查打回后重写结果无法解析：{}", error))
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("审查打回后重写失败：{}", error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn review_revised_shard(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
) -> Result<ReviewDecision, String> {
    let prompt = build_batch_review_decision_prompt_with_context(
        shard,
        rewrites,
        settings,
        core_prompt,
        shard_context,
    );
    let output = generate_text(
        &state.client,
        profile,
        api_key,
        "你是中文小说改写终审专家。请复判打回重写后的稿件是否已经合格，只输出合法 JSON。",
        &prompt,
        true,
    )
    .await;
    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次审查复判",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(true)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            parse_review_decision_output(&output.text)
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次审查复判",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("审查复判失败：{}", error))
        }
    }
}

fn append_review_warning_file(
    state: &AppState,
    novel_id: &str,
    shard_label: &str,
    second_decision: &ReviewDecision,
    third_decision: &ReviewDecision,
) -> String {
    let novel_title = state
        .conn
        .lock()
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
                params![novel_id],
                row_to_novel,
            )
            .ok()
            .map(|novel| novel.title)
        })
        .unwrap_or_else(|| novel_id.to_string());
    append_review_warning_file_for_title(
        &state.app_dir,
        &state.data_dir,
        &novel_title,
        shard_label,
        second_decision,
        third_decision,
    )
}

fn append_review_warning_file_for_title(
    app_dir: &Path,
    data_dir: &Path,
    novel_title: &str,
    shard_label: &str,
    second_decision: &ReviewDecision,
    third_decision: &ReviewDecision,
) -> String {
    let file_name = format!("{}_审查警告.log", sanitize_file_name(novel_title));
    let content = format!(
        "\n===== {} =====\n小说：{}\n分片：{}\n结果：第三次审查仍未通过，程序已保存第二次重写稿并继续处理后续分片。\n\n第二次审查问题：\n{}\n\n第三次审查问题：\n{}\n",
        Utc::now().to_rfc3339(),
        novel_title,
        shard_label,
        format_review_issues(&second_decision.issues),
        format_review_issues(&third_decision.issues)
    );

    let root_path = app_dir.join(&file_name);
    match append_text_file(&root_path, &content) {
        Ok(()) => root_path.to_string_lossy().to_string(),
        Err(root_error) => {
            let fallback_path = data_dir.join(&file_name);
            match append_text_file(&fallback_path, &content) {
                Ok(()) => format!(
                    "{}（写入软件根目录失败，已改写入应用数据目录：{}）",
                    fallback_path.to_string_lossy(),
                    root_error
                ),
                Err(fallback_error) => format!(
                    "警告日志写入失败；软件根目录错误：{}；应用数据目录错误：{}",
                    root_error, fallback_error
                ),
            }
        }
    }
}

fn append_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(to_string)?;
    file.write_all(content.as_bytes()).map_err(to_string)
}

fn format_review_issues(issues: &[String]) -> String {
    if issues.is_empty() {
        "未提供具体问题。".to_string()
    } else {
        issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {}", idx + 1, issue))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[allow(clippy::too_many_arguments)]
async fn retry_rewrite_shard_after_parse_error(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    review_enabled: bool,
    parse_error: &str,
    bad_output: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let retry_context = format!(
        "{}\n\n修复重试：上一次改写输出无法解析，错误：{}。请完全重新输出当前分片，只输出当前分片要求的章节。每章必须包含原样章节开始标记、改写后标题、非空正文和原样章节结束标记。正文不能留空，不能输出当前分片外章节。",
        shard_context.trim(),
        parse_error
    );
    let base_prompt = build_batch_rewrite_prompt_with_context(
        shard,
        canon_text,
        settings,
        core_prompt,
        retry_context.trim(),
    );
    let prompt = format!(
        "{}\n\n上一次无法解析的输出如下，仅供你避开格式错误，不要照抄空正文或错误边界：\n{}",
        base_prompt,
        truncate_text(bad_output, 12_000)
    );
    let output = generate_text(
        &state.client,
        profile,
        api_key,
        "你是中文小说改写格式修复助手。必须重新输出当前分片的完整百合改写结果。必须逐字保留输入中的章节边界标记，只输出当前输入章节的边界标记、标题和非空正文，不要输出输入外章节。",
        &prompt,
        false,
    )
    .await;

    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写重试",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(review_enabled)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_batch_rewrite_output(&output.text, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次改写重试解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    Err(format!(
                        "解析失败后已自动重试，但重试输出仍无法解析：{}",
                        error
                    ))
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写重试",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("解析失败后自动重试也失败：{}", error))
        }
    }
}

fn split_chapters_for_parallelism(
    chapters: &[Chapter],
    rewrite_parallelism: usize,
) -> Vec<Vec<Chapter>> {
    if chapters.is_empty() {
        return Vec::new();
    }
    let parallelism = normalize_rewrite_parallelism(rewrite_parallelism).min(chapters.len());
    if parallelism <= 1 {
        return vec![chapters.to_vec()];
    }
    let chunk_size = chapters.len().div_ceil(parallelism);
    chapters
        .chunks(chunk_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn estimate_requests_for_chapters(
    chapters: &[Chapter],
    rewrite_parallelism: usize,
    review_enabled: bool,
) -> usize {
    if chapters.is_empty() {
        return 0;
    }
    let shard_count = split_chapters_for_parallelism(chapters, rewrite_parallelism).len();
    shard_count * if review_enabled { 7 } else { 2 }
}

fn estimate_wait_stages_for_chapters(chapters: &[Chapter], review_enabled: bool) -> usize {
    if chapters.is_empty() {
        0
    } else if review_enabled {
        7
    } else {
        2
    }
}

fn chapter_text_chars(chapter: &Chapter) -> usize {
    chapter.title.chars().count() + chapter.original_text.chars().count()
}

#[derive(Default)]
struct RecentModelStats {
    success_calls: usize,
    failed_calls: usize,
    total_elapsed_seconds: f64,
    elapsed_samples: usize,
    total_input_chars: usize,
    input_samples: usize,
    total_output_chars: usize,
    output_samples: usize,
}

impl RecentModelStats {
    fn average_call_seconds(&self) -> Option<f64> {
        if self.elapsed_samples == 0 {
            None
        } else {
            Some(self.total_elapsed_seconds / self.elapsed_samples as f64)
        }
    }

    fn average_input_chars(&self) -> Option<usize> {
        self.total_input_chars.checked_div(self.input_samples)
    }

    fn average_output_chars(&self) -> Option<usize> {
        self.total_output_chars.checked_div(self.output_samples)
    }
}

fn load_recent_model_stats(
    conn: &Connection,
    profile_id: &str,
) -> Result<RecentModelStats, String> {
    let mut stmt = conn
        .prepare(
            "SELECT status, content FROM ai_logs WHERE profile_id = ?1 ORDER BY created_at DESC LIMIT 80",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![profile_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    let mut stats = RecentModelStats::default();
    for (status, content) in rows {
        if status == "success" {
            stats.success_calls += 1;
            if let Some(value) = extract_usize_after_label(&content, "输入字符数：") {
                stats.total_input_chars += value;
                stats.input_samples += 1;
            }
            if let Some(value) = extract_usize_after_label(&content, "输出字符数：") {
                stats.total_output_chars += value;
                stats.output_samples += 1;
            }
            if let Some(value) = extract_f64_after_label(&content, "AI 调用耗时：") {
                stats.total_elapsed_seconds += value;
                stats.elapsed_samples += 1;
            }
        } else if status == "error" {
            stats.failed_calls += 1;
        }
    }
    Ok(stats)
}

fn extract_usize_after_label(text: &str, label: &str) -> Option<usize> {
    extract_value_after_label(text, label)?
        .parse::<usize>()
        .ok()
}

fn extract_f64_after_label(text: &str, label: &str) -> Option<f64> {
    extract_value_after_label(text, label)?.parse::<f64>().ok()
}

fn extract_value_after_label(text: &str, label: &str) -> Option<String> {
    let rest = text.split_once(label)?.1.trim_start();
    let value = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn format_shard_label(
    batch_label: &str,
    shard_index: usize,
    shard_total: usize,
    chapters: &[Chapter],
) -> String {
    if shard_total <= 1 {
        return batch_label.to_string();
    }
    match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => {
            format!(
                "{} · 分片 {}/{} · 第{}章",
                batch_label,
                shard_index + 1,
                shard_total,
                first.index
            )
        }
        (Some(first), Some(last)) => format!(
            "{} · 分片 {}/{} · 第{}-{}章",
            batch_label,
            shard_index + 1,
            shard_total,
            first.index,
            last.index
        ),
        _ => format!("{} · 分片 {}/{}", batch_label, shard_index + 1, shard_total),
    }
}

fn format_shard_context(
    shard_index: usize,
    shard_total: usize,
    rewrite_parallelism: usize,
    batch_label: &str,
    chapters: &[Chapter],
) -> String {
    if shard_total <= 1 {
        return "当前为不并发模式，本次输入就是完整选中批次。".to_string();
    }
    let chapter_list = chapters
        .iter()
        .map(|chapter| format!("第{}章", chapter.index))
        .collect::<Vec<_>>()
        .join("、");
    let chapter_range = match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => format!("第{}章", first.index),
        (Some(first), Some(last)) => format!("第{}-{}章", first.index, last.index),
        _ => "空分片".to_string(),
    };
    format!(
        "当前输入是 {} 拆分出的并发分片 {}/{}，本分片实际只包含 {}：{}。只能处理和输出这些章节，严禁输出本分片外的任何章节、标题、正文或章节边界标记。所有分片共享同一份小说设定、一致性资产、姓名女性化规则和章节边界规则。请严格遵循这些全局规则，保持姓名映射、称谓、文风、剧情承接和女性化设定一致；不要因为只看到当前分片就改变人物设定或重置关系进展。当前设置的并发请求数为 {}。",
        batch_label,
        shard_index + 1,
        shard_total,
        chapter_range,
        chapter_list,
        normalize_rewrite_parallelism(rewrite_parallelism)
    )
}

#[allow(dead_code)]
async fn start_rewrite_legacy(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (chapters, canon_assets, settings, core_prompt) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        let chapters = load_chapters_for_batch(&conn, &novel_id, &batch_id)?
            .into_iter()
            .filter(|chapter| chapter.analysis_status == "completed")
            .collect::<Vec<_>>();
        (
            chapters,
            load_canon_assets(&conn, &novel_id)?,
            settings,
            load_core_prompt(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有已完成分析的内容，请先分析该批次。".to_string());
    }
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    let canon_text = build_compact_canon_text(&canon_assets);

    for chapter in chapters {
        update_job(
            &state,
            &job.id,
            "running",
            chapter.index,
            &format!("正在改写 {}", chapter.title),
        )?;
        set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
        let prompt =
            build_rewrite_prompt_with_settings(&chapter, &canon_text, &settings, &core_prompt);
        match generate_text(
            &state.client,
            &profile,
            &api_key,
            "你是中文小说改写助手，任务是把男女主文本改写为自然的双女主百合文本。只输出改写后的标题和正文。",
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
                    "章节改写",
                    Some(&chapter.title),
                    "success",
                    &format_model_log_content(&output, &profile, None),
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
    load_job(&state, &job_id)
}

fn load_job(state: &State<'_, AppState>, job_id: &str) -> Result<Job, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
        params![job_id],
        row_to_job,
    )
    .map_err(to_string)
}

#[tauri::command]
fn export_novel(
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

#[tauri::command]
fn open_github_url() -> Result<(), String> {
    open_url_in_default_browser(GITHUB_REPOSITORY_URL)
}

#[tauri::command]
async fn check_for_updates(state: State<'_, AppState>) -> Result<UpdateCheckResult, String> {
    fetch_latest_update(&state.client).await
}

#[tauri::command]
async fn download_latest_update(
    state: State<'_, AppState>,
) -> Result<UpdateDownloadResult, String> {
    let update = fetch_latest_update(&state.client).await?;
    let response = state
        .client
        .get(&update.asset_download_url)
        .header("User-Agent", "YuriRewrite")
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(to_string)?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes);
        return Err(format!(
            "下载失败 HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let output_dir = resolve_update_download_dir(&state)?;
    fs::create_dir_all(&output_dir).map_err(to_string)?;
    let output_path = output_dir.join(sanitize_file_name(&update.asset_name));
    fs::write(&output_path, bytes).map_err(to_string)?;
    Ok(UpdateDownloadResult {
        path: output_path.to_string_lossy().to_string(),
        version: update.latest_version,
    })
}

fn build_rewritten_export_body(chapters: &[Chapter]) -> Result<String, String> {
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

fn resolve_rewrite_export_dir(conn: &Connection, data_dir: &Path) -> Result<PathBuf, String> {
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

fn chinese_batch_label(index: i64) -> String {
    format!("第{}批", chinese_number(index))
}

fn chinese_number(value: i64) -> String {
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

fn chinese_digit(value: i64) -> &'static str {
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

async fn fetch_latest_update(client: &Client) -> Result<UpdateCheckResult, String> {
    let response = client
        .get(GITHUB_LATEST_RELEASE_URL)
        .header("User-Agent", "YuriRewrite")
        .send()
        .await
        .map_err(to_string)?;
    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        let body = response.text().await.map_err(to_string)?;
        return Err(format!(
            "检查更新失败 HTTP {}: {}",
            status,
            compact_error_body(&body)
        ));
    }

    let latest_tag = release_tag_from_url(&final_url)
        .ok_or_else(|| format!("无法从 GitHub 最新发布地址解析版本：{}", final_url))?;
    let latest_version = normalize_release_version(&latest_tag);
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let release_url = format!("{}/releases/tag/{}", GITHUB_REPOSITORY_URL, latest_tag);
    let asset_name = portable_zip_name(&latest_version);
    let asset_download_url = format!(
        "{}/releases/download/{}/{}",
        GITHUB_REPOSITORY_URL, latest_tag, asset_name
    );

    Ok(UpdateCheckResult {
        current_version: current_version.clone(),
        latest_version: latest_version.clone(),
        latest_tag,
        is_latest: !is_newer_version(&latest_version, &current_version),
        release_url,
        asset_name,
        asset_download_url,
    })
}

fn release_tag_from_url(url: &str) -> Option<String> {
    let clean_url = url.split(['?', '#']).next().unwrap_or(url);
    let segments = clean_url
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect::<Vec<_>>();
    segments
        .windows(2)
        .find(|pair| pair[0] == "tag")
        .map(|pair| pair[1].to_string())
}

fn portable_zip_name(version: &str) -> String {
    format!(
        "YuriRewrite-v{}-windows-x64.zip",
        normalize_release_version(version)
    )
}

fn normalize_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let candidate_parts = version_number_parts(candidate);
    let current_parts = version_number_parts(current);
    let max_len = candidate_parts.len().max(current_parts.len()).max(1);
    for idx in 0..max_len {
        let left = *candidate_parts.get(idx).unwrap_or(&0);
        let right = *current_parts.get(idx).unwrap_or(&0);
        if left != right {
            return left > right;
        }
    }
    false
}

fn version_number_parts(version: &str) -> Vec<u64> {
    normalize_release_version(version)
        .split(['.', '-', '+'])
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

fn resolve_update_download_dir(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(path) = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'export_dir'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    Ok(default_download_dir().unwrap_or_else(|| state.data_dir.join("updates")))
}

fn default_download_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|home| home.join("Downloads"))
        .filter(|path| path.exists())
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Downloads"))
                .filter(|path| path.exists())
        })
}

fn open_url_in_default_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", url])
        .status()
        .map_err(to_string)?;

    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status().map_err(to_string)?;

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(to_string)?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("无法打开链接：{}", url))
    }
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
    let matches = chapter_heading_matches(text);
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

fn chapter_heading_matches(text: &str) -> Vec<regex::Match<'_>> {
    let heading_re = chapter_heading_regex();
    let matches = heading_re
        .find_iter(text)
        .filter(|mat| is_plausible_strict_heading_line(mat.as_str()))
        .collect::<Vec<_>>();
    if matches
        .iter()
        .any(|mat| is_numbered_strict_chapter_heading(mat.as_str()))
    {
        return matches;
    }
    let loose_heading_re = loose_numbered_chapter_heading_regex();
    let loose_matches = loose_heading_re
        .find_iter(text)
        .filter(|mat| is_plausible_loose_numbered_heading_line(mat.as_str()))
        .collect::<Vec<_>>();
    if loose_numbered_headings_are_plausible(text, &loose_matches) {
        let mut merged_matches = matches
            .into_iter()
            .filter(|mat| {
                !is_loose_container_heading(mat.as_str())
                    && !is_loose_metadata_heading(mat.as_str())
            })
            .chain(loose_matches)
            .collect::<Vec<_>>();
        merged_matches.sort_by_key(|mat| mat.start());
        merged_matches
            .into_iter()
            .fold(Vec::new(), |mut deduped, mat| {
                if deduped
                    .last()
                    .is_none_or(|last: &regex::Match<'_>| last.start() != mat.start())
                {
                    deduped.push(mat);
                }
                deduped
            })
    } else if !matches.is_empty() {
        matches
    } else {
        Vec::new()
    }
}

fn chapter_heading_regex() -> Regex {
    Regex::new(
        r#"(?m)^[\s\u{feff}　]*(?:={2,6}[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][^=\r\n]{0,80}={2,6}|={2,6}[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[^=\r\n]{0,80}={2,6}|[【〔［「『《（(\[]?[ \t　]*(?:正文[ \t　]*)?第[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　]*[章节回卷部集篇话幕节页季段册夜案场弹折更][ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:卷|篇|部|章|回|幕|册|节)[ \t　]*[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[上中下前后终外][ \t　]*(?:卷|篇|部|章|册)[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|(?:Chapter|CHAPTER|chapter|Chap\.?|CH\.?|ch\.?|Section|SECTION|section|Part|PART|part|Episode|EPISODE|episode|No\.?|NO\.?|no\.?)[ \t　]*[0-9０-９IVXLCDMivxlcdm]+[ \t　:：、.．\-—_·|]*[^\r\n]{0,80}|[【〔［「『《（(\[]?[ \t　]*(?:序章|楔子|引子|引言|序言|序幕|前言|终章|尾声|后记|番外(?:篇|章)?|特别篇|外传|插曲|间章|简介|文案|作品相关|上架感言|完本感言)[ \t　]*[】〕］」』》）)\]]?[ \t　:：、.．\-—_·|]*[^\r\n]{0,80})[\t 　]*\r?$"#,
    )
    .expect("valid chapter regex")
}

fn is_plausible_strict_heading_line(line: &str) -> bool {
    let core = line
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || ch == '\u{feff}'
                || ch == '　'
                || ch == '='
                || matches!(
                    ch,
                    '【' | '】'
                        | '〔'
                        | '〕'
                        | '［'
                        | '］'
                        | '「'
                        | '」'
                        | '『'
                        | '』'
                        | '《'
                        | '》'
                        | '（'
                        | '）'
                        | '('
                        | ')'
                        | '['
                        | ']'
                )
        })
        .trim();
    let compact = core
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if starts_with_inline_round_phrase(&compact) {
        return false;
    }
    if strict_numbered_heading_looks_like_body_sentence(&compact) {
        return false;
    }
    if [
        "章正文",
        "节正文",
        "回正文",
        "卷正文",
        "部正文",
        "集正文",
        "篇正文",
        "话正文",
        "幕正文",
        "页正文",
        "季正文",
        "段正文",
        "册正文",
        "夜正文",
        "案正文",
        "场正文",
        "弹正文",
        "折正文",
        "更正文",
    ]
    .iter()
    .any(|pattern| compact.contains(pattern))
    {
        return false;
    }
    [
        "序章正文",
        "楔子正文",
        "引子正文",
        "引言正文",
        "序言正文",
        "序幕正文",
        "前言正文",
        "终章正文",
        "尾声正文",
        "后记正文",
        "番外正文",
        "番外篇正文",
        "番外章正文",
        "特别篇正文",
        "外传正文",
        "插曲正文",
        "间章正文",
    ]
    .iter()
    .all(|pattern| !compact.starts_with(pattern))
        && !special_heading_content_looks_like_body(&compact)
}

fn starts_with_inline_round_phrase(compact: &str) -> bool {
    let round_re = Regex::new(
        r#"^(?:第)?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+回合"#,
    )
    .expect("valid round phrase regex");
    if !round_re.is_match(compact) {
        return false;
    }
    compact
        .chars()
        .any(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'))
        || compact.contains("回合的")
}

fn special_heading_content_looks_like_body(compact: &str) -> bool {
    for keyword in [
        "作品相关",
        "上架感言",
        "完本感言",
        "番外篇",
        "番外章",
        "特别篇",
        "序章",
        "楔子",
        "引子",
        "引言",
        "序言",
        "序幕",
        "前言",
        "终章",
        "尾声",
        "后记",
        "番外",
        "外传",
        "插曲",
        "间章",
        "简介",
        "文案",
    ] {
        if let Some(rest) = compact.strip_prefix(keyword) {
            let rest = rest.trim_matches(|ch| {
                matches!(
                    ch,
                    '：' | ':'
                        | '、'
                        | '-'
                        | '—'
                        | '_'
                        | '·'
                        | '|'
                        | '。'
                        | '.'
                        | '．'
                        | '！'
                        | '!'
                        | '？'
                        | '?'
                )
            });
            if rest.is_empty() {
                return false;
            }
            return [
                "写", "中", "里", "是", "的", "时", "我", "也", "就", "到", "说", "提", "已经",
                "终于", "无", "不", "没", "没有", "大家", "今天", "明天", "这", "那", "继续",
                "感谢", "谢谢", "各位", "看到",
            ]
            .iter()
            .any(|prefix| rest.starts_with(prefix));
        }
    }
    false
}

fn strict_numbered_heading_looks_like_body_sentence(compact: &str) -> bool {
    let numbered_re = Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+([章节回集话幕节页季段夜案场弹折更])(.+)$"#,
    )
    .expect("valid strict numbered heading parser regex");
    let Some(captures) = numbered_re.captures(compact) else {
        return false;
    };
    let unit = captures.get(1).map_or("", |mat| mat.as_str());
    let rest = captures
        .get(2)
        .map_or("", |mat| mat.as_str())
        .trim_matches(|ch| {
            matches!(
                ch,
                '：' | ':' | '、' | '-' | '—' | '_' | '·' | '|' | '.' | '．' | ' ' | '　'
            )
        });
    if rest.is_empty() {
        return false;
    }

    let rest_len = rest.chars().count();
    let has_sentence_punctuation = rest
        .chars()
        .any(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'));
    let prose_like_prefix = [
        "是", "的", "了", "在", "从", "到", "把", "被", "让", "和", "与", "又", "却", "就", "便",
        "都", "还", "也", "向", "将", "能", "会", "要", "问", "说", "看到", "发现", "遇到", "魔力",
    ]
    .iter()
    .any(|prefix| rest.starts_with(prefix));

    unit == "回"
        && ((has_sentence_punctuation && rest_len > 14) || (prose_like_prefix && rest_len > 18))
}

fn is_numbered_strict_chapter_heading(line: &str) -> bool {
    let compact = compact_heading_line(line);
    let numbered_re = Regex::new(
        r#"^(?:正文)?第[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[章节回集话幕节页季段夜案场弹折更]"#,
    )
    .expect("valid numbered strict chapter regex");
    numbered_re.is_match(&compact)
        || compact.starts_with("Chapter")
        || compact.starts_with("CHAPTER")
        || compact.starts_with("chapter")
        || compact.starts_with("Chap")
        || compact.starts_with("CH.")
        || compact.starts_with("ch.")
        || compact.starts_with("Section")
        || compact.starts_with("SECTION")
        || compact.starts_with("section")
        || compact.starts_with("Part")
        || compact.starts_with("PART")
        || compact.starts_with("part")
        || compact.starts_with("Episode")
        || compact.starts_with("EPISODE")
        || compact.starts_with("episode")
        || compact.starts_with("No.")
        || compact.starts_with("NO.")
        || compact.starts_with("no.")
}

fn is_loose_container_heading(line: &str) -> bool {
    let compact = compact_heading_line(line);
    let container_re = Regex::new(
        r#"^(?:第?[0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+[卷部]|[卷部][0-9０-９零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]+|[上中下前后终外][卷部])"#,
    )
    .expect("valid loose container heading regex");
    container_re.is_match(&compact)
}

fn is_loose_metadata_heading(line: &str) -> bool {
    matches!(
        compact_heading_line(line).as_str(),
        "文案" | "作品相关" | "上架感言" | "完本感言"
    )
}

fn compact_heading_line(line: &str) -> String {
    line.trim_matches(|ch: char| {
        ch.is_whitespace()
            || ch == '\u{feff}'
            || ch == '　'
            || ch == '='
            || matches!(
                ch,
                '【' | '】'
                    | '〔'
                    | '〕'
                    | '［'
                    | '］'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
                    | '《'
                    | '》'
                    | '（'
                    | '）'
                    | '('
                    | ')'
                    | '['
                    | ']'
            )
    })
    .chars()
    .filter(|ch| !ch.is_whitespace())
    .collect()
}

fn loose_numbered_chapter_heading_regex() -> Regex {
    Regex::new(
        r#"(?m)^[ \t\u{feff}　]*(?:[（(]?[ \t　]*)?(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*(?:[）)][ \t　]*)?(?:[ \t　]+|[、.．:：\-—_·|][ \t　]*)[^\r\n]{1,60}[ \t　]*\r?$"#,
    )
    .expect("valid loose numbered chapter regex")
}

fn loose_numbered_headings_are_plausible(text: &str, matches: &[regex::Match<'_>]) -> bool {
    if matches.len() < 2 {
        return false;
    }
    let ordinals = matches
        .iter()
        .filter_map(|mat| parse_loose_numbered_heading_ordinal(mat.as_str()))
        .collect::<Vec<_>>();
    if ordinals.len() != matches.len() || ordinals.first().is_none_or(|value| *value > 3) {
        return false;
    }
    if !loose_numbered_ordinals_are_plausible(&ordinals) {
        return false;
    }
    loose_numbered_heading_bodies_are_plausible(text, matches)
}

fn loose_numbered_ordinals_are_plausible(ordinals: &[u64]) -> bool {
    let Some(first) = ordinals.first().copied() else {
        return false;
    };
    if first > 3 {
        return false;
    }

    let allowed_glitches = (ordinals.len() / 20).max(2);
    let allowed_resets = (ordinals.len() / 20).max(3);
    let mut glitches = 0usize;
    let mut resets = 0usize;
    let mut expected = first;
    for ordinal in ordinals {
        if *ordinal == expected {
            expected += 1;
        } else if *ordinal <= 3 && expected > 10 {
            resets += 1;
            if resets > allowed_resets {
                return false;
            }
            expected = *ordinal + 1;
        } else if *ordinal < expected {
            glitches += 1;
            if glitches > allowed_glitches {
                return false;
            }
        } else if *ordinal == expected + 1 {
            glitches += 1;
            if glitches > allowed_glitches {
                return false;
            }
            expected = *ordinal + 1;
        } else {
            return false;
        }
    }
    true
}

fn is_plausible_loose_numbered_heading_line(line: &str) -> bool {
    let Some(title) = loose_numbered_heading_title(line) else {
        return false;
    };
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 40 {
        return false;
    }
    if loose_numbered_heading_looks_like_date_marker(line, title) {
        return false;
    }
    if loose_numbered_title_looks_like_body_sentence(line, title) {
        return false;
    }
    if ["列表", "列表项", "选项", "步骤", "序号"]
        .iter()
        .any(|word| title.contains(word))
    {
        return false;
    }
    let meaningful = title
        .chars()
        .filter(|ch| ch.is_alphanumeric() || is_cjk_char(*ch))
        .count();
    if meaningful < 2 {
        return false;
    }
    let symbol_count = title
        .chars()
        .filter(|ch| {
            !ch.is_alphanumeric()
                && !is_cjk_char(*ch)
                && !ch.is_whitespace()
                && *ch != '\u{feff}'
                && *ch != '　'
        })
        .count();
    symbol_count * 2 <= title.chars().count()
}

fn loose_numbered_title_looks_like_body_sentence(line: &str, title: &str) -> bool {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let starts_with_chinese_ordinal = trimmed.chars().next().is_some_and(|ch| {
        "零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O".contains(ch)
    });
    if starts_with_chinese_ordinal
        && title
            .chars()
            .next()
            .is_some_and(|ch| "零〇一二两三四五六七八九十百千万".contains(ch))
        && title.contains('岁')
    {
        return true;
    }

    let punctuation_count = title
        .chars()
        .filter(|ch| matches!(ch, '，' | '。' | '！' | '？' | '；' | ';'))
        .count();
    punctuation_count >= 2 && title.chars().count() > 24
}

fn loose_numbered_heading_looks_like_date_marker(line: &str, title: &str) -> bool {
    let Some(ordinal) = parse_loose_numbered_heading_ordinal(line) else {
        return false;
    };
    if !(1900..=2099).contains(&ordinal) {
        return false;
    }

    let title = title.trim();
    let date_tail_re =
        Regex::new(r#"^[0-9０-９]{1,2}(?:[.．/\-—年月日]|$)"#).expect("valid date tail regex");
    date_tail_re.is_match(title)
}

fn loose_numbered_heading_bodies_are_plausible(text: &str, matches: &[regex::Match<'_>]) -> bool {
    let body_lengths = matches
        .iter()
        .enumerate()
        .map(|(idx, mat)| {
            let start = mat.end();
            let end = matches.get(idx + 1).map_or(text.len(), |next| next.start());
            text[start..end]
                .trim()
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .count()
        })
        .collect::<Vec<_>>();
    if body_lengths.iter().any(|len| *len < 20) {
        return false;
    }
    let total = body_lengths.iter().sum::<usize>();
    total / body_lengths.len() >= 20
}

fn loose_numbered_heading_title(line: &str) -> Option<&str> {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let ordinal_re = loose_numbered_heading_ordinal_prefix_regex();
    let mat = ordinal_re.find(trimmed)?;
    Some(trimmed[mat.end()..].trim())
}

fn loose_numbered_heading_ordinal_prefix_regex() -> Regex {
    Regex::new(
        r#"^[（(]?[ \t　]*(?:[0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})[ \t　]*[）)]?[ \t　]*(?:[、.．:：\-—_·|]|[ \t　]+)"#,
    )
    .expect("valid loose numbered heading ordinal prefix regex")
}

fn parse_loose_numbered_heading_ordinal(line: &str) -> Option<u64> {
    let trimmed =
        line.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '　');
    let ordinal_re = Regex::new(
        r#"^[（(]?[ \t　]*([0-9０-９]{1,5}|[零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟萬O]{1,12})"#,
    )
    .expect("valid loose numbered heading ordinal parser regex");
    let token = ordinal_re.captures(trimmed)?.get(1)?.as_str();
    parse_fullwidth_digits(token).or_else(|| parse_chinese_ordinal(token))
}

fn is_cjk_char(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9fff | 0xf900..=0xfaff)
}

fn parse_fullwidth_digits(token: &str) -> Option<u64> {
    let mut normalized = String::new();
    for ch in token.chars() {
        let digit = match ch {
            '0'..='9' => ch,
            '０' => '0',
            '１' => '1',
            '２' => '2',
            '３' => '3',
            '４' => '4',
            '５' => '5',
            '６' => '6',
            '７' => '7',
            '８' => '8',
            '９' => '9',
            _ => return None,
        };
        normalized.push(digit);
    }
    normalized.parse::<u64>().ok()
}

fn parse_chinese_ordinal(token: &str) -> Option<u64> {
    let mut total = 0;
    let mut section = 0;
    let mut number = 0;
    let mut seen = false;
    for ch in token.chars() {
        if let Some(value) = chinese_ordinal_digit(ch) {
            number = value;
            seen = true;
        } else if let Some(unit) = chinese_ordinal_unit(ch) {
            seen = true;
            if unit == 10_000 {
                section = (section + number.max(1)) * unit;
                total += section;
                section = 0;
            } else {
                section += number.max(1) * unit;
            }
            number = 0;
        } else {
            return None;
        }
    }
    if !seen {
        return None;
    }
    let value = total + section + number;
    (value > 0).then_some(value)
}

fn chinese_ordinal_digit(ch: char) -> Option<u64> {
    match ch {
        '零' | '〇' | 'O' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '贰' | '两' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn chinese_ordinal_unit(ch: char) -> Option<u64> {
    match ch {
        '十' | '拾' => Some(10),
        '百' | '佰' => Some(100),
        '千' | '仟' => Some(1000),
        '万' | '萬' => Some(10_000),
        _ => None,
    }
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
    for kind in ["姓名映射表", "人物卡", "人物关系", "地点", "伏笔", "术语表"] {
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
        let first = chunk.first().ok_or_else(|| "批次内容为空。".to_string())?;
        let last = chunk.last().ok_or_else(|| "批次内容为空。".to_string())?;
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
    let chapters = stmt
        .query_map(params![novel_id], row_to_chapter)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}

fn load_chapters_for_batch(
    conn: &Connection,
    novel_id: &str,
    batch_id: &str,
) -> Result<Vec<Chapter>, String> {
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
        .query_map(
            params![novel_id, batch.start_chapter, batch.end_chapter],
            row_to_chapter,
        )
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
        "SELECT novel_id, protagonist_name, rewritten_protagonist_name, additional_feminize_names, bust, body_type, rewrite_mode, advanced_settings, updated_at FROM novel_settings WHERE novel_id = ?1",
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
    let settings =
        load_novel_settings(conn, novel_id)?.ok_or_else(|| "请先填写设定".to_string())?;
    if settings.protagonist_name.trim().is_empty()
        || settings.bust.trim().is_empty()
        || settings.body_type.trim().is_empty()
        || settings.rewrite_mode.trim().is_empty()
    {
        return Err("请先填写设定".to_string());
    }
    Ok(settings)
}

fn load_canon_assets(conn: &Connection, novel_id: &str) -> Result<Vec<CanonAsset>, String> {
    let mut stmt = conn
        .prepare("SELECT novel_id, kind, content, updated_at FROM canon_assets WHERE novel_id = ?1 ORDER BY kind")
        .map_err(to_string)?;
    let assets = stmt
        .query_map(params![novel_id], |row| {
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

fn load_canon_asset_content(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
) -> Result<Option<String>, String> {
    match conn.query_row(
        "SELECT content FROM canon_assets WHERE novel_id = ?1 AND kind = ?2",
        params![novel_id, kind],
        |row| row.get::<_, String>(0),
    ) {
        Ok(content) => Ok(Some(content)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(to_string(error)),
    }
}

fn load_model_profile(
    state: &State<'_, AppState>,
    profile_id: &str,
) -> Result<ModelProfile, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, name, provider, base_url, model, temperature, thinking_mode, updated_at, api_key FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| {
            let id: String = row.get(0)?;
            let db_api_key: Option<String> = row.get(8)?;
            Ok(ModelProfile {
                has_api_key: read_api_key(&id).is_ok()
                    || db_api_key.as_deref().is_some_and(|value| !value.is_empty()),
                id,
                name: row.get(1)?,
                provider: row.get(2)?,
                base_url: row.get(3)?,
                model: row.get(4)?,
                temperature: row.get(5)?,
                thinking_mode: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .map_err(to_string)
}

fn is_mimo_profile(profile: &ModelProfile) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = profile.base_url.to_ascii_lowercase();
    let model = profile.model.to_ascii_lowercase();
    provider.contains("mimo")
        || provider.contains("xiaomi")
        || base.contains("mimo")
        || base.contains("xiaomi")
        || model.contains("mimo-")
}

fn load_review_profile_for_run(
    state: &State<'_, AppState>,
    rewrite_profile: &ModelProfile,
    review_enabled: bool,
    review_profile_id: Option<&str>,
) -> Result<(Option<ModelProfile>, Option<String>), String> {
    if !review_enabled {
        return Ok((None, None));
    }
    let profile = match normalize_review_profile_id(review_profile_id) {
        Some(profile_id) => load_model_profile(state, &profile_id)?,
        None => rewrite_profile.clone(),
    };
    let api_key = read_stored_api_key(state, &profile.id)?;
    Ok((Some(profile), Some(api_key)))
}

fn prepare_prompt_for_profile(
    profile: &ModelProfile,
    system: &str,
    user: &str,
) -> (String, String) {
    if is_mimo_profile(profile) {
        (
            sanitize_prompt_for_mimo(system),
            sanitize_prompt_for_mimo(user),
        )
    } else {
        (system.to_string(), user.to_string())
    }
}

fn sanitize_prompt_for_mimo(text: &str) -> String {
    let replacements = [
        ("身材：巨乳", "身形风格：成熟曲线"),
        ("身材：平胸", "身形风格：清瘦纤细"),
        ("体型：萝莉", "体型：娇小少女感"),
        ("巨乳", "成熟曲线"),
        ("平胸", "清瘦纤细"),
        ("萝莉", "娇小少女感"),
    ];
    let mut sanitized = text.to_string();
    for (from, to) in replacements {
        sanitized = sanitized.replace(from, to);
    }
    sanitized
}

async fn generate_text(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
    prefer_json_output: bool,
) -> Result<ModelOutput, String> {
    let started = Instant::now();
    let (system, user) = prepare_prompt_for_profile(profile, system, user);
    let input_chars = system.chars().count() + user.chars().count();
    let mut output = if profile.provider.to_lowercase().contains("gemini") {
        generate_gemini(client, profile, api_key, &system, &user).await
    } else {
        generate_openai_compatible(client, profile, api_key, &system, &user, prefer_json_output)
            .await
    }?;
    // When the model returns empty content but has reasoning (thinking / reasoning_content),
    // try to extract the trailing JSON from the reasoning as the actual output text.
    // This handles DeepSeek-family models that sometimes spend all tokens on reasoning tokens,
    // leaving the content field empty while the real structured output sits at the end of reasoning.
    if output.text.trim().is_empty() {
        if let Some(ref reasoning) = output.reasoning {
            if let Some(extracted) = extract_tailing_json_from_text(reasoning) {
                output.text = extracted.to_string();
            }
        }
    }

    output.input_chars = input_chars;
    output.output_chars = output.text.chars().count();
    output.elapsed_ms = started.elapsed().as_millis();
    Ok(output)
}

async fn generate_openai_compatible(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
    prefer_json_output: bool,
) -> Result<ModelOutput, String> {
    let base = profile.base_url.trim().trim_end_matches('/');
    let model = normalize_model_name(base, &profile.model);
    let endpoint = if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{}/chat/completions", base)
    };
    let mut payload = json!({
        "model": model,
        "temperature": profile.temperature,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    if prefer_json_output && is_deepseek_profile(profile, base, &model) {
        payload["response_format"] = json!({ "type": "json_object" });
    }
    let added_thinking_control =
        apply_openai_compatible_thinking_control(&mut payload, profile, base, &model);
    let response = client
        .post(&endpoint)
        .bearer_auth(api_key.trim())
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?;
    let mut retried_without_thinking = false;
    let (value, raw_response) = match response_json_or_error(response).await {
        Ok(result) => result,
        Err(error) if added_thinking_control => {
            let mut retry_payload = payload;
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("reasoning_effort");
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("reasoning");
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("thinking");
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("thinking_budget");
            let retry_response = client
                .post(endpoint)
                .bearer_auth(api_key.trim())
                .json(&retry_payload)
                .send()
                .await
                .map_err(to_string)?;
            let retry_result =
                response_json_or_error(retry_response)
                    .await
                    .map_err(|retry_error| {
                        format!("{}；移除思考模式参数重试后仍失败：{}", error, retry_error)
                    })?;
            retried_without_thinking = true;
            retry_result
        }
        Err(error) => return Err(error),
    };
    if let Some(error) = openai_content_filter_error(&value, &model) {
        return Err(error);
    }
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
        input_chars: 0,
        output_chars: 0,
        elapsed_ms: 0,
        retried_without_thinking,
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
    let endpoint = format!("{}/models/{}:generateContent", base, profile.model.trim());
    let mut payload = json!({
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
    let added_thinking_control = apply_gemini_thinking_control(&mut payload, profile);
    let response = client
        .post(&endpoint)
        .header("x-goog-api-key", api_key.trim())
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?;
    let mut retried_without_thinking = false;
    let (value, raw_response) = match response_json_or_error(response).await {
        Ok(result) => result,
        Err(error) if added_thinking_control => {
            let mut retry_payload = payload;
            if let Some(generation_config) = retry_payload
                .get_mut("generationConfig")
                .and_then(serde_json::Value::as_object_mut)
            {
                generation_config.remove("thinkingConfig");
            }
            let retry_response = client
                .post(endpoint)
                .header("x-goog-api-key", api_key.trim())
                .json(&retry_payload)
                .send()
                .await
                .map_err(to_string)?;
            let retry_result =
                response_json_or_error(retry_response)
                    .await
                    .map_err(|retry_error| {
                        format!("{}；移除思考模式参数重试后仍失败：{}", error, retry_error)
                    })?;
            retried_without_thinking = true;
            retry_result
        }
        Err(error) => return Err(error),
    };
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
        input_chars: 0,
        output_chars: 0,
        elapsed_ms: 0,
        retried_without_thinking,
    })
}

async fn response_json_or_error(
    response: reqwest::Response,
) -> Result<(serde_json::Value, String), String> {
    let status = response.status();
    let body = response.text().await.map_err(to_string)?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, compact_error_body(&body)));
    }
    let value = serde_json::from_str(&body)
        .map_err(|error| format!("模型响应不是合法 JSON: {}；原始响应：{}", error, body))?;
    Ok((value, body))
}

fn openai_content_filter_error(value: &serde_json::Value, model: &str) -> Option<String> {
    let choice = &value["choices"][0];
    let finish_reason = choice["finish_reason"].as_str().unwrap_or_default();
    let content = choice["message"]["content"].as_str().unwrap_or_default();
    let content_lower = content.to_ascii_lowercase();
    if finish_reason == "content_filter"
        || content_lower.contains("request was rejected")
        || content_lower.contains("considered high risk")
    {
        Some(format!(
            "模型内容安全策略拦截，未返回可解析文本。模型：{}；finish_reason：{}；返回内容：{}。可尝试降低创意模式强度、关闭复检、减少单次章节数，或更换对长篇改写更宽松的模型。",
            model,
            if finish_reason.is_empty() { "未知" } else { finish_reason },
            if content.trim().is_empty() { "空" } else { content.trim() }
        ))
    } else {
        None
    }
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

fn normalize_thinking_mode(input: Option<&str>) -> Result<String, String> {
    let mode = input.unwrap_or("auto").trim().to_ascii_lowercase();
    match mode.as_str() {
        "" | "auto" => Ok("auto".to_string()),
        "off" => Ok("off".to_string()),
        "on" => Ok("on".to_string()),
        _ => Err("思考模式只能是 auto、off 或 on。".to_string()),
    }
}

fn is_deepseek_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("deepseek") || base.contains("deepseek") || model.contains("deepseek")
}

fn is_kimi_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("kimi")
        || provider.contains("moonshot")
        || base.contains("moonshot")
        || base.contains("kimi")
        || model.contains("kimi")
}

fn is_siliconflow_profile(profile: &ModelProfile, base_url: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    provider.contains("siliconflow") || base.contains("siliconflow")
}

fn apply_openai_compatible_thinking_control(
    payload: &mut serde_json::Value,
    profile: &ModelProfile,
    base_url: &str,
    model: &str,
) -> bool {
    match profile.thinking_mode.as_str() {
        "off" => apply_reasoning_parameter(payload, profile, base_url, model, false),
        "on" => apply_reasoning_parameter(payload, profile, base_url, model, true),
        _ => false,
    }
}

fn apply_reasoning_parameter(
    payload: &mut serde_json::Value,
    profile: &ModelProfile,
    base_url: &str,
    model: &str,
    enabled: bool,
) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model_lower = model.to_ascii_lowercase();

    if base.contains("openrouter") {
        payload["reasoning"] = if enabled {
            json!({ "enabled": true, "effort": "medium" })
        } else {
            json!({ "effort": "none" })
        };
        return true;
    }

    if is_deepseek_profile(profile, base_url, model) {
        payload["thinking"] = json!({ "type": if enabled { "enabled" } else { "disabled" } });
        if enabled {
            payload["reasoning_effort"] = json!("high");
        }
        return true;
    }

    if is_kimi_profile(profile, base_url, model) {
        payload["thinking"] = json!({ "type": if enabled { "enabled" } else { "disabled" } });
        return true;
    }

    if is_siliconflow_profile(profile, base_url) {
        payload["thinking_budget"] = json!(if enabled { 1024 } else { 0 });
        return true;
    }

    if base.contains("api.openai.com") || is_openai_reasoning_model(&model_lower) {
        payload["reasoning_effort"] = json!(if enabled { "medium" } else { "none" });
        return true;
    }

    if provider.contains("grok") || model_lower.contains("grok") {
        payload["reasoning_effort"] = json!(if enabled { "medium" } else { "none" });
        return true;
    }

    false
}

fn is_openai_reasoning_model(model: &str) -> bool {
    matches!(
        model,
        value if value.starts_with("o1")
            || value.starts_with("o3")
            || value.starts_with("o4")
            || value.starts_with("gpt-5")
    )
}

fn apply_gemini_thinking_control(payload: &mut serde_json::Value, profile: &ModelProfile) -> bool {
    let mode = profile.thinking_mode.as_str();
    if mode == "auto" {
        return false;
    }

    let model = profile.model.to_ascii_lowercase();
    let thinking_config = if model.contains("2.5") {
        if mode == "off" {
            json!({ "thinkingBudget": 0 })
        } else {
            json!({ "thinkingBudget": -1 })
        }
    } else if mode == "off" {
        json!({ "thinkingLevel": "minimal" })
    } else {
        json!({ "thinkingLevel": "high" })
    };

    payload["generationConfig"]["thinkingConfig"] = thinking_config;
    true
}

#[allow(dead_code)]
fn build_novel_settings_prompt(settings: &NovelSettings) -> String {
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "留空，由 AI 按姓名女性化规则生成".to_string()
    } else {
        settings.rewritten_protagonist_name.trim().to_string()
    };
    let additional = if settings.additional_feminize_names.trim().is_empty() {
        "无".to_string()
    } else {
        settings.additional_feminize_names.clone()
    };
    let additional = if settings.advanced_settings.trim().is_empty() {
        additional
    } else {
        format!(
            "{}\n\n高级设定：{}",
            additional,
            settings.advanced_settings.trim()
        )
    };
    format!(
        r#"小说基本设定：
- 主角原姓名：{}
- 主角改写后姓名：{}
- 其他需要女性化的人物姓名：{}
- 身材：{}
- 体型：{}

姓名女性化规则：
1. 如果“主角改写后姓名”不是留空，必须把主角统一改为该姓名，标题和正文都必须遵守，不得自行生成其他主角新名。
2. 如果“主角改写后姓名”留空，主角姓名必须女性化，不能保留明显男性化姓名；优先保留姓氏，名字部分用同音字或近音字替换为更女性化的字。
3. 示例：萧炎 -> 萧妍；李火旺 -> 李火婉。
4. 其他需要女性化的人物姓名只在文本中实际出现时处理，未出现则忽略。
5. 分析和改写必须维护一致的姓名映射，避免同一人物前后姓名不一致。"#,
        settings.protagonist_name, rewritten_name, additional, settings.bust, settings.body_type
    )
}

fn format_batch_label(chapters: &[Chapter]) -> String {
    match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) if first.index == last.index => format!("第{}章", first.index),
        (Some(first), Some(last)) => format!("第{}-{}章", first.index, last.index),
        _ => "空批次".to_string(),
    }
}

fn build_compact_canon_text(assets: &[CanonAsset]) -> String {
    if assets.is_empty() {
        return "无".to_string();
    }

    let compacted = assets
        .iter()
        .filter_map(|asset| {
            let content = compact_canon_asset_content(&asset.kind, &asset.content);
            if content.trim().is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", asset.kind, content))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if compacted.trim().is_empty() {
        "无".to_string()
    } else {
        compacted
    }
}

fn compact_canon_asset_content(kind: &str, content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    let deduped = lines.join("\n");
    let max_chars = canon_asset_char_limit(kind);
    if deduped.chars().count() <= max_chars {
        return deduped;
    }

    let head_limit = max_chars / 2;
    let tail_limit = max_chars.saturating_sub(head_limit);
    format!(
        "{}\n\n[一致性资产已压缩：省略中间重复或历史内容]\n\n{}",
        take_chars(&deduped, head_limit),
        take_last_chars(&deduped, tail_limit)
    )
}

fn canon_asset_char_limit(kind: &str) -> usize {
    match kind {
        "姓名映射表" => 12_000,
        "AI分析汇总" => 4_000,
        "人物卡" | "人物关系" => 6_000,
        "伏笔" | "术语表" => 5_000,
        "地点" => 3_000,
        _ => 3_000,
    }
}

fn take_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn take_last_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

fn build_rewrite_settings_prompt(settings: &NovelSettings) -> String {
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "留空，由 AI 按姓名女性化规则生成".to_string()
    } else {
        settings.rewritten_protagonist_name.trim().to_string()
    };
    let forced_name_rule = if settings.rewritten_protagonist_name.trim().is_empty() {
        "当前未指定主角改写后姓名：AI 必须按同音或近音原则为主角生成女性化姓名，并在全批次保持一致。".to_string()
    } else {
        format!(
            "强制姓名规则：用户已指定主角改写后姓名为“{}”。改写标题、正文、称谓映射和后续复检时，主角姓名必须统一为“{}”；不得自行改成其他姓名，也不得保留主角原姓名“{}”。",
            settings.rewritten_protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
            settings.protagonist_name.trim()
        )
    };
    let additional_names = if settings.additional_feminize_names.trim().is_empty() {
        "无".to_string()
    } else {
        settings.additional_feminize_names.clone()
    };
    let advanced_settings = if settings.advanced_settings.trim().is_empty() {
        "无".to_string()
    } else {
        settings.advanced_settings.trim().to_string()
    };

    format!(
        r#"小说基本设定：
- 主角原姓名：{}
- 主角改写后姓名：{}
- 其他需要女性化的人物姓名：{}
- 身材：{}
- 体型：{}
- 改写模式：{}

{}

高级设定：
{}

姓名女性化规则：
1. {}
2. 章节标题和正文都必须检查主角姓名，标题中出现主角原名、男性化称号或男性身份时也必须改成女性化表达。
3. 如果用户未指定主角改写后姓名，优先保留姓氏，名字部分用同音字或近音字替换为更女性化的字；如果用户已指定，则以用户指定姓名为最高优先级。
4. 示例：萧炎 -> 萧妍；李火旺 -> 李火婉。
5. 其他需要女性化的人物姓名只在文本中实际出现时处理，未出现则忽略。
6. 一致性资产中的“姓名映射表”优先级最高；凡是映射表中已有 `source -> target`，标题和正文都必须统一替换为 target，不得自行生成同一人物的其他女性化姓名。
7. 改写必须维护一致的姓名映射，避免同一人物前后姓名不一致；并发分片和后续批次也必须继续使用同一份映射表。

核心目标：
让没读过原文的读者阅读改写后的标题和正文时，看不出主角改写前曾是男性。凡是与主角有关的男性化姓名、代词、称谓、身份、身体特征、外貌气质、动作习惯、社会评价、亲密互动暗示，都必须改成自然的女性化表达；不能只删除男性化信息，也不能留下“男主”“少年郎”“公子”“他作为男人”等残留痕迹。

人物性别与代词一致性规则：
1. 只允许主角、用户填写的“其他需要女性化的人物姓名”、以及一致性资产“姓名映射表”中明确存在映射的人物进行性别转换。
2. 其他未指定人物必须保持原文性别、身份、称谓和人称代词：原文男性配角继续使用男性身份与“他/父亲/兄弟/少爷/公子”等符合原文的表达；原文女性配角继续使用女性身份与“她/母亲/姐妹/小姐”等符合原文的表达。
3. 不得因为百合改写目标而把所有重要配角、敌人、长辈、师父、兄弟、父亲或旁观者都改成女性；也不得在不同章节中让同一配角一会儿是男性、一会儿是女性。
4. 对性别不明或原文暂未明确的人物，应保持中性称呼或沿用原文称谓，等一致性资产或原文后续明确后再固定；不要凭空改成女性或男性。
5. 改写时必须参考一致性资产中的人物卡、人物关系、姓名映射表和原文上下文，确保每个人物的性别、代词、称谓、亲属关系和社会身份跨章节一致。

一致性硬性要求：
1. 人物外貌特征必须前后一致。发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态一旦由原文、设定或一致性资产确立，后续章节不得随意改变；例如上一章是金发，下一章不能无理由变成红发。
2. 如果原文没有明确外貌，不要每章随机发明互相矛盾的新特征；需要补充女性化描写时，应使用与已建立设定兼容的细节，并保持后续复用。
3. 人物关系和百合向情绪推进必须连续。暧昧、信任、依赖、吃醋、保护欲、亲密距离等变化要承接前文，不能上一章刚建立的关系下一章突然重置。
4. 称谓、代词、身份和旁人态度必须统一。主角已经女性化后，旁人对她的称呼、视线、互动距离、社会评价也要自然匹配女性身份，不能在不同章节反复摇摆。
5. 新增女性化细节必须服务当前剧情和人物状态，不得为了强调性别而制造与原文战力、性格、伏笔、剧情逻辑冲突的描写。"#,
        settings.protagonist_name,
        rewritten_name,
        additional_names,
        settings.bust,
        settings.body_type,
        rewrite_mode_label(&settings.rewrite_mode),
        rewrite_mode_prompt(&settings.rewrite_mode),
        advanced_settings,
        forced_name_rule
    )
}

fn rewrite_mode_label(mode: &str) -> &'static str {
    match mode {
        "creative" => "创意模式",
        _ => "严谨模式",
    }
}

fn rewrite_mode_prompt(mode: &str) -> &'static str {
    match mode {
        "creative" => {
            r#"改写模式规则：当前为创意模式，此规则优先级高于普通的“中度再创作”约束。
1. 必须让读者在每章都能明确感知主角已经从男性变为女性，而不是只替换姓名和代词。
2. 在不改变主线、关键事件、章节顺序和核心逻辑的前提下，主动补充女性化细节：女性外貌、身形仪态、神态反应、衣着/发丝/气息等可感知细节，以及旁人看待女性主角时的称谓、距离感、保护欲、亲密互动或误会。
3. 原文涉及男性身体、男性身份、男性社会称呼、男性动作习惯、男性气质展示时，必须改写为与设定身材和体型一致的女性表达；不能只删除这些内容。
4. 主角与周围人物互动时，应自然体现她作为女性后的关系变化，例如语气、肢体距离、旁人态度、暧昧张力、同性亲密感和百合向情绪推进。
5. 每章至少在关键场景中增加或强化 2-4 处女性化感知点；战斗、修炼、对话、日常和情感场景都要优先寻找可自然植入的位置。
6. 新增内容必须贴合原剧情和原文风格，不要写成与当前情节无关的堆砌描写，不得破坏已有伏笔、战力逻辑和人物动机。"#
        }
        _ => {
            "改写模式规则：当前为严谨模式。AI 必须更加忠于原文，不做过大改动，不对主角添加过多额外女性化描写；但必要的女性化描写不能减少，原文本身已有的男性化描写在改写后必须自然转换为女性化描写。"
        }
    }
}

fn analysis_chapter_start_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_ANALYSIS_CHAPTER_START index={} id={}>>>",
        chapter.index, chapter.id
    )
}

fn analysis_chapter_end_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_ANALYSIS_CHAPTER_END index={} id={}>>>",
        chapter.index, chapter.id
    )
}

fn chapter_start_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_REWRITE_CHAPTER_START index={} id={}>>>",
        chapter.index, chapter.id
    )
}

fn chapter_end_marker(chapter: &Chapter) -> String {
    format!(
        "<<<YURI_REWRITE_CHAPTER_END index={} id={}>>>",
        chapter.index, chapter.id
    )
}

fn build_batch_chapter_text(chapters: &[Chapter], use_rewrite_text: bool) -> String {
    chapters
        .iter()
        .map(|chapter| {
            let text = if use_rewrite_text {
                chapter
                    .rewrite_text
                    .as_deref()
                    .unwrap_or(&chapter.original_text)
            } else {
                &chapter.original_text
            };
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                chapter_start_marker(chapter),
                chapter.title,
                text.trim(),
                chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_batch_analysis_chapter_text(chapters: &[Chapter]) -> String {
    chapters
        .iter()
        .map(|chapter| {
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                analysis_chapter_start_marker(chapter),
                chapter.title,
                chapter.original_text.trim(),
                analysis_chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_batch_rewrite_text(chapters: &[Chapter], rewrites: &[ParsedChapterRewrite]) -> String {
    chapters
        .iter()
        .zip(rewrites.iter())
        .map(|(chapter, rewrite)| {
            debug_assert_eq!(chapter.id, rewrite.id);
            debug_assert_eq!(chapter.index, rewrite.index);
            format!(
                "{}\n标题：{}\n正文：\n{}\n{}",
                chapter_start_marker(chapter),
                rewrite.title,
                rewrite.text.trim(),
                chapter_end_marker(chapter)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn parse_batch_analysis_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let json_error = match parse_batch_analysis_json_output(output, expected_chapters) {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error,
    };

    if output.contains("YURI_ANALYSIS_CHAPTER_START") {
        return parse_batch_analysis_marker_output(output, expected_chapters).map_err(|marker_error| {
            format!(
                "AI 分析输出既不是合法批次 JSON，也不是有效章节边界格式。JSON 解析错误：{}；边界格式解析错误：{}",
                json_error, marker_error
            )
        });
    }

    Err(json_error)
}

fn parse_batch_analysis_json_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let value = parse_jsonish_value(output)
        .map_err(|error| format!("AI 分析输出不是合法 JSON：{}", error))?;
    if let Ok(batch_json) = extract_batch_level_analysis_json(&value) {
        return Ok(vec![ParsedChapterAnalysis {
            id: expected_chapters
                .first()
                .ok_or_else(|| "缺少待分析章节。".to_string())?
                .id
                .clone(),
            json: batch_json,
        }]);
    }

    let items = match &value {
        serde_json::Value::Object(map) => map
            .get("chapters")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "AI 分析 JSON 缺少 chapters 数组。".to_string())?,
        serde_json::Value::Array(items) => items,
        _ => return Err("AI 分析 JSON 必须是对象或数组。".to_string()),
    };

    if items.len() != expected_chapters.len() {
        return Err(format!(
            "AI 分析 JSON 章节数量不匹配：期望 {} 章，实际 {} 章。",
            expected_chapters.len(),
            items.len()
        ));
    }

    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (item, chapter) in items.iter().zip(expected_chapters.iter()) {
        let item_object = item
            .as_object()
            .ok_or_else(|| format!("章节 {} 的分析项必须是 JSON 对象。", chapter.index))?;
        if let Some(index) = item_object
            .get("index")
            .or_else(|| item_object.get("chapter_index"))
            .and_then(serde_json::Value::as_i64)
        {
            if index != chapter.index {
                return Err(format!(
                    "AI 分析 JSON 章节顺序不匹配：期望第 {} 章，实际第 {} 章。",
                    chapter.index, index
                ));
            }
        }
        if let Some(id) = item_object
            .get("id")
            .or_else(|| item_object.get("chapter_id"))
            .and_then(serde_json::Value::as_str)
        {
            if id != chapter.id {
                return Err(format!(
                    "AI 分析 JSON 章节 id 不匹配：期望 {}，实际 {}。",
                    chapter.id, id
                ));
            }
        }

        let analysis_value = item_object.get("analysis").unwrap_or(item);
        let mut analysis = analysis_value
            .as_object()
            .ok_or_else(|| format!("章节 {} 的 analysis 必须是 JSON 对象。", chapter.index))?
            .clone();
        analysis.remove("id");
        analysis.remove("chapter_id");
        analysis.remove("index");
        analysis.remove("chapter_index");
        analysis.remove("title");
        analysis.remove("chapter_title");
        if analysis.is_empty() {
            return Err(format!("章节 {} 的分析 JSON 为空。", chapter.index));
        }
        let json = serde_json::to_string_pretty(&serde_json::Value::Object(analysis))
            .map_err(to_string)?;
        parsed.push(ParsedChapterAnalysis {
            id: chapter.id.clone(),
            json,
        });
    }

    Ok(parsed)
}

fn extract_batch_level_analysis_json(value: &serde_json::Value) -> Result<String, String> {
    let candidate = value
        .get("batch_assets")
        .or_else(|| value.get("consistency_assets"))
        .or_else(|| value.get("assets"))
        .or_else(|| value.get("analysis"))
        .unwrap_or(value);
    let object = candidate
        .as_object()
        .ok_or_else(|| "批次级分析 JSON 必须是对象。".to_string())?;
    if object.contains_key("chapters") {
        return Err("检测到逐章 chapters 输出。".to_string());
    }
    let useful_fields = [
        "outline",
        "characters",
        "relationships",
        "locations",
        "foreshadowing",
        "terms",
        "names",
    ];
    if !useful_fields
        .iter()
        .any(|field| object.contains_key(*field))
    {
        return Err("批次级分析 JSON 缺少一致性资产字段。".to_string());
    }
    serde_json::to_string_pretty(candidate).map_err(to_string)
}

fn parse_batch_analysis_marker_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let mut cursor = output.replace("\r\n", "\n").replace('\r', "\n");
    let mut parsed = Vec::with_capacity(expected_chapters.len());

    for chapter in expected_chapters {
        let start_marker = analysis_chapter_start_marker(chapter);
        let end_marker = analysis_chapter_end_marker(chapter);
        let start_pos = cursor
            .find(&start_marker)
            .ok_or_else(|| format!("AI 输出缺少章节分析开始标记：{}", start_marker))?;
        if !cursor[..start_pos].trim().is_empty() {
            return Err(format!(
                "AI 输出在章节 {} 分析开始标记前包含多余内容。",
                chapter.index
            ));
        }
        let after_start = cursor[start_pos + start_marker.len()..].to_string();
        let end_pos = after_start
            .find(&end_marker)
            .ok_or_else(|| format!("AI 输出缺少章节分析结束标记：{}", end_marker))?;
        let block = after_start[..end_pos].trim();
        if block.trim().is_empty() {
            return Err(format!("章节 {} 的分析 JSON 为空。", chapter.index));
        }
        let value = parse_jsonish_value(block)
            .map_err(|error| format!("章节 {} 的分析 JSON 无效：{}", chapter.index, error))?;
        let normalized = serde_json::to_string_pretty(&value).map_err(to_string)?;
        parsed.push(ParsedChapterAnalysis {
            id: chapter.id.clone(),
            json: normalized,
        });
        cursor = after_start[end_pos + end_marker.len()..].to_string();
    }

    if !cursor.trim().is_empty() {
        return Err("AI 输出在最后一个章节分析结束标记后包含多余内容。".to_string());
    }
    Ok(parsed)
}

fn parse_batch_rewrite_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let marker_error = parse_batch_rewrite_marker_output(&normalized, expected_chapters).err();
    if marker_error.is_none() {
        return parse_batch_rewrite_marker_output(&normalized, expected_chapters);
    }
    if marker_error
        .as_deref()
        .is_some_and(|error| error.contains("章节顺序不匹配"))
    {
        return Err(marker_error.unwrap());
    }

    match parse_markerless_rewrite_output(&normalized, expected_chapters) {
        Ok(parsed) => Ok(parsed),
        Err(fallback_error) => Err(match marker_error {
            Some(error) => format!("{}；兜底解析也失败：{}", error, fallback_error),
            None => fallback_error,
        }),
    }
}

fn parse_batch_rewrite_marker_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut cursor = output.to_string();
    let mut parsed = Vec::with_capacity(expected_chapters.len());

    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start_marker = chapter_start_marker(chapter);
        let end_marker = chapter_end_marker(chapter);
        let (start_pos, start_len) = find_rewrite_marker(&cursor, chapter, "START")
            .ok_or_else(|| format!("AI 输出缺少章节开始标记：{}", start_marker))?;
        let before_start = cursor[..start_pos].trim();
        if !before_start.is_empty() && !before_start.contains("YURI_REWRITE_CHAPTER_START") {
            return Err(format!(
                "AI 输出在章节 {} 开始标记前包含多余内容。",
                chapter.index
            ));
        }
        if contains_expected_rewrite_start_marker(before_start, &expected_chapters[idx + 1..]) {
            return Err(format!(
                "AI 输出章节顺序不匹配：在章节 {} 前出现了当前分片内的后续章节标记。",
                chapter.index
            ));
        }
        let after_start = cursor[start_pos + start_len..].to_string();
        let (block, next_cursor) =
            if let Some((end_pos, end_len)) = find_rewrite_marker(&after_start, chapter, "END") {
                (
                    after_start[..end_pos].to_string(),
                    after_start[end_pos + end_len..].to_string(),
                )
            } else if let Some(next_chapter) = expected_chapters.get(idx + 1) {
                let next_start_marker = chapter_start_marker(next_chapter);
                let (next_pos, _) = find_rewrite_marker(&after_start, next_chapter, "START")
                    .ok_or_else(|| {
                        format!(
                            "AI 输出缺少章节结束标记：{}，且无法定位下一章开始标记：{}",
                            end_marker, next_start_marker
                        )
                    })?;
                (
                    after_start[..next_pos].to_string(),
                    after_start[next_pos..].to_string(),
                )
            } else if !after_start.trim().is_empty() {
                (after_start, String::new())
            } else {
                return Err(format!("AI 输出缺少章节结束标记：{}", end_marker));
            };
        let (title, text) = clean_rewrite_block(&block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
        cursor = next_cursor;
    }

    let trailing = cursor.trim();
    if !trailing.is_empty() && !trailing.contains("YURI_REWRITE_CHAPTER_START") {
        return Err("AI 输出在最后一个章节结束标记后包含多余内容。".to_string());
    }
    Ok(parsed)
}

fn find_rewrite_marker(text: &str, chapter: &Chapter, kind: &str) -> Option<(usize, usize)> {
    let exact = if kind == "START" {
        chapter_start_marker(chapter)
    } else {
        chapter_end_marker(chapter)
    };
    if let Some(pos) = text.find(&exact) {
        return Some((pos, exact.len()));
    }

    let pattern = format!(
        r#"<<<\s*YURI_REWRITE_CHAPTER_{}\s+index\s*=\s*{}(?:\s+id\s*=\s*[^>\s]+)?\s*>>>"#,
        kind, chapter.index
    );
    let regex = Regex::new(&pattern).ok()?;
    regex
        .find(text)
        .map(|mat| (mat.start(), mat.end() - mat.start()))
}

fn contains_expected_rewrite_start_marker(text: &str, chapters: &[Chapter]) -> bool {
    chapters
        .iter()
        .any(|chapter| find_rewrite_marker(text, chapter, "START").is_some())
}

fn parse_markerless_rewrite_output(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let normalized = strip_rewrite_marker_lines(output);
    if normalized.trim().is_empty() {
        return Err("AI 输出为空，无法兜底解析。".to_string());
    }

    if let Ok(parsed) = parse_markerless_by_title_labels(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if let Ok(parsed) = parse_markerless_by_expected_titles(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if let Ok(parsed) = parse_markerless_by_heading_regex(&normalized, expected_chapters) {
        return Ok(parsed);
    }
    if expected_chapters.len() == 1 {
        let (title, text) = clean_rewrite_block(&normalized, &expected_chapters[0].title);
        if !text.trim().is_empty() {
            return Ok(vec![ParsedChapterRewrite {
                id: expected_chapters[0].id.clone(),
                index: expected_chapters[0].index,
                title,
                text,
            }]);
        }
    }

    Err("无法从无 marker 输出中稳定拆回当前分片章节。".to_string())
}

fn strip_rewrite_marker_lines(output: &str) -> String {
    output
        .trim()
        .trim_start_matches("```text")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("<<<YURI_REWRITE_CHAPTER_START")
                && !trimmed.starts_with("<<<YURI_REWRITE_CHAPTER_END")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_markerless_by_title_labels(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let lines = output.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("标题：") || trimmed.starts_with("标题:") {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if starts.len() != expected_chapters.len() {
        return Err("标题行数量与分片章节数量不匹配。".to_string());
    }

    parse_markerless_line_blocks(&lines, &starts, expected_chapters)
}

fn parse_markerless_by_heading_regex(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let lines = output.lines().collect::<Vec<_>>();
    let heading_re = chapter_heading_regex();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if !matches!(trimmed, "正文" | "正文：" | "正文:")
                && heading_re.is_match(trimmed)
                && is_plausible_strict_heading_line(trimmed)
            {
                Some(idx)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if starts.len() != expected_chapters.len() {
        return Err("章节标题数量与分片章节数量不匹配。".to_string());
    }

    parse_markerless_line_blocks(&lines, &starts, expected_chapters)
}

fn parse_markerless_line_blocks(
    lines: &[&str],
    starts: &[usize],
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start = starts[idx];
        let end = starts.get(idx + 1).copied().unwrap_or(lines.len());
        let block = lines[start..end].join("\n");
        let (title, text) = clean_rewrite_block(&block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的兜底改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
    }
    Ok(parsed)
}

fn parse_markerless_by_expected_titles(
    output: &str,
    expected_chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let mut positions = Vec::with_capacity(expected_chapters.len());
    let mut search_from = 0usize;
    for chapter in expected_chapters {
        let title = chapter.title.trim();
        if title.is_empty() {
            return Err("章节标题为空，无法按标题兜底解析。".to_string());
        }
        let relative = output[search_from..]
            .find(title)
            .ok_or_else(|| format!("兜底解析找不到章节标题：{}", title))?;
        let pos = search_from + relative;
        positions.push(pos);
        search_from = pos + title.len();
    }

    let mut parsed = Vec::with_capacity(expected_chapters.len());
    for (idx, chapter) in expected_chapters.iter().enumerate() {
        let start = positions[idx];
        let end = positions.get(idx + 1).copied().unwrap_or(output.len());
        let block = output[start..end].trim();
        let (title, text) = clean_rewrite_block(block, &chapter.title);
        if text.trim().is_empty() {
            return Err(format!("章节 {} 的兜底改写正文为空。", chapter.index));
        }
        parsed.push(ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title,
            text,
        });
    }
    Ok(parsed)
}

fn clean_rewrite_block(block: &str, fallback_title: &str) -> (String, String) {
    let mut lines = block.trim().lines().collect::<Vec<_>>();
    let mut title = fallback_title.trim().to_string();
    if lines.first().is_some_and(|line| {
        line.trim_start().starts_with("标题：") || line.trim_start().starts_with("标题:")
    }) {
        let title_line = lines.remove(0).trim().to_string();
        let parsed_title = title_line
            .strip_prefix("标题：")
            .or_else(|| title_line.strip_prefix("标题:"))
            .unwrap_or("")
            .trim();
        if !parsed_title.is_empty() {
            title = parsed_title.to_string();
        }
    }
    if lines
        .first()
        .is_some_and(|line| matches!(line.trim(), "正文：" | "正文:" | "正文"))
    {
        lines.remove(0);
    }
    (title, lines.join("\n").trim().to_string())
}

fn mark_chapters_rewrite_failed(
    state: &State<'_, AppState>,
    chapters: &[Chapter],
) -> Result<(), String> {
    for chapter in chapters {
        set_chapter_status(state, &chapter.id, "rewrite_status", "failed")?;
    }
    Ok(())
}

fn save_parsed_rewrites(
    state: &State<'_, AppState>,
    rewrites: Vec<ParsedChapterRewrite>,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    for rewrite in rewrites {
        tx.execute(
            "UPDATE chapters SET title = ?1, rewrite_text = ?2, rewrite_status = 'completed' WHERE id = ?3",
            params![rewrite.title.trim(), rewrite.text.trim(), rewrite.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    Ok(())
}

fn mark_chapters_analysis_failed(
    state: &State<'_, AppState>,
    chapters: &[Chapter],
) -> Result<(), String> {
    for chapter in chapters {
        set_chapter_status(state, &chapter.id, "analysis_status", "failed")?;
    }
    Ok(())
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
        build_rewrite_settings_prompt(settings),
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
fn build_batch_rewrite_prompt_with_settings(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
) -> String {
    build_batch_rewrite_prompt_with_context(chapters, canon_text, settings, "", "")
}

fn build_core_prompt_section(core_prompt: &str) -> String {
    let core_prompt = core_prompt.trim();
    if core_prompt.is_empty() {
        return "最高优先级核心设定：无。".to_string();
    }
    format!(
        "最高优先级核心设定：\n以下内容是用户设置的全局写作规则，优先级高于本次改写中的其他风格、描写、节奏和表达要求。必须在不破坏章节边界、姓名映射、角色性别规则、原文主线和逻辑的前提下，始终按这些文风、描写方式、语气、节奏和其他全局要求改写。\n{}",
        core_prompt
    )
}

fn build_batch_rewrite_prompt_with_context(
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"{}

改写要求：
1. 将原本男女性别叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序、战力逻辑、人物动机和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 标题和正文都必须改写：标题中的主角原名、男性身份、男性称谓、男性化意象也要同步女性化。
4. 清除所有原男性主角痕迹，包括姓名、代词、身体描述、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示；所有相关内容都要自然转换为女性主角表达。
5. 主角姓名和指定 NPC 姓名必须严格使用一致性资产中的“姓名映射表”。没有映射时才按同音或近音原则女性化，优先保留姓氏；例如萧炎改为萧妍，李火旺改为李火婉。
6. 按基本设定中的身材和体型调整外貌、动作和互动细节，不要出现与设定冲突的描写。
7. 只有主角、用户指定的额外女性化人物、以及姓名映射表中明确存在映射的人物可以性别转换；其他配角、敌人、长辈、师父、兄弟、父亲、旁观者必须保持原文性别、身份、称谓和人称代词，不得跨章节忽男忽女。
8. 对未指定性转的人物，原文男性继续使用男性代词/称谓，原文女性继续使用女性代词/称谓，性别不明者保持原文称谓或中性表达，等原文或一致性资产明确后再固定。
9. 人物外貌特征必须前后一致。发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态一旦由原文、设定或一致性资产确立，后续章节不得随意改变；例如上一章是金发，下一章不能无理由变成红发。
10. 如果原文没有明确外貌，不要每章随机发明互相矛盾的新特征；需要补充女性化描写时，应使用与已建立设定兼容的细节，并保持后续复用。
11. 百合向关系推进必须承接前文。暧昧、信任、依赖、吃醋、保护欲、亲密距离、旁人态度和称谓变化要符合当前剧情阶段，不能突然重置或跳跃。
12. 女性化细节要覆盖正文和标题，也要覆盖旁人的视线、评价、互动距离和社会称呼；但新增内容必须服务当前剧情，不得破坏原文战力、伏笔、人物性格和逻辑。
13. 输入可能是完整批次，也可能是并发分片；必须一次性改写当前输入中实际出现的全部章节，不要逐章分开回答。
14. 每章必须以输入中对应的 `<<<YURI_REWRITE_CHAPTER_START ...>>>` 开始标记开头，并以对应的 `<<<YURI_REWRITE_CHAPTER_END ...>>>` 结束标记结尾；marker 中的 index 和 id 必须逐字复制，不得省略、改写或自行生成。
15. 只输出当前输入章节的边界标记、改写后标题和正文，不要解释、不要 Markdown 包裹，不要输出当前输入之外的章节。

{}

并发分片上下文：
{}

一致性资产：
{}

当前输入章节：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_settings_prompt(settings),
        shard_context,
        canon_text,
        build_batch_chapter_text(chapters, false)
    )
}

#[allow(dead_code)]
fn build_batch_review_prompt_with_settings(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> String {
    build_batch_review_prompt_with_context(chapters, rewrites, settings, "")
}

fn build_batch_review_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请复检并自动修正以下批次改写稿。

重点检查：
1. 主角姓名是否已按规则女性化，且全批次一致。
2. 每章标题是否也完成女性化，标题里不能残留主角男性姓名、男性身份或男性称谓。
3. 其他指定姓名只在出现时女性化，且前后一致。
4. 人称代词、称谓、身体描写、外貌气质、社会称呼、动作习惯和互动细节是否仍残留男性主角痕迹。
5. 身材、体型和高级设定是否被遵守。
6. 如果当前为创意模式，检查每章关键场景是否有足够清晰的女性化感知点；若只是替换姓名/代词，应主动补充贴合原剧情的女性外貌、神态、互动距离、称谓变化、百合向情绪张力等细节。
7. 改写后的标题和正文是否能让没读过原文的读者看不出主角原本是男性。
8. 人物外貌特征是否前后一致：发色、瞳色、身高、体型、胸部设定、年龄感、标志性服饰、伤痕、气质和能力状态不能在不同章节无理由变化。
9. 百合向关系推进是否承接前文：暧昧、信任、依赖、吃醋、保护欲、亲密距离、称谓和旁人态度不能突然重置或跳跃。
10. 女性化补充是否贴合剧情和一致性资产，不能为了强调性别而破坏原文战力、伏笔、人物性格和逻辑。
11. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲、旁观者是否被误改性别；同一人物在不同章节中的他/她、先生/小姐、父亲/母亲、兄弟/姐妹、少爷/小姐等代词和称谓是否前后一致。
12. 章节内部和章节之间是否有逻辑不通、缺句、重复、边界错乱。

输出要求：
1. 如果发现问题，直接在正文中修正。
2. 如果没有问题，原样输出改写稿。
3. 每章必须以输入中对应的 `<<<YURI_REWRITE_CHAPTER_START ...>>>` 开始标记开头，并以对应的 `<<<YURI_REWRITE_CHAPTER_END ...>>>` 结束标记结尾；marker 中的 index 和 id 必须逐字复制，不得省略、改写或自行生成。
4. 只输出当前输入章节的边界标记、修正后标题和正文，不要解释、不要 Markdown 包裹，不要输出当前输入之外的章节。

{}

并发分片上下文：
{}

待复检改写稿：
{}"#,
        build_rewrite_settings_prompt(settings),
        shard_context,
        build_batch_rewrite_text(chapters, rewrites)
    )
}

#[allow(dead_code)]
fn build_rewrite_prompt_with_settings(
    chapter: &Chapter,
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
) -> String {
    format!(
        r#"{}

改写要求：
1. 将原本男女主叙事自然改写为双女主百合叙事。
2. 采用中度再创作：保留主线、冲突、章节顺序和关键伏笔，但可以调整互动、细节动作、称谓、外貌描述和关系推进。
3. 标题和正文都必须改写，标题中的主角原名、男性身份、男性称谓、男性化意象也要同步女性化。
4. 清除所有原男主痕迹，包括姓名、代词、身体描写、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
5. 主角姓名必须按同音或近音原则女性化，例如萧炎改为萧妍，李火旺改为李火婉；其他指定姓名只在本章出现时女性化。
6. 按基本设定中的身材和体型调整外貌、动作和互动细节，不要出现与设定冲突的描写。
7. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲和旁观者必须保持原文性别、代词、称谓和身份一致，不得因为百合改写目标被误改成女性或跨章节忽男忽女。
8. 保持中文网文可读性，只输出改写后的标题和正文，不要解释。

{}

一致性资产：
{}

章节标题：{}

原章节：
{}"#,
        build_core_prompt_section(core_prompt),
        build_rewrite_settings_prompt(settings),
        canon_text,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
fn build_batch_analysis_prompt(chapters: &[Chapter]) -> String {
    build_batch_analysis_prompt_with_context(chapters, "")
}

fn build_batch_analysis_prompt_with_context(chapters: &[Chapter], shard_context: &str) -> String {
    let (start_index, end_index) = match (chapters.first(), chapters.last()) {
        (Some(first), Some(last)) => (first.index, last.index),
        _ => (0, 0),
    };
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请只基于原文分析以下整个批次，并输出一个合法 JSON 对象。

输出结构必须是：
{{
  "batch": {{
    "start_index": {},
    "end_index": {},
    "chapter_count": {}
  }},
  "outline": ["本批次原文主线、关键事件和状态变化，按时间顺序概括"],
  "characters": ["本批次出现的重要人物、别名、原文性别线索、原文人称代词、身份、称谓、外貌、性格、动机、能力或状态变化"],
  "relationships": ["本批次人物关系与关系变化"],
  "locations": ["本批次地点、场景和空间关系"],
  "foreshadowing": ["本批次伏笔、悬念、回收或关键信息"],
  "terms": ["本批次术语、组织、物品、功法、系统规则等"],
  "names": ["本批次出现的人名、称谓、别名、指代对象、对应人物的原文性别或性别不明状态"]
}}

要求：
1. 输入可能是完整批次，也可能是并发分片；必须一次性分析当前输入中实际出现的全部章节。
2. 只输出一份当前输入级一致性资产，不要按章节逐章输出，不要输出 `chapters` 数组。
3. 不要补充原文没有的信息，不要改变原文人物、姓名、关系或剧情。
4. 必须尽量记录人物的原文性别线索、代词、称谓和亲属身份；无法确定时写“性别不明”，不要猜测。
5. 不要提出任何后续处理方向。
6. JSON 字符串内部如果需要换行，必须写成 `\n`，不要在字符串里输出真实换行或其他控制字符。
7. 只输出 JSON，不要解释、不要 Markdown。

并发分片上下文：
{}

当前输入章节：
{}"#,
        start_index,
        end_index,
        chapters.len(),
        shard_context,
        build_batch_analysis_chapter_text(chapters)
    )
}

#[allow(dead_code)]
fn build_analysis_prompt(chapter: &Chapter) -> String {
    format!(
        r#"请只基于原文分析以下章节，并输出合法 JSON：
{{
  "outline": "本章原文大纲",
  "characters": ["原文人物、别名、原文性别线索、原文人称代词、身份、称谓、外貌、性格、动机、能力或状态变化"],
  "relationships": ["原文人物关系与关系变化"],
  "locations": ["原文地点、场景和空间关系"],
  "foreshadowing": ["原文伏笔、悬念、回收或关键信息"],
  "terms": ["原文术语、组织、物品、功法、系统规则等"],
  "names": ["原文出现的人名、称谓、别名、指代对象、对应人物的原文性别或性别不明状态"]
}}

要求：
1. 只提取和维护原文一致性资产。
2. 不要提出任何后续处理方向。
3. 不要补充原文没有的信息，不要改变原文人物、姓名、关系或剧情。
4. 必须尽量记录人物的原文性别线索、代词、称谓和亲属身份；无法确定时写“性别不明”，不要猜测。
5. 只输出 JSON，不要 Markdown。

章节标题：{}

章节正文：
{}"#,
        chapter.title,
        truncate_text(&chapter.original_text, 30_000)
    )
}

#[allow(dead_code)]
fn build_analysis_prompt_legacy(chapter: &Chapter) -> String {
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
3. 标题和正文都必须改写，标题中的主角原名、男性身份、男性称谓、男性化意象也要同步女性化。
4. 清除所有原男主痕迹，包括姓名、代词、身体描写、外貌气质、社会称呼、动作习惯、旁人称谓和亲密互动中的性别暗示。
5. 未指定性转的配角、敌人、长辈、师父、兄弟、父亲和旁观者必须保持原文性别、代词、称谓和身份一致，不得因为百合改写目标被误改成女性或跨章节忽男忽女。
6. 保持中文网文可读性，只输出改写后的标题和正文，不要解释。

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
    upsert_canon_asset(
        conn,
        novel_id,
        "人物卡",
        &collect_analysis_field(&rows, "characters"),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "人物关系",
        &collect_analysis_field(&rows, "relationships"),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "地点",
        &collect_analysis_field(&rows, "locations"),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "伏笔",
        &collect_analysis_field(&rows, "foreshadowing"),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "术语表",
        &collect_analysis_terms(&rows),
        &now,
    )?;
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
                .get("terms")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("原文术语：\n{}", text));
            }
            if let Some(text) = value
                .get("names")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("原文姓名与称谓：\n{}", text));
            }
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
        serde_json::Value::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    }
}

fn fill_empty_canon_assets_from_analysis(
    conn: &Connection,
    novel_id: &str,
) -> rusqlite::Result<()> {
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
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "人物卡",
        &collect_analysis_field(&rows, "characters"),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "人物关系",
        &collect_analysis_field(&rows, "relationships"),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "地点",
        &collect_analysis_field(&rows, "locations"),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "伏笔",
        &collect_analysis_field(&rows, "foreshadowing"),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "术语表",
        &collect_analysis_terms(&rows),
        &now,
    )?;
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
            Ok(format!(
                "## {}\n{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?
            ))
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

fn create_job(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_type: &str,
    total: i64,
) -> Result<Job, String> {
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

fn prepare_auto_run(state: &State<'_, AppState>, novel_id: &str) -> Result<i64, String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    let resume_from = runs
        .get(novel_id)
        .filter(|control| control.status == "paused")
        .map(|control| control.completed_batches)
        .unwrap_or(0);
    if let Some(control) = runs.get(novel_id) {
        if control.status == "running" || control.status == "pause_requested" {
            return Err("一键分析改写正在运行，请先暂停或终止当前任务。".to_string());
        }
    }
    runs.insert(
        novel_id.to_string(),
        AutoRunControl {
            status: "running".to_string(),
            completed_batches: resume_from,
            job_id: None,
        },
    );
    Ok(resume_from)
}

fn register_auto_run_job(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_id: &str,
    completed_batches: i64,
) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    let control = runs
        .entry(novel_id.to_string())
        .or_insert_with(|| AutoRunControl {
            status: "running".to_string(),
            completed_batches,
            job_id: None,
        });
    control.status = "running".to_string();
    control.completed_batches = completed_batches;
    control.job_id = Some(job_id.to_string());
    Ok(())
}

fn set_auto_run_completed(
    state: &State<'_, AppState>,
    novel_id: &str,
    completed_batches: i64,
) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(novel_id) {
        control.completed_batches = completed_batches;
    }
    Ok(())
}

fn requested_auto_run_stop(
    state: &State<'_, AppState>,
    novel_id: &str,
) -> Result<Option<String>, String> {
    let runs = state.auto_runs.lock().map_err(to_string)?;
    Ok(runs.get(novel_id).and_then(|control| {
        if control.status == "pause_requested" {
            Some(AUTO_RUN_PAUSED.to_string())
        } else if control.status == "terminate_requested" {
            Some(AUTO_RUN_TERMINATED.to_string())
        } else {
            None
        }
    }))
}

fn request_auto_run_stop(
    state: &State<'_, AppState>,
    novel_id: &str,
    status: &str,
) -> Result<Job, String> {
    let (job_id, completed_batches, message, job_status) = {
        let mut runs = state.auto_runs.lock().map_err(to_string)?;
        let control = runs
            .get_mut(novel_id)
            .ok_or_else(|| "当前没有正在运行的一键分析改写任务。".to_string())?;
        control.status = status.to_string();
        let job_id = control
            .job_id
            .clone()
            .ok_or_else(|| "当前一键任务尚未创建进度记录。".to_string())?;
        let message = if status == "terminate_requested" {
            "正在终止一键分析改写，当前未输出批次将不会保存。"
        } else {
            "正在暂停一键分析改写，当前未输出批次将从头重跑。"
        };
        let job_status = if status == "terminate_requested" {
            "terminating"
        } else {
            "pausing"
        };
        (
            job_id,
            control.completed_batches,
            message.to_string(),
            job_status.to_string(),
        )
    };
    update_job(state, &job_id, &job_status, completed_batches, &message)?;
    load_job(state, &job_id)
}

fn finish_stopped_auto_run(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    status_marker: &str,
) -> Result<Job, String> {
    if status_marker == AUTO_RUN_TERMINATED {
        let message = "一键分析改写已终止。下次点击将从头开始新的执行。";
        update_job(state, &job.id, "terminated", completed_batches, message)?;
        emit_job_progress(app, &job, "terminated", completed_batches, message);
        clear_auto_run(state, &job.novel_id)?;
    } else {
        let message = format!(
            "一键分析改写已暂停。继续后将从第 {} 批重新开始。",
            completed_batches + 1
        );
        update_job(state, &job.id, "paused", completed_batches, &message)?;
        emit_job_progress(app, &job, "paused", completed_batches, &message);
        let mut runs = state.auto_runs.lock().map_err(to_string)?;
        runs.insert(
            job.novel_id.clone(),
            AutoRunControl {
                status: "paused".to_string(),
                completed_batches,
                job_id: Some(job.id.clone()),
            },
        );
    }
    load_job(state, &job.id)
}

fn clear_auto_run(state: &State<'_, AppState>, novel_id: &str) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    runs.remove(novel_id);
    Ok(())
}

async fn next_auto_join<T: Send + 'static>(
    tasks: &mut tokio::task::JoinSet<T>,
    state: &State<'_, AppState>,
    novel_id: &str,
) -> Result<Option<T>, String> {
    loop {
        tokio::select! {
            result = tasks.join_next() => {
                return match result {
                    Some(result) => result.map(Some).map_err(to_string),
                    None => Ok(None),
                };
            }
            _ = tokio::time::sleep(Duration::from_millis(300)) => {
                if let Some(status) = requested_auto_run_stop(state, novel_id)? {
                    tasks.abort_all();
                    return Err(status);
                }
            }
        }
    }
}

fn emit_job_progress(
    app: &AppHandle,
    job: &Job,
    status: &str,
    current_chapter: i64,
    message: &str,
) {
    let progress = JobProgress {
        id: job.id.clone(),
        novel_id: job.novel_id.clone(),
        job_type: job.job_type.clone(),
        status: status.to_string(),
        current_chapter,
        total_chapters: job.total_chapters,
        message: message.to_string(),
    };
    let _ = app.emit("job-progress", progress);
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
    let db_api_key = conn
        .query_row(
            "SELECT api_key FROM model_profiles WHERE id = ?1",
            params![profile_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(to_string)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未保存 API Key，请填写 API Key 后点击保存。".to_string())?;
    let _ = write_api_key(profile_id, &db_api_key);
    Ok(db_api_key)
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
        rewritten_protagonist_name: row.get(2)?,
        additional_feminize_names: row.get(3)?,
        bust: row.get(4)?,
        body_type: row.get(5)?,
        rewrite_mode: row.get(6)?,
        advanced_settings: row.get(7)?,
        updated_at: row.get(8)?,
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

#[allow(clippy::too_many_arguments)]
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

fn diagnosis_check(name: &str, status: &str, message: &str) -> ModelDiagnosisCheck {
    ModelDiagnosisCheck {
        name: name.to_string(),
        status: status.to_string(),
        message: message.to_string(),
    }
}

fn build_model_diagnosis(
    checks: Vec<ModelDiagnosisCheck>,
    recommended_thinking_mode: Option<&str>,
) -> ModelDiagnosis {
    let status = if checks.iter().any(|check| check.status == "failed") {
        "failed"
    } else if checks.iter().any(|check| check.status == "warning") {
        "warning"
    } else {
        "ok"
    };
    ModelDiagnosis {
        status: status.to_string(),
        recommended_thinking_mode: recommended_thinking_mode.map(str::to_string),
        checks,
    }
}

fn append_diagnosis_log(
    state: &State<'_, AppState>,
    profile_id: &str,
    diagnosis: &ModelDiagnosis,
) -> Result<(), String> {
    let content = diagnosis
        .checks
        .iter()
        .map(|check| format!("- {} [{}] {}", check.name, check.status, check.message))
        .collect::<Vec<_>>()
        .join("\n");
    append_ai_log(
        state,
        None,
        profile_id,
        "模型诊断",
        None,
        if diagnosis.status == "failed" {
            "error"
        } else {
            "success"
        },
        &format!("诊断状态：{}\n{}", diagnosis.status, content),
        None,
        None,
    )
}

fn compact_log_line(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        format!("{}...", take_chars(&compact, max_chars))
    }
}

fn format_model_log_content(
    output: &ModelOutput,
    profile: &ModelProfile,
    review_enabled: Option<bool>,
) -> String {
    let review_label = match review_enabled {
        Some(true) => "开启",
        Some(false) => "关闭",
        None => "不适用",
    };
    format!(
        "调用统计：\n- 输入字符数：{}\n- 输出字符数：{}\n- AI 调用耗时：{:.2} 秒\n- 复检：{}\n- 思考模式：{}\n\n{}",
        output.input_chars,
        output.output_chars,
        output.elapsed_ms as f64 / 1000.0,
        review_label,
        profile.thinking_mode,
        output.text.trim()
    )
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        value.push_str("\n\n[由于上下文限制，本章后续内容已截断。]");
    }
    value
}
/// Extract the trailing JSON object or array from a text such as reasoning / thinking content.
/// When a model puts all output into reasoning_content and leaves content empty, the actual
/// structured output (review decision, analysis result, etc.) often appears as the last JSON block
/// inside the reasoning text.
fn extract_tailing_json_from_text(text: &str) -> Option<&str> {
    // Find the last candidate '{' and '[' positions.
    let last_brace = text.rfind('{');
    let last_bracket = text.rfind('[');
    // Try brace first (most review/analysis outputs are objects), then bracket.
    let mut candidates = Vec::new();
    if let Some(pos) = last_brace { candidates.push(pos); }
    if let Some(pos) = last_bracket { candidates.push(pos); }
    for start in candidates {
        let candidate = &text[start..];
        if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn normalize_jsonish(text: &str) -> String {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

fn parse_jsonish_value(text: &str) -> Result<serde_json::Value, String> {
if text.trim().is_empty() {
    return Err("AI 返回了空响应正文（模型可能将所有 token 消耗在思维链 reasoning 中，content 字段为空白，且无法从 reasoning 中提取到有效的 JSON）".to_string());
}

    let normalized = normalize_jsonish(text);
    match serde_json::from_str::<serde_json::Value>(&normalized) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            let repaired = escape_unescaped_json_control_chars(&normalized);
            if repaired != normalized {
                serde_json::from_str::<serde_json::Value>(&repaired).map_err(|second_error| {
                    format!("{}；修复控制字符后仍失败：{}", first_error, second_error)
                })
            } else {
                Err(first_error.to_string())
            }
        }
    }
}

fn escape_unescaped_json_control_chars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                output.push(ch);
                escaped = true;
            }
            '"' => {
                output.push(ch);
                in_string = !in_string;
            }
            '\n' if in_string => output.push_str("\\n"),
            '\r' if in_string => output.push_str("\\r"),
            '\t' if in_string => output.push_str("\\t"),
            ch if in_string && ch.is_control() => {
                output.push_str(&format!("\\u{:04X}", ch as u32));
            }
            _ => output.push(ch),
        }
    }

    output
}

fn normalize_name_list(input: &str) -> String {
    input
        .split(['\n', '\r', ',', '，', '、', ';', '；'])
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
    redact_sensitive_text(&error.to_string())
}

fn redact_sensitive_text(text: &str) -> String {
    let query_secret_re = Regex::new(r"(?i)([?&](?:key|api_key|access_token|token)=)[^&\s]+")
        .expect("valid secret query regex");
    let bearer_re =
        Regex::new(r"(?i)(authorization:\s*bearer\s+)[^\s,;]+").expect("valid bearer regex");
    let redacted = query_secret_re.replace_all(text, "${1}[REDACTED]");
    bearer_re
        .replace_all(&redacted, "${1}[REDACTED]")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chapter(index: i64, title: &str, original_text: &str) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: title.to_string(),
            original_text: original_text.to_string(),
            analysis_json: None,
            rewrite_text: None,
            analysis_status: "completed".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    #[test]
    fn batch_rewrite_markers_round_trip() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            "{}\n标题：第一章\n正文：\n改写一\n{}\n\n{}\n标题：第二章\n正文：\n改写二\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0]),
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert_eq!(parsed[0].index, 1);
        assert_eq!(parsed[0].title, "第一章");
        assert_eq!(parsed[0].text, "改写一");
        assert_eq!(parsed[1].id, "chapter-2");
        assert_eq!(parsed[1].index, 2);
        assert_eq!(parsed[1].title, "第二章");
        assert_eq!(parsed[1].text, "改写二");
    }

    #[test]
    fn batch_rewrite_parser_extracts_rewritten_title() {
        let chapters = vec![sample_chapter(1, "第一章 男儿志", "原文一")];
        let output = format!(
            "{}\n标题：第一章 少女志\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "第一章 少女志");
        assert_eq!(parsed[0].text, "改写一");
    }

    #[test]
    fn batch_rewrite_parser_accepts_marker_with_wrong_id_when_index_matches() {
        let chapters = vec![sample_chapter(4, "第四章", "原文四")];
        let output = "<<<YURI_REWRITE_CHAPTER_START index=4 id=model-made-up-id>>>\n标题：第四章\n正文：\n改写四\n<<<YURI_REWRITE_CHAPTER_END index=4 id=model-made-up-id>>>";

        let parsed = parse_batch_rewrite_output(output, &chapters)
            .expect("index-matched marker should recover from wrong id");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].index, 4);
        assert_eq!(parsed[0].text, "改写四");
    }

    #[test]
    fn batch_rewrite_parser_recovers_markerless_title_body_output() {
        let chapters = vec![
            sample_chapter(4, "第四章", "原文四"),
            sample_chapter(5, "第五章", "原文五"),
            sample_chapter(6, "第六章", "原文六"),
        ];
        let output = "标题：第四章\n正文：\n改写四\n\n标题：第五章\n正文：\n改写五\n\n标题：第六章\n正文：\n改写六";

        let parsed = parse_batch_rewrite_output(output, &chapters)
            .expect("title/body output should be used as fallback");

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].index, 4);
        assert_eq!(parsed[1].text, "改写五");
        assert_eq!(parsed[2].index, 6);
    }

    #[test]
    fn batch_rewrite_parser_ignores_non_marker_intro_before_first_marker() {
        let chapters = vec![sample_chapter(4, "第四章", "原文四")];
        let output = format!(
            "好的，以下是当前分片。\n\n{}\n标题：第四章\n正文：\n改写四\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters)
            .expect("non-marker intro should not break marker parsing");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "改写四");
    }

    #[test]
    fn batch_rewrite_parser_rejects_missing_or_out_of_order_markers() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let missing_second = format!(
            "{}\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_rewrite_output(&missing_second, &chapters).is_err());

        let out_of_order = format!(
            "{}\n正文：\n改写二\n{}\n\n{}\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1]),
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_rewrite_output(&out_of_order, &chapters).is_err());
    }

    #[test]
    fn batch_rewrite_parser_accepts_missing_end_marker_when_boundary_is_clear() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let missing_first_end = format!(
            "{}\n正文：\n改写一\n\n{}\n正文：\n改写二\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1])
        );
        let parsed = parse_batch_rewrite_output(&missing_first_end, &chapters)
            .expect("next start marker is enough to recover missing end marker");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "改写一");
        assert_eq!(parsed[1].text, "改写二");

        let missing_last_end = format!(
            "{}\n正文：\n改写一\n{}\n\n{}\n正文：\n改写二",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0]),
            chapter_start_marker(&chapters[1])
        );
        let parsed = parse_batch_rewrite_output(&missing_last_end, &chapters)
            .expect("final non-empty block is enough to recover missing final end marker");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].text, "改写二");
    }

    #[test]
    fn batch_rewrite_parser_ignores_extra_unexpected_chapters_after_expected_shard() {
        let expected = vec![
            sample_chapter(25, "第二十五章", "原文二十五"),
            sample_chapter(26, "第二十六章", "原文二十六"),
            sample_chapter(27, "第二十七章", "原文二十七"),
        ];
        let extra = [
            sample_chapter(28, "第二十八章", "原文二十八"),
            sample_chapter(29, "第二十九章", "原文二十九"),
            sample_chapter(30, "第三十章", "原文三十"),
        ];
        let mut output = String::new();
        for chapter in expected.iter().chain(extra.iter()) {
            output.push_str(&format!(
                "{}\n标题：{}\n正文：\n改写{}\n{}\n\n",
                chapter_start_marker(chapter),
                chapter.title,
                chapter.index,
                chapter_end_marker(chapter)
            ));
        }

        let parsed = parse_batch_rewrite_output(&output, &expected)
            .expect("extra unexpected chapter markers should be ignored after expected shard");

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].index, 25);
        assert_eq!(parsed[2].index, 27);
    }

    #[test]
    fn batch_analysis_markers_round_trip() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            "{}\n{{\"outline\":\"大纲一\",\"characters\":[\"萧炎\"],\"relationships\":[],\"locations\":[],\"foreshadowing\":[],\"terms\":[],\"names\":[\"萧炎\"]}}\n{}\n\n{}\n{{\"outline\":\"大纲二\",\"characters\":[\"药老\"],\"relationships\":[],\"locations\":[],\"foreshadowing\":[],\"terms\":[],\"names\":[\"药老\"]}}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            analysis_chapter_end_marker(&chapters[0]),
            analysis_chapter_start_marker(&chapters[1]),
            analysis_chapter_end_marker(&chapters[1])
        );

        let parsed = parse_batch_analysis_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("大纲一"));
        assert_eq!(parsed[1].id, "chapter-2");
        assert!(parsed[1].json.contains("大纲二"));
    }

    #[test]
    fn batch_analysis_json_output_round_trip_without_markers() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            r#"{{
  "chapters": [
    {{
      "index": 1,
      "id": "{}",
      "title": "第一章",
      "analysis": {{
        "outline": "大纲一",
        "characters": ["萧炎"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["萧炎"]
      }}
    }},
    {{
      "index": 2,
      "id": "{}",
      "title": "第二章",
      "analysis": {{
        "outline": "大纲二",
        "characters": ["药老"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["药老"]
      }}
    }}
  ]
}}"#,
            chapters[0].id, chapters[1].id
        );

        let parsed = parse_batch_analysis_output(&output, &chapters).expect("valid json output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("大纲一"));
        assert_eq!(parsed[1].id, "chapter-2");
        assert!(parsed[1].json.contains("大纲二"));
    }

    #[test]
    fn batch_analysis_json_output_accepts_batch_level_assets() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = r#"{
  "batch": {"start_index": 1, "end_index": 2, "chapter_count": 2},
  "outline": ["萧炎进入大厅并遇见药老。"],
  "characters": ["萧炎：少年。", "药老：神秘人物。"],
  "relationships": ["萧炎与药老建立联系。"],
  "locations": ["大厅"],
  "foreshadowing": ["药老身份仍有悬念。"],
  "terms": ["斗气"],
  "names": ["萧炎", "药老"]
}"#;

        let parsed =
            parse_batch_analysis_output(output, &chapters).expect("valid batch-level output");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("萧炎进入大厅"));
        assert!(parsed[0].json.contains("斗气"));
    }

    #[test]
    fn batch_analysis_json_output_repairs_control_chars_inside_strings() {
        let chapters = vec![sample_chapter(1, "第一章", "原文一")];
        let output = format!(
            r#"{{
  "chapters": [
    {{
      "index": 1,
      "id": "{}",
      "title": "第一章",
      "analysis": {{
        "outline": "第一行
第二行",
        "characters": ["萧炎	少年"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["萧炎"]
      }}
    }}
  ]
}}"#,
            chapters[0].id
        );

        let parsed = parse_batch_analysis_output(&output, &chapters)
            .expect("control characters inside JSON strings should be repaired");

        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].json.contains("\\n"));
        assert!(parsed[0].json.contains("\\t"));
    }

    #[test]
    fn batch_analysis_parser_rejects_missing_out_of_order_or_invalid_json() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let valid_first = "{\"outline\":\"大纲一\"}";
        let valid_second = "{\"outline\":\"大纲二\"}";
        let missing_second = format!(
            "{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            valid_first,
            analysis_chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_analysis_output(&missing_second, &chapters).is_err());

        let out_of_order = format!(
            "{}\n{}\n{}\n\n{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[1]),
            valid_second,
            analysis_chapter_end_marker(&chapters[1]),
            analysis_chapter_start_marker(&chapters[0]),
            valid_first,
            analysis_chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_analysis_output(&out_of_order, &chapters).is_err());

        let invalid_json = format!(
            "{}\nnot-json\n{}\n\n{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            analysis_chapter_end_marker(&chapters[0]),
            analysis_chapter_start_marker(&chapters[1]),
            valid_second,
            analysis_chapter_end_marker(&chapters[1])
        );
        assert!(parse_batch_analysis_output(&invalid_json, &chapters).is_err());
    }

    #[test]
    fn export_body_contains_only_completed_rewrites() {
        let mut completed = sample_chapter(1, "第一章", "不应导出的原文一");
        completed.rewrite_status = "completed".to_string();
        completed.rewrite_text = Some("已改写正文一".to_string());

        let mut pending = sample_chapter(2, "第二章", "不应导出的原文二");
        pending.rewrite_text = Some("未完成改写也不导出".to_string());

        let body =
            build_rewritten_export_body(&[completed, pending]).expect("has completed rewrite");

        assert!(body.contains("第一章"));
        assert!(body.contains("已改写正文一"));
        assert!(!body.contains("第二章"));
        assert!(!body.contains("不应导出的原文"));
        assert!(!body.contains("未完成改写也不导出"));
    }

    #[test]
    fn chinese_batch_label_formats_common_batch_indices() {
        assert_eq!(chinese_batch_label(1), "第一批");
        assert_eq!(chinese_batch_label(2), "第二批");
        assert_eq!(chinese_batch_label(10), "第十批");
        assert_eq!(chinese_batch_label(12), "第十二批");
        assert_eq!(chinese_batch_label(30), "第三十批");
    }

    #[test]
    fn analysis_prompt_does_not_include_rewrite_instructions() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let prompt = build_batch_analysis_prompt(&[chapter]);

        for forbidden in ["百合", "改写", "女性化", "代词替换", "双女主"] {
            assert!(
                !prompt.contains(forbidden),
                "prompt contains forbidden term: {forbidden}"
            );
        }
    }

    #[test]
    fn app_review_setting_defaults_off_and_can_be_enabled() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        assert!(!load_review_enabled(&conn).expect("load default review setting"));
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load default parallelism"),
            6
        );

        save_review_enabled(&conn, true).expect("enable review");
        assert!(load_review_enabled(&conn).expect("load enabled review setting"));

        save_review_enabled(&conn, false).expect("disable review");
        assert!(!load_review_enabled(&conn).expect("load disabled review setting"));

        save_rewrite_parallelism(&conn, 10).expect("save parallelism");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load parallelism"),
            10
        );
        save_rewrite_parallelism(&conn, 2).expect("normalize invalid parallelism");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load normalized parallelism"),
            6
        );
    }

    #[test]
    fn core_prompt_can_be_saved_loaded_and_cleared() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        assert_eq!(
            load_core_prompt(&conn).expect("load default core prompt"),
            ""
        );

        save_core_prompt(&conn, "  文风克制，动作描写细腻。  ").expect("save core prompt");
        assert_eq!(
            load_core_prompt(&conn).expect("load saved core prompt"),
            "文风克制，动作描写细腻。"
        );

        save_core_prompt(&conn, "   ").expect("clear core prompt");
        assert_eq!(
            load_core_prompt(&conn).expect("load cleared core prompt"),
            ""
        );
    }

    #[test]
    fn estimate_requests_include_analysis_rewrite_and_optional_review() {
        let chapters = (1..=30)
            .map(|idx| sample_chapter(idx, &format!("第{}章", idx), "原文"))
            .collect::<Vec<_>>();

        assert_eq!(estimate_requests_for_chapters(&chapters, 6, false), 12);
        assert_eq!(estimate_requests_for_chapters(&chapters, 6, true), 42);
        assert_eq!(estimate_requests_for_chapters(&chapters[..3], 10, true), 21);
        assert_eq!(estimate_requests_for_chapters(&[], 6, true), 0);
    }

    #[test]
    fn estimate_wait_stages_follow_pipeline_not_shard_count() {
        let chapters = (1..=30)
            .map(|idx| sample_chapter(idx, &format!("第{}章", idx), "原文"))
            .collect::<Vec<_>>();

        assert_eq!(split_chapters_for_parallelism(&chapters, 6).len(), 6);
        assert_eq!(estimate_wait_stages_for_chapters(&chapters, false), 2);
        assert_eq!(estimate_wait_stages_for_chapters(&chapters, true), 7);
        assert_eq!(estimate_wait_stages_for_chapters(&[], true), 0);
    }

    #[test]
    fn review_warning_file_appends_per_novel() {
        let temp_dir = env::temp_dir().join(format!("yuri-rewrite-warning-{}", Uuid::new_v4()));
        let app_dir = temp_dir.join("app");
        let data_dir = temp_dir.join("data");
        let second_decision = ReviewDecision {
            approved: false,
            issues: vec!["第二次仍缺少外貌描写".to_string()],
        };
        let third_decision = ReviewDecision {
            approved: false,
            issues: vec!["第三次仍未满足核心设定".to_string()],
        };

        let first_path = append_review_warning_file_for_title(
            &app_dir,
            &data_dir,
            "测试:小说",
            "第1-3章",
            &second_decision,
            &third_decision,
        );
        let second_path = append_review_warning_file_for_title(
            &app_dir,
            &data_dir,
            "测试:小说",
            "第4-6章",
            &second_decision,
            &third_decision,
        );

        assert_eq!(first_path, second_path);
        let content = fs::read_to_string(&first_path).expect("read warning log");
        assert!(content.contains("测试:小说"));
        assert!(content.contains("第1-3章"));
        assert!(content.contains("第4-6章"));
        assert!(content.contains("已保存第二次重写稿并继续处理后续分片"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn recent_model_stats_default_to_no_history() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        let stats = load_recent_model_stats(&conn, "missing-profile").expect("load stats");

        assert_eq!(stats.success_calls, 0);
        assert_eq!(stats.failed_calls, 0);
        assert_eq!(stats.average_call_seconds(), None);
        assert_eq!(stats.average_input_chars(), None);
        assert_eq!(stats.average_output_chars(), None);
    }

    #[test]
    fn recent_model_stats_parse_log_content() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, created_at) VALUES (?1, NULL, ?2, '测试', NULL, 'success', ?3, ?4)",
            params![
                "log-1",
                "profile-1",
                "调用统计：\n- 输入字符数：120\n- 输出字符数：30\n- AI 调用耗时：2.50 秒\n\n正文",
                Utc::now().to_rfc3339()
            ],
        )
        .expect("insert success log");
        conn.execute(
            "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, created_at) VALUES (?1, NULL, ?2, '测试', NULL, 'error', 'HTTP 401', ?3)",
            params!["log-2", "profile-1", Utc::now().to_rfc3339()],
        )
        .expect("insert error log");

        let stats = load_recent_model_stats(&conn, "profile-1").expect("load stats");

        assert_eq!(stats.success_calls, 1);
        assert_eq!(stats.failed_calls, 1);
        assert_eq!(stats.average_call_seconds(), Some(2.5));
        assert_eq!(stats.average_input_chars(), Some(120));
        assert_eq!(stats.average_output_chars(), Some(30));
    }

    #[test]
    fn model_diagnosis_status_uses_worst_check() {
        let ok = build_model_diagnosis(vec![diagnosis_check("连接", "ok", "ok")], None);
        assert_eq!(ok.status, "ok");

        let warning = build_model_diagnosis(
            vec![
                diagnosis_check("连接", "ok", "ok"),
                diagnosis_check("JSON", "warning", "unstable"),
            ],
            Some("auto"),
        );
        assert_eq!(warning.status, "warning");
        assert_eq!(warning.recommended_thinking_mode.as_deref(), Some("auto"));

        let failed = build_model_diagnosis(
            vec![
                diagnosis_check("连接", "warning", "slow"),
                diagnosis_check("API Key", "failed", "bad key"),
            ],
            None,
        );
        assert_eq!(failed.status, "failed");
    }

    #[test]
    fn rewrite_parallelism_splits_batch_into_contiguous_shards() {
        let chapters = (1..=30)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();

        let six = split_chapters_for_parallelism(&chapters, 6);
        assert_eq!(six.len(), 6);
        assert!(six.iter().all(|shard| shard.len() == 5));
        assert_eq!(six[0][0].index, 1);
        assert_eq!(six[5][4].index, 30);

        let three = split_chapters_for_parallelism(&chapters, 3);
        assert_eq!(three.len(), 3);
        assert!(three.iter().all(|shard| shard.len() == 10));

        let ten = split_chapters_for_parallelism(&chapters, 10);
        assert_eq!(ten.len(), 10);
        assert!(ten.iter().all(|shard| shard.len() == 3));

        let single = split_chapters_for_parallelism(&chapters, 1);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].len(), 30);
    }

    #[test]
    fn rewrite_recovery_split_halves_large_failed_shards() {
        let chapters = (1..=10)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();

        let (left, right) = split_chapters_for_rewrite_recovery(&chapters).expect("split ten");
        assert_eq!(left.len(), 5);
        assert_eq!(right.len(), 5);
        assert_eq!(left[0].index, 1);
        assert_eq!(right[0].index, 6);

        let (left, right) =
            split_chapters_for_rewrite_recovery(&chapters[..3]).expect("split three");
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 1);
        assert!(split_chapters_for_rewrite_recovery(&chapters[..1]).is_none());
    }

    #[test]
    fn shard_context_limits_model_to_current_shard_chapters() {
        let chapters = (25..=27)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();

        let context = format_shard_context(8, 10, 10, "第1-30章", &chapters);

        assert!(context.contains("分片 9/10"));
        assert!(context.contains("第25-27章"));
        assert!(context.contains("第25章、第26章、第27章"));
        assert!(context.contains("严禁输出本分片外的任何章节"));
    }

    #[test]
    fn canon_assets_are_compacted_before_rewrite_prompt() {
        let huge_content = (0..1_200)
            .map(|index| format!("人物设定行{index}：很长的一致性资产内容。"))
            .collect::<Vec<_>>()
            .join("\n");
        let assets = vec![CanonAsset {
            novel_id: "novel-1".to_string(),
            kind: "AI分析汇总".to_string(),
            content: huge_content.clone(),
            updated_at: "now".to_string(),
        }];

        let compact = build_compact_canon_text(&assets);

        assert!(compact.contains("AI分析汇总"));
        assert!(compact.contains("一致性资产已压缩"));
        assert!(compact.chars().count() < huge_content.chars().count());
        assert!(compact.chars().count() < 4_500);
    }

    #[test]
    fn rewrite_settings_prompt_includes_selected_rewrite_mode() {
        let strict_settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let mut creative_settings = strict_settings.clone();
        creative_settings.rewrite_mode = "creative".to_string();

        let strict_prompt = build_rewrite_settings_prompt(&strict_settings);
        let creative_prompt = build_rewrite_settings_prompt(&creative_settings);

        assert!(strict_prompt.contains("严谨模式"));
        assert!(strict_prompt.contains("更加忠于原文"));
        assert!(strict_prompt.contains("不对主角添加过多额外女性化描写"));
        assert!(strict_prompt.contains("章节标题和正文都必须检查主角姓名"));
        assert!(strict_prompt.contains("看不出主角改写前曾是男性"));
        assert!(strict_prompt.contains("男性化姓名、代词、称谓、身份、身体特征"));
        assert!(strict_prompt.contains("人物外貌特征必须前后一致"));
        assert!(strict_prompt.contains("上一章是金发，下一章不能无理由变成红发"));
        assert!(strict_prompt.contains("人物关系和百合向情绪推进必须连续"));
        assert!(strict_prompt.contains("只允许主角、用户填写的“其他需要女性化的人物姓名”"));
        assert!(strict_prompt.contains("其他未指定人物必须保持原文性别、身份、称谓和人称代词"));
        assert!(strict_prompt.contains("不得因为百合改写目标而把所有重要配角"));
        assert!(creative_prompt.contains("创意模式"));
        assert!(creative_prompt.contains("优先级高于普通的“中度再创作”约束"));
        assert!(creative_prompt.contains("每章都能明确感知主角已经从男性变为女性"));
        assert!(creative_prompt.contains("每章至少在关键场景中增加或强化 2-4 处女性化感知点"));
        assert!(creative_prompt.contains("同性亲密感和百合向情绪推进"));
    }

    #[test]
    fn batch_rewrite_prompt_puts_core_prompt_before_rewrite_rules() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let chapters = vec![sample_chapter(1, "第一章", "萧炎走进大厅。")];
        let prompt = build_batch_rewrite_prompt_with_context(
            &chapters,
            "一致性资产",
            &settings,
            "文风克制，动作描写细腻，保留轻小说对白节奏。",
            "",
        );

        let core_pos = prompt
            .find("最高优先级核心设定")
            .expect("core prompt section");
        let rewrite_pos = prompt.find("改写要求").expect("rewrite rules");
        assert!(core_pos < rewrite_pos);
        assert!(prompt.contains("文风克制，动作描写细腻"));
        assert!(prompt.contains("优先级高于本次改写中的其他风格"));
    }

    #[test]
    fn rewrite_settings_prompt_includes_forced_rewritten_protagonist_name() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };

        let prompt = build_rewrite_settings_prompt(&settings);

        assert!(prompt.contains("主角改写后姓名：萧妍"));
        assert!(prompt.contains("强制姓名规则"));
        assert!(prompt.contains("主角姓名必须统一为“萧妍”"));
        assert!(prompt.contains("不得自行改成其他姓名"));
    }

    #[test]
    fn name_mapping_asset_persists_forced_and_generated_names() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "林动\n唐三".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let content = build_name_mapping_asset_content(
            &settings,
            vec![
                NameMappingEntry {
                    source: "林动".to_string(),
                    target: "林彤".to_string(),
                },
                NameMappingEntry {
                    source: "唐三".to_string(),
                    target: fallback_feminized_name("唐三"),
                },
            ],
        )
        .expect("valid mapping content");
        let entries = parse_name_mapping_entries(&content);
        let prompt = build_rewrite_settings_prompt(&settings);

        assert!(content.contains("\"protagonist\""));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "萧炎" && entry.target == "萧妍"));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "林动" && entry.target == "林彤"));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "唐三" && entry.target == "唐姗"));
        assert!(prompt.contains("姓名映射表"));
        assert!(prompt.contains("并发分片和后续批次也必须继续使用同一份映射表"));
    }

    #[test]
    fn batch_rewrite_prompt_requires_yuri_and_appearance_consistency() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };

        let prompt = build_batch_rewrite_prompt_with_settings(
            &[chapter],
            "姓名映射表：萧炎 -> 萧妍",
            &settings,
        );

        assert!(prompt.contains("双女主百合叙事"));
        assert!(prompt.contains("清除所有原男性主角痕迹"));
        assert!(prompt.contains("人物外貌特征必须前后一致"));
        assert!(prompt.contains("上一章是金发，下一章不能无理由变成红发"));
        assert!(prompt.contains("百合向关系推进必须承接前文"));
        assert!(prompt.contains("不能突然重置或跳跃"));
        assert!(prompt.contains("其他配角、敌人、长辈、师父、兄弟、父亲、旁观者必须保持原文性别"));
        assert!(prompt.contains("原文男性继续使用男性代词/称谓"));
    }

    #[test]
    fn review_prompt_checks_creative_mode_strength() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "creative".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "萧妍走进大厅。".to_string(),
        };
        let prompt = build_batch_review_prompt_with_settings(&[chapter], &[rewrite], &settings);

        assert!(prompt.contains("如果当前为创意模式"));
        assert!(prompt.contains("只是替换姓名/代词"));
        assert!(prompt.contains("每章标题是否也完成女性化"));
        assert!(prompt.contains("女性外貌、神态、互动距离、称谓变化、百合向情绪张力"));
        assert!(prompt.contains("看不出主角原本是男性"));
        assert!(prompt.contains("人物外貌特征是否前后一致"));
        assert!(prompt.contains("百合向关系推进是否承接前文"));
        assert!(prompt.contains("不能为了强调性别而破坏原文战力"));
        assert!(
            prompt.contains("未指定性转的配角、敌人、长辈、师父、兄弟、父亲、旁观者是否被误改性别")
        );
        assert!(prompt.contains("同一人物在不同章节中的他/她"));
    }

    #[test]
    fn analysis_prompt_tracks_original_gender_pronouns_without_rewrite_rules() {
        let chapter = sample_chapter(1, "第一章", "萧炎和父亲说话，旁边的少女点头。");
        let prompt = build_batch_analysis_prompt(&[chapter]);

        assert!(prompt.contains("原文性别线索"));
        assert!(prompt.contains("原文人称代词"));
        assert!(prompt.contains("性别不明"));
        assert!(!prompt.contains("百合"));
        assert!(!prompt.contains("女性化"));
        assert!(!prompt.contains("代词替换"));
    }

    #[test]
    fn deepseek_detection_covers_official_and_proxy_configs() {
        let mut profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "DeepSeek".to_string(),
            provider: "OpenAI 兼容".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            model: "deepseek-chat".to_string(),
            temperature: 0.7,
            thinking_mode: "auto".to_string(),
            has_api_key: true,
            updated_at: "now".to_string(),
        };
        assert!(is_deepseek_profile(
            &profile,
            "https://api.deepseek.com/v1",
            "deepseek-chat"
        ));

        profile.base_url = "https://example-proxy.invalid/v1".to_string();
        profile.model = "deepseek-v4-pro".to_string();
        assert!(is_deepseek_profile(
            &profile,
            "https://example-proxy.invalid/v1",
            "deepseek-v4-pro"
        ));

        profile.model = "gpt-4o".to_string();
        assert!(!is_deepseek_profile(
            &profile,
            "https://example-proxy.invalid/v1",
            "gpt-4o"
        ));
    }

    #[test]
    fn thinking_mode_parameters_are_provider_specific() {
        let mut profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "OpenRouter".to_string(),
            provider: "openai-compatible".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "anthropic/claude-sonnet-4".to_string(),
            temperature: 0.7,
            thinking_mode: "off".to_string(),
            has_api_key: true,
            updated_at: "now".to_string(),
        };

        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["reasoning"]["effort"], "none");

        profile.base_url = "https://api.openai.com/v1".to_string();
        profile.model = "gpt-5.1".to_string();
        profile.thinking_mode = "on".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["reasoning_effort"], "medium");

        profile.base_url = "https://api.deepseek.com/v1".to_string();
        profile.model = "deepseek-v4-pro".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking"]["type"], "disabled");

        profile.base_url = "https://api.moonshot.ai/v1".to_string();
        profile.model = "kimi-k2.5".to_string();
        profile.thinking_mode = "on".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking"]["type"], "enabled");

        profile.base_url = "https://api.siliconflow.cn/v1".to_string();
        profile.model = "Qwen/Qwen3-235B-A22B-Thinking-2507".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking_budget"], 0);

        profile.provider = "gemini".to_string();
        profile.model = "gemini-2.5-flash".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({ "generationConfig": {} });
        assert!(apply_gemini_thinking_control(&mut payload, &profile));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
    }

    #[test]
    fn mimo_prompts_are_sanitized_to_reduce_content_filter_risk() {
        let profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "MiMo".to_string(),
            provider: "openai-compatible".to_string(),
            base_url: "https://api.xiaomimimo.com/v1".to_string(),
            model: "mimo-v2.5-pro".to_string(),
            temperature: 0.7,
            thinking_mode: "auto".to_string(),
            has_api_key: true,
            updated_at: "now".to_string(),
        };

        let (system, user) = prepare_prompt_for_profile(
            &profile,
            "双女主百合文本",
            "百合向关系、亲密互动暗示、身体描写、体型：萝莉、身材：巨乳、平胸",
        );

        assert!(system.contains("双女主百合文本"));
        assert!(user.contains("百合向关系"));
        assert!(user.contains("亲密互动暗示"));
        assert!(user.contains("身体描写"));
        assert!(user.contains("体型：娇小少女感"));
        assert!(user.contains("身形风格：成熟曲线"));
        assert!(user.contains("清瘦纤细"));
        assert!(!user.contains("巨乳"));
        assert!(!user.contains("萝莉"));
        assert!(!user.contains("平胸"));
    }

    #[test]
    fn openai_content_filter_response_is_reported_before_parsing() {
        let value = json!({
            "choices": [{
                "finish_reason": "content_filter",
                "message": {
                    "content": "The request was rejected because it was considered high risk"
                }
            }]
        });

        let error = openai_content_filter_error(&value, "mimo-v2.5-pro").expect("content filter");

        assert!(error.contains("模型内容安全策略拦截"));
        assert!(error.contains("mimo-v2.5-pro"));
        assert!(error.contains("content_filter"));
    }

    #[test]
    fn update_check_parses_release_redirect_url_without_api() {
        let tag =
            release_tag_from_url("https://github.com/3minto1/Yuri-Rewrite/releases/tag/v0.1.2")
                .expect("release tag");

        assert_eq!(tag, "v0.1.2");
        assert_eq!(
            portable_zip_name(&normalize_release_version(&tag)),
            "YuriRewrite-v0.1.2-windows-x64.zip"
        );
        assert!(is_newer_version("0.1.2", "0.1.1"));
        assert!(!is_newer_version("0.1.1", "0.1.1"));
    }

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
            "第1夜 雨中旧案",
            "第2案 无声证词",
            "第3场 天台重逢",
            "第4弹 她的反击",
            "第5折 灯下回身",
            "第6更 月色正好",
            "【特别篇】",
            "=== 第五章 娜儿 ===",
            "===楔子===",
            "===引言===",
            "序言",
            "序幕 神树之下",
            "番外篇 她们后来",
            "===番外 她们后来===",
        ] {
            assert!(
                heading_re.is_match(title),
                "expected heading match: {title}"
            );
        }
    }

    #[test]
    fn chapter_heading_regex_handles_windows_crlf_lines() {
        let text = "第1章 陨落的天才\r\n这里是第一章正文。\r\n第2章 斗气大陆\r\n这里是第二章正文。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第1章 陨落的天才");
        assert_eq!(split.chapters[1].title, "第2章 斗气大陆");
    }

    #[test]
    fn strict_chapter_headings_reject_loose_numbers_and_pure_symbols() {
        let heading_re = chapter_heading_regex();
        for title in [
            "1、这就是标题",
            "二十四、我瞎编的标题",
            "（11）我奶常山赵子龙",
            "====================================",
            "=== 起 ===",
        ] {
            assert!(
                !heading_re.is_match(title),
                "expected non-strict heading rejection: {title}"
            );
        }
    }

    #[test]
    fn loose_numbered_headings_are_used_only_without_standard_headings() {
        let text = "001 不应作为章节\n这一行会留在正文里。\n第1章 正式开始\n这里是内容甲\n第2章 继续推进\n这里是内容乙";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第1章 正式开始");
        assert!(split.chapters[0].original_text.contains("这里是内容甲"));
    }

    #[test]
    fn loose_numbered_headings_detect_sequential_numbered_titles() {
        let text = "001 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n002 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n003 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "001 初遇");
        assert_eq!(split.chapters[1].title, "002 再会");
        assert_eq!(split.chapters[2].title, "003 终局");
    }

    #[test]
    fn loose_numbered_headings_allow_punctuation_inside_titles() {
        let text = "1、遇事不决，量子力学\n这里是第一章正文，章节内容足够完整，不是普通列表项。\n2、巨型boss？更兴奋了\n这里是第二章正文，剧情继续推进并保持较长正文。\n3、不要完美，要夸张。\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "1、遇事不决，量子力学");
        assert_eq!(split.chapters[1].title, "2、巨型boss？更兴奋了");
        assert_eq!(split.chapters[2].title, "3、不要完美，要夸张。");
    }

    #[test]
    fn loose_numbered_headings_handle_windows_crlf_lines() {
        let text = "简介：\r\n这里是简介，可能包含主角名字。\r\n1、丧尸娘与不死美人的娇躯\r\n这里是第一章正文，章节内容足够完整，不是普通列表项。\r\n2、身为丧尸怎么能不吃人呢\r\n这里是第二章正文，剧情继续推进并保持较长正文。\r\n3、身材真是极品\r\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 4);
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[2].title, "2、身为丧尸怎么能不吃人呢");
    }

    #[test]
    fn loose_numbered_headings_are_used_when_only_volume_headings_are_strict() {
        let text = "第一卷\n1、丧尸娘与不死美人的娇躯\n这里是第一章正文，主角醒来后确认环境和身份，章节内容足够完整。\n2、身为丧尸怎么能不吃人呢\n这里是第二章正文，剧情继续推进，标题使用纯数字顿号格式。\n3、身材真是极品\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[1].title, "2、身为丧尸怎么能不吃人呢");
        assert_eq!(split.chapters[2].title, "3、身材真是极品");
    }

    #[test]
    fn loose_numbered_headings_ignore_volume_titles_and_prose_like_round_lines() {
        let text = "简介：\n作为堂堂魔族第一王女，只想每天喝喝小茶，看看风景，平稳度日。\n第1卷.新的归宿？\n1.败啦？\n这里是第一章正文，主角在战场上完成计划，章节内容足够完整。\n2.她似乎在撩我！\n这里是第二章正文，剧情继续推进，纯数字标题后面带有正式标题。\n第二回是手里的甜甜圈从手里滑出去然后手忙脚乱在空中惊险拦截避免落地。\n这是一句正文，不应该被识别成章节标题。\n3.过往\n这里是第三章正文，继续展开人物行动和场景变化。\n第一回魔力链接失败了，喊圣剑过来，西莱特菈说她正在睡觉休息。\n这也是正文里的普通句子。\n上架感言\n上架感言无需说的太多，今晚上架，大家可以提前点一下上架预订。\n4.火祸降罚\n这里是第四章正文，继续展开人物行动和场景变化。\n5.真实实力？\n这里是第五章正文，继续展开人物行动和场景变化。\n6.说到做到\n这里是第六章正文，继续展开人物行动和场景变化。\n7.懂了！\n这里是第七章正文，继续展开人物行动和场景变化。\n8.商讨\n这里是第八章正文，继续展开人物行动和场景变化。\n9.如何处置？\n这里是第九章正文，继续展开人物行动和场景变化。\n10.寒意\n这里是第十章正文，继续展开人物行动和场景变化。\n11.找到啦！\n这里是第十一章正文，继续展开人物行动和场景变化。\n12.湖与鱼\n这里是第十二章正文，继续展开人物行动和场景变化。\n第2卷.雪落花飞月盈时\n1.新卷开头\n这里是第二卷第一章正文，卷内编号重新开始但仍然是章节。\n2.雪落花飞\n这里是第二卷第二章正文，继续展开人物行动和场景变化。\n2023.7.3——2025.6.30\n这里是日期记录，不应该被识别成章节标题。\n0.说明\n这里是番外或插图说明，也可能需要保留为可处理章节。\n1.神明的邀约\n这里是附录第一章正文，编号再次重置也应该保留。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(split.chapters.len(), 17, "{titles:?}");
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1.败啦？");
        assert_eq!(split.chapters[2].title, "2.她似乎在撩我！");
        assert!(split.chapters[2]
            .original_text
            .contains("第二回是手里的甜甜圈"));
        assert!(split.chapters[3]
            .original_text
            .contains("第一回魔力链接失败了"));
        assert!(split.chapters[3]
            .original_text
            .contains("上架感言无需说的太多"));
        assert_eq!(split.chapters[13].title, "1.新卷开头");
        assert!(split.chapters[14].original_text.contains("2023.7.3"));
        assert_eq!(split.chapters[15].title, "0.说明");
        assert_eq!(split.chapters[16].title, "1.神明的邀约");
    }

    #[test]
    fn loose_numbered_headings_override_intro_volume_and_prose_like_special_matches() {
        let text = "简介：\n【变身+丧尸娘+系统+末世+自恋向】\n第一卷\n1、丧尸娘与不死美人的娇躯\n这里是第一章正文，主角醒来后确认环境和身份，章节内容足够完整。\n2、身为丧尸怎么能不吃人呢\n这里是第二章正文，剧情继续推进，标题使用纯数字顿号格式。\n上架感言\n看到评论区有读者在关心作者的精神状态，作者表示自己状态很好。\n3、身材真是极品\n这里是第三章正文，继续展开人物行动和场景变化。\n　　话一落音，只见手机屏幕上的内容就投影在了空气中，分辨率精细无比。\n4、比人类更加高等的生物\n这里是第四章正文，继续展开人物行动和场景变化。\n5、一分饱也算饱\n这里是第五章正文，继续展开人物行动和场景变化。\n6、真长舌妇\n这里是第六章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(split.chapters.len(), 7, "{titles:?}");
        assert_eq!(split.chapters[0].title, "简介：");
        assert!(split.chapters[0].original_text.contains("丧尸娘"));
        assert_eq!(split.chapters[1].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[2].title, "2、身为丧尸怎么能不吃人呢");
        assert!(split.chapters[2].original_text.contains("上架感言"));
        assert!(split.chapters[3].original_text.contains("话一落音"));
        assert_eq!(split.chapters[4].title, "4、比人类更加高等的生物");
    }

    #[test]
    fn loose_numbered_headings_allow_small_numbering_typos() {
        let text = "1、开局\n这里是第一章正文，章节内容足够完整，不是普通编号列表。\n2、推进\n这里是第二章正文，剧情继续推进并保持较长正文。\n3、转折\n这里是第三章正文，继续展开人物行动和场景变化。\n3、编号笔误\n这里是第四章正文，但是标题编号误写成了三，仍然应作为章节。\n5、收束\n这里是第五章正文，前文线索继续回收并形成完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 5);
        assert_eq!(split.chapters[3].title, "3、编号笔误");
        assert_eq!(split.chapters[4].title, "5、收束");
    }

    #[test]
    fn loose_numbered_headings_allow_single_digit_typo_inside_long_run() {
        let mut text = String::new();
        for idx in 1..=112 {
            let shown = if idx == 109 { 9 } else { idx };
            text.push_str(&format!("{shown}、第{idx}章标题\n"));
            text.push_str("这里是章节正文，内容长度足够说明它不是普通列表项，剧情持续推进。\n");
        }
        let split = split_chapters("novel-1", &text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 112);
        assert_eq!(split.chapters[108].title, "9、第109章标题");
        assert_eq!(split.chapters[109].title, "110、第110章标题");
    }

    #[test]
    fn loose_numbered_headings_handle_long_run_with_real_world_numbering_glitches() {
        let mut text =
            String::from("简介：\n这里是简介，可能包含主角名字，也应该进入改写流程。\n第一卷\n");
        for idx in 1..=441 {
            let shown = match idx {
                65 => 64,
                109 => 9,
                126 | 135 => continue,
                435 => 434,
                _ => idx,
            };
            text.push_str(&format!("{shown}、第{idx}章标题\n"));
            text.push_str("这里是章节正文，内容长度足够说明它不是普通列表项，剧情持续推进。\n");
        }
        let split = split_chapters("novel-1", &text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1、第1章标题");
        assert_eq!(split.chapters[65].title, "64、第65章标题");
        assert!(split
            .chapters
            .iter()
            .any(|chapter| chapter.title == "9、第109章标题"));
        assert_eq!(
            split.chapters.last().map(|chapter| chapter.title.as_str()),
            Some("441、第441章标题")
        );
    }

    #[test]
    fn loose_numbered_headings_detect_sequential_chinese_numbered_titles() {
        let text = "一 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n二 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n三 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "一 初遇");
        assert_eq!(split.chapters[1].title, "二 再会");
        assert_eq!(split.chapters[2].title, "三 终局");
    }

    #[test]
    fn loose_numbered_headings_reject_nonsequential_numbered_lists() {
        let text =
            "1 普通列表项\n这里是内容甲\n3 跳号列表项\n这里是内容乙\n7 又一个列表项\n这里是内容丙";
        let split = split_chapters("novel-1", text);

        assert!(!split.detected_chapters);
        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapters[0].title, "自动分段 1");
    }

    #[test]
    fn loose_numbered_headings_reject_sentence_like_numbered_lines() {
        let text = "一 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n二 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n三 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。\n五、六岁的孩子，自然没有什么男女之别，琅玡仍旧只顾着玩闹。\n后面还有普通正文，不能因为句首中文数字和顿号就切出新章节。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert!(split.chapters[2]
            .original_text
            .contains("五、六岁的孩子，自然没有什么男女之别"));
    }

    #[test]
    fn loose_numbered_headings_reject_symbol_separator_lines() {
        let text = "====================================\n正文开头有装饰分隔符。\n1 普通列表项\n这里是内容甲\n2 又一个列表项\n这里是内容乙";
        let split = split_chapters("novel-1", text);

        assert!(!split.detected_chapters);
        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapters[0].title, "自动分段 1");
    }

    #[test]
    fn special_headings_keep_preface_and_interleaved_extra_chapters() {
        let text = "===楔子===\n高大的树木茂密得连阳光也无法透入，这里是开篇内容。\n===第一章 觉醒日===\n第一章正文继续展开，主角正式登场。\n===番外 她们后来===\n番外正文穿插在正常章节之间，也应该作为独立章节保留。\n===第二章 武魂觉醒===\n第二章正文继续推进。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 4);
        assert_eq!(split.chapters[0].title, "===楔子===");
        assert!(split.chapters[0].original_text.contains("开篇内容"));
        assert_eq!(split.chapters[1].title, "===第一章 觉醒日===");
        assert_eq!(split.chapters[2].title, "===番外 她们后来===");
        assert!(split.chapters[2].original_text.contains("番外正文"));
        assert_eq!(split.chapters[3].title, "===第二章 武魂觉醒===");
    }

    #[test]
    fn strict_headings_reject_round_phrase_inside_body() {
        let text = "第415章 挑战\n第一回合的接触，武器便是被击落，这一幕即使看台上的人也怔住了。\n第416章 家传玉片\n真正的下一章正文。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第415章 挑战");
        assert!(split.chapters[0]
            .original_text
            .contains("第一回合的接触，武器便是被击落"));
        assert_eq!(split.chapters[1].title, "第416章 家传玉片");
    }

    #[test]
    fn postscript_heading_is_kept_but_postscript_body_sentence_is_not_split() {
        let text = "第417章 最后的选拔赛\n最后一章正文。\n后记。\n后记写到这里，也该是结束的时候了，可我还是有些舍不得。\n谢谢看到这里的读者。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第417章 最后的选拔赛");
        assert_eq!(split.chapters[1].title, "后记。");
        assert!(split.chapters[1]
            .original_text
            .contains("后记写到这里，也该是结束的时候了"));
    }

    #[test]
    fn extract_tailing_json_from_reasoning_content() {
        // Reasoning text with JSON object at the end
        let reasoning = "审查分析：改写稿基本合格。输出JSON。{\n  \"approved\": true,\n  \"summary\": \"通过\"\n}";
        let extracted = extract_tailing_json_from_text(reasoning)
            .expect("should extract trailing JSON object");
        let value: serde_json::Value =
            serde_json::from_str(extracted).expect("extracted text must be valid JSON");
        assert_eq!(value["approved"], true);
        assert_eq!(value["summary"], "通过");

        // No valid JSON anywhere should return None.
        let plain = "这是一段普通的思考文字，不包含任何 JSON 结构。";
        assert!(extract_tailing_json_from_text(plain).is_none());

        // JSON array at the end.
        let reasoning_array = "思考中...最终输出：[\n  {\"name\": \"萧炎\", \"gender\": \"male\"},\n  {\"name\": \"萧妍\", \"gender\": \"female\"}\n]";
        let extracted_array = extract_tailing_json_from_text(reasoning_array)
            .expect("should extract trailing JSON array");
        let value_array: serde_json::Value =
            serde_json::from_str(extracted_array).expect("extracted text must be valid JSON");
        assert_eq!(value_array.as_array().unwrap().len(), 2);

        // Empty text.
        assert!(extract_tailing_json_from_text("").is_none());
        assert!(extract_tailing_json_from_text("   ").is_none());
    }
}
