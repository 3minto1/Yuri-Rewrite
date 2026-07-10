use crate::commands::settings::{
    load_core_prompt, load_review_profile_id, load_rewrite_parallelism,
};
use crate::domain::{
    AppState, Chapter, ModelProfile, NovelSettings, RewriteAbApplyResult, RewriteAbCandidate,
    RewriteAbChapterDetail, RewriteAbChapterSummary, RewriteAbChoice, RewriteAbEstimate,
    RewriteAbModelSummary, RewriteAbRunDetail, RewriteAbRunSummary, RewriteAbStageTarget,
};
use crate::services::estimation::{load_recent_model_stats, RecentModelStats};
use crate::services::shard_context::build_contiguous_shard_work;
use crate::task_control::CancellableTaskPermit;
use crate::{
    build_relevant_canon_text, chapter_has_source_body, ensure_name_mapping_asset,
    load_canon_assets, load_chapter_batches, load_chapters, load_chapters_for_batch,
    load_model_profile, load_review_profile_for_run, read_stored_api_key,
    rewrite_batch_with_parallelism, to_string,
};
use chrono::Utc;
use futures_util::{stream, StreamExt};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use tauri::{Emitter, State};
use uuid::Uuid;

const AB_JOB_TYPE: &str = "rewrite_ab";
const AB_TERMINATED: &str = "A/B 改写已终止。";

#[derive(Debug, Serialize, Deserialize)]
struct RewriteAbInputSnapshot {
    settings: NovelSettings,
    core_prompt: String,
    canon_text: String,
    review_profile: Option<ModelProfile>,
    parallelism: usize,
}

#[derive(Debug)]
struct StoredAbRun {
    novel_id: String,
    status: String,
    snapshot: RewriteAbInputSnapshot,
    review_enabled: bool,
}

#[derive(Debug)]
struct AbWorkItem {
    slot: String,
    profile: ModelProfile,
    api_key: String,
    chapters: Vec<Chapter>,
}

#[derive(Debug)]
struct StoredAbModel {
    slot: String,
    profile: ModelProfile,
}

fn hash_text(parts: impl IntoIterator<Item = String>) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn batch_fingerprint(chapters: &[Chapter]) -> String {
    hash_text(chapters.iter().map(|chapter| chapter.id.clone()))
}

fn canonical_fingerprint(
    title: &str,
    rewrite_text: Option<&str>,
    ai_rewrite_text: Option<&str>,
    rewrite_edited_at: Option<&str>,
    rewrite_status: &str,
) -> String {
    hash_text([
        title.to_string(),
        rewrite_text.unwrap_or("").to_string(),
        ai_rewrite_text.unwrap_or("").to_string(),
        rewrite_edited_at.unwrap_or("").to_string(),
        rewrite_status.to_string(),
    ])
}

fn validate_profile_ids(profile_ids: &[String]) -> Result<(), String> {
    if !(2..=3).contains(&profile_ids.len()) {
        return Err("A/B 改写必须选择 2 至 3 个模型配置。".to_string());
    }
    let unique = profile_ids.iter().collect::<HashSet<_>>();
    if unique.len() != profile_ids.len() {
        return Err("A/B 改写不能重复选择同一个模型配置。".to_string());
    }
    Ok(())
}

fn validate_batch_chapters(chapters: &[Chapter]) -> Result<Vec<Chapter>, String> {
    let eligible = chapters
        .iter()
        .filter(|chapter| chapter_has_source_body(chapter))
        .cloned()
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        return Err("当前批次没有可用于 A/B 改写的正文。".to_string());
    }
    if eligible
        .iter()
        .any(|chapter| chapter.analysis_status != "completed")
    {
        return Err("当前批次仍有正文未完成分析，请先完成整批分析。".to_string());
    }
    Ok(eligible)
}

fn ensure_no_paused_auto_run(state: &State<'_, AppState>, novel_id: &str) -> Result<(), String> {
    if state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .contains_key(novel_id)
    {
        Err("当前小说存在运行中或暂停的一键任务，请先终止该任务。".to_string())
    } else {
        Ok(())
    }
}

fn load_batch(
    conn: &Connection,
    novel_id: &str,
    batch_id: &str,
) -> Result<(String, Vec<Chapter>), String> {
    let batch = load_chapter_batches(conn, novel_id)?
        .into_iter()
        .find(|batch| batch.id == batch_id)
        .ok_or_else(|| "未找到要进行 A/B 改写的批次。".to_string())?;
    let chapters = load_chapters_for_batch(conn, novel_id, batch_id)?;
    Ok((batch.label, chapters))
}

#[tauri::command]
pub(crate) fn estimate_rewrite_ab(
    novel_id: String,
    batch_id: String,
    profile_ids: Vec<String>,
    review_enabled: bool,
    state: State<'_, AppState>,
) -> Result<RewriteAbEstimate, String> {
    validate_profile_ids(&profile_ids)?;
    let profiles = profile_ids
        .iter()
        .map(|profile_id| load_model_profile(&state, profile_id))
        .collect::<Result<Vec<_>, _>>()?;
    let (chapters, configured_parallelism, recent_stats, existing_run_id, review_profile_id) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let (_, chapters) = load_batch(&conn, &novel_id, &batch_id)?;
        let chapters = validate_batch_chapters(&chapters)?;
        let existing_run_id =
            find_existing_run_id(&conn, &novel_id, &batch_fingerprint(&chapters))?;
        let configured_parallelism = load_rewrite_parallelism(&conn)?;
        let review_profile_id = if review_enabled {
            load_review_profile_id(&conn)?
        } else {
            None
        };
        let mut stats_profile_ids = profile_ids.clone();
        if let Some(review_profile_id) = review_profile_id.as_ref() {
            stats_profile_ids.push(review_profile_id.clone());
        }
        let recent_stats = load_aggregate_recent_model_stats(&conn, &stats_profile_ids)?;
        (
            chapters,
            configured_parallelism,
            recent_stats,
            existing_run_id,
            review_profile_id,
        )
    };
    let review_profile = review_profile_id
        .as_deref()
        .filter(|review_id| !profile_ids.iter().any(|profile_id| profile_id == review_id))
        .map(|review_id| load_model_profile(&state, review_id))
        .transpose()?;
    let mut effective_profiles = profiles.iter().collect::<Vec<_>>();
    if let Some(review_profile) = review_profile.as_ref() {
        effective_profiles.push(review_profile);
    }
    let parallelism = state
        .rate_limits
        .effective_parallelism(configured_parallelism, &effective_profiles)?;
    let shard_count = chapters.len().min(parallelism.max(1));
    let estimated_requests = estimate_ab_request_count(shard_count, profiles.len(), review_enabled);
    let average_call_seconds = recent_stats.average_call_seconds();
    Ok(RewriteAbEstimate {
        existing_run_id,
        chapter_count: chapters.len(),
        model_count: profile_ids.len(),
        shard_count,
        estimated_requests,
        estimated_seconds: estimate_ab_queue_seconds(
            estimated_requests,
            parallelism,
            average_call_seconds,
        ),
        average_call_seconds,
        recent_success_calls: recent_stats.success_calls,
    })
}

fn estimate_ab_request_count(
    shard_count: usize,
    model_count: usize,
    review_enabled: bool,
) -> usize {
    shard_count * model_count * if review_enabled { 2 } else { 1 }
}

fn load_aggregate_recent_model_stats(
    conn: &Connection,
    profile_ids: &[String],
) -> Result<RecentModelStats, String> {
    let mut aggregate = RecentModelStats::default();
    let mut seen = HashSet::new();
    for profile_id in profile_ids {
        if seen.insert(profile_id.as_str()) {
            aggregate.merge(load_recent_model_stats(conn, profile_id)?);
        }
    }
    Ok(aggregate)
}

fn estimate_ab_queue_seconds(
    estimated_requests: usize,
    parallelism: usize,
    average_call_seconds: Option<f64>,
) -> Option<f64> {
    average_call_seconds.map(|average| {
        let waves = estimated_requests.div_ceil(parallelism.max(1));
        average * waves as f64
    })
}

fn find_existing_run_id(
    conn: &Connection,
    novel_id: &str,
    fingerprint: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT id FROM rewrite_ab_runs
         WHERE novel_id = ?1 AND batch_fingerprint = ?2
         ORDER BY created_at DESC LIMIT 1",
        params![novel_id, fingerprint],
        |row| row.get(0),
    )
    .optional()
    .map_err(to_string)
}

