use crate::domain::{AiLog, AiLogDaySummary, AppState, TokenUsageDay, TokenUsageModel, TokenUsageReport};
use crate::{row_to_ai_log, to_string};
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use tauri::State;

const AI_LOG_VISIBLE_DAYS: i64 = 7;

fn log_day_bounds(date: NaiveDate) -> Result<(String, String), String> {
    let start = Local
        .from_local_datetime(
            &date
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| "日志日期无效。".to_string())?,
        )
        .single()
        .ok_or_else(|| "日志日期无法转换为本地时间。".to_string())?;
    let next_date = date
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| "日志日期范围无效。".to_string())?;
    let end = Local
        .from_local_datetime(
            &next_date
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| "日志日期无效。".to_string())?,
        )
        .single()
        .ok_or_else(|| "日志日期无法转换为本地时间。".to_string())?;
    Ok((
        start.with_timezone(&Utc).to_rfc3339(),
        end.with_timezone(&Utc).to_rfc3339(),
    ))
}

fn parse_log_date(date: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map_err(|_| "日志日期格式无效。".to_string())
}

fn count_logs_for_day(
    conn: &Connection,
    novel_id: Option<&str>,
    date: NaiveDate,
) -> Result<usize, String> {
    let (start, end) = log_day_bounds(date)?;
    let count: i64 = if let Some(novel_id) = novel_id {
        conn.query_row(
            "SELECT COUNT(*) FROM ai_logs
             WHERE (novel_id = ?1 OR novel_id IS NULL)
               AND datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)",
            params![novel_id, start, end],
            |row| row.get(0),
        )
        .map_err(to_string)?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM ai_logs
             WHERE datetime(created_at) >= datetime(?1)
               AND datetime(created_at) < datetime(?2)",
            params![start, end],
            |row| row.get(0),
        )
        .map_err(to_string)?
    };
    Ok(count.max(0) as usize)
}

#[tauri::command]
pub(crate) fn list_ai_log_days(
    novel_id: Option<String>,
    state: State<AppState>,
) -> Result<Vec<AiLogDaySummary>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let today = Local::now().date_naive();
    let mut days = Vec::new();
    for offset in 0..AI_LOG_VISIBLE_DAYS {
        let date = today - Duration::days(offset);
        days.push(AiLogDaySummary {
            date: date.format("%Y-%m-%d").to_string(),
            count: count_logs_for_day(&conn, novel_id.as_deref(), date)?,
        });
    }
    Ok(days)
}

#[tauri::command]
pub(crate) fn list_ai_logs_by_date(
    novel_id: Option<String>,
    date: String,
    state: State<AppState>,
) -> Result<Vec<AiLog>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let date = parse_log_date(&date)?;
    list_ai_logs_by_date_from_connection(&conn, novel_id.as_deref(), date)
}

