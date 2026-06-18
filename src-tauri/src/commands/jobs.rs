use crate::domain::{AppState, AutoRunRecovery, Job};
use crate::{row_to_job, to_string};
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub(crate) fn get_job(job_id: String, state: State<AppState>) -> Result<Job, String> {
    load_job(&state, &job_id)
}

#[tauri::command]
pub(crate) fn list_auto_run_recoveries(
    state: State<AppState>,
) -> Result<Vec<AutoRunRecovery>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT novel_id, start_batch_index, next_batch_index, status, pause_reason, phase, batch_index, profile_ids, job_id FROM auto_run_checkpoints ORDER BY updated_at DESC",
        )
        .map_err(to_string)?;
    let rows = stmt.query_map([], |row| {
        let profile_json: String = row.get(7)?;
        let profile_ids = serde_json::from_str(&profile_json).unwrap_or_default();
        let job_id: Option<String> = row.get(8)?;
        let job = job_id
            .as_deref()
            .and_then(|id| {
                conn.query_row(
                    "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
                    params![id],
                    crate::row_to_job,
                )
                .ok()
            });
        Ok(AutoRunRecovery {
            novel_id: row.get(0)?,
            start_batch_index: row.get(1)?,
            next_batch_index: row.get(2)?,
            status: row.get(3)?,
            pause_reason: row.get(4)?,
            phase: row.get(5)?,
            batch_index: row.get(6)?,
            profile_ids,
            job,
        })
    })
    .map_err(to_string)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(to_string)?;
    Ok(rows)
}

pub(crate) fn load_job(state: &State<'_, AppState>, job_id: &str) -> Result<Job, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
        params![job_id],
        row_to_job,
    )
    .map_err(to_string)
}