#[tauri::command]
pub(crate) async fn start_rewrite_ab(
    novel_id: String,
    batch_id: String,
    profile_ids: Vec<String>,
    review_enabled: bool,
    replace_run_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<RewriteAbRunDetail, String> {
    validate_profile_ids(&profile_ids)?;
    ensure_no_paused_auto_run(&state, &novel_id)?;

    let profiles = profile_ids
        .iter()
        .map(|profile_id| load_model_profile(&state, profile_id))
        .collect::<Result<Vec<_>, _>>()?;
    let api_keys = profiles
        .iter()
        .map(|profile| read_stored_api_key(&state, &profile.id))
        .collect::<Result<Vec<_>, _>>()?;
    let (settings, core_prompt, parallelism, review_profile_id, batch_label, all_batch_chapters) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = crate::require_novel_settings(&conn, &novel_id)?;
        let (batch_label, chapters) = load_batch(&conn, &novel_id, &batch_id)?;
        (
            settings,
            load_core_prompt(&conn)?,
            load_rewrite_parallelism(&conn)?,
            load_review_profile_id(&conn)?,
            batch_label,
            chapters,
        )
    };
    let chapters = validate_batch_chapters(&all_batch_chapters)?;
    let (review_profile, review_api_key) = load_review_profile_for_run(
        &state,
        &profiles[0],
        review_enabled,
        review_profile_id.as_deref(),
    )?;
    let mut active_profile_ids = profile_ids.iter().map(String::as_str).collect::<Vec<_>>();
    if let Some(review_profile) = review_profile.as_ref() {
        if !profile_ids.iter().any(|id| id == &review_profile.id) {
            active_profile_ids.push(review_profile.id.as_str());
        }
    }
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, active_profile_ids, "A/B 改写")?;

    ensure_name_mapping_asset(&state, &novel_id, &profiles[0], &api_keys[0], &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, &novel_id)?
    };
    let canon_text = build_relevant_canon_text(&canon_assets, &chapters, &settings);
    let snapshot = RewriteAbInputSnapshot {
        settings,
        core_prompt,
        canon_text,
        review_profile,
        parallelism,
    };
    if review_enabled {
        if let Some(profile) = snapshot.review_profile.as_ref() {
            if !profiles.iter().any(|candidate| candidate.id == profile.id) {
                let _ = review_api_key
                    .as_deref()
                    .ok_or_else(|| "复检模型缺少 API Key。".to_string())?;
            }
        }
    }

    let run_id = create_rewrite_ab_run(
        &state,
        &novel_id,
        &batch_id,
        &batch_label,
        &chapters,
        &profiles,
        &snapshot,
        review_enabled,
        replace_run_id.as_deref(),
    )?;
    let cancellation = match state.rewrite_ab_tasks.register(&novel_id) {
        Ok(cancellation) => cancellation,
        Err(error) => {
            finish_ab_run_partial(&state, &run_id, "", &error)?;
            return load_rewrite_ab_run_from_state(&state, &run_id);
        }
    };
    let _ = emit_rewrite_ab_progress(&state, &run_id, "running", "正在生成 A/B 候选", None);
    execute_rewrite_ab_run(&state, &run_id, &cancellation).await
}

#[allow(clippy::too_many_arguments)]
fn create_rewrite_ab_run(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_id: &str,
    batch_label: &str,
    chapters: &[Chapter],
    profiles: &[ModelProfile],
    snapshot: &RewriteAbInputSnapshot,
    review_enabled: bool,
    replace_run_id: Option<&str>,
) -> Result<String, String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let fingerprint = batch_fingerprint(chapters);
    let existing = conn
        .query_row(
            "SELECT id, status FROM rewrite_ab_runs
             WHERE novel_id = ?1 AND batch_fingerprint = ?2
             ORDER BY created_at DESC LIMIT 1",
            params![novel_id, fingerprint],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(to_string)?;
    if let Some((existing_id, existing_status)) = existing {
        if replace_run_id != Some(existing_id.as_str()) {
            return Err(format!(
                "当前批次已有 A/B 实验（{}，状态 {}）。确认替换后请提交 replaceRunId。",
                existing_id, existing_status
            ));
        }
        if existing_status == "running" {
            return Err("当前批次的 A/B 实验仍在运行，不能替换。".to_string());
        }
    } else if replace_run_id.is_some() {
        return Err("要替换的 A/B 实验已不存在，请刷新后重试。".to_string());
    }

    let run_id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let snapshot_json = serde_json::to_string(snapshot).map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    if let Some(existing_id) = replace_run_id {
        tx.execute(
            "DELETE FROM rewrite_ab_runs WHERE id = ?1",
            params![existing_id],
        )
        .map_err(to_string)?;
    }
    tx.execute(
        "INSERT INTO jobs (
            id, novel_id, job_type, status, current_chapter, total_chapters,
            message, created_at, updated_at
         ) VALUES (?1, ?2, ?3, 'running', 0, ?4, '正在准备 A/B 改写', ?5, ?5)",
        params![
            job_id,
            novel_id,
            AB_JOB_TYPE,
            (chapters.len() * profiles.len()) as i64,
            now
        ],
    )
    .map_err(to_string)?;
    tx.execute(
        "INSERT INTO rewrite_ab_runs (
            id, novel_id, batch_id, batch_label, batch_fingerprint,
            input_snapshot_json, job_id, status, review_enabled, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8, ?9, ?9)",
        params![
            run_id,
            novel_id,
            batch_id,
            batch_label,
            fingerprint,
            snapshot_json,
            job_id,
            review_enabled,
            now
        ],
    )
    .map_err(to_string)?;
    for (index, profile) in profiles.iter().enumerate() {
        let slot = slot_for_index(index)?;
        let profile_json = serde_json::to_string(profile).map_err(to_string)?;
        tx.execute(
            "INSERT INTO rewrite_ab_models (run_id, slot, profile_id, profile_snapshot_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![run_id, slot, profile.id, profile_json],
        )
        .map_err(to_string)?;
    }
    for chapter in chapters {
        let baseline = load_chapter_baseline(&tx, &chapter.id)?;
        let source_fingerprint = canonical_fingerprint(
            &baseline.title,
            baseline.rewrite_text.as_deref(),
            baseline.ai_rewrite_text.as_deref(),
            baseline.rewrite_edited_at.as_deref(),
            &baseline.rewrite_status,
        );
        tx.execute(
            "INSERT INTO rewrite_ab_chapters (
                run_id, chapter_id, chapter_index, original_title, original_text,
                analysis_json, analysis_status, baseline_title, baseline_rewrite_text,
                baseline_ai_rewrite_text, baseline_rewrite_edited_at, baseline_rewrite_status,
                source_fingerprint, selected_slot
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
            params![
                run_id,
                chapter.id,
                chapter.index,
                chapter.title,
                chapter.original_text,
                chapter.analysis_json,
                chapter.analysis_status,
                baseline.title,
                baseline.rewrite_text,
                baseline.ai_rewrite_text,
                baseline.rewrite_edited_at,
                baseline.rewrite_status,
                source_fingerprint
            ],
        )
        .map_err(to_string)?;
        for index in 0..profiles.len() {
            tx.execute(
                "INSERT INTO rewrite_ab_candidates (
                    run_id, chapter_id, slot, status, updated_at
                 ) VALUES (?1, ?2, ?3, 'pending', ?4)",
                params![run_id, chapter.id, slot_for_index(index)?, now],
            )
            .map_err(to_string)?;
        }
    }
    tx.commit().map_err(to_string)?;
    Ok(run_id)
}

struct ChapterBaseline {
    title: String,
    rewrite_text: Option<String>,
    ai_rewrite_text: Option<String>,
    rewrite_edited_at: Option<String>,
    rewrite_status: String,
}

fn load_chapter_baseline(
    conn: &Transaction<'_>,
    chapter_id: &str,
) -> Result<ChapterBaseline, String> {
    conn.query_row(
        "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at, rewrite_status
         FROM chapters WHERE id = ?1",
        params![chapter_id],
        |row| {
            Ok(ChapterBaseline {
                title: row.get(0)?,
                rewrite_text: row.get(1)?,
                ai_rewrite_text: row.get(2)?,
                rewrite_edited_at: row.get(3)?,
                rewrite_status: row.get(4)?,
            })
        },
    )
    .map_err(to_string)
}

fn slot_for_index(index: usize) -> Result<&'static str, String> {
    match index {
        0 => Ok("A"),
        1 => Ok("B"),
        2 => Ok("C"),
        _ => Err("A/B 改写只支持 A、B、C 三个槽位。".to_string()),
    }
}

