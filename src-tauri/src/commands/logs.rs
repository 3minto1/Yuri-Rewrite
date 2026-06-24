use crate::domain::{AiLog, AppState, TokenUsageDay, TokenUsageModel, TokenUsageReport};
use crate::{row_to_ai_log, to_string};
use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use tauri::State;

#[tauri::command]
pub(crate) fn list_ai_logs(
    novel_id: Option<String>,
    state: State<AppState>,
) -> Result<Vec<AiLog>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    if let Some(novel_id) = novel_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, created_at FROM ai_logs WHERE novel_id = ?1 OR novel_id IS NULL ORDER BY created_at DESC LIMIT 80",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map(params![novel_id], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, created_at FROM ai_logs ORDER BY created_at DESC LIMIT 80",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map([], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    }
}

#[tauri::command]
pub(crate) fn clear_ai_logs(
    novel_id: Option<String>,
    state: State<AppState>,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    clear_ai_logs_from_connection(&conn, novel_id.as_deref())
}

fn clear_ai_logs_from_connection(conn: &Connection, novel_id: Option<&str>) -> Result<(), String> {
    if let Some(novel_id) = novel_id {
        conn.execute(
            "DELETE FROM ai_logs WHERE novel_id = ?1 OR novel_id IS NULL",
            params![novel_id],
        )
        .map_err(to_string)?;
    } else {
        conn.execute("DELETE FROM ai_logs", []).map_err(to_string)?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_token_usage_stats(
    start_date: String,
    end_date: String,
    state: State<AppState>,
) -> Result<TokenUsageReport, String> {
    let start = NaiveDate::parse_from_str(start_date.trim(), "%Y-%m-%d")
        .map_err(|_| "开始日期格式无效。".to_string())?;
    let end = NaiveDate::parse_from_str(end_date.trim(), "%Y-%m-%d")
        .map_err(|_| "结束日期格式无效。".to_string())?;
    if start > end {
        return Err("开始日期不能晚于结束日期。".to_string());
    }
    if (end - start).num_days() > 366 {
        return Err("单次统计范围不能超过 366 天。".to_string());
    }
    let conn = state.conn.lock().map_err(to_string)?;
    build_token_usage_report(&conn, start, end)
}

fn build_token_usage_report(
    conn: &Connection,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<TokenUsageReport, String> {
    #[derive(Default)]
    struct Aggregate {
        profile_name: String,
        model: String,
        requests: usize,
        input_tokens: usize,
        output_tokens: usize,
        days: BTreeMap<String, (usize, usize, usize)>,
    }

    let mut stmt = conn
        .prepare(
            "SELECT profile_id,
                    profile_name,
                    model,
                    input_tokens,
                    output_tokens,
                    created_at
             FROM token_usage_records
             ORDER BY created_at",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as usize,
                row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as usize,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(to_string)?;

    let mut aggregates = BTreeMap::<String, Aggregate>::new();
    for row in rows {
        let (profile_id, profile_name, model, input_tokens, output_tokens, created_at) =
            row.map_err(to_string)?;
        let local_date = DateTime::parse_from_rfc3339(&created_at)
            .map_err(to_string)?
            .with_timezone(&Local)
            .date_naive();
        if local_date < start || local_date > end {
            continue;
        }
        let date = local_date.format("%Y-%m-%d").to_string();
        let aggregate = aggregates.entry(profile_id).or_default();
        aggregate.profile_name = profile_name;
        aggregate.model = model;
        aggregate.requests += 1;
        aggregate.input_tokens += input_tokens;
        aggregate.output_tokens += output_tokens;
        let day = aggregate.days.entry(date).or_default();
        day.0 += 1;
        day.1 += input_tokens;
        day.2 += output_tokens;
    }

    let mut report = TokenUsageReport {
        start_date: start.format("%Y-%m-%d").to_string(),
        end_date: end.format("%Y-%m-%d").to_string(),
        requests: 0,
        input_tokens: 0,
        output_tokens: 0,
        models: Vec::with_capacity(aggregates.len()),
    };
    for (profile_id, aggregate) in aggregates {
        report.requests += aggregate.requests;
        report.input_tokens += aggregate.input_tokens;
        report.output_tokens += aggregate.output_tokens;
        report.models.push(TokenUsageModel {
            profile_id,
            profile_name: aggregate.profile_name,
            model: aggregate.model,
            requests: aggregate.requests,
            input_tokens: aggregate.input_tokens,
            output_tokens: aggregate.output_tokens,
            days: aggregate
                .days
                .into_iter()
                .map(|(date, values)| TokenUsageDay {
                    date,
                    requests: values.0,
                    input_tokens: values.1,
                    output_tokens: values.2,
                })
                .collect(),
        });
    }
    report.models.sort_by(|left, right| {
        (right.input_tokens + right.output_tokens).cmp(&(left.input_tokens + left.output_tokens))
    });
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    #[test]
    fn groups_token_usage_by_model_and_local_date() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO model_profiles (
                id, name, provider, base_url, model, temperature, thinking_mode, updated_at
             ) VALUES ('profile-1', '主力模型', 'openai-compatible', 'https://example.com', 'model-a', 0.7, 'auto', 'now')",
            [],
        )
        .expect("insert profile");
        for (id, input, output, created_at) in [
            ("log-1", 100_i64, 20_i64, "2026-06-18T12:00:00+08:00"),
            ("log-2", 200_i64, 40_i64, "2026-06-18T13:00:00+08:00"),
            ("log-3", 300_i64, 60_i64, "2026-06-19T12:00:00+08:00"),
        ] {
            conn.execute(
                "INSERT INTO token_usage_records (
                    id, profile_id, profile_name, model,
                    input_tokens, output_tokens, created_at
                 ) VALUES (?1, 'profile-1', '主力模型', 'model-a', ?2, ?3, ?4)",
                params![id, input, output, created_at],
            )
            .expect("insert token usage");
        }
        let report = build_token_usage_report(
            &conn,
            NaiveDate::from_ymd_opt(2026, 6, 18).expect("start date"),
            NaiveDate::from_ymd_opt(2026, 6, 19).expect("end date"),
        )
        .expect("build report");
        assert_eq!(report.requests, 3);
        assert_eq!(report.input_tokens, 600);
        assert_eq!(report.output_tokens, 120);
        assert_eq!(report.models.len(), 1);
        assert_eq!(report.models[0].model, "model-a");
        assert_eq!(report.models[0].days.len(), 2);
        assert_eq!(report.models[0].days[0].requests, 2);
    }

    #[test]
    fn clearing_logs_preserves_token_usage_history() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO ai_logs (
                id, novel_id, profile_id, action, status, content, created_at
             ) VALUES (
                'log-1', 'novel-1', 'profile-1', '改写', 'success', 'ok',
                '2026-06-22T10:00:00+08:00'
             )",
            [],
        )
        .expect("insert log");
        conn.execute(
            "INSERT INTO token_usage_records (
                id, novel_id, profile_id, profile_name, model,
                input_tokens, output_tokens, created_at
             ) VALUES (
                'log-1', 'novel-1', 'profile-1', '已删除模型', 'model-a',
                500, 125, '2026-06-22T10:00:00+08:00'
             )",
            [],
        )
        .expect("insert usage");

        clear_ai_logs_from_connection(&conn, Some("novel-1")).expect("clear logs");

        let log_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_logs", [], |row| row.get(0))
            .expect("count logs");
        assert_eq!(log_count, 0);
        let report = build_token_usage_report(
            &conn,
            NaiveDate::from_ymd_opt(2026, 6, 22).expect("start date"),
            NaiveDate::from_ymd_opt(2026, 6, 22).expect("end date"),
        )
        .expect("build report");
        assert_eq!(report.requests, 1);
        assert_eq!(report.input_tokens, 500);
        assert_eq!(report.output_tokens, 125);
        assert_eq!(report.models[0].profile_name, "已删除模型");
        assert_eq!(report.models[0].model, "model-a");
    }
}
