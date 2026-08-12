use crate::ai::{
    anthropic_models_endpoint, anthropic_request, gemini_models_endpoint, generate_text,
    normalize_thinking_mode, openai_models_endpoint,
};
use crate::commands::rewrite_ab::model_has_unfinished_rewrite_ab;
use crate::credentials::{
    combine_rollback_error, delete_api_key_if_present, restore_api_key_snapshot, snapshot_api_key,
    write_api_key, ApiKeySnapshot, ApiKeyStorage,
};
use crate::domain::{
    AppState, DiscoveredModel, ModelDiagnosis, ModelDiscoveryInput, ModelDiscoveryResult,
    ModelProfile, ModelProfileInput, ModelTestResult,
};
use crate::{
    api_key_storage, api_key_storage_from_values, append_ai_log, append_diagnosis_log,
    build_model_diagnosis, compact_log_line, diagnosis_check, format_model_log_content,
    load_model_profile, parse_jsonish_value, read_stored_api_key, to_string,
};
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::{RequestBuilder, Response, StatusCode, Url};
use rusqlite::params;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;
use tauri::State;
use uuid::Uuid;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const MODEL_DISCOVERY_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MODEL_DISCOVERY_MAX_MODELS: usize = 1000;

#[tauri::command]
pub(crate) fn save_model_profile(
    input: ModelProfileInput,
    state: State<AppState>,
) -> Result<ModelProfile, String> {
    if let Some(profile_id) = input.id.as_deref() {
        if state.active_tasks.profile_is_active(profile_id)? {
            return Err("当前模型正在被任务使用，任务结束前不能修改配置。".to_string());
        }
    }
    if !(0.0..=2.0).contains(&input.temperature) {
        return Err("Temperature 必须在 0 到 2 之间。".to_string());
    }
    if !(0.0..=1.0).contains(&input.top_p) {
        return Err("Top P 必须在 0 到 1 之间。".to_string());
    }
    let thinking_mode = normalize_thinking_mode(input.thinking_mode.as_deref())?;
    let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let updated_at = Utc::now().to_rfc3339();
    let api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "********")
        .map(str::to_string);
    let mut db_api_key_fallback = None;
    let mut system_credential_snapshot = None;
    if let Some(value) = &api_key {
        match snapshot_api_key(&id) {
            Ok(snapshot) => match write_api_key(&id, value) {
                Ok(()) => system_credential_snapshot = Some(snapshot),
                Err(write_error) => {
                    let restore_result = restore_api_key_snapshot(&id, &snapshot);
                    if matches!(snapshot, ApiKeySnapshot::Present(_)) || restore_result.is_err() {
                        return Err(combine_rollback_error(
                            format!("系统凭据写入失败，模型配置未保存：{write_error}"),
                            restore_result,
                            "恢复原系统凭据",
                        ));
                    }
                    db_api_key_fallback = Some(value.clone());
                }
            },
            Err(_) => {
                // Credential Manager may be unavailable. Keep the documented database fallback
                // without mutating an unknown system-credential state.
                db_api_key_fallback = Some(value.clone());
            }
        }
    }
    let conn = match state.conn.lock() {
        Ok(conn) => conn,
        Err(error) => {
            let database_error = to_string(error);
            return Err(match system_credential_snapshot.as_ref() {
                Some(snapshot) => combine_rollback_error(
                    database_error,
                    restore_api_key_snapshot(&id, snapshot),
                    "恢复原系统凭据",
                ),
                None => database_error,
            });
        }
    };
    let profile = ModelProfile {
        id: id.clone(),
        name: input.name,
        provider: input.provider,
        base_url: input.base_url,
        model: input.model,
        temperature: input.temperature,
        top_p: input.top_p,
        thinking_mode,
        prompt_obfuscation_enabled: input.prompt_obfuscation_enabled,
        has_api_key: false,
        api_key_storage: ApiKeyStorage::None.as_str().to_string(),
        updated_at,
    };

    let save_result = conn.execute(
        r#"
        INSERT INTO model_profiles (
            id, name, provider, base_url, model, temperature, top_p, thinking_mode,
            prompt_obfuscation_enabled, updated_at, api_key
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            provider = excluded.provider,
            base_url = excluded.base_url,
            model = excluded.model,
            temperature = excluded.temperature,
            top_p = excluded.top_p,
            thinking_mode = excluded.thinking_mode,
            prompt_obfuscation_enabled = excluded.prompt_obfuscation_enabled,
            updated_at = excluded.updated_at,
            api_key = CASE
                WHEN ?11 IS NOT NULL THEN excluded.api_key
                WHEN ?12 IS NOT NULL THEN NULL
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
            profile.top_p,
            profile.thinking_mode,
            profile.prompt_obfuscation_enabled,
            profile.updated_at,
            db_api_key_fallback,
            api_key
        ],
    );
    if let Err(error) = save_result {
        let database_error = to_string(error);
        return Err(match system_credential_snapshot.as_ref() {
            Some(snapshot) => combine_rollback_error(
                database_error,
                restore_api_key_snapshot(&id, snapshot),
                "恢复原系统凭据",
            ),
            None => database_error,
        });
    }
    let storage = api_key_storage(&conn, &id);
    let mut profile = profile;
    profile.has_api_key = storage != ApiKeyStorage::None;
    profile.api_key_storage = storage.as_str().to_string();
    Ok(profile)
}

