use crate::domain::{AppState, ModelOutput, ModelProfile};
use crate::model_support::ModelResponseError;
use crate::rate_limit::{
    is_rate_limit_retry_exhausted, parse_retry_after, RateLimitCoordinator, RateLimitScope,
    MAX_RATE_LIMIT_RETRIES, RATE_LIMIT_RETRY_EXHAUSTED,
};
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

pub(crate) fn is_doubao_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("doubao")
        || provider.contains("volcengine")
        || provider.contains("volces")
        || provider.contains("bytedance")
        || provider.contains("火山")
        || base.contains("volcengine")
        || base.contains("volces")
        || base.contains("ark.cn-")
        || model.contains("doubao")
        || model.contains("seed-")
}

pub(crate) fn is_openai_official_profile(profile: &ModelProfile, base_url: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    base.contains("api.openai.com")
        || base.contains("openai.azure.com")
        || (provider == "openai" && !base.contains("compatible"))
}

pub(crate) fn is_kimi_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("kimi")
        || provider.contains("moonshot")
        || base.contains("kimi")
        || base.contains("moonshot")
        || model.contains("kimi-")
        || model.contains("moonshot")
}

pub(crate) fn is_siliconflow_profile(profile: &ModelProfile, base_url: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    provider.contains("siliconflow") || base.contains("siliconflow")
}

pub(crate) fn is_zhipu_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("zhipu")
        || provider.contains("bigmodel")
        || provider.contains("智谱")
        || base.contains("bigmodel")
        || base.contains("zhipu")
        || base.contains("z.ai")
        || model.starts_with("glm-")
}

pub(crate) fn is_minimax_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("minimax")
        || base.contains("minimax")
        || model.contains("minimax")
        || model == "m2-her"
        || model.starts_with("m2-")
}

pub(crate) fn is_claude_profile(profile: &ModelProfile, base_url: &str, model: &str) -> bool {
    let provider = profile.provider.to_ascii_lowercase();
    let base = base_url.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    provider.contains("anthropic")
        || provider.contains("claude")
        || base.contains("anthropic")
        || model.starts_with("claude-")
}

fn siliconflow_model_supports_json_mode(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    if model.contains("deepseek-r1") {
        return false;
    }
    if model.contains("deepseek-v3") {
        return model.contains("v3.1") || model.contains("v3.2");
    }
    true
}

fn permissive_json_schema_response_format() -> serde_json::Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": "yuri_rewrite_json_response",
            "schema": {
                "type": "object",
                "properties": {},
                "additionalProperties": true
            }
        }
    })
}

pub(crate) fn apply_openai_compatible_output_limit(
    payload: &mut serde_json::Value,
    profile: &ModelProfile,
    base_url: &str,
    model: &str,
    prefer_json_output: bool,
) -> bool {
    let output_limit = if prefer_json_output { 16_384 } else { 65_536 };

    if is_deepseek_profile(profile, base_url, model) {
        payload["max_tokens"] = json!(output_limit);
        return true;
    }

    if is_mimo_profile(profile)
        || is_doubao_profile(profile, base_url, model)
        || is_openai_official_profile(profile, base_url)
        || is_kimi_profile(profile, base_url, model)
        || is_minimax_profile(profile, base_url, model)
    {
        payload["max_completion_tokens"] = json!(output_limit);
        return true;
    }

    if is_zhipu_profile(profile, base_url, model)
        || is_siliconflow_profile(profile, base_url)
        || is_claude_profile(profile, base_url, model)
    {
        payload["max_tokens"] = json!(output_limit);
        return true;
    }

    false
}

pub(crate) fn openai_compatible_json_response_format(
    profile: &ModelProfile,
    base_url: &str,
    model: &str,
) -> Option<serde_json::Value> {
    if is_doubao_profile(profile, base_url, model) {
        return Some(permissive_json_schema_response_format());
    }

    if is_openai_official_profile(profile, base_url) || is_kimi_profile(profile, base_url, model) {
        return Some(permissive_json_schema_response_format());
    }

    if is_siliconflow_profile(profile, base_url) {
        return siliconflow_model_supports_json_mode(model)
            .then(|| json!({ "type": "json_object" }));
    }

    if is_zhipu_profile(profile, base_url, model) {
        return Some(json!({ "type": "json_object" }));
    }

    if is_deepseek_profile(profile, base_url, model) {
        return Some(json!({ "type": "json_object" }));
    }

    None
}

