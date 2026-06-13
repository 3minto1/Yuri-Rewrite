use super::common::*;
use crate::domain::{ModelOutput, ModelProfile};
use reqwest::Client;
use serde_json::json;

pub(crate) async fn generate_openai_compatible(
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
        .map_err(format_request_error)?;
    let mut retried_without_thinking = false;
    let (value, raw_response) = match response_json_or_error(response).await {
        Ok(result) => result,
        Err(error) if added_thinking_control && error.permits_thinking_retry() => {
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
                .map_err(format_request_error)?;
            let retry_result =
                response_json_or_error(retry_response)
                    .await
                    .map_err(|retry_error| {
                        format!("{}；移除思考模式参数重试后仍失败：{}", error, retry_error)
                    })?;
            retried_without_thinking = true;
            retry_result
        }
        Err(error) => return Err(error.to_string()),
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
