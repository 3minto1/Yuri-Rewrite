use super::common::*;
use crate::domain::{ModelOutput, ModelProfile};
use crate::model_support::ModelResponseError;
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Value};

pub(crate) async fn generate_anthropic(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
    prefer_json_output: bool,
    output_limit_override: Option<usize>,
) -> Result<ModelOutput, ModelResponseError> {
    let base = profile.base_url.trim().trim_end_matches('/');
    let model = normalize_model_name(base, &profile.model);
    let endpoint = anthropic_messages_endpoint(base);
    let output_limit =
        output_limit_override.unwrap_or(if prefer_json_output { 16_384 } else { 65_536 });
    let mut payload = json!({
        "model": model,
        "max_tokens": output_limit,
        "system": system,
        "messages": [{"role": "user", "content": user}]
    });

    if anthropic_model_accepts_sampling(&model) {
        payload["temperature"] = json!(profile.temperature);
        if profile.top_p < 1.0 {
            payload["top_p"] = json!(profile.top_p);
        }
    }
    let added_thinking_control =
        apply_anthropic_thinking_control(&mut payload, profile, base, &model);
    if added_thinking_control {
        payload
            .as_object_mut()
            .expect("payload is an object")
            .remove("temperature");
        payload
            .as_object_mut()
            .expect("payload is an object")
            .remove("top_p");
    }

    let response = anthropic_request(client.post(&endpoint), base, api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| ModelResponseError::other(format_request_error(error)))?;
    let mut retried_without_thinking = false;
    let (value, raw_response) = match response_json_or_error(response).await {
        Ok(result) => result,
        Err(error) if added_thinking_control && error.permits_thinking_retry() => {
            let mut retry_payload = payload;
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("thinking");
            retry_payload
                .as_object_mut()
                .expect("payload is an object")
                .remove("output_config");
            let retry_response = anthropic_request(client.post(&endpoint), base, api_key)
                .json(&retry_payload)
                .send()
                .await
                .map_err(|retry_error| {
                    ModelResponseError::other(format_request_error(retry_error))
                })?;
            let result = response_json_or_error(retry_response)
                .await
                .map_err(|retry_error| {
                    ModelResponseError::other(format!(
                        "{}；移除思考模式参数重试后仍失败：{}",
                        error, retry_error
                    ))
                })?;
            retried_without_thinking = true;
            result
        }
        Err(error) => return Err(error),
    };
    let (text, reasoning) = parse_anthropic_content(&value)?;
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

fn anthropic_messages_endpoint(base: &str) -> String {
    let normalized = base.trim_end_matches('/');
    if normalized.ends_with("/v1/messages") || normalized.ends_with("/messages") {
        normalized.to_string()
    } else if normalized.ends_with("/v1") {
        format!("{normalized}/messages")
    } else {
        format!("{normalized}/v1/messages")
    }
}

fn anthropic_request(request: RequestBuilder, base: &str, api_key: &str) -> RequestBuilder {
    let request = request
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", api_key.trim());
    if base.to_ascii_lowercase().contains("api.anthropic.com") {
        request
    } else {
        request.bearer_auth(api_key.trim())
    }
}

fn anthropic_model_accepts_sampling(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    !(model.starts_with("claude-opus-4-7") || model.starts_with("claude-opus-4-8"))
}

fn apply_anthropic_thinking_control(
    payload: &mut Value,
    profile: &ModelProfile,
    base_url: &str,
    model: &str,
) -> bool {
    let enabled = match profile.thinking_mode.as_str() {
        "off" => false,
        "on" => true,
        _ => return false,
    };
    let model_lower = model.to_ascii_lowercase();

    if model_lower.starts_with("claude-opus-4-7")
        || model_lower.starts_with("claude-opus-4-8")
        || model_lower.starts_with("claude-opus-4-6")
        || model_lower.starts_with("claude-sonnet-4-6")
    {
        if enabled {
            payload["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
            payload["output_config"] = json!({ "effort": "high" });
            return true;
        }
        return false;
    }

    if model_lower.starts_with("claude-haiku-4-5") {
        if enabled {
            payload["thinking"] = json!({ "type": "enabled", "budget_tokens": 16_384 });
            return true;
        }
        return false;
    }

    if is_minimax_profile(profile, base_url, model) && model_lower.contains("minimax-m3") {
        payload["thinking"] = json!({
            "type": if enabled { "adaptive" } else { "disabled" }
        });
        return true;
    }

    if is_deepseek_profile(profile, base_url, model) && model_lower.starts_with("deepseek-v4") {
        payload["thinking"] = json!({
            "type": if enabled { "enabled" } else { "disabled" }
        });
        if enabled {
            payload["output_config"] = json!({ "effort": "high" });
        }
        return true;
    }

    if is_doubao_profile(profile, base_url, model)
        || is_zhipu_profile(profile, base_url, model)
        || is_kimi_profile(profile, base_url, model)
        || is_mimo_profile(profile)
        || is_siliconflow_profile(profile, base_url)
    {
        payload["thinking"] = json!({
            "type": if enabled { "enabled" } else { "disabled" }
        });
        return true;
    }

    false
}

fn parse_anthropic_content(value: &Value) -> Result<(String, Option<String>), ModelResponseError> {
    if let Some(text) = value["content"].as_str() {
        return Ok((text.to_string(), None));
    }
    let blocks = value["content"].as_array().ok_or_else(|| {
        ModelResponseError::other(format!("Anthropic 响应缺少 content 内容块：{value}"))
    })?;
    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    for block in blocks {
        match block["type"].as_str().unwrap_or_default() {
            "text" => {
                if let Some(text) = block["text"].as_str().filter(|text| !text.is_empty()) {
                    text_parts.push(text);
                }
            }
            "thinking" => {
                if let Some(thinking) = block["thinking"]
                    .as_str()
                    .filter(|thinking| !thinking.is_empty())
                {
                    reasoning_parts.push(thinking);
                }
            }
            _ => {}
        }
    }
    if text_parts.is_empty() && reasoning_parts.is_empty() {
        return Err(ModelResponseError::other(format!(
            "Anthropic 响应缺少可用文本：{value}"
        )));
    }
    Ok((
        text_parts.join("\n\n"),
        (!reasoning_parts.is_empty()).then(|| reasoning_parts.join("\n\n")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(base_url: &str, model: &str, thinking_mode: &str) -> ModelProfile {
        ModelProfile {
            id: "profile-1".to_string(),
            name: "Anthropic 测试".to_string(),
            provider: "anthropic".to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            temperature: 0.7,
            top_p: 1.0,
            thinking_mode: thinking_mode.to_string(),
            prompt_obfuscation_enabled: false,
            has_api_key: true,
            api_key_storage: "system".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn builds_messages_endpoint_without_duplicate_segments() {
        assert_eq!(
            anthropic_messages_endpoint("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            anthropic_messages_endpoint("https://api.deepseek.com/anthropic"),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
        assert_eq!(
            anthropic_messages_endpoint("https://example.com/v1"),
            "https://example.com/v1/messages"
        );
        assert_eq!(
            anthropic_messages_endpoint("https://example.com/v1/messages"),
            "https://example.com/v1/messages"
        );
    }

    #[test]
    fn parses_text_and_thinking_blocks_separately() {
        let value = json!({
            "content": [
                {"type": "thinking", "thinking": "先检查上下文"},
                {"type": "text", "text": "正文一"},
                {"type": "text", "text": "正文二"}
            ]
        });
        let (text, reasoning) = parse_anthropic_content(&value).expect("valid response");
        assert_eq!(text, "正文一\n\n正文二");
        assert_eq!(reasoning.as_deref(), Some("先检查上下文"));
    }

    #[test]
    fn applies_model_specific_anthropic_thinking_controls() {
        let claude = profile("https://api.anthropic.com", "claude-opus-4-8", "on");
        let mut claude_payload = json!({});
        assert!(apply_anthropic_thinking_control(
            &mut claude_payload,
            &claude,
            &claude.base_url,
            &claude.model
        ));
        assert_eq!(claude_payload["thinking"]["type"], json!("adaptive"));

        let deepseek = profile(
            "https://api.deepseek.com/anthropic",
            "deepseek-v4-pro",
            "off",
        );
        let mut deepseek_payload = json!({});
        assert!(apply_anthropic_thinking_control(
            &mut deepseek_payload,
            &deepseek,
            &deepseek.base_url,
            &deepseek.model
        ));
        assert_eq!(deepseek_payload["thinking"]["type"], json!("disabled"));
    }
}