fn list_ai_logs_by_date_from_connection(
    conn: &Connection,
    novel_id: Option<&str>,
    date: NaiveDate,
) -> Result<Vec<AiLog>, String> {
    let (start, end) = log_day_bounds(date)?;
    if let Some(novel_id) = novel_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, created_at
                 FROM ai_logs
                 WHERE (novel_id = ?1 OR novel_id IS NULL)
                   AND datetime(created_at) >= datetime(?2)
                   AND datetime(created_at) < datetime(?3)
                 ORDER BY datetime(created_at) DESC",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map(params![novel_id, start, end], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, novel_id, profile_id, action, chapter_title, status, content, reasoning, raw_response, finish_reason, created_at
                 FROM ai_logs
                 WHERE datetime(created_at) >= datetime(?1)
                   AND datetime(created_at) < datetime(?2)
                 ORDER BY datetime(created_at) DESC",
            )
            .map_err(to_string)?;
        let logs = stmt
            .query_map(params![start, end], row_to_ai_log)
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        Ok(logs)
    }
}

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
    use crate::repositories::logs::cleanup_old_ai_logs;

    fn insert_ai_log(
        conn: &Connection,
        id: &str,
        novel_id: Option<&str>,
        action: &str,
        content: &str,
        created_at: &str,
    ) {
        conn.execute(
            "INSERT INTO ai_logs (
                id, novel_id, profile_id, action, status, content, created_at
             ) VALUES (?1, ?2, 'profile-1', ?3, 'success', ?4, ?5)",
            params![id, novel_id, action, content, created_at],
        )
        .expect("insert ai log");
    }

    fn local_midday_rfc3339(date: NaiveDate) -> String {
        Local
            .from_local_datetime(
                &date
                    .and_hms_opt(12, 0, 0)
                    .expect("valid local midday"),
            )
            .single()
            .expect("local midday")
            .to_rfc3339()
    }

    #[test]
    fn lists_all_logs_for_selected_date_in_newest_order() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        let date = NaiveDate::from_ymd_opt(2026, 6, 18).expect("date");
        insert_ai_log(&conn, "log-1", Some("novel-1"), "旧日志", "first", "2026-06-18T10:00:00+08:00");
        insert_ai_log(&conn, "log-2", Some("novel-1"), "新日志", "second", "2026-06-18T12:00:00+08:00");
        insert_ai_log(&conn, "log-3", Some("novel-1"), "次日日志", "next", "2026-06-19T00:30:00+08:00");

        let rows = list_ai_logs_by_date_from_connection(&conn, None, date).expect("list logs");

        assert_eq!(rows.iter().map(|log| log.id.as_str()).collect::<Vec<_>>(), ["log-2", "log-1"]);
    }

    #[test]
    fn date_log_queries_include_global_logs_for_selected_novel() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        let date = NaiveDate::from_ymd_opt(2026, 6, 18).expect("date");
        insert_ai_log(&conn, "novel-log", Some("novel-1"), "小说日志", "novel", "2026-06-18T10:00:00+08:00");
        insert_ai_log(&conn, "global-log", None, "全局日志", "global", "2026-06-18T11:00:00+08:00");
        insert_ai_log(&conn, "other-log", Some("novel-2"), "其他小说", "other", "2026-06-18T12:00:00+08:00");

        let rows = list_ai_logs_by_date_from_connection(&conn, Some("novel-1"), date).expect("list logs");

        assert_eq!(count_logs_for_day(&conn, Some("novel-1"), date).expect("count logs"), 2);
        assert_eq!(
            rows.iter().map(|log| log.id.as_str()).collect::<Vec<_>>(),
            ["global-log", "novel-log"]
        );
    }

    #[test]
    fn cleanup_old_ai_logs_preserves_recent_logs_and_token_usage() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        let today = Local::now().date_naive();
        let recent_at = local_midday_rfc3339(today - Duration::days(6));
        let old_at = local_midday_rfc3339(today - Duration::days(7));
        insert_ai_log(&conn, "recent-log", Some("novel-1"), "近期日志", "recent", &recent_at);
        insert_ai_log(&conn, "old-log", Some("novel-1"), "旧日志", "old", &old_at);
        conn.execute(
            "INSERT INTO token_usage_records (
                id, novel_id, profile_id, profile_name, model,
                input_tokens, output_tokens, created_at
             ) VALUES (
                'old-log', 'novel-1', 'profile-1', '模型', 'model-a',
                100, 25, ?1
             )",
            params![old_at],
        )
        .expect("insert token usage");

        cleanup_old_ai_logs(&conn).expect("cleanup logs");

        let log_ids = conn
            .prepare("SELECT id FROM ai_logs ORDER BY id")
            .expect("prepare log query")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query logs")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect logs");
        assert_eq!(log_ids, ["recent-log"]);
        let usage_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM token_usage_records", [], |row| row.get(0))
            .expect("count usage");
        assert_eq!(usage_count, 1);
    }

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
