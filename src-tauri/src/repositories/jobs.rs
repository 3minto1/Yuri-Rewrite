use crate::domain::{AppState, Job};
use crate::to_string;
use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

pub(crate) fn create_job(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_type: &str,
    total: i64,
) -> Result<Job, String> {
    let now = Utc::now().to_rfc3339();
    let job = Job {
        id: Uuid::new_v4().to_string(),
        novel_id: novel_id.to_string(),
        job_type: job_type.to_string(),
        status: "running".to_string(),
        current_chapter: 0,
        total_chapters: total,
        message: "任务已开始".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "INSERT INTO jobs (id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![job.id, job.novel_id, job.job_type, job.status, job.current_chapter, job.total_chapters, job.message, job.created_at, job.updated_at],
    )
    .map_err(to_string)?;
    Ok(job)
}

pub(crate) fn update_job(
    state: &State<'_, AppState>,
    job_id: &str,
    status: &str,
    current_chapter: i64,
    message: &str,
) -> Result<(), String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "UPDATE jobs SET status = ?1, current_chapter = ?2, message = ?3, updated_at = ?4 WHERE id = ?5",
        params![status, current_chapter, message, Utc::now().to_rfc3339(), job_id],
    )
    .map_err(to_string)?;
    Ok(())
}