pub(crate) fn apply_gemini_json_response_format(
    payload: &mut serde_json::Value,
    prefer_json_output: bool,
) -> bool {
    if !prefer_json_output {
        return false;
    }
    payload["generationConfig"]["responseMimeType"] = json!("application/json");
    true
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
    } else if is_zhipu_profile(profile, &profile.base_url, &profile.model) {
        (
            sanitize_prompt_for_zhipu(system),
            sanitize_prompt_for_zhipu(user),
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

pub(crate) fn sanitize_prompt_for_zhipu(text: &str) -> String {
    let replacements = [("身材：巨乳", "身材：丰满"), ("巨乳", "身材丰满")];
    let mut sanitized = text.to_string();
    for (from, to) in replacements {
        sanitized = sanitized.replace(from, to);
    }
    sanitized
}

pub(crate) async fn generate_text(
    client: &Client,
    rate_limiter: Option<RateLimitCoordinator>,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
    prefer_json_output: bool,
) -> Result<ModelOutput, String> {
    let scope = RateLimitScope::for_profile(profile);
    let mut rate_limit_attempts = 0usize;
    loop {
        if let Some(rate_limiter) = rate_limiter.as_ref() {
            if let Some(delay) = rate_limiter.cooldown_delay(&scope)? {
                tokio::time::sleep(delay).await;
            }
        }
        match generate_text_once(client, profile, api_key, system, user, prefer_json_output).await {
            Ok(output) => {
                if let Some(rate_limiter) = rate_limiter.as_ref() {
                    rate_limiter.record_success(&scope)?;
                }
                return Ok(output);
            }
            Err(error) if error.is_rate_limited() => {
                if rate_limit_attempts >= MAX_RATE_LIMIT_RETRIES {
                    return Err(format!(
                        "{}：{}。请降低并发、等待额度恢复或更换模型后重试。",
                        RATE_LIMIT_RETRY_EXHAUSTED, error
                    ));
                }
                rate_limit_attempts += 1;
                if let Some(rate_limiter) = rate_limiter.as_ref() {
                    let _ = rate_limiter.record_rate_limit(
                        &scope,
                        error.retry_after(),
                        rate_limit_attempts,
                    )?;
                } else {
                    let delay =
                        crate::rate_limit::default_backoff_delay(&scope, rate_limit_attempts);
                    tokio::time::sleep(delay).await;
                }
            }
            Err(error) => {
                let message = error.to_string();
                if is_rate_limit_retry_exhausted(&message) {
                    return Err(message);
                }
                return Err(message);
            }
        }
    }
}

async fn generate_text_once(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
    prefer_json_output: bool,
) -> Result<ModelOutput, ModelResponseError> {
    let started = Instant::now();
    let (system, user) = prepare_prompt_for_profile(profile, system, user);
    let input_chars = system.chars().count() + user.chars().count();
    let mut output = if profile.provider.to_lowercase().contains("gemini") {
        super::gemini::generate_gemini(client, profile, api_key, &system, &user, prefer_json_output)
            .await
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
    let retry_after = parse_retry_after(
        response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
    );
    let body = response
        .text()
        .await
        .map_err(|error| ModelResponseError::other(format_request_error(error)))?;
    if !status.is_success() {
        return Err(ModelResponseError::provider(
            status.as_u16(),
            compact_error_body(&body),
            retry_after,
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
        "模型请求超时（最长等待 20 分钟），请检查网络或降低单次处理量。".to_string()
    } else if error.is_connect() {
        format!("无法连接模型服务：{}", to_string(error))
    } else {
        to_string(error)
    }
}

pub(crate) fn is_recoverable_network_error(message: &str) -> bool {
    let trimmed = message.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("HTTP ")
        || trimmed.contains("模型请求超时")
        || trimmed.to_ascii_lowercase().contains("timeout")
        || trimmed.to_ascii_lowercase().contains("timed out")
    {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.contains("无法连接模型服务")
        || lower.contains("error sending request")
        || lower.contains("connection closed")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("dns error")
        || lower.contains("failed to lookup address")
        || lower.contains("tcp connect error")
        || lower.contains("network error")
        || lower.contains("operation was aborted")
        || trimmed.contains("远程主机强迫关闭")
        || trimmed.contains("连接被重置")
        || trimmed.contains("连接已关闭")
        || trimmed.contains("连接失败")
}

pub(crate) fn is_recoverable_model_format_error(message: &str) -> bool {
    [
        "分析输出格式多次修复后仍无法解析",
        "分析输出格式修复重试调用失败",
        "审查决策无法解析",
        "格式修复重试后仍失败",
        "格式修复重试调用失败",
    ]
    .iter()
    .any(|needle| message.contains(needle))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(provider: &str, base_url: &str, model: &str) -> ModelProfile {
        ModelProfile {
            id: "profile-1".to_string(),
            name: "测试模型".to_string(),
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            temperature: 0.7,
            thinking_mode: "auto".to_string(),
            has_api_key: true,
            api_key_storage: "system".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn output_limit_uses_deepseek_max_tokens() {
        let profile = profile("DeepSeek", "https://api.deepseek.com", "deepseek-v4-flash");
        let mut payload = json!({});
        assert!(apply_openai_compatible_output_limit(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model,
            false
        ));
        assert_eq!(payload["max_tokens"], json!(65_536));
        assert!(payload.get("max_completion_tokens").is_none());
    }

    #[test]
    fn output_limit_uses_completion_tokens_for_mimo_and_doubao() {
        for profile in [
            profile("MiMo", "https://api.xiaomimimo.com/v1", "mimo-v2.5-pro"),
            profile(
                "OpenAI 兼容",
                "https://ark.cn-beijing.volces.com/api/v3",
                "doubao-seed-2-0-lite-260428",
            ),
        ] {
            let mut payload = json!({});
            assert!(apply_openai_compatible_output_limit(
                &mut payload,
                &profile,
                &profile.base_url,
                &profile.model,
                false
            ));
            assert_eq!(payload["max_completion_tokens"], json!(65_536));
            assert!(payload.get("max_tokens").is_none());
        }
    }

    #[test]
    fn output_limit_uses_completion_tokens_for_openai_kimi_and_minimax() {
        for profile in [
            profile("OpenAI", "https://api.openai.com/v1", "gpt-5"),
            profile("OpenAI 兼容", "https://api.moonshot.cn/v1", "kimi-k2.6"),
            profile("MiniMax", "https://api.minimax.io/v1", "MiniMax-M2.7"),
        ] {
            let mut payload = json!({});
            assert!(apply_openai_compatible_output_limit(
                &mut payload,
                &profile,
                &profile.base_url,
                &profile.model,
                false
            ));
            assert_eq!(payload["max_completion_tokens"], json!(65_536));
            assert!(payload.get("max_tokens").is_none());
        }
    }

    #[test]
    fn output_limit_uses_max_tokens_for_zhipu_siliconflow_and_claude() {
        for profile in [
            profile("智谱", "https://open.bigmodel.cn/api/paas/v4", "glm-5.2"),
            profile(
                "OpenAI 兼容",
                "https://api.siliconflow.cn/v1",
                "Qwen/Qwen3.5-122B-A10B",
            ),
            profile(
                "Claude",
                "https://api.anthropic.com/v1",
                "claude-sonnet-4-6",
            ),
        ] {
            let mut payload = json!({});
            assert!(apply_openai_compatible_output_limit(
                &mut payload,
                &profile,
                &profile.base_url,
                &profile.model,
                false
            ));
            assert_eq!(payload["max_tokens"], json!(65_536));
            assert!(payload.get("max_completion_tokens").is_none());
        }
    }

    #[test]
    fn json_output_limit_is_smaller() {
        let profile = profile("DeepSeek", "https://api.deepseek.com", "deepseek-v4-pro");
        let mut payload = json!({});
        assert!(apply_openai_compatible_output_limit(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model,
            true
        ));
        assert_eq!(payload["max_tokens"], json!(16_384));
    }

    #[test]
    fn output_limit_does_not_affect_unknown_openai_compatible_provider() {
        let profile = profile("OpenAI 兼容", "https://example.com/v1", "some-model");
        let mut payload = json!({});
        assert!(!apply_openai_compatible_output_limit(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model,
            false
        ));
        assert_eq!(payload, json!({}));
    }

    #[test]
    fn json_response_format_uses_schema_for_doubao() {
        let profile = profile(
            "OpenAI 兼容",
            "https://ark.cn-beijing.volces.com/api/v3",
            "doubao-seed-2-0-lite-260428",
        );
        let response_format =
            openai_compatible_json_response_format(&profile, &profile.base_url, &profile.model)
                .expect("doubao supports structured json output");

        assert_eq!(response_format["type"], json!("json_schema"));
        assert_eq!(
            response_format["json_schema"]["name"],
            json!("yuri_rewrite_json_response")
        );
        assert_eq!(
            response_format["json_schema"]["schema"]["type"],
            json!("object")
        );
    }

    #[test]
    fn json_response_format_uses_schema_for_openai_and_kimi() {
        for profile in [
            profile("OpenAI 兼容", "https://api.openai.com/v1", "gpt-5"),
            profile("OpenAI 兼容", "https://api.moonshot.cn/v1", "kimi-k2.6"),
        ] {
            let response_format =
                openai_compatible_json_response_format(&profile, &profile.base_url, &profile.model)
                    .expect("provider supports structured json output");

            assert_eq!(response_format["type"], json!("json_schema"));
            assert_eq!(
                response_format["json_schema"]["schema"]["additionalProperties"],
                json!(true)
            );
        }
    }

    #[test]
    fn json_response_format_keeps_deepseek_json_object() {
        let profile = profile("DeepSeek", "https://api.deepseek.com", "deepseek-v4-pro");
        let response_format =
            openai_compatible_json_response_format(&profile, &profile.base_url, &profile.model)
                .expect("deepseek json object response format");

        assert_eq!(response_format, json!({ "type": "json_object" }));
    }

    #[test]
    fn json_response_format_uses_json_object_for_zhipu() {
        let profile = profile("智谱", "https://open.bigmodel.cn/api/paas/v4", "glm-5.2");
        let response_format =
            openai_compatible_json_response_format(&profile, &profile.base_url, &profile.model)
                .expect("zhipu supports json object response format");

        assert_eq!(response_format, json!({ "type": "json_object" }));
    }

    #[test]
    fn json_response_format_uses_json_object_for_supported_siliconflow_models() {
        let profile = profile(
            "OpenAI 兼容",
            "https://api.siliconflow.cn/v1",
            "Qwen/Qwen3.5-122B-A10B",
        );
        let response_format =
            openai_compatible_json_response_format(&profile, &profile.base_url, &profile.model)
                .expect("siliconflow model supports json mode");

        assert_eq!(response_format, json!({ "type": "json_object" }));
    }

    #[test]
    fn json_response_format_skips_siliconflow_deepseek_r1_and_v3_base_models() {
        for model in ["deepseek-ai/DeepSeek-R1", "deepseek-ai/DeepSeek-V3"] {
            let profile = profile("OpenAI 兼容", "https://api.siliconflow.cn/v1", model);
            assert!(openai_compatible_json_response_format(
                &profile,
                &profile.base_url,
                &profile.model
            )
            .is_none());
        }

        let profile = profile(
            "OpenAI 兼容",
            "https://api.siliconflow.cn/v1",
            "deepseek-ai/DeepSeek-V3.2",
        );
        assert!(openai_compatible_json_response_format(
            &profile,
            &profile.base_url,
            &profile.model
        )
        .is_some());
    }

    #[test]
    fn gemini_json_response_format_is_applied_only_for_json_requests() {
        let mut payload = json!({ "generationConfig": { "temperature": 0.7 } });
        assert!(!apply_gemini_json_response_format(&mut payload, false));
        assert!(payload["generationConfig"]
            .get("responseMimeType")
            .is_none());

        assert!(apply_gemini_json_response_format(&mut payload, true));
        assert_eq!(
            payload["generationConfig"]["responseMimeType"],
            json!("application/json")
        );
    }

    #[test]
    fn json_response_format_is_not_added_for_unknown_provider() {
        let profile = profile("OpenAI 兼容", "https://example.com/v1", "some-model");
        assert!(openai_compatible_json_response_format(
            &profile,
            &profile.base_url,
            &profile.model
        )
        .is_none());
    }

    #[test]
    fn zhipu_prompt_sanitization_softens_sensitive_body_terms() {
        let profile = profile("智谱", "https://open.bigmodel.cn/api/paas/v4", "glm-5.2");
        let (system, user) = prepare_prompt_for_profile(
            &profile,
            "系统规则：巨乳设定需要自然处理",
            "身材：巨乳\n请改写为百合文本",
        );

        assert_eq!(system, "系统规则：身材丰满设定需要自然处理");
        assert_eq!(user, "身材：丰满\n请改写为百合文本");
    }

    #[test]
    fn model_format_errors_are_recoverable_for_auto_run() {
        assert!(is_recoverable_model_format_error(
            "分析输出格式多次修复后仍无法解析：control character"
        ));
        assert!(is_recoverable_model_format_error(
            "分析输出格式修复重试调用失败：HTTP 500"
        ));
        assert!(is_recoverable_model_format_error(
            "第1-30章：审查决策无法解析：expected value；格式修复重试后仍失败：expected `,`"
        ));
        assert!(is_recoverable_model_format_error(
            "第1-30章：审查决策无法解析：expected value；格式修复重试调用失败：timeout"
        ));
        assert!(!is_recoverable_model_format_error(
            "AI 输出缺少章节结束标记"
        ));
        assert!(!is_recoverable_model_format_error("HTTP 401: unauthorized"));
    }
}
