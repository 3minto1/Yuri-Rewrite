use crate::domain::AppState;
use crate::model_support::model_output_finish_reason;
use crate::{to_string, truncate_text};
use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

pub(crate) fn extract_token_usage(raw_response: Option<&str>) -> Option<(usize, usize)> {
    let value = serde_json::from_str::<serde_json::Value>(raw_response?).ok()?;
    if let Some(usage) = value.get("usage") {
        let input = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let output = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        return Some((input, output));
    }
    if let Some(usage) = value.get("usageMetadata") {
        let input = usage
            .get("promptTokenCount")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let total = usage
            .get("totalTokenCount")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as usize);
        let output = total.map(|value| value.saturating_sub(input)).unwrap_or_else(|| {
            usage
                .get("candidatesTokenCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize
                + usage
                    .get("thoughtsTokenCount")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as usize
        });
        return Some((input, output));
    }
    if value.get("choices").is_some() || value.get("candidates").is_some() {
        return Some((0, 0));
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_ai_log(
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
    let token_usage = extract_token_usage(raw_response);
    let finish_reason = raw_response.and_then(model_output_finish_reason);
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, input_tokens, output_tokens, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            finish_reason,
            token_usage.map(|usage| usage.0 as i64),
            token_usage.map(|usage| usage.1 as i64),
            Utc::now().to_rfc3339()
        ],
    )
    .map_err(to_string)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::extract_token_usage;

    #[test]
    fn extracts_openai_and_gemini_token_usage() {
        assert_eq!(
            extract_token_usage(Some(
                r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":120,"completion_tokens":45}}"#
            )),
            Some((120, 45))
        );
        assert_eq!(
            extract_token_usage(Some(
                r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]}}],"usageMetadata":{"promptTokenCount":80,"candidatesTokenCount":20,"thoughtsTokenCount":10,"totalTokenCount":110}}"#
            )),
            Some((80, 30))
        );
    }

    #[test]
    fn counts_model_responses_without_usage_as_zero_tokens() {
        assert_eq!(
            extract_token_usage(Some(
                r#"{"choices":[{"message":{"content":"ok"}}]}"#
            )),
            Some((0, 0))
        );
        assert_eq!(extract_token_usage(Some(r#"{"status":"ok"}"#)), None);
    }
}