#[tauri::command]
pub(crate) async fn retry_rewrite_ab(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<RewriteAbRunDetail, String> {
    let (novel_id, status, profile_ids) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let (novel_id, status): (String, String) = conn
            .query_row(
                "SELECT novel_id, status FROM rewrite_ab_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| format!("未找到 A/B 实验：{}", to_string(error)))?;
        let profile_ids = load_ab_profile_ids(&conn, &run_id)?;
        (novel_id, status, profile_ids)
    };
    if !matches!(status.as_str(), "partial" | "ready") {
        return Err("只有部分失败或待补全的 A/B 实验可以重试。".to_string());
    }
    ensure_no_paused_auto_run(&state, &novel_id)?;
    let _active_task = state
        .active_tasks
        .acquire(&novel_id, profile_ids.iter(), "A/B 改写重试")?;
    let cancellation = state.rewrite_ab_tasks.register(&novel_id)?;
    {
        let mut conn = state.conn.lock().map_err(to_string)?;
        prepare_rewrite_ab_retry(&mut conn, &run_id)?;
    }
    let _ = emit_rewrite_ab_progress(&state, &run_id, "running", "正在重试 A/B 改写", None);
    execute_rewrite_ab_run(&state, &run_id, &cancellation).await
}

fn prepare_rewrite_ab_retry(conn: &mut Connection, run_id: &str) -> Result<String, String> {
    let tx = conn.transaction().map_err(to_string)?;
    let (novel_id, status): (String, String) = tx
        .query_row(
            "SELECT novel_id, status FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(to_string)?;
    if !matches!(status.as_str(), "partial" | "ready") {
        return Err("只有部分失败或待补全的 A/B 实验可以重试。".to_string());
    }
    let total: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM rewrite_ab_candidates WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    let completed: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM rewrite_ab_candidates
             WHERE run_id = ?1 AND status = 'completed'",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    let job_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO jobs (
            id, novel_id, job_type, status, current_chapter, total_chapters,
            message, created_at, updated_at
         ) VALUES (?1, ?2, ?3, 'running', ?4, ?5, '正在重试 A/B 改写', ?6, ?6)",
        params![job_id, novel_id, AB_JOB_TYPE, completed, total, now],
    )
    .map_err(to_string)?;
    let changed = tx
        .execute(
            "UPDATE rewrite_ab_runs
             SET job_id = ?2, status = 'running', updated_at = ?3
             WHERE id = ?1 AND status IN ('partial', 'ready')",
            params![run_id, job_id, now],
        )
        .map_err(to_string)?;
    if changed != 1 {
        return Err("A/B 实验状态已变化，未创建重试任务。".to_string());
    }
    tx.commit().map_err(to_string)?;
    Ok(job_id)
}

async fn execute_rewrite_ab_run(
    state: &State<'_, AppState>,
    run_id: &str,
    cancellation: &CancellableTaskPermit<'_>,
) -> Result<RewriteAbRunDetail, String> {
    match execute_rewrite_ab_run_inner(state, run_id, cancellation).await {
        Ok(detail) => Ok(detail),
        Err(error) => {
            if let Err(finalize_error) = finish_ab_run_partial(state, run_id, "", &error) {
                return Err(format!(
                    "{error}；同时将 A/B 实验收尾为部分失败时出错：{finalize_error}"
                ));
            }
            load_rewrite_ab_run_from_state(state, run_id).map_err(|load_error| {
                format!("{error}；A/B 实验已标记为部分失败，但刷新结果失败：{load_error}")
            })
        }
    }
}

async fn execute_rewrite_ab_run_inner(
    state: &State<'_, AppState>,
    run_id: &str,
    cancellation: &CancellableTaskPermit<'_>,
) -> Result<RewriteAbRunDetail, String> {
    let (run, models, all_chapters) = {
        let conn = state.conn.lock().map_err(to_string)?;
        (
            load_stored_run(&conn, run_id)?,
            load_stored_models(&conn, run_id)?,
            load_stored_chapters(&conn, run_id)?,
        )
    };
    if run.status != "running" {
        return Err("A/B 实验当前不处于运行状态。".to_string());
    }
    let review_api_key = match run.snapshot.review_profile.as_ref() {
        Some(profile) if run.review_enabled => match read_stored_api_key(state, &profile.id) {
            Ok(api_key) => Some(api_key),
            Err(error) => {
                finish_ab_run_partial(state, run_id, "", &error)?;
                return load_rewrite_ab_run_from_state(state, run_id);
            }
        },
        _ => None,
    };

    let mut effective_profiles = models
        .iter()
        .map(|model| &model.profile)
        .collect::<Vec<_>>();
    if let Some(review_profile) = run.snapshot.review_profile.as_ref() {
        if !effective_profiles
            .iter()
            .any(|profile| profile.id == review_profile.id)
        {
            effective_profiles.push(review_profile);
        }
    }
    let total_parallelism = state
        .rate_limits
        .effective_parallelism(run.snapshot.parallelism, &effective_profiles)?;
    let mut per_slot_work = Vec::new();
    for model in models {
        let pending = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_pending_chapters(&conn, run_id, &model.slot, &all_chapters)?
        };
        if pending.is_empty() {
            continue;
        }
        let api_key = match read_stored_api_key(state, &model.profile.id) {
            Ok(api_key) => api_key,
            Err(error) => {
                finish_ab_run_partial(state, run_id, &model.slot, &error)?;
                return load_rewrite_ab_run_from_state(state, run_id);
            }
        };
        mark_candidate_slot_running(state, run_id, &model.slot, &pending)?;
        per_slot_work.push(
            build_contiguous_shard_work(&all_chapters, &pending, total_parallelism)
                .into_iter()
                .map(|work| AbWorkItem {
                    slot: model.slot.clone(),
                    profile: model.profile.clone(),
                    api_key: api_key.clone(),
                    chapters: work.chapters,
                })
                .collect::<VecDeque<_>>(),
        );
    }
    let mut work_items = Vec::new();
    while per_slot_work.iter().any(|work| !work.is_empty()) {
        for work in &mut per_slot_work {
            if let Some(item) = work.pop_front() {
                work_items.push(item);
            }
        }
    }
    let run_novel_id = run.novel_id.as_str();
    let all_chapters_ref = all_chapters.as_slice();
    let canon_text = run.snapshot.canon_text.as_str();
    let settings = &run.snapshot.settings;
    let core_prompt = run.snapshot.core_prompt.as_str();
    let review_profile = run.snapshot.review_profile.as_ref();
    let review_api_key_ref = review_api_key.as_deref();
    let review_enabled = run.review_enabled;
    let tasks = stream::iter(work_items.into_iter().map(|item| {
        let target = RewriteAbStageTarget {
            run_id: run_id.to_string(),
            slot: item.slot.clone(),
        };
        async move {
            let rewrites = rewrite_batch_with_parallelism(
                state,
                run_novel_id,
                &item.profile,
                &item.api_key,
                all_chapters_ref,
                &item.chapters,
                canon_text,
                settings,
                core_prompt,
                review_enabled,
                review_profile,
                review_api_key_ref,
                1,
                None,
            )
            .await
            .map_err(|error| (item.slot.clone(), error))?;
            stage_rewrite_ab_shard(state, &target, &rewrites).map_err(|error| (item.slot, error))
        }
    }))
    .buffer_unordered(total_parallelism.max(1));
    futures_util::pin_mut!(tasks);
    loop {
        let next = tokio::select! {
            result = tasks.next() => result,
            _ = cancellation.cancelled() => {
                finish_ab_run_partial(state, run_id, "", AB_TERMINATED)?;
                return load_rewrite_ab_run_from_state(state, run_id);
            }
        };
        let Some(result) = next else {
            break;
        };
        if let Err((slot, error)) = result {
            finish_ab_run_partial(state, run_id, &slot, &error)?;
            return load_rewrite_ab_run_from_state(state, run_id);
        }
    }
    finish_ab_run_ready(state, run_id)?;
    load_rewrite_ab_run_from_state(state, run_id)
}

fn load_stored_run(conn: &Connection, run_id: &str) -> Result<StoredAbRun, String> {
    conn.query_row(
        "SELECT id, novel_id, COALESCE(job_id, ''), status, input_snapshot_json, review_enabled
         FROM rewrite_ab_runs WHERE id = ?1",
        params![run_id],
        |row| {
            let snapshot_json: String = row.get(4)?;
            let snapshot = serde_json::from_str::<RewriteAbInputSnapshot>(&snapshot_json).map_err(
                |error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        snapshot_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                },
            )?;
            Ok(StoredAbRun {
                novel_id: row.get(1)?,
                status: row.get(3)?,
                snapshot,
                review_enabled: row.get::<_, i64>(5)? != 0,
            })
        },
    )
    .map_err(to_string)
}

fn load_stored_models(conn: &Connection, run_id: &str) -> Result<Vec<StoredAbModel>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT slot, profile_snapshot_json FROM rewrite_ab_models
             WHERE run_id = ?1 ORDER BY slot",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            let snapshot_json: String = row.get(1)?;
            let profile =
                serde_json::from_str::<ModelProfile>(&snapshot_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        snapshot_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            Ok(StoredAbModel {
                slot: row.get(0)?,
                profile,
            })
        })
        .map_err(to_string)?;
    let models = rows.collect::<Result<Vec<_>, _>>().map_err(to_string)?;
    Ok(models)
}