#[tauri::command]
pub(crate) fn delete_model_profile(
    profile_id: String,
    state: State<AppState>,
) -> Result<(), String> {
    let paused_auto_run_uses_profile = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .values()
        .any(|control| control.profile_ids.contains(&profile_id));
    if state.active_tasks.profile_is_active(&profile_id)? || paused_auto_run_uses_profile {
        return Err("当前模型正在被任务使用，请等待任务结束或先终止任务。".to_string());
    }
    {
        let conn = state.conn.lock().map_err(to_string)?;
        if model_has_unfinished_rewrite_ab(&conn, &profile_id)? {
            return Err(
                "当前模型仍被未完成的 A/B 实验使用，请先完成、重试或删除该实验。".to_string(),
            );
        }
    }
    let credential_snapshot = snapshot_api_key(&profile_id)
        .map_err(|error| format!("读取系统凭据失败，模型配置未删除：{error}"))?;
    delete_api_key_if_present(&profile_id)
        .map_err(|error| format!("删除系统凭据失败，模型配置未删除：{}", to_string(error)))?;
    let mut conn = match state.conn.lock() {
        Ok(conn) => conn,
        Err(error) => {
            return Err(combine_rollback_error(
                to_string(error),
                restore_api_key_snapshot(&profile_id, &credential_snapshot),
                "恢复系统凭据",
            ));
        }
    };
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(error) => {
            return Err(combine_rollback_error(
                to_string(error),
                restore_api_key_snapshot(&profile_id, &credential_snapshot),
                "恢复系统凭据",
            ));
        }
    };
    let delete_result = (|| -> Result<(), String> {
        tx.execute(
            "DELETE FROM model_profiles WHERE id = ?1",
            params![profile_id],
        )
        .map_err(to_string)?;
        tx.execute(
            "DELETE FROM ai_logs WHERE profile_id = ?1",
            params![profile_id],
        )
        .map_err(to_string)?;
        tx.execute(
            "DELETE FROM app_settings
             WHERE key IN ('selected_profile_id', 'review_profile_id', 'analysis_profile_id')
               AND value = ?1",
            params![profile_id],
        )
        .map_err(to_string)?;
        tx.commit().map_err(to_string)?;
        Ok(())
    })();
    if let Err(error) = delete_result {
        return Err(combine_rollback_error(
            error,
            restore_api_key_snapshot(&profile_id, &credential_snapshot),
            "恢复系统凭据",
        ));
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_model_profiles(state: State<AppState>) -> Result<Vec<ModelProfile>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider, base_url, model, temperature, top_p, thinking_mode,
                    prompt_obfuscation_enabled, updated_at, api_key
             FROM model_profiles ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let profiles = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let db_api_key: Option<String> = row.get(10)?;
            let storage = api_key_storage_from_values(&id, db_api_key.as_deref());
            Ok(ModelProfile {
                has_api_key: storage != ApiKeyStorage::None,
                api_key_storage: storage.as_str().to_string(),
                id,
                name: row.get(1)?,
                provider: row.get(2)?,
                base_url: row.get(3)?,
                model: row.get(4)?,
                temperature: row.get(5)?,
                top_p: row.get(6)?,
                thinking_mode: row.get(7)?,
                prompt_obfuscation_enabled: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(profiles)
}

#[tauri::command]
pub(crate) async fn discover_models(
    input: ModelDiscoveryInput,
    state: State<'_, AppState>,
) -> Result<ModelDiscoveryResult, String> {
    let provider = input.provider.trim().to_ascii_lowercase();
    if !matches!(
        provider.as_str(),
        "openai-compatible" | "anthropic" | "gemini"
    ) {
        return Err("不支持当前 Provider，无法自动获取模型。".to_string());
    }
    validate_discovery_base_url(&input.base_url)?;
    let draft_api_key = usable_draft_api_key(input.api_key.as_deref());
    let api_key = match draft_api_key {
        Some(value) => value.to_string(),
        None => {
            let profile_id = input
                .profile_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "请先填写 API Key，或选择一个已保存凭据的模型配置。".to_string())?;
            read_stored_api_key(&state, profile_id)
                .map_err(|_| "无法读取已保存的 API Key，请重新填写后再试。".to_string())?
        }
    };

    let discovery = async {
        match provider.as_str() {
            "openai-compatible" => {
                discover_openai_models(&state.client, &input.base_url, &api_key).await
            }
            "anthropic" => {
                discover_anthropic_models(&state.client, &input.base_url, &api_key).await
            }
            "gemini" => discover_gemini_models(&state.client, &input.base_url, &api_key).await,
            _ => unreachable!(),
        }
    };
    let result = tokio::time::timeout(MODEL_DISCOVERY_TIMEOUT, discovery)
        .await
        .map_err(|_| "获取模型列表超过 20 秒，已停止请求。请检查网络或 Base URL。".to_string())??;
    if result.models.is_empty() {
        return Err("连接成功，但账号可见模型列表为空。请确认该凭据拥有模型列表权限，或继续手工填写模型名。".to_string());
    }
    Ok(result)
}

fn usable_draft_api_key(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "********")
}

