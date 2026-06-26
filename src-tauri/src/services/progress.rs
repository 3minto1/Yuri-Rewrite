use crate::domain::{ActiveShardProgress, AppState, Chapter, JobProgress};
use crate::task_control::AutoRunProgressState;
use crate::{row_to_job, to_string, update_job};
use rusqlite::params;
use std::collections::HashSet;
use tauri::{Emitter, State};

fn phase_label(phase: &str) -> &'static str {
    match phase {
        "analysis" => "分析",
        "rewrite" => "改写",
        "review" => "审查",
        "revision" => "修复",
        "final_review" => "终审",
        "export" => "导出",
        _ => "处理中",
    }
}

pub(crate) fn begin_auto_batch_progress(
    state: &State<'_, AppState>,
    novel_id: &str,
    phase: &str,
    batch_index: i64,
    batch_total: i64,
    batch_label: &str,
) -> Result<(), String> {
    if !state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .contains_key(novel_id)
    {
        return Ok(());
    }
    let job_id = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(novel_id)
        .and_then(|control| control.job_id.clone());
    state.auto_run_progress.lock().map_err(to_string)?.insert(
        novel_id.to_string(),
        AutoRunProgressState {
            job_id,
            phase: Some(phase.to_string()),
            batch_index: Some(batch_index),
            batch_total: Some(batch_total),
            batch_label: Some(batch_label.to_string()),
            ..AutoRunProgressState::default()
        },
    );
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn begin_job_progress(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_id: &str,
    phase: &str,
    batch_label: &str,
) -> Result<(), String> {
    state.auto_run_progress.lock().map_err(to_string)?.insert(
        novel_id.to_string(),
        AutoRunProgressState {
            job_id: Some(job_id.to_string()),
            phase: Some(phase.to_string()),
            batch_index: Some(1),
            batch_total: Some(1),
            batch_label: Some(batch_label.to_string()),
            ..AutoRunProgressState::default()
        },
    );
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn clear_job_progress(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_id: &str,
) -> Result<(), String> {
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    if progress
        .get(novel_id)
        .and_then(|entry| entry.job_id.as_deref())
        .is_some_and(|active_job_id| active_job_id == job_id)
    {
        progress.remove(novel_id);
    }
    Ok(())
}

pub(crate) fn set_auto_progress_shard_total(
    state: &State<'_, AppState>,
    novel_id: &str,
    phase: &str,
    shard_total: usize,
    chapter_total: usize,
    completed_chapter_ids: HashSet<String>,
) -> Result<(), String> {
    let auto_job_id = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(novel_id)
        .and_then(|control| control.job_id.clone());
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    if !progress.contains_key(novel_id) {
        let Some(job_id) = auto_job_id.clone() else {
            return Ok(());
        };
        progress.insert(
            novel_id.to_string(),
            AutoRunProgressState {
                job_id: Some(job_id),
                ..AutoRunProgressState::default()
            },
        );
    }
    let entry = progress.entry(novel_id.to_string()).or_default();
    if entry.job_id.is_none() {
        entry.job_id = auto_job_id;
    }
    entry.phase = Some(phase.to_string());
    entry.shard_total = shard_total;
    entry.completed_shards.clear();
    entry.chapter_total = chapter_total;
    entry.completed_chapter_ids = completed_chapter_ids;
    entry.active_shards.clear();
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn set_auto_progress_phase(
    state: &State<'_, AppState>,
    novel_id: &str,
    phase: &str,
) -> Result<(), String> {
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    let Some(entry) = progress.get_mut(novel_id) else {
        return Ok(());
    };
    entry.phase = Some(phase.to_string());
    entry.active_shards.clear();
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn report_auto_shard_started(
    state: &State<'_, AppState>,
    novel_id: &str,
    phase: &str,
    shard_index: usize,
    shard_total: usize,
    shard: &[Chapter],
) -> Result<(), String> {
    let Some(first) = shard.first() else {
        return Ok(());
    };
    let last = shard.last().unwrap_or(first);
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    let Some(entry) = progress.get_mut(novel_id) else {
        return Ok(());
    };
    entry.phase = Some(phase.to_string());
    entry.shard_total = shard_total;
    entry.active_shards.insert(
        shard_index,
        ActiveShardProgress {
            index: shard_index + 1,
            total: shard_total,
            start_chapter: first.index,
            end_chapter: last.index,
            phase: phase.to_string(),
        },
    );
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn report_auto_shard_phase(
    state: &State<'_, AppState>,
    novel_id: &str,
    shard_index: usize,
    phase: &str,
) -> Result<(), String> {
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    let Some(entry) = progress.get_mut(novel_id) else {
        return Ok(());
    };
    entry.phase = Some(phase.to_string());
    if let Some(shard) = entry.active_shards.get_mut(&shard_index) {
        shard.phase = phase.to_string();
    }
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn report_auto_shard_phase_for_chapters(
    state: &State<'_, AppState>,
    novel_id: &str,
    chapters: &[Chapter],
    phase: &str,
) -> Result<(), String> {
    let Some(first) = chapters.first() else {
        return Ok(());
    };
    let last = chapters.last().unwrap_or(first);
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    let Some(entry) = progress.get_mut(novel_id) else {
        return Ok(());
    };
    entry.phase = Some(phase.to_string());
    if let Some(shard) = entry
        .active_shards
        .values_mut()
        .find(|shard| shard.start_chapter == first.index && shard.end_chapter == last.index)
    {
        shard.phase = phase.to_string();
    }
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

pub(crate) fn report_auto_shard_completed(
    state: &State<'_, AppState>,
    novel_id: &str,
    shard_index: usize,
    chapters: &[Chapter],
) -> Result<(), String> {
    let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
    let Some(entry) = progress.get_mut(novel_id) else {
        return Ok(());
    };
    entry.active_shards.remove(&shard_index);
    entry.completed_shards.insert(shard_index);
    entry
        .completed_chapter_ids
        .extend(chapters.iter().map(|chapter| chapter.id.clone()));
    drop(progress);
    emit_auto_runtime_progress(state, novel_id)
}

fn emit_auto_runtime_progress(state: &State<'_, AppState>, novel_id: &str) -> Result<(), String> {
    let control = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(novel_id)
        .cloned();
    let progress_state = state
        .auto_run_progress
        .lock()
        .map_err(to_string)?
        .get(novel_id)
        .cloned()
        .unwrap_or_default();
    let job_id = progress_state.job_id.as_deref().or_else(|| {
        control
            .as_ref()
            .and_then(|control| control.job_id.as_deref())
    });
    let Some(job_id) = job_id else {
        return Ok(());
    };
    let conn = state.conn.lock().map_err(to_string)?;
    let job = conn
        .query_row(
            "SELECT id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at FROM jobs WHERE id = ?1",
            params![job_id],
            row_to_job,
        )
        .map_err(to_string)?;
    drop(conn);
    let phase = progress_state.phase.as_deref().unwrap_or("rewrite");
    let batch_index = progress_state.batch_index.unwrap_or(0);
    let batch_total = progress_state.batch_total.unwrap_or(job.total_chapters);
    let shard_completed = progress_state.completed_shards.len();
    let shard_total = progress_state.shard_total;
    let chapter_completed = progress_state.completed_chapter_ids.len();
    let chapter_total = progress_state.chapter_total;
    let message = if shard_total > 0 {
        format!(
            "第 {}/{} 批 · {} · 章节已完成 {}/{} · 分片已完成 {}/{}",
            batch_index,
            batch_total,
            phase_label(phase),
            chapter_completed,
            chapter_total,
            shard_completed,
            shard_total
        )
    } else {
        format!(
            "第 {}/{} 批 · {}",
            batch_index,
            batch_total,
            phase_label(phase)
        )
    };
    let current_chapter = control
        .as_ref()
        .map(|control| {
            control
                .completed_batches
                .saturating_sub(control.start_batch_index)
        })
        .unwrap_or_else(|| {
            i64::try_from(chapter_completed)
                .unwrap_or(i64::MAX)
                .min(job.total_chapters)
        });
    update_job(state, &job.id, "running", current_chapter, &message)?;
    let payload = JobProgress {
        id: job.id,
        novel_id: job.novel_id,
        job_type: job.job_type,
        status: "running".to_string(),
        current_chapter,
        total_chapters: job.total_chapters,
        message,
        phase: progress_state.phase,
        batch_index: progress_state.batch_index,
        batch_total: progress_state.batch_total,
        batch_label: progress_state.batch_label,
        shard_completed: Some(shard_completed),
        shard_total: Some(shard_total),
        chapter_completed: Some(chapter_completed),
        chapter_total: Some(chapter_total),
        active_shards: Some(progress_state.active_shards.into_values().collect()),
    };
    let _ = state.app.emit("job-progress", payload);
    Ok(())
}
