use crate::domain::AppState;
use crate::model_support::model_output_finish_reason;
use crate::{to_string, truncate_text};
use chrono::{Duration, Local, TimeZone, Utc};
use rusqlite::{params, Connection};
use tauri::State;
use uuid::Uuid;

const AI_LOG_RETENTION_DAYS: i64 = 7;

pub(crate) fn cleanup_old_ai_logs(conn: &Connection) -> Result<(), String> {
    let today = Local::now().date_naive();
    let cutoff_date = today - Duration::days(AI_LOG_RETENTION_DAYS - 1);
    let cutoff = Local
        .from_local_datetime(
            &cutoff_date
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| "日志保留日期无效。".to_string())?,
        )
        .single()
        .ok_or_else(|| "日志保留日期无法转换为本地时间。".to_string())?
        .with_timezone(&Utc)
        .to_rfc3339();
    conn.execute(
        "DELETE FROM ai_logs WHERE datetime(created_at) < datetime(?1)",
        params![cutoff],
    )
    .map_err(to_string)?;
    Ok(())
}

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
        let output = total
            .map(|value| value.saturating_sub(input))
            .unwrap_or_else(|| {
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
    let mut conn = state.conn.lock().map_err(to_string)?;
    persist_ai_log(
        &mut conn,
        novel_id,
        profile_id,
        action,
        chapter_title,
        status,
        content,
        reasoning,
        raw_response,
        finish_reason.as_deref(),
        token_usage,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_ai_log(
    conn: &mut Connection,
    novel_id: Option<&str>,
    profile_id: &str,
    action: &str,
    chapter_title: Option<&str>,
    status: &str,
    content: &str,
    reasoning: Option<&str>,
    raw_response: Option<&str>,
    finish_reason: Option<&str>,
    token_usage: Option<(usize, usize)>,
) -> Result<(), String> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "INSERT INTO ai_logs (id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, input_tokens, output_tokens, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            id,
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
            created_at
        ],
    )
    .map_err(to_string)?;
    if let Some((input_tokens, output_tokens)) = token_usage {
        tx.execute(
            "INSERT INTO token_usage_records (
                id, novel_id, profile_id, profile_name, model,
                input_tokens, output_tokens, created_at
             ) VALUES (
                ?1, ?2, ?3,
                COALESCE((SELECT name FROM model_profiles WHERE id = ?3), ?3),
                COALESCE((SELECT model FROM model_profiles WHERE id = ?3), ?3),
                ?4, ?5, ?6
             )",
            params![
                id,
                novel_id,
                profile_id,
                input_tokens as i64,
                output_tokens as i64,
                created_at
            ],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    cleanup_old_ai_logs(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{extract_token_usage, persist_ai_log};
    use crate::db::init_db;
    use rusqlite::Connection;

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
            extract_token_usage(Some(r#"{"choices":[{"message":{"content":"ok"}}]}"#)),
            Some((0, 0))
        );
        assert_eq!(extract_token_usage(Some(r#"{"status":"ok"}"#)), None);
    }

    #[test]
    fn persists_log_and_token_usage_snapshot_together() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO model_profiles (
                id, name, provider, base_url, model, temperature, thinking_mode, updated_at
             ) VALUES (
                'profile-1', '快照模型', 'openai-compatible', 'https://example.com',
                'model-a', 0.7, 'auto', 'now'
             )",
            [],
        )
        .expect("insert profile");

        persist_ai_log(
            &mut conn,
            Some("novel-1"),
            "profile-1",
            "分析",
            Some("第一章"),
            "success",
            "ok",
            None,
            Some(r#"{"usage":{"prompt_tokens":120,"completion_tokens":45}}"#),
            Some("stop"),
            Some((120, 45)),
        )
        .expect("persist log and usage");

        let log_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_logs", [], |row| row.get(0))
            .expect("count logs");
        let usage: (String, String, i64, i64) = conn
            .query_row(
                "SELECT profile_name, model, input_tokens, output_tokens
                 FROM token_usage_records",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load usage");
        assert_eq!(log_count, 1);
        assert_eq!(
            usage,
            ("快照模型".to_string(), "model-a".to_string(), 120, 45)
        );
    }
}
