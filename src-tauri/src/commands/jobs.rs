use crate::domain::{AppState, Job};
use crate::{row_to_job, to_string};
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub(crate) fn get_job(job_id: String, state: State<AppState>) -> Result<Job, String> {
    load_job(&state, &job_id)
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
