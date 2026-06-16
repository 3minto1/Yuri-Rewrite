use super::common::*;
use crate::domain::{ModelOutput, ModelProfile};
use crate::model_support::{parse_gemini_parts, ModelResponseError};
use reqwest::Client;
use serde_json::json;

pub(crate) async fn generate_gemini(
    client: &Client,
    profile: &ModelProfile,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<ModelOutput, ModelResponseError> {
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
        .map_err(|error| ModelResponseError::other(format_request_error(error)))?;
    let mut retried_without_thinking = false;
    let (value, raw_response) = match response_json_or_error(response).await {
        Ok(result) => result,
        Err(error) if added_thinking_control && error.permits_thinking_retry() => {
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
                .map_err(|error| ModelResponseError::other(format_request_error(error)))?;
            let retry_result =
                response_json_or_error(retry_response)
                    .await
                    .map_err(|retry_error| {
                        ModelResponseError::other(format!(
                            "{}；移除思考模式参数重试后仍失败：{}",
                            error, retry_error
                        ))
                    })?;
            retried_without_thinking = true;
            retry_result
        }
        Err(error) => return Err(error),
    };
    let (text, reasoning) = parse_gemini_parts(&value).map_err(ModelResponseError::other)?;
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