fn validate_discovery_base_url(base_url: &str) -> Result<(), String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("请先填写 Base URL。".to_string());
    }
    let parsed = Url::parse(trimmed).map_err(|_| "Base URL 格式不正确。".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Base URL 必须是有效的 HTTP 或 HTTPS 地址。".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Base URL 不能包含用户名或密码，请改用 API Key 字段。".to_string());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Base URL 不能包含查询参数或片段。".to_string());
    }
    Ok(())
}

async fn discover_openai_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<ModelDiscoveryResult, String> {
    let endpoint = openai_models_endpoint(base_url);
    let value = send_discovery_request(
        openai_discovery_request(client, endpoint, api_key),
        "OpenAI 兼容",
        api_key,
    )
    .await?;
    let models = parse_openai_models(&value)?;
    Ok(finalize_models(models, false))
}

async fn discover_anthropic_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<ModelDiscoveryResult, String> {
    let endpoint = anthropic_models_endpoint(base_url);
    let mut after_id: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut models = Vec::new();
    let mut truncated = false;
    loop {
        let mut url = Url::parse(&endpoint)
            .map_err(|_| "Base URL 无法构造 Anthropic 模型列表地址。".to_string())?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("limit", "100");
            if let Some(cursor) = after_id.as_deref() {
                query.append_pair("after_id", cursor);
            }
        }
        let value = send_discovery_request(
            anthropic_request(client.get(url), base_url, api_key),
            "Anthropic",
            api_key,
        )
        .await?;
        let page = parse_anthropic_models(&value)?;
        models.extend(page.models);
        if models.len() >= MODEL_DISCOVERY_MAX_MODELS {
            truncated = page.has_more || models.len() > MODEL_DISCOVERY_MAX_MODELS;
            break;
        }
        if !page.has_more {
            break;
        }
        let cursor = page
            .last_id
            .ok_or_else(|| "Anthropic 模型列表声明还有下一页，但没有返回分页游标。".to_string())?;
        if !seen_cursors.insert(cursor.clone()) {
            return Err("Anthropic 模型列表返回了重复分页游标，已停止获取。".to_string());
        }
        after_id = Some(cursor);
    }
    Ok(finalize_models(models, truncated))
}