fn load_stored_chapters(conn: &Connection, run_id: &str) -> Result<Vec<Chapter>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT chapter_id, chapter_index, original_title, original_text,
                    analysis_json, analysis_status, baseline_rewrite_text,
                    baseline_rewrite_edited_at, baseline_rewrite_status,
                    (SELECT novel_id FROM rewrite_ab_runs WHERE id = ?1)
             FROM rewrite_ab_chapters WHERE run_id = ?1 ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok(Chapter {
                id: row.get(0)?,
                novel_id: row.get(9)?,
                index: row.get(1)?,
                title: row.get(2)?,
                original_text: row.get(3)?,
                analysis_json: row.get(4)?,
                rewrite_text: row.get(6)?,
                rewrite_edited: row.get::<_, Option<String>>(7)?.is_some(),
                single_rewrite_original_available: false,
                analysis_status: row.get(5)?,
                rewrite_status: row.get(8)?,
            })
        })
        .map_err(to_string)?;
    let chapters = rows.collect::<Result<Vec<_>, _>>().map_err(to_string)?;
    Ok(chapters)
}

fn load_pending_chapters(
    conn: &Connection,
    run_id: &str,
    slot: &str,
    chapters: &[Chapter],
) -> Result<Vec<Chapter>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT chapter_id FROM rewrite_ab_candidates
             WHERE run_id = ?1 AND slot = ?2 AND status != 'completed'",
        )
        .map_err(to_string)?;
    let pending_ids = stmt
        .query_map(params![run_id, slot], |row| row.get::<_, String>(0))
        .map_err(to_string)?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters
        .iter()
        .filter(|chapter| pending_ids.contains(&chapter.id))
        .cloned()
        .collect())
}

fn mark_candidate_slot_running(
    state: &State<'_, AppState>,
    run_id: &str,
    slot: &str,
    chapters: &[Chapter],
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    for chapter in chapters {
        tx.execute(
            "UPDATE rewrite_ab_candidates
             SET status = 'running', title = NULL, content = NULL,
                 review_summary = NULL, error = NULL, updated_at = ?4
             WHERE run_id = ?1 AND chapter_id = ?2 AND slot = ?3
               AND status != 'completed'",
            params![run_id, chapter.id, slot, now],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)
}

fn finish_ab_run_partial(
    state: &State<'_, AppState>,
    run_id: &str,
    slot: &str,
    error: &str,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE rewrite_ab_candidates
         SET status = 'failed', error = CASE WHEN slot = ?2 THEN ?3 ELSE '同组分片失败或任务终止，已取消未完成请求：' || ?3 END,
             updated_at = ?4
         WHERE run_id = ?1 AND status IN ('pending', 'running')",
        params![run_id, slot, error, now],
    )
    .map_err(to_string)?;
    tx.execute(
        "UPDATE rewrite_ab_runs SET status = 'partial', updated_at = ?2 WHERE id = ?1",
        params![run_id, now],
    )
    .map_err(to_string)?;
    tx.execute(
        "UPDATE jobs SET status = 'failed', message = ?2, updated_at = ?3
         WHERE id = (SELECT job_id FROM rewrite_ab_runs WHERE id = ?1)",
        params![run_id, error, now],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    drop(conn);
    let _ = emit_rewrite_ab_progress(state, run_id, "failed", error, Some(slot));
    Ok(())
}

fn finish_ab_run_ready(state: &State<'_, AppState>, run_id: &str) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    let remaining: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM rewrite_ab_candidates
             WHERE run_id = ?1 AND status != 'completed'",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    let (status, job_status, message) = if remaining == 0 {
        ("ready", "completed", "A/B 候选生成完成")
    } else {
        ("partial", "failed", "A/B 候选仍有缺失，可重试失败候选")
    };
    tx.execute(
        "UPDATE rewrite_ab_runs SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![run_id, status, now],
    )
    .map_err(to_string)?;
    let completed: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM rewrite_ab_candidates
             WHERE run_id = ?1 AND status = 'completed'",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    tx.execute(
        "UPDATE jobs SET status = ?2, current_chapter = ?3, message = ?4, updated_at = ?5
         WHERE id = (SELECT job_id FROM rewrite_ab_runs WHERE id = ?1)",
        params![run_id, job_status, completed, message, now],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    drop(conn);
    let _ = emit_rewrite_ab_progress(state, run_id, job_status, message, None);
    Ok(())
}

