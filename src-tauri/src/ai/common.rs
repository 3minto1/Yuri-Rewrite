use crate::domain::{AppState, ModelOutput, ModelProfile};
use crate::model_support::ModelResponseError;
use crate::{
    extract_tailing_json_from_text, load_model_profile, normalize_review_profile_id,
    read_stored_api_key, to_string,
};
use reqwest::Client;
use serde_json::json;
use std::time::Instant;
use tauri::State;

pub(crate) fn is_mimo_profile(profile: &ModelProfile) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = profile.base_url.to_ascii_lowercase();
    let model = profile.model.to_ascii_lowercase();
    provider.contains("mimo")
        || provider.contains("xiaomi")
        || base.contains("mimo")
        || base.contains("xiaomi")
        || model.contains("mimo-")
}

pub(crate) fn load_review_profile_for_run(
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

pub(crate) fn prepare_prompt_for_profile(
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

pub(crate) fn sanitize_prompt_for_mimo(text: &str) -> String {
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

pub(crate) async fn generate_text(
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
        super::gemini::generate_gemini(client, profile, api_key, &system, &user).await
    } else {
        super::openai::generate_openai_compatible(
            client,
            profile,
            api_key,
            &system,
            &user,
            prefer_json_output,
        )
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

pub(crate) async fn response_json_or_error(
    response: reqwest::Response,
) -> Result<(serde_json::Value, String), ModelResponseError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ModelResponseError::other(format_request_error(error)))?;
    if !status.is_success() {
        return Err(ModelResponseError::provider(
            status.as_u16(),
            compact_error_body(&body),
        ));
    }
    let value = serde_json::from_str(&body).map_err(|error| {
        ModelResponseError::other(format!(
            "模型响应不是合法 JSON: {}；原始响应：{}",
            error, body
        ))
    })?;
    Ok((value, body))
}

pub(crate) fn format_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "模型请求超时（最长等待 15 分钟），请检查网络或降低单次处理量。".to_string()
    } else if error.is_connect() {
        format!("无法连接模型服务：{}", to_string(error))
    } else {
        to_string(error)
    }
}

pub(crate) fn openai_content_filter_error(
    value: &serde_json::Value,
    model: &str,
) -> Option<String> {
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

pub(crate) fn compact_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "响应体为空".to_string();
    }
    serde_json::from_str::<serde_json::Value>(trimmed)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

pub(crate) fn normalize_model_name(base_url: &str, model: &str) -> String {
    let trimmed = model.trim();
    if base_url.to_ascii_lowercase().contains("api.deepseek.com") {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn normalize_thinking_mode(input: Option<&str>) -> Result<String, String> {
    let mode = input.unwrap_or("auto").trim().to_ascii_lowercase();
    match mode.as_str() {
        "" | "auto" => Ok("auto".to_string()),
        "off" => Ok("off".to_string()),
        "on" => Ok("on".to_string()),
        _ => Err("思考模式只能是 auto、off 或 on。".to_string()),
    }
}

pub(crate) fn is_deepseek_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("deepseek") || base.contains("deepseek") || model.contains("deepseek")
}

pub(crate) fn is_kimi_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("kimi")
        || provider.contains("moonshot")
        || base.contains("moonshot")
        || base.contains("kimi")
        || model.contains("kimi")
}

pub(crate) fn is_siliconflow_profile(profile: &ModelProfile, base_url: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    provider.contains("siliconflow") || base.contains("siliconflow")
}

pub(crate) fn apply_openai_compatible_thinking_control(
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

pub(crate) fn apply_reasoning_parameter(
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

pub(crate) fn is_openai_reasoning_model(model: &str) -> bool {
    matches!(
        model,
        value if value.starts_with("o1")
            || value.starts_with("o3")
            || value.starts_with("o4")
            || value.starts_with("gpt-5")
    )
}

pub(crate) fn apply_gemini_thinking_control(
    payload: &mut serde_json::Value,
    profile: &ModelProfile,
) -> bool {
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