async fn discover_gemini_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<ModelDiscoveryResult, String> {
    let endpoint = gemini_models_endpoint(base_url);
    let mut page_token: Option<String> = None;
    let mut seen_tokens = HashSet::new();
    let mut models = Vec::new();
    let mut truncated = false;
    loop {
        let mut url = Url::parse(&endpoint)
            .map_err(|_| "Base URL 无法构造 Gemini 模型列表地址。".to_string())?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("pageSize", "1000");
            if let Some(token) = page_token.as_deref() {
                query.append_pair("pageToken", token);
            }
        }
        let value = send_discovery_request(
            gemini_discovery_request(client, url, api_key),
            "Gemini",
            api_key,
        )
        .await?;
        let page = parse_gemini_models(&value)?;
        models.extend(page.models);
        if models.len() >= MODEL_DISCOVERY_MAX_MODELS {
            truncated = page.next_page_token.is_some() || models.len() > MODEL_DISCOVERY_MAX_MODELS;
            break;
        }
        let Some(token) = page.next_page_token else {
            break;
        };
        if !seen_tokens.insert(token.clone()) {
            return Err("Gemini 模型列表返回了重复分页游标，已停止获取。".to_string());
        }
        page_token = Some(token);
    }
    Ok(finalize_models(models, truncated))
}

fn openai_discovery_request(
    client: &reqwest::Client,
    endpoint: impl reqwest::IntoUrl,
    api_key: &str,
) -> RequestBuilder {
    client.get(endpoint).bearer_auth(api_key.trim())
}

fn gemini_discovery_request(
    client: &reqwest::Client,
    endpoint: impl reqwest::IntoUrl,
    api_key: &str,
) -> RequestBuilder {
    client
        .get(endpoint)
        .header("x-goog-api-key", api_key.trim())
}

async fn send_discovery_request(
    request: RequestBuilder,
    provider_label: &str,
    api_key: &str,
) -> Result<Value, String> {
    let response = request
        .timeout(MODEL_DISCOVERY_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                format!("{provider_label} 模型列表请求超时，请检查网络或 Base URL。")
            } else if error.is_connect() {
                format!("无法连接 {provider_label} 模型列表接口，请检查网络或 Base URL。")
            } else {
                format!("{provider_label} 模型列表请求失败，请检查网络和接口配置。")
            }
        })?;
    parse_discovery_response(response, provider_label, api_key).await
}

async fn parse_discovery_response(
    response: Response,
    provider_label: &str,
    api_key: &str,
) -> Result<Value, String> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length > MODEL_DISCOVERY_MAX_BODY_BYTES as u64)
    {
        return Err(format!("{provider_label} 模型列表响应过大，已停止读取。"));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| format!("读取 {provider_label} 模型列表响应失败。"))?;
        if body.len().saturating_add(chunk.len()) > MODEL_DISCOVERY_MAX_BODY_BYTES {
            return Err(format!("{provider_label} 模型列表响应过大，已停止读取。"));
        }
        body.extend_from_slice(&chunk);
    }
    let body_text = String::from_utf8_lossy(&body);
    if !status.is_success() {
        return Err(discovery_status_error(
            status,
            provider_label,
            &body_text,
            api_key,
        ));
    }
    serde_json::from_slice(&body).map_err(|_| format!("{provider_label} 模型列表返回了无效 JSON。"))
}