fn emit_rewrite_ab_progress(
    state: &State<'_, AppState>,
    run_id: &str,
    status: &str,
    message: &str,
    slot: Option<&str>,
) -> Result<(), String> {
    let (novel_id, job_id, completed, total) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let (novel_id, job_id): (String, String) = conn
            .query_row(
                "SELECT novel_id, COALESCE(job_id, '') FROM rewrite_ab_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(to_string)?;
        let completed = conn
            .query_row(
                "SELECT COUNT(*) FROM rewrite_ab_candidates
                 WHERE run_id = ?1 AND status = 'completed'",
                params![run_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_string)?;
        let total = conn
            .query_row(
                "SELECT COUNT(*) FROM rewrite_ab_candidates WHERE run_id = ?1",
                params![run_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_string)?;
        (novel_id, job_id, completed, total)
    };
    let mut payload = json!({
        "id": job_id,
        "novel_id": novel_id,
        "job_type": AB_JOB_TYPE,
        "status": status,
        "current_chapter": completed,
        "total_chapters": total,
        "message": message,
        "phase": "rewrite",
        "candidate_completed": completed,
        "candidate_total": total,
    });
    if let Some(slot) = slot {
        payload["candidate_slot"] = json!(slot);
    }
    state.app.emit("job-progress", payload).map_err(to_string)
}

pub(crate) fn stage_rewrite_ab_shard(
    state: &State<'_, AppState>,
    target: &RewriteAbStageTarget,
    rewrites: &[crate::domain::ParsedChapterRewrite],
) -> Result<(), String> {
    let (novel_id, job_id, completed, total) = {
        let mut conn = state.conn.lock().map_err(to_string)?;
        let tx = conn.transaction().map_err(to_string)?;
        let now = Utc::now().to_rfc3339();
        let review_enabled: bool = tx
            .query_row(
                "SELECT review_enabled FROM rewrite_ab_runs WHERE id = ?1",
                params![target.run_id],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .map_err(to_string)?;
        for rewrite in rewrites {
            let changed = tx
                .execute(
                    "UPDATE rewrite_ab_candidates
                     SET status = 'completed', title = ?4, content = ?5,
                         review_summary = ?6, error = NULL, updated_at = ?7
                     WHERE run_id = ?1 AND chapter_id = ?2 AND slot = ?3",
                    params![
                        target.run_id,
                        rewrite.id,
                        target.slot,
                        rewrite.title.trim(),
                        rewrite.text.trim(),
                        review_enabled.then_some("已完成独立复检"),
                        now
                    ],
                )
                .map_err(to_string)?;
            if changed != 1 {
                return Err(format!("A/B 候选分片包含不属于实验的章节：{}", rewrite.id));
            }
        }
        tx.execute(
            "UPDATE rewrite_ab_runs SET updated_at = ?2 WHERE id = ?1",
            params![target.run_id, now],
        )
        .map_err(to_string)?;
        let (novel_id, job_id): (String, String) = tx
            .query_row(
                "SELECT novel_id, COALESCE(job_id, '') FROM rewrite_ab_runs WHERE id = ?1",
                params![target.run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(to_string)?;
        let completed = tx
            .query_row(
                "SELECT COUNT(*) FROM rewrite_ab_candidates
                 WHERE run_id = ?1 AND status = 'completed'",
                params![target.run_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_string)?;
        let total = tx
            .query_row(
                "SELECT COUNT(*) FROM rewrite_ab_candidates WHERE run_id = ?1",
                params![target.run_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(to_string)?;
        tx.execute(
            "UPDATE jobs SET current_chapter = ?2, message = ?3, updated_at = ?4
             WHERE id = ?1",
            params![
                job_id,
                completed,
                format!("已完成 {completed}/{total} 个 A/B 候选"),
                now
            ],
        )
        .map_err(to_string)?;
        tx.commit().map_err(to_string)?;
        (novel_id, job_id, completed, total)
    };
    state
        .app
        .emit(
            "job-progress",
            json!({
                "id": job_id,
                "novel_id": novel_id,
                "job_type": AB_JOB_TYPE,
                "status": "running",
                "current_chapter": completed,
                "total_chapters": total,
                "message": format!("已完成 {completed}/{total} 个 A/B 候选"),
                "phase": "rewrite",
                "candidate_completed": completed,
                "candidate_total": total,
                "candidate_slot": target.slot,
            }),
        )
        .map_err(to_string)
}

#[tauri::command]
pub(crate) fn terminate_rewrite_ab(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<RewriteAbRunDetail, String> {
    let novel_id = {
        let conn = state.conn.lock().map_err(to_string)?;
        conn.query_row(
            "SELECT novel_id FROM rewrite_ab_runs WHERE id = ?1 AND status = 'running'",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| "当前 A/B 实验未在运行。".to_string())?
    };
    if state.rewrite_ab_tasks.cancel(&novel_id)? {
        load_rewrite_ab_run_from_state(&state, &run_id)
    } else {
        Err("当前 A/B 实验没有可终止的运行任务。".to_string())
    }
}

#[tauri::command]
pub(crate) fn list_rewrite_ab_runs(
    novel_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<RewriteAbRunSummary>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare("SELECT id FROM rewrite_ab_runs WHERE novel_id = ?1 ORDER BY created_at DESC")
        .map_err(to_string)?;
    let ids = stmt
        .query_map(params![novel_id], |row| row.get::<_, String>(0))
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    ids.iter().map(|id| load_run_summary(&conn, id)).collect()
}

#[tauri::command]
pub(crate) fn get_rewrite_ab_run(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<RewriteAbRunDetail, String> {
    load_rewrite_ab_run_from_state(&state, &run_id)
}

fn load_rewrite_ab_run_from_state(
    state: &State<'_, AppState>,
    run_id: &str,
) -> Result<RewriteAbRunDetail, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    load_run_detail(&conn, run_id)
}

fn load_run_summary(conn: &Connection, run_id: &str) -> Result<RewriteAbRunSummary, String> {
    conn.query_row(
        "SELECT id, novel_id, batch_id, batch_label, batch_fingerprint, status,
                review_enabled, created_at, updated_at,
                (SELECT COUNT(*) FROM rewrite_ab_models WHERE run_id = rewrite_ab_runs.id),
                (SELECT COUNT(*) FROM rewrite_ab_chapters WHERE run_id = rewrite_ab_runs.id),
                (SELECT COUNT(*) FROM rewrite_ab_candidates
                 WHERE run_id = rewrite_ab_runs.id AND status = 'completed'),
                (SELECT COUNT(*) FROM rewrite_ab_candidates WHERE run_id = rewrite_ab_runs.id),
                (SELECT COUNT(*) FROM rewrite_ab_chapters
                 WHERE run_id = rewrite_ab_runs.id AND selected_slot IS NOT NULL)
         FROM rewrite_ab_runs WHERE id = ?1",
        params![run_id],
        |row| {
            Ok(RewriteAbRunSummary {
                id: row.get(0)?,
                novel_id: row.get(1)?,
                batch_id: row.get(2)?,
                batch_label: row.get(3)?,
                batch_fingerprint: row.get(4)?,
                status: row.get(5)?,
                review_enabled: row.get::<_, i64>(6)? != 0,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                model_count: row.get::<_, i64>(9)? as usize,
                chapter_count: row.get::<_, i64>(10)? as usize,
                completed_candidates: row.get::<_, i64>(11)? as usize,
                total_candidates: row.get::<_, i64>(12)? as usize,
                selected_chapters: row.get::<_, i64>(13)? as usize,
            })
        },
    )
    .map_err(to_string)
}

fn load_run_detail(conn: &Connection, run_id: &str) -> Result<RewriteAbRunDetail, String> {
    let summary = load_run_summary(conn, run_id)?;
    let models = load_model_summaries(conn, run_id)?;
    let chapter_rows = {
        let mut stmt = conn
            .prepare(
                "SELECT chapter_id, chapter_index, original_title, selected_slot
                 FROM rewrite_ab_chapters WHERE run_id = ?1 ORDER BY chapter_index",
            )
            .map_err(to_string)?;
        let rows = stmt
            .query_map(params![run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(to_string)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(to_string)?
    };
    let mut chapters = Vec::with_capacity(chapter_rows.len());
    for (chapter_id, chapter_index, title, selected_slot) in chapter_rows {
        let mut stmt = conn
            .prepare(
                "SELECT slot, status FROM rewrite_ab_candidates
                 WHERE run_id = ?1 AND chapter_id = ?2 ORDER BY slot",
            )
            .map_err(to_string)?;
        let statuses = stmt
            .query_map(params![run_id, chapter_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(to_string)?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(to_string)?;
        chapters.push(RewriteAbChapterSummary {
            chapter_id,
            chapter_index,
            title,
            selected_slot,
            candidate_statuses: statuses,
        });
    }
    Ok(RewriteAbRunDetail {
        summary,
        models,
        chapters,
    })
}

fn load_model_summaries(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<RewriteAbModelSummary>, String> {
    load_stored_models(conn, run_id).map(|models| {
        models
            .into_iter()
            .map(|model| RewriteAbModelSummary {
                slot: model.slot,
                profile_id: model.profile.id,
                profile_name: model.profile.name,
                provider: model.profile.provider,
                model: model.profile.model,
            })
            .collect()
    })
}

#[tauri::command]
pub(crate) fn get_rewrite_ab_chapter(
    run_id: String,
    chapter_id: String,
    state: State<'_, AppState>,
) -> Result<RewriteAbChapterDetail, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let (
        chapter_index,
        original_title,
        original_text,
        baseline_title,
        baseline_rewrite_text,
        selected_slot,
    ) = conn
        .query_row(
            "SELECT chapter_index, original_title, original_text, baseline_title,
                    baseline_rewrite_text, selected_slot
             FROM rewrite_ab_chapters WHERE run_id = ?1 AND chapter_id = ?2",
            params![run_id, chapter_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|error| format!("未找到 A/B 实验章节：{}", to_string(error)))?;
    let models = load_model_summaries(&conn, &run_id)?
        .into_iter()
        .map(|model| (model.slot.clone(), model))
        .collect::<HashMap<_, _>>();
    let mut stmt = conn
        .prepare(
            "SELECT slot, status, title, content, review_summary, error
             FROM rewrite_ab_candidates
             WHERE run_id = ?1 AND chapter_id = ?2 ORDER BY slot",
        )
        .map_err(to_string)?;
    let candidates = stmt
        .query_map(params![run_id, chapter_id], |row| {
            let slot: String = row.get(0)?;
            let model = models
                .get(&slot)
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            Ok(RewriteAbCandidate {
                slot,
                profile_id: model.profile_id.clone(),
                profile_name: model.profile_name.clone(),
                model: model.model.clone(),
                status: row.get(1)?,
                title: row.get(2)?,
                rewrite_text: row.get(3)?,
                review_summary: row.get(4)?,
                error: row.get(5)?,
            })
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(RewriteAbChapterDetail {
        run_id,
        chapter_id,
        chapter_index,
        original_title,
        original_text,
        baseline_title,
        baseline_rewrite_text,
        selected_slot,
        candidates,
    })
}

#[tauri::command]
pub(crate) fn save_rewrite_ab_choices(
    run_id: String,
    choices: Vec<RewriteAbChoice>,
    replace_all: Option<bool>,
    state: State<'_, AppState>,
) -> Result<RewriteAbRunDetail, String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    save_rewrite_ab_choices_in_connection(
        &mut conn,
        &run_id,
        &choices,
        replace_all.unwrap_or(false),
    )?;
    load_run_detail(&conn, &run_id)
}

fn save_rewrite_ab_choices_in_connection(
    conn: &mut Connection,
    run_id: &str,
    choices: &[RewriteAbChoice],
    replace_all: bool,
) -> Result<(), String> {
    let tx = conn.transaction().map_err(to_string)?;
    let status: String = tx
        .query_row(
            "SELECT status FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    if status == "running" {
        return Err("A/B 候选仍在生成，暂时不能保存选稿。".to_string());
    }
    let mut seen = HashSet::new();
    for choice in choices {
        if !seen.insert(&choice.chapter_id) {
            return Err(format!("章节 {} 出现重复选稿。", choice.chapter_id));
        }
    }
    if replace_all {
        let chapter_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM rewrite_ab_chapters WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(to_string)?;
        if choices.len() != chapter_count as usize {
            return Err(format!(
                "整批替换必须覆盖全部 {chapter_count} 章，当前仅提交 {} 章。",
                choices.len()
            ));
        }
    }
    for choice in choices {
        let completed: bool = tx
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM rewrite_ab_candidates
                    WHERE run_id = ?1 AND chapter_id = ?2 AND slot = ?3
                      AND status = 'completed' AND trim(COALESCE(content, '')) != ''
                 )",
                params![run_id, choice.chapter_id, choice.slot],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .map_err(to_string)?;
        if !completed {
            return Err(format!(
                "章节 {} 的候选 {} 尚未完成，不能选用。",
                choice.chapter_id, choice.slot
            ));
        }
    }
    if replace_all {
        tx.execute(
            "UPDATE rewrite_ab_chapters SET selected_slot = NULL WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(to_string)?;
    }
    for choice in choices {
        tx.execute(
            "UPDATE rewrite_ab_chapters SET selected_slot = ?3
             WHERE run_id = ?1 AND chapter_id = ?2",
            params![run_id, choice.chapter_id, choice.slot],
        )
        .map_err(to_string)?;
    }
    tx.execute(
        "UPDATE rewrite_ab_runs SET updated_at = ?2 WHERE id = ?1",
        params![run_id, Utc::now().to_rfc3339()],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn apply_rewrite_ab_choices(
    run_id: String,
    force_overwrite: bool,
    state: State<'_, AppState>,
) -> Result<RewriteAbApplyResult, String> {
    apply_or_restore_rewrite_ab(&run_id, force_overwrite, false, &state)
}

#[tauri::command]
pub(crate) fn restore_rewrite_ab_baseline(
    run_id: String,
    force_overwrite: bool,
    state: State<'_, AppState>,
) -> Result<RewriteAbApplyResult, String> {
    apply_or_restore_rewrite_ab(&run_id, force_overwrite, true, &state)
}

fn apply_or_restore_rewrite_ab(
    run_id: &str,
    force_overwrite: bool,
    restore: bool,
    state: &State<'_, AppState>,
) -> Result<RewriteAbApplyResult, String> {
    let novel_id = {
        let conn = state.conn.lock().map_err(to_string)?;
        conn.query_row(
            "SELECT novel_id FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?
    };
    ensure_no_paused_auto_run(state, &novel_id)?;
    let _active_task = state.active_tasks.acquire(
        &novel_id,
        std::iter::empty::<&str>(),
        if restore {
            "撤销 A/B 选稿"
        } else {
            "应用 A/B 选稿"
        },
    )?;
    let mut conn = state.conn.lock().map_err(to_string)?;
    let mut result =
        apply_or_restore_rewrite_ab_in_connection(&mut conn, run_id, force_overwrite, restore)?;
    if result.status != "conflict" {
        result.chapters = Some(load_chapters(&conn, &novel_id)?);
    }
    Ok(result)
}

fn apply_or_restore_rewrite_ab_in_connection(
    conn: &mut Connection,
    run_id: &str,
    force_overwrite: bool,
    restore: bool,
) -> Result<RewriteAbApplyResult, String> {
    let status: String = conn
        .query_row(
            "SELECT status FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    if restore && status != "applied" {
        return Err("只有已经应用的 A/B 实验可以撤销。".to_string());
    }
    if !restore && !matches!(status.as_str(), "ready" | "partial" | "applied") {
        return Err("当前 A/B 实验尚不能应用选稿。".to_string());
    }

    let rows = load_apply_rows(conn, run_id)?;
    if rows.is_empty() {
        return Err("A/B 实验不包含可应用的章节。".to_string());
    }
    if !restore
        && rows
            .iter()
            .any(|row| row.selected_slot.is_none() || row.candidate_content.is_none())
    {
        return Err("请为当前批次的每个章节选择一个已完成候选。".to_string());
    }
    let mut conflicts = Vec::new();
    for row in &rows {
        let current = load_current_chapter_state(conn, &row.chapter_id)?;
        let expected = if restore {
            row.applied_fingerprint
                .clone()
                .ok_or_else(|| "A/B 实验缺少已应用稿指纹，无法安全撤销。".to_string())?
        } else {
            row.source_fingerprint.clone()
        };
        if current.fingerprint != expected {
            conflicts.push(row.chapter_id.clone());
        }
    }
    if !conflicts.is_empty() && !force_overwrite {
        return Ok(RewriteAbApplyResult {
            status: "conflict".to_string(),
            conflict_chapter_ids: conflicts,
            chapters: None,
        });
    }

    let tx = conn.transaction().map_err(to_string)?;
    for row in &rows {
        if restore {
            tx.execute(
                "UPDATE chapters SET title = ?2, rewrite_text = ?3, ai_rewrite_text = ?4,
                        rewrite_edited_at = ?5, rewrite_status = ?6 WHERE id = ?1",
                params![
                    row.chapter_id,
                    row.baseline_title,
                    row.baseline_rewrite_text,
                    row.baseline_ai_rewrite_text,
                    row.baseline_rewrite_edited_at,
                    row.baseline_rewrite_status
                ],
            )
            .map_err(to_string)?;
            tx.execute(
                "UPDATE rewrite_ab_chapters
                 SET applied_slot = NULL, applied_fingerprint = NULL
                 WHERE run_id = ?1 AND chapter_id = ?2",
                params![run_id, row.chapter_id],
            )
            .map_err(to_string)?;
        } else {
            tx.execute(
                "DELETE FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
                params![row.chapter_id],
            )
            .map_err(to_string)?;
            tx.execute(
                "UPDATE chapters SET title = ?2, rewrite_text = ?3, ai_rewrite_text = ?3,
                        rewrite_edited_at = NULL, rewrite_status = 'completed' WHERE id = ?1",
                params![
                    row.chapter_id,
                    row.candidate_title
                        .as_deref()
                        .unwrap_or(&row.baseline_title),
                    row.candidate_content
                ],
            )
            .map_err(to_string)?;
            let applied_title = row
                .candidate_title
                .as_deref()
                .unwrap_or(&row.baseline_title);
            let applied_content = row
                .candidate_content
                .as_deref()
                .ok_or_else(|| "所选 A/B 候选正文缺失。".to_string())?;
            let applied_fingerprint = canonical_fingerprint(
                applied_title,
                Some(applied_content),
                Some(applied_content),
                None,
                "completed",
            );
            tx.execute(
                "UPDATE rewrite_ab_chapters
                 SET applied_slot = selected_slot, applied_fingerprint = ?3
                 WHERE run_id = ?1 AND chapter_id = ?2",
                params![run_id, row.chapter_id, applied_fingerprint],
            )
            .map_err(to_string)?;
        }
    }
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE rewrite_ab_runs SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![run_id, if restore { "ready" } else { "applied" }, now],
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    Ok(RewriteAbApplyResult {
        status: if restore { "restored" } else { "applied" }.to_string(),
        conflict_chapter_ids: Vec::new(),
        chapters: None,
    })
}

struct ApplyRow {
    chapter_id: String,
    baseline_title: String,
    baseline_rewrite_text: Option<String>,
    baseline_ai_rewrite_text: Option<String>,
    baseline_rewrite_edited_at: Option<String>,
    baseline_rewrite_status: String,
    source_fingerprint: String,
    selected_slot: Option<String>,
    candidate_title: Option<String>,
    candidate_content: Option<String>,
    applied_fingerprint: Option<String>,
}

fn load_apply_rows(conn: &Connection, run_id: &str) -> Result<Vec<ApplyRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT c.chapter_id, c.baseline_title, c.baseline_rewrite_text,
                    c.baseline_ai_rewrite_text, c.baseline_rewrite_edited_at,
                    c.baseline_rewrite_status, c.source_fingerprint, c.selected_slot,
                    candidate.title, candidate.content, c.applied_fingerprint
             FROM rewrite_ab_chapters c
             LEFT JOIN rewrite_ab_candidates candidate
               ON candidate.run_id = c.run_id
              AND candidate.chapter_id = c.chapter_id
              AND candidate.slot = c.selected_slot
              AND candidate.status = 'completed'
             WHERE c.run_id = ?1 ORDER BY c.chapter_index",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok(ApplyRow {
                chapter_id: row.get(0)?,
                baseline_title: row.get(1)?,
                baseline_rewrite_text: row.get(2)?,
                baseline_ai_rewrite_text: row.get(3)?,
                baseline_rewrite_edited_at: row.get(4)?,
                baseline_rewrite_status: row.get(5)?,
                source_fingerprint: row.get(6)?,
                selected_slot: row.get(7)?,
                candidate_title: row.get(8)?,
                candidate_content: row.get(9)?,
                applied_fingerprint: row.get(10)?,
            })
        })
        .map_err(to_string)?;
    let apply_rows = rows.collect::<Result<Vec<_>, _>>().map_err(to_string)?;
    Ok(apply_rows)
}

struct CurrentChapterState {
    fingerprint: String,
}

fn load_current_chapter_state(
    conn: &Connection,
    chapter_id: &str,
) -> Result<CurrentChapterState, String> {
    conn.query_row(
        "SELECT title, rewrite_text, ai_rewrite_text, rewrite_edited_at, rewrite_status
         FROM chapters WHERE id = ?1",
        params![chapter_id],
        |row| {
            let title: String = row.get(0)?;
            let rewrite_text: Option<String> = row.get(1)?;
            let ai_rewrite_text: Option<String> = row.get(2)?;
            let rewrite_edited_at: Option<String> = row.get(3)?;
            let rewrite_status: String = row.get(4)?;
            Ok(CurrentChapterState {
                fingerprint: canonical_fingerprint(
                    &title,
                    rewrite_text.as_deref(),
                    ai_rewrite_text.as_deref(),
                    rewrite_edited_at.as_deref(),
                    &rewrite_status,
                ),
            })
        },
    )
    .map_err(to_string)
}

#[tauri::command]
pub(crate) fn delete_rewrite_ab_run(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let novel_id = {
        let conn = state.conn.lock().map_err(to_string)?;
        conn.query_row(
            "SELECT novel_id FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?
    };
    let _active_task =
        state
            .active_tasks
            .acquire(&novel_id, std::iter::empty::<&str>(), "删除 A/B 实验")?;
    let conn = state.conn.lock().map_err(to_string)?;
    let status: String = conn
        .query_row(
            "SELECT status FROM rewrite_ab_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(to_string)?;
    if status == "running" {
        return Err("A/B 实验仍在运行，请先终止。".to_string());
    }
    conn.execute("DELETE FROM rewrite_ab_runs WHERE id = ?1", params![run_id])
        .map_err(to_string)?;
    Ok(())
}

fn load_ab_profile_ids(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT profile_id FROM rewrite_ab_models WHERE run_id = ?1
             UNION
             SELECT CAST(json_extract(input_snapshot_json, '$.review_profile.id') AS TEXT)
             FROM rewrite_ab_runs
             WHERE id = ?1
               AND json_extract(input_snapshot_json, '$.review_profile.id') IS NOT NULL
             ORDER BY 1",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![run_id], |row| row.get::<_, String>(0))
        .map_err(to_string)?;
    let profile_ids = rows.collect::<Result<Vec<_>, _>>().map_err(to_string)?;
    Ok(profile_ids)
}

pub(crate) fn restore_orphaned_rewrite_ab_state(conn: &Connection) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE rewrite_ab_candidates SET status = 'failed',
                error = '应用上次运行时中断，可重试未完成候选。', updated_at = ?1
         WHERE status = 'running'",
        params![now],
    )
    .map_err(to_string)?;
    conn.execute(
        "UPDATE rewrite_ab_runs SET status = 'partial', updated_at = ?1
         WHERE status = 'running'",
        params![now],
    )
    .map_err(to_string)?;
    conn.execute(
        "UPDATE jobs SET status = 'failed', message = 'A/B 改写因应用退出而中断，可重试未完成候选',
                updated_at = ?1
         WHERE job_type = 'rewrite_ab' AND status = 'running'",
        params![now],
    )
    .map_err(to_string)?;
    Ok(())
}

pub(crate) fn model_has_unfinished_rewrite_ab(
    conn: &Connection,
    profile_id: &str,
) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM rewrite_ab_models model
            JOIN rewrite_ab_runs run ON run.id = model.run_id
            WHERE model.profile_id = ?1 AND run.status IN ('running', 'partial')
         ) OR EXISTS(
            SELECT 1 FROM rewrite_ab_runs run
            WHERE run.status IN ('running', 'partial')
              AND json_extract(run.input_snapshot_json, '$.review_profile.id') = ?1
         )",
        params![profile_id],
        |row| Ok(row.get::<_, i64>(0)? != 0),
    )
    .map_err(to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    #[test]
    fn validates_two_or_three_unique_profiles() {
        assert!(validate_profile_ids(&["a".into(), "b".into()]).is_ok());
        assert!(validate_profile_ids(&["a".into(), "b".into(), "c".into()]).is_ok());
        assert!(validate_profile_ids(&["a".into()]).is_err());
        assert!(validate_profile_ids(&["a".into(), "a".into()]).is_err());
        assert!(validate_profile_ids(&["a".into(), "b".into(), "c".into(), "d".into()]).is_err());
    }

    #[test]
    fn startup_recovery_preserves_completed_candidates() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_minimal_run(&conn);
        conn.execute(
            "UPDATE rewrite_ab_candidates SET status = 'completed', content = '完成稿'
             WHERE chapter_id = 'c1' AND slot = 'A'",
            [],
        )
        .expect("complete candidate");

        restore_orphaned_rewrite_ab_state(&conn).expect("recover state");
        let completed: String = conn
            .query_row(
                "SELECT status FROM rewrite_ab_candidates
                 WHERE run_id = 'run-1' AND chapter_id = 'c1' AND slot = 'A'",
                [],
                |row| row.get(0),
            )
            .expect("read completed");
        let interrupted: String = conn
            .query_row(
                "SELECT status FROM rewrite_ab_candidates
                 WHERE run_id = 'run-1' AND chapter_id = 'c1' AND slot = 'B'",
                [],
                |row| row.get(0),
            )
            .expect("read interrupted");
        assert_eq!(completed, "completed");
        assert_eq!(interrupted, "failed");
    }

    #[test]
    fn unfinished_run_blocks_model_deletion_but_ready_run_does_not() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_minimal_run(&conn);
        assert!(model_has_unfinished_rewrite_ab(&conn, "p1").expect("query running"));
        conn.execute(
            "UPDATE rewrite_ab_runs
             SET input_snapshot_json = '{\"review_profile\":{\"id\":\"review\"}}'",
            [],
        )
        .expect("store review snapshot");
        assert!(model_has_unfinished_rewrite_ab(&conn, "review").expect("query review model"));
        conn.execute("UPDATE rewrite_ab_runs SET status = 'ready'", [])
            .expect("mark ready");
        assert!(!model_has_unfinished_rewrite_ab(&conn, "p1").expect("query ready"));
        assert!(!model_has_unfinished_rewrite_ab(&conn, "review").expect("query ready review"));
    }

    #[test]
    fn applies_candidates_and_force_restores_exact_baseline_after_conflict() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_apply_run(&conn, true, "completed");

        let applied = apply_or_restore_rewrite_ab_in_connection(&mut conn, "run-1", false, false)
            .expect("apply candidate");
        assert_eq!(applied.status, "applied");
        let applied_row: (String, String, String, String) = conn
            .query_row(
                "SELECT chapters.title, chapters.rewrite_text,
                        rewrite_ab_chapters.applied_slot,
                        rewrite_ab_chapters.applied_fingerprint
                 FROM chapters
                 JOIN rewrite_ab_chapters ON rewrite_ab_chapters.chapter_id = chapters.id
                 WHERE chapters.id = 'c1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("read applied row");
        assert_eq!(applied_row.0, "候选标题");
        assert_eq!(applied_row.1, "候选正文");
        assert_eq!(applied_row.2, "A");
        assert!(!applied_row.3.is_empty());

        conn.execute(
            "UPDATE chapters SET rewrite_text = '用户人工修改' WHERE id = 'c1'",
            [],
        )
        .expect("edit applied text");
        let conflict = apply_or_restore_rewrite_ab_in_connection(&mut conn, "run-1", false, true)
            .expect("return restore conflict");
        assert_eq!(conflict.status, "conflict");
        assert_eq!(conflict.conflict_chapter_ids, ["c1"]);
        let still_edited: String = conn
            .query_row(
                "SELECT rewrite_text FROM chapters WHERE id = 'c1'",
                [],
                |row| row.get(0),
            )
            .expect("read edited text");
        assert_eq!(still_edited, "用户人工修改");

        let restored = apply_or_restore_rewrite_ab_in_connection(&mut conn, "run-1", true, true)
            .expect("force restore baseline");
        assert_eq!(restored.status, "restored");
        let baseline: (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT chapters.title, chapters.rewrite_text, chapters.ai_rewrite_text,
                        chapters.rewrite_status, rewrite_ab_chapters.applied_slot,
                        rewrite_ab_chapters.applied_fingerprint
                 FROM chapters
                 JOIN rewrite_ab_chapters ON rewrite_ab_chapters.chapter_id = chapters.id
                 WHERE chapters.id = 'c1'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .expect("read restored baseline");
        assert_eq!(baseline.0, "原标题");
        assert_eq!(baseline.1, "原正式稿");
        assert_eq!(baseline.2, "原 AI 稿");
        assert_eq!(baseline.3, "completed");
        assert_eq!(baseline.4, None);
        assert_eq!(baseline.5, None);
    }

    #[test]
    fn incomplete_selection_never_writes_any_chapter() {
        for (selected, candidate_status) in [(false, "completed"), (true, "pending")] {
            let mut conn = Connection::open_in_memory().expect("open database");
            init_db(&conn).expect("initialize database");
            seed_apply_run(&conn, selected, candidate_status);

            let error = apply_or_restore_rewrite_ab_in_connection(&mut conn, "run-1", false, false)
                .expect_err("incomplete choices must fail");
            assert!(error.contains("每个章节选择"));
            let current: (String, String) = conn
                .query_row(
                    "SELECT title, rewrite_text FROM chapters WHERE id = 'c1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read unchanged chapter");
            assert_eq!(current, ("原标题".to_string(), "原正式稿".to_string()));
        }
    }

    #[test]
    fn deleting_novel_cascades_all_ab_tables() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_minimal_run(&conn);

        conn.execute("DELETE FROM novels WHERE id = 'n1'", [])
            .expect("delete novel");
        for table in [
            "rewrite_ab_candidates",
            "rewrite_ab_chapters",
            "rewrite_ab_models",
            "rewrite_ab_runs",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count rows");
            assert_eq!(count, 0, "table {table} should cascade");
        }
    }

    #[test]
    fn every_retry_creates_and_atomically_selects_a_new_job() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_minimal_run(&conn);
        conn.execute_batch(
            "UPDATE rewrite_ab_runs SET status = 'partial';
             UPDATE jobs SET status = 'failed' WHERE id = 'j1';",
        )
        .expect("prepare first retry");

        let first = prepare_rewrite_ab_retry(&mut conn, "run-1").expect("first retry");
        assert_ne!(first, "j1");
        let selected: String = conn
            .query_row(
                "SELECT job_id FROM rewrite_ab_runs WHERE id = 'run-1'",
                [],
                |row| row.get(0),
            )
            .expect("selected first job");
        assert_eq!(selected, first);

        conn.execute(
            "UPDATE rewrite_ab_runs SET status = 'partial' WHERE id = 'run-1'",
            [],
        )
        .expect("prepare second retry");
        let second = prepare_rewrite_ab_retry(&mut conn, "run-1").expect("second retry");
        assert_ne!(second, first);
        let job_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .expect("count retry jobs");
        assert_eq!(job_count, 3);

        let failure =
            prepare_rewrite_ab_retry(&mut conn, "run-1").expect_err("running run cannot retry");
        assert!(failure.contains("可以重试"));
        let unchanged_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .expect("count after rejected retry");
        assert_eq!(unchanged_count, 3);
    }

    #[test]
    fn estimate_lookup_uses_ordered_chapter_fingerprint() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_minimal_run(&conn);

        assert_eq!(
            find_existing_run_id(&conn, "n1", "hash").expect("find existing"),
            Some("run-1".to_string())
        );
        assert_eq!(
            find_existing_run_id(&conn, "n1", "different").expect("find missing"),
            None
        );
    }

    #[test]
    fn estimate_aggregates_recent_model_history_and_queue_waves() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        conn.execute(
            "INSERT INTO ai_logs (
                id, profile_id, action, status, content, created_at
             ) VALUES ('l1', 'p1', '改写', 'success',
                '输入字符数：1000\n输出字符数：500\nAI 调用耗时：10.0 秒', 'now')",
            [],
        )
        .expect("insert p1 stats");
        conn.execute(
            "INSERT INTO ai_logs (
                id, profile_id, action, status, content, created_at
             ) VALUES ('l2', 'p2', '改写', 'success',
                '输入字符数：1000\n输出字符数：500\nAI 调用耗时：20.0 秒', 'now')",
            [],
        )
        .expect("insert p2 stats");

        let stats = load_aggregate_recent_model_stats(
            &conn,
            &["p1".to_string(), "p2".to_string(), "p1".to_string()],
        )
        .expect("aggregate stats");
        assert_eq!(stats.success_calls, 2);
        assert_eq!(stats.average_call_seconds(), Some(15.0));
        assert_eq!(estimate_ab_queue_seconds(10, 3, Some(15.0)), Some(60.0));
        assert_eq!(estimate_ab_queue_seconds(10, 3, None), None);
    }

    #[test]
    fn estimate_request_count_contains_only_drafts_and_optional_review() {
        assert_eq!(estimate_ab_request_count(6, 2, false), 12);
        assert_eq!(estimate_ab_request_count(6, 2, true), 24);
        assert_eq!(estimate_ab_request_count(0, 3, true), 0);
    }

    #[test]
    fn replace_all_choices_allows_unselected_failures_but_requires_complete_coverage() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        seed_apply_run(&conn, true, "completed");
        conn.execute(
            "INSERT INTO rewrite_ab_models (run_id, slot, profile_id, profile_snapshot_json)
             VALUES ('run-1', 'B', 'p2', '{}')",
            [],
        )
        .expect("insert B model");
        conn.execute(
            "INSERT INTO rewrite_ab_candidates (
                run_id, chapter_id, slot, status, title, content, updated_at
             ) VALUES ('run-1', 'c1', 'B', 'failed', 'B 标题', 'B 正文', 'now')",
            [],
        )
        .expect("insert failed B candidate");

        let choice_a = [RewriteAbChoice {
            chapter_id: "c1".to_string(),
            slot: "A".to_string(),
        }];
        save_rewrite_ab_choices_in_connection(&mut conn, "run-1", &choice_a, true)
            .expect("completed A can be selected while B failed");
        assert_eq!(selected_slot(&conn), Some("A".to_string()));

        conn.execute(
            "UPDATE rewrite_ab_candidates SET status = 'completed'
             WHERE run_id = 'run-1' AND chapter_id = 'c1' AND slot = 'B'",
            [],
        )
        .expect("complete B candidate");
        let incomplete = save_rewrite_ab_choices_in_connection(&mut conn, "run-1", &[], true)
            .expect_err("replace all must cover every chapter");
        assert!(incomplete.contains("覆盖全部"));
        assert_eq!(selected_slot(&conn), Some("A".to_string()));

        let choice_b = [RewriteAbChoice {
            chapter_id: "c1".to_string(),
            slot: "B".to_string(),
        }];
        save_rewrite_ab_choices_in_connection(&mut conn, "run-1", &choice_b, true)
            .expect("replace every choice");
        assert_eq!(selected_slot(&conn), Some("B".to_string()));
    }

    fn selected_slot(conn: &Connection) -> Option<String> {
        conn.query_row(
            "SELECT selected_slot FROM rewrite_ab_chapters
             WHERE run_id = 'run-1' AND chapter_id = 'c1'",
            [],
            |row| row.get(0),
        )
        .expect("read selected slot")
    }

    fn seed_minimal_run(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO novels (id, title, source_path, encoding, status, created_at)
            VALUES ('n1', 'N', 'source.txt', 'UTF-8', 'imported', 'now');
            INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text,
                analysis_status, rewrite_status
            ) VALUES ('c1', 'n1', 1, '第一章', '正文', 'completed', 'pending');
            INSERT INTO jobs (
                id, novel_id, job_type, status, current_chapter, total_chapters,
                message, created_at, updated_at
            ) VALUES ('j1', 'n1', 'rewrite_ab', 'running', 0, 2, '', 'now', 'now');
            INSERT INTO rewrite_ab_runs (
                id, novel_id, batch_id, batch_label, batch_fingerprint,
                input_snapshot_json, job_id, status, created_at, updated_at
            ) VALUES ('run-1', 'n1', 'b1', '第1批', 'hash', '{}', 'j1', 'running', 'now', 'now');
            INSERT INTO rewrite_ab_models (run_id, slot, profile_id, profile_snapshot_json)
            VALUES ('run-1', 'A', 'p1', '{}'), ('run-1', 'B', 'p2', '{}');
            INSERT INTO rewrite_ab_chapters (
                run_id, chapter_id, chapter_index, original_title, original_text,
                analysis_status, baseline_title, baseline_rewrite_status, source_fingerprint
            ) VALUES ('run-1', 'c1', 1, '第一章', '正文', 'completed', '第一章', 'pending', 'hash');
            INSERT INTO rewrite_ab_candidates (run_id, chapter_id, slot, status, updated_at)
            VALUES ('run-1', 'c1', 'A', 'running', 'now'),
                   ('run-1', 'c1', 'B', 'running', 'now');
            "#,
        )
        .expect("seed run");
    }

    fn seed_apply_run(conn: &Connection, selected: bool, candidate_status: &str) {
        let source_fingerprint = canonical_fingerprint(
            "原标题",
            Some("原正式稿"),
            Some("原 AI 稿"),
            Some("edited-at"),
            "completed",
        );
        conn.execute_batch(
            r#"
            INSERT INTO novels (id, title, source_path, encoding, status, created_at)
            VALUES ('n1', 'N', 'source.txt', 'UTF-8', 'imported', 'now');
            INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text, analysis_json,
                rewrite_text, ai_rewrite_text, rewrite_edited_at,
                analysis_status, rewrite_status
            ) VALUES (
                'c1', 'n1', 1, '原标题', '原文', '{}',
                '原正式稿', '原 AI 稿', 'edited-at', 'completed', 'completed'
            );
            INSERT INTO rewrite_ab_runs (
                id, novel_id, batch_id, batch_label, batch_fingerprint,
                input_snapshot_json, status, created_at, updated_at
            ) VALUES ('run-1', 'n1', 'b1', '第1批', 'hash', '{}', 'ready', 'now', 'now');
            INSERT INTO rewrite_ab_models (run_id, slot, profile_id, profile_snapshot_json)
            VALUES ('run-1', 'A', 'p1', '{}');
            "#,
        )
        .expect("seed apply base");
        conn.execute(
            "INSERT INTO rewrite_ab_chapters (
                run_id, chapter_id, chapter_index, original_title, original_text,
                analysis_json, analysis_status, baseline_title, baseline_rewrite_text,
                baseline_ai_rewrite_text, baseline_rewrite_edited_at,
                baseline_rewrite_status, source_fingerprint, selected_slot
             ) VALUES (
                'run-1', 'c1', 1, '原标题', '原文', '{}', 'completed',
                '原标题', '原正式稿', '原 AI 稿', 'edited-at', 'completed', ?1, ?2
             )",
            params![source_fingerprint, selected.then_some("A")],
        )
        .expect("seed apply chapter");
        conn.execute(
            "INSERT INTO rewrite_ab_candidates (
                run_id, chapter_id, slot, status, title, content, updated_at
             ) VALUES ('run-1', 'c1', 'A', ?1, '候选标题', '候选正文', 'now')",
            params![candidate_status],
        )
        .expect("seed apply candidate");
    }
}
