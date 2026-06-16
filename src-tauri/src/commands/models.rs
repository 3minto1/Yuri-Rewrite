use crate::ai::{generate_text, normalize_thinking_mode};
use crate::credentials::{delete_api_key_if_present, read_api_key, write_api_key, ApiKeyStorage};
use crate::domain::{AppState, ModelDiagnosis, ModelProfile, ModelProfileInput, ModelTestResult};
use crate::{
    api_key_storage, api_key_storage_from_values, append_ai_log, append_diagnosis_log,
    build_model_diagnosis, compact_log_line, diagnosis_check, format_model_log_content,
    load_model_profile, parse_jsonish_value, read_stored_api_key, to_string,
};
use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

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
        if write_api_key(&id, value).is_err() {
            db_api_key_fallback = Some(value.clone());
        }
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
        has_api_key: false,
        api_key_storage: ApiKeyStorage::None.as_str().to_string(),
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
    let existing_key = read_api_key(&profile_id).ok();
    delete_api_key_if_present(&profile_id)
        .map_err(|error| format!("删除系统凭据失败，模型配置未删除：{}", to_string(error)))?;
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(error) => {
            if let Some(api_key) = existing_key {
                let _ = write_api_key(&profile_id, &api_key);
            }
            return Err(to_string(error));
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
        tx.commit().map_err(to_string)?;
        Ok(())
    })();
    if let Err(error) = delete_result {
        if let Some(api_key) = existing_key {
            let _ = write_api_key(&profile_id, &api_key);
        }
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_model_profiles(state: State<AppState>) -> Result<Vec<ModelProfile>, String> {
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