fn discovery_status_error(
    status: StatusCode,
    provider_label: &str,
    body: &str,
    api_key: &str,
) -> String {
    let summary = safe_response_excerpt(body, api_key);
    let base = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            format!("{provider_label} 拒绝了凭据或模型列表权限（HTTP {status}）。")
        }
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => format!(
            "当前 {provider_label} 服务不支持自动获取模型，或 Base URL 不正确（HTTP {status}）。请继续手工填写模型名。"
        ),
        StatusCode::TOO_MANY_REQUESTS => {
            format!("{provider_label} 模型列表请求过于频繁（HTTP 429），请稍后重试。")
        }
        _ => format!("{provider_label} 模型列表请求失败（HTTP {status}）。"),
    };
    if summary.is_empty() {
        base
    } else {
        format!("{base} 服务商响应：{summary}")
    }
}

fn safe_response_excerpt(body: &str, api_key: &str) -> String {
    let redacted = if api_key.trim().is_empty() {
        body.to_string()
    } else {
        body.replace(api_key.trim(), "[已隐藏凭据]")
    };
    let compact = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(500).collect()
}

fn discovered_model(
    id: &str,
    display_name: Option<&str>,
    owner: Option<&str>,
) -> Option<DiscoveredModel> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    Some(DiscoveredModel {
        id: id.to_string(),
        display_name: display_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        owner: owner
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn parse_openai_models(value: &Value) -> Result<Vec<DiscoveredModel>, String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "OpenAI 兼容模型列表响应缺少 data 数组，无法识别模型。".to_string())?;
    Ok(data
        .iter()
        .filter_map(|item| {
            discovered_model(
                item.get("id")?.as_str()?,
                item.get("display_name")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str),
                item.get("owned_by")
                    .or_else(|| item.get("owner"))
                    .and_then(Value::as_str),
            )
        })
        .collect())
}

struct AnthropicModelPage {
    models: Vec<DiscoveredModel>,
    has_more: bool,
    last_id: Option<String>,
}

fn parse_anthropic_models(value: &Value) -> Result<AnthropicModelPage, String> {
    let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        "Anthropic 模型列表响应缺少 data 数组，第三方兼容服务可能不支持自动获取。".to_string()
    })?;
    let models = data
        .iter()
        .filter_map(|item| {
            discovered_model(
                item.get("id")?.as_str()?,
                item.get("display_name")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str),
                item.get("owned_by")
                    .or_else(|| item.get("owner"))
                    .and_then(Value::as_str),
            )
        })
        .collect::<Vec<_>>();
    let last_id = value
        .get("last_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| models.last().map(|model| model.id.clone()));
    Ok(AnthropicModelPage {
        models,
        has_more: value
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        last_id,
    })
}

struct GeminiModelPage {
    models: Vec<DiscoveredModel>,
    next_page_token: Option<String>,
}

