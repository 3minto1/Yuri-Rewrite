use serde_json::Value;
use std::fmt;

#[derive(Debug)]
pub(crate) struct ModelResponseError {
    status: Option<u16>,
    body: String,
    message: String,
}

impl ModelResponseError {
    pub(crate) fn provider(status: u16, body: String) -> Self {
        Self {
            status: Some(status),
            message: format!("HTTP {status}: {body}"),
            body,
        }
    }

    pub(crate) fn other(message: String) -> Self {
        Self {
            status: None,
            body: String::new(),
            message,
        }
    }

    pub(crate) fn permits_thinking_retry(&self) -> bool {
        self.status
            .is_some_and(|status| should_retry_without_thinking(status, &self.body))
    }
}

impl fmt::Display for ModelResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) fn parse_gemini_parts(value: &Value) -> Result<(String, Option<String>), String> {
    let parts = value["candidates"][0]["content"]["parts"]
        .as_array()
        .ok_or_else(|| format!("Gemini 响应缺少正文 parts: {value}"))?;
    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    for part in parts {
        let Some(text) = part["text"]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        if part["thought"].as_bool().unwrap_or(false) {
            reasoning_parts.push(text);
        } else {
            text_parts.push(text);
        }
    }
    if text_parts.is_empty() && reasoning_parts.is_empty() {
        return Err(format!("Gemini 响应缺少可用文本: {value}"));
    }
    Ok((
        text_parts.join("\n\n"),
        (!reasoning_parts.is_empty()).then(|| reasoning_parts.join("\n\n")),
    ))
}

pub(crate) fn model_output_truncation_error(raw_response: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(raw_response).ok()?;
    let reason = value["choices"][0]["finish_reason"]
        .as_str()
        .or_else(|| value["candidates"][0]["finishReason"].as_str())
        .or_else(|| value["stop_reason"].as_str())
        .or_else(|| value["incomplete_details"]["reason"].as_str())?;
    let normalized = reason.trim().to_ascii_lowercase();
    let truncated = matches!(
        normalized.as_str(),
        "length"
            | "max_tokens"
            | "max_token"
            | "max_output_tokens"
            | "max_completion_tokens"
            | "token_limit"
    );
    truncated.then(|| {
        format!(
            "模型输出因达到长度上限被截断（结束原因：{}），当前结果不完整",
            reason.trim()
        )
    })
}

pub(crate) fn should_retry_without_thinking(status: u16, body: &str) -> bool {
    if !matches!(status, 400 | 422) {
        return false;
    }
    let body = body.to_ascii_lowercase();
    let mentions_thinking = [
        "reasoning_effort",
        "thinking_budget",
        "thinkingconfig",
        "thinking_config",
        "thinking",
        "reasoning",
    ]
    .iter()
    .any(|keyword| body.contains(keyword));
    let reports_incompatibility = [
        "unsupported",
        "not supported",
        "unknown",
        "unrecognized",
        "invalid parameter",
        "invalid field",
        "extra inputs",
        "unexpected",
        "不支持",
        "未知参数",
        "无效参数",
    ]
    .iter()
    .any(|keyword| body.contains(keyword));
    mentions_thinking && reports_incompatibility
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gemini_parts_separate_reasoning_and_final_text() {
        let value = json!({
            "candidates": [{"content": {"parts": [
                {"thought": true, "text": "思考一"},
                {"text": "最终正文一"},
                {"thought": true, "text": "思考二"},
                {"text": "最终正文二"}
            ]}}]
        });
        let (text, reasoning) = parse_gemini_parts(&value).expect("valid parts");
        assert_eq!(text, "最终正文一\n\n最终正文二");
        assert_eq!(reasoning.as_deref(), Some("思考一\n\n思考二"));
    }

    #[test]
    fn gemini_parts_allow_reasoning_only_for_trailing_json_recovery() {
        let value = json!({
            "candidates": [{"content": {"parts": [{"thought": true, "text": "分析\n{\"ok\":true}"}]}}]
        });
        let (text, reasoning) = parse_gemini_parts(&value).expect("reasoning only");
        assert!(text.is_empty());
        assert!(reasoning.is_some());
    }

    #[test]
    fn thinking_retry_only_accepts_explicit_parameter_errors() {
        assert!(should_retry_without_thinking(
            400,
            "unsupported parameter: reasoning_effort"
        ));
        assert!(should_retry_without_thinking(
            422,
            "thinkingConfig is not supported"
        ));
        assert!(!should_retry_without_thinking(
            401,
            "invalid reasoning token"
        ));
        assert!(!should_retry_without_thinking(429, "thinking rate limit"));
        assert!(!should_retry_without_thinking(500, "unsupported thinking"));
        assert!(!should_retry_without_thinking(400, "invalid model name"));
    }

    #[test]
    fn detects_provider_output_length_truncation() {
        for value in [
            json!({"choices": [{"finish_reason": "length"}]}),
            json!({"candidates": [{"finishReason": "MAX_TOKENS"}]}),
            json!({"stop_reason": "max_tokens"}),
            json!({"incomplete_details": {"reason": "max_output_tokens"}}),
        ] {
            let error = model_output_truncation_error(&value.to_string())
                .expect("length truncation should be detected");
            assert!(error.contains("达到长度上限被截断"));
        }

        assert!(model_output_truncation_error(
            &json!({"choices": [{"finish_reason": "stop"}]}).to_string()
        )
        .is_none());
        assert!(model_output_truncation_error("not json").is_none());
    }
}
