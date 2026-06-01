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
struct ExportResult {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CanonAssetInput {
    kind: String,
    content: String,
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
            save_model_profile,
            list_model_profiles,
            test_model_profile,
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

        CREATE INDEX IF NOT EXISTS idx_chapters_novel ON chapters(novel_id, chapter_index);
        CREATE INDEX IF NOT EXISTS idx_jobs_novel ON jobs(novel_id, created_at);
        "#,
    )
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
    let chapters = split_chapters(&novel.id, &text);
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![novel.id, novel.title, novel.source_path, novel.encoding, novel.status, novel.created_at],
    )
    .map_err(to_string)?;

    for chapter in chapters {
        conn.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
            params![chapter.id, chapter.novel_id, chapter.index, chapter.title, chapter.original_text],
        )
        .map_err(to_string)?;
    }

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
    let canon_assets = load_canon_assets(&conn, &novel.id)?;
    Ok(NovelDetail {
        novel,
        chapters,
        canon_assets,
    })
}

#[tauri::command]
fn save_model_profile(input: ModelProfileInput, state: State<AppState>) -> Result<ModelProfile, String> {
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let updated_at = Utc::now().to_rfc3339();
    let profile = ModelProfile {
        id: id.clone(),
        name: input.name,
        provider: input.provider,
        base_url: input.base_url,
        model: input.model,
        temperature: input.temperature,
        has_api_key: input.api_key.as_ref().is_some_and(|value| !value.trim().is_empty())
            || read_api_key(&id).is_ok(),
        updated_at,
    };

    if let Some(api_key) = input.api_key {
        if !api_key.trim().is_empty() {
            write_api_key(&id, &api_key)?;
        }
    }

    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        r#"
        INSERT INTO model_profiles (id, name, provider, base_url, model, temperature, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            base_url = excluded.base_url,
            model = excluded.model,
            temperature = excluded.temperature,
            updated_at = excluded.updated_at
        "#,
        params![
            profile.id,
            profile.name,
            profile.provider,
            profile.base_url,
            profile.model,
            profile.temperature,
            profile.updated_at
        ],
    )
    .map_err(to_string)?;

    Ok(profile)
}

#[tauri::command]
fn list_model_profiles(state: State<AppState>) -> Result<Vec<ModelProfile>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider, base_url, model, temperature, updated_at FROM model_profiles ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let profiles = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(ModelProfile {
            has_api_key: read_api_key(&id).is_ok(),
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
async fn test_model_profile(profile_id: String, state: State<'_, AppState>) -> Result<ModelTestResult, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_api_key(&profile.id)?;
    match generate_text(
        &state.client,
        &profile,
        &api_key,
        "你是一个连接测试助手。只回复一句中文。",
        "请回复：连接成功。",
    )
    .await
    {
        Ok(text) => Ok(ModelTestResult {
            ok: true,
            message: text,
        }),
        Err(error) => Ok(ModelTestResult {
            ok: false,
            message: error,
        }),
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
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_api_key(&profile.id)?;
    let chapters = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_chapters(&conn, &novel_id)?
    };
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
            Ok(text) => {
                let normalized = normalize_jsonish(&text);
                let conn = state.conn.lock().map_err(to_string)?;
                conn.execute(
                    "UPDATE chapters SET analysis_json = ?1, analysis_status = 'completed' WHERE id = ?2",
                    params![normalized, chapter.id],
                )
                .map_err(to_string)?;
                merge_analysis_into_canon(&conn, &novel_id).map_err(to_string)?;
            }
            Err(error) => {
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
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_api_key(&profile.id)?;
    let (chapters, canon_assets) = {
        let conn = state.conn.lock().map_err(to_string)?;
        (load_chapters(&conn, &novel_id)?, load_canon_assets(&conn, &novel_id)?)
    };
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
        let prompt = build_rewrite_prompt(&chapter, &canon_text);
        match generate_text(
            &state.client,
            &profile,
            &api_key,
            "你是中文小说改写助手，任务是把男女主文本改写为自然的双女主百合文本。只输出改写后的正文。",
            &prompt,
        )
        .await
        {
            Ok(text) => {
                let conn = state.conn.lock().map_err(to_string)?;
                conn.execute(
                    "UPDATE chapters SET rewrite_text = ?1, rewrite_status = 'completed' WHERE id = ?2",
                    params![text.trim(), chapter.id],
                )
                .map_err(to_string)?;
            }
            Err(error) => {
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
    let safe_title = sanitize_file_name(&novel.title);
    let extension = if format == "markdown" { "md" } else { "txt" };
    let output_path = state
        .data_dir
        .join("exports")
        .join(format!("{}-rewrite.{}", safe_title, extension));
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

fn split_chapters(novel_id: &str, text: &str) -> Vec<Chapter> {
    let heading_re =
        Regex::new(r"(?m)^\s*((第[0-9零〇一二两三四五六七八九十百千万]+[章节回卷部].*)|(Chapter\s+[0-9IVXLCDM]+.*))\s*$")
            .expect("valid chapter regex");
    let matches = heading_re.find_iter(text).collect::<Vec<_>>();
    if matches.is_empty() {
        return chunk_without_headings(novel_id, text);
    }

    let mut chapters = Vec::new();
    for (idx, mat) in matches.iter().enumerate() {
        let start = mat.start();
        let content_start = mat.end();
        let end = matches.get(idx + 1).map_or(text.len(), |next| next.start());
        let title = text[start..content_start].trim().to_string();
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
    chapters
}

fn chunk_without_headings(novel_id: &str, text: &str) -> Vec<Chapter> {
    let chars = text.chars().collect::<Vec<_>>();
    let chunk_size = 8_000;
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
        "SELECT id, name, provider, base_url, model, temperature, updated_at FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| {
            let id: String = row.get(0)?;
            Ok(ModelProfile {
                has_api_key: read_api_key(&id).is_ok(),
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
) -> Result<String, String> {
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
) -> Result<String, String> {
    let base = profile.base_url.trim().trim_end_matches('/');
    let endpoint = if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{}/chat/completions", base)
    };
    let payload = json!({
        "model": profile.model,
        "temperature": profile.temperature,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let value: serde_json::Value = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?
        .error_for_status()
        .map_err(to_string)?
        .json()
        .await
        .map_err(to_string)?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("模型响应缺少 choices[0].message.content: {}", value))
}

async fn generate_gemini(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let base = if profile.base_url.trim().is_empty() {
        "https://generativelanguage.googleapis.com/v1beta".to_string()
    } else {
        profile.base_url.trim().trim_end_matches('/').to_string()
    };
    let endpoint = format!("{}/models/{}:generateContent?key={}", base, profile.model, api_key);
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
    let value: serde_json::Value = client
        .post(endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(to_string)?
        .error_for_status()
        .map_err(to_string)?
        .json()
        .await
        .map_err(to_string)?;
    value["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("Gemini 响应缺少正文: {}", value))
}

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