fn parse_gemini_models(value: &Value) -> Result<GeminiModelPage, String> {
    let data = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "Gemini 模型列表响应缺少 models 数组，无法识别模型。".to_string())?;
    let models = data
        .iter()
        .filter(|item| {
            item.get("supportedGenerationMethods")
                .and_then(Value::as_array)
                .is_some_and(|methods| {
                    methods
                        .iter()
                        .any(|method| method.as_str() == Some("generateContent"))
                })
        })
        .filter_map(|item| {
            let id = item
                .get("name")?
                .as_str()?
                .strip_prefix("models/")
                .unwrap_or(item.get("name")?.as_str()?);
            discovered_model(
                id,
                item.get("displayName").and_then(Value::as_str),
                Some("Google"),
            )
        })
        .collect();
    Ok(GeminiModelPage {
        models,
        next_page_token: value
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn finalize_models(models: Vec<DiscoveredModel>, truncated: bool) -> ModelDiscoveryResult {
    let mut unique = BTreeMap::new();
    for model in models {
        unique.entry(model.id.clone()).or_insert(model);
    }
    let exceeded_limit = unique.len() > MODEL_DISCOVERY_MAX_MODELS;
    let models = unique
        .into_values()
        .take(MODEL_DISCOVERY_MAX_MODELS)
        .collect();
    let warnings = if truncated || exceeded_limit {
        vec![format!(
            "账号可见模型超过 {MODEL_DISCOVERY_MAX_MODELS} 项，仅显示排序后的前 {MODEL_DISCOVERY_MAX_MODELS} 项。"
        )]
    } else {
        Vec::new()
    };
    ModelDiscoveryResult { models, warnings }
}

#[tauri::command]
pub(crate) async fn test_model_profile(
    profile_id: String,
    state: State<'_, AppState>,
) -> Result<ModelTestResult, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    match generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
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
pub(crate) async fn diagnose_model_profile(
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
        Some(state.rate_limits.clone()),
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
        Some(state.rate_limits.clone()),
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

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn draft_key_ignores_empty_and_masked_values() {
        assert_eq!(
            usable_draft_api_key(Some("  draft-secret  ")),
            Some("draft-secret")
        );
        assert_eq!(usable_draft_api_key(Some("********")), None);
        assert_eq!(usable_draft_api_key(Some("  ")), None);
        assert_eq!(usable_draft_api_key(None), None);
    }

    #[test]
    fn discovery_requests_use_provider_auth_headers() {
        let client = reqwest::Client::new();
        let openai = openai_discovery_request(&client, "https://example.com/v1/models", "secret")
            .build()
            .expect("OpenAI request");
        assert_eq!(
            openai
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer secret")
        );

        let gemini =
            gemini_discovery_request(&client, "https://example.com/v1beta/models", "secret")
                .build()
                .expect("Gemini request");
        assert_eq!(
            gemini
                .headers()
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("secret")
        );
    }

    #[test]
    fn parses_and_sorts_openai_models_without_guessing_capabilities() {
        let parsed = parse_openai_models(&json!({
            "data": [
                {"id": "z-model", "owned_by": "account"},
                {"id": "a-model", "name": "Alpha"},
                {"id": "a-model", "name": "Duplicate"}
            ]
        }))
        .expect("valid models");
        let result = finalize_models(parsed, false);
        assert_eq!(
            result
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a-model", "z-model"]
        );
        assert_eq!(result.models[0].display_name.as_deref(), Some("Alpha"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn parses_anthropic_pagination_metadata() {
        let page = parse_anthropic_models(&json!({
            "data": [{"id": "claude-test", "display_name": "Claude Test"}],
            "has_more": true,
            "last_id": "claude-test"
        }))
        .expect("valid Anthropic page");
        assert!(page.has_more);
        assert_eq!(page.last_id.as_deref(), Some("claude-test"));
        assert_eq!(page.models[0].display_name.as_deref(), Some("Claude Test"));
    }

    #[test]
    fn filters_gemini_models_by_generate_content_support() {
        let page = parse_gemini_models(&json!({
            "models": [
                {
                    "name": "models/gemini-generate",
                    "displayName": "Gemini Generate",
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/embedding-only",
                    "supportedGenerationMethods": ["embedContent"]
                }
            ],
            "nextPageToken": "next"
        }))
        .expect("valid Gemini page");
        assert_eq!(page.models.len(), 1);
        assert_eq!(page.models[0].id, "gemini-generate");
        assert_eq!(page.next_page_token.as_deref(), Some("next"));
    }

    #[test]
    fn caps_results_and_redacts_credentials_from_errors() {
        let models = (0..1005)
            .map(|index| DiscoveredModel {
                id: format!("model-{index:04}"),
                display_name: None,
                owner: None,
            })
            .collect();
        let result = finalize_models(models, false);
        assert_eq!(result.models.len(), MODEL_DISCOVERY_MAX_MODELS);
        assert_eq!(result.warnings.len(), 1);

        let error = discovery_status_error(
            StatusCode::UNAUTHORIZED,
            "OpenAI 兼容",
            r#"{"error":"secret-key rejected"}"#,
            "secret-key",
        );
        assert!(!error.contains("secret-key"));
        assert!(error.contains("[已隐藏凭据]"));
    }

    #[test]
    fn reports_common_http_failures_in_chinese() {
        assert!(
            discovery_status_error(StatusCode::NOT_FOUND, "Anthropic", "", "key")
                .contains("不支持自动获取模型")
        );
        assert!(
            discovery_status_error(StatusCode::TOO_MANY_REQUESTS, "Gemini", "", "key")
                .contains("请求过于频繁")
        );
    }
}
