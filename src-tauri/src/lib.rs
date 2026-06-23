mod ai;
mod commands;
mod credentials;
mod db;
mod domain;
mod model_support;
mod rate_limit;
mod repositories;
mod services;
mod task_control;
mod text;

use ai::*;
use chrono::Utc;
use commands::{
    analysis::*, auto_run::*, export::*, frontend_errors::*, jobs::*, logs::*, models::*,
    novels::*, rewrite::*, settings::*, updates::*, workspace::*,
};
use credentials::{classify_api_key_storage, read_api_key, write_api_key, ApiKeyStorage};
use db::init_db;
use domain::*;
#[cfg(test)]
use futures_util::stream::FuturesUnordered;
use futures_util::{future::BoxFuture, stream, FutureExt, StreamExt};
use model_support::model_output_truncation_error;
use rate_limit::RateLimitCoordinator;
use regex::Regex;
use repositories::{chapters::*, jobs::*, logs::*};
use reqwest::Client;
use rusqlite::{params, Connection};
#[cfg(test)]
use serde_json::json;
use services::progress::*;
use services::{estimation::*, shard_context::*};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use task_control::{
    should_terminate_paused_run, ActiveTaskRegistry, AutoRunControl, CancellableTaskRegistry,
};
use tauri::{AppHandle, Emitter, Manager, State};
use text::*;
use uuid::Uuid;

const GITHUB_REPOSITORY_URL: &str = "https://github.com/3minto1/Yuri-Rewrite";
const GITHUB_LATEST_RELEASE_URL: &str = "https://github.com/3minto1/Yuri-Rewrite/releases/latest";
const GITHUB_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/3minto1/Yuri-Rewrite/releases/latest";
const AUTO_RUN_PAUSED: &str = "__YURI_AUTO_RUN_PAUSED__";
const AUTO_RUN_TERMINATED: &str = "__YURI_AUTO_RUN_TERMINATED__";
const SYSTEM_ANALYSIS_EXPERT: &str = "你是严谨的中文长篇小说结构分析专家，擅长从原文中提取事实、人物、关系、地点、术语和性别线索。工作方式必须精确、克制、基于证据；只输出合法 JSON，不输出 Markdown 或解释。";
const SYSTEM_ANALYSIS_JSON_REPAIR: &str = "你是中文小说分析 JSON 格式修复专家，只负责把输入修复为合法 JSON 对象，不新增事实、不改写正文、不输出 Markdown 或解释。";
const SYSTEM_NAME_MAPPING_EXPERT: &str = "你是中文小说姓名女性化映射专家，擅长在保留姓氏、读音和人物辨识度的前提下生成稳定姓名映射。必须只输出合法 JSON，不输出 Markdown 或解释。";
const SYSTEM_REWRITE_EXPERT: &str = "你是资深中文长篇小说改写专家，擅长在保持原文主线、人物逻辑和章节边界的前提下，将男女性别叙事自然改写为双女主百合文本。工作方式：先遵守输入中的规则、设定和一致性资产，再处理当前章节正文。输出必须只包含当前输入章节的 marker、标题和正文，不解释、不输出输入外章节。";
const SYSTEM_REWRITE_FORMAT_REPAIR: &str = "你是中文小说改写格式修复专家，擅长修复章节边界、缺失 marker、空正文和截断式输出问题。必须重新输出当前输入章节的完整百合改写结果，逐字保留章节 marker，只输出 marker、标题和非空正文，不解释。";
const SYSTEM_REVIEW_DECISION_EXPERT: &str = "你是严谨的中文小说改写审查专家，擅长依据规则核对姓名、性别、逻辑、一致性和章节边界。只判断会导致打回的 blocking 问题，不做润色，不直接改写正文。必须只输出合法 JSON。";
const SYSTEM_REVIEW_FINAL_EXPERT: &str = "你是中文小说改写终审专家，擅长复判打回重写后的稿件是否已解决 blocking 问题。只输出合法 JSON，不解释，不补充非阻断建议。";
const SYSTEM_REVIEW_JSON_REPAIR: &str = "你是 JSON 格式修复专家，只负责把审查决策修复为合法 JSON，不重新审查正文，不新增或删除问题，不输出解释。";
const SYSTEM_TARGETED_REVISION_EXPERT: &str = "你是中文小说定向修复专家，擅长只修复指定章节中的 blocking 问题，同时保持未指定章节和只读上下文不变。只输出目标章节，逐字保留目标章节 marker，不输出相邻只读章节。";
const SYSTEM_REVIEW_REVISION_EXPERT: &str = "你是中文小说审查打回重写专家，擅长根据审查问题清单重写当前分片，同时保持原文主线、姓名映射、人物性别和章节边界稳定。必须严格保留章节 marker，只输出当前输入章节。";
const SYSTEM_REVIEW_REVISION_REPAIR: &str = "你是中文小说审查打回重写格式修复专家，擅长在不改变修复目标的前提下补全 marker、标题和正文。必须按审查问题重新输出当前分片完整改写稿，逐字保留全部章节开始和结束标记，不解释。";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let app_dir = env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| data_dir.clone());
            fs::create_dir_all(&data_dir)?;
            fs::create_dir_all(data_dir.join("exports"))?;
            cleanup_deletion_trash(&data_dir);
            let conn = Connection::open(data_dir.join("yuri-rewrite.sqlite3"))?;
            init_db(&conn)?;
            let restored_auto_runs = restore_auto_run_controls(&conn)?;
            restore_orphaned_rewrite_statuses(&conn)?;
            let client = Client::builder()
                .connect_timeout(Duration::from_secs(20))
                .timeout(Duration::from_secs(20 * 60))
                .build()?;
            app.manage(AppState {
                app: app.handle().clone(),
                conn: Mutex::new(conn),
                client,
                data_dir,
                app_dir,
                auto_runs: Mutex::new(restored_auto_runs),
                auto_run_progress: Mutex::new(HashMap::new()),
                active_tasks: ActiveTaskRegistry::default(),
                single_rewrite_tasks: CancellableTaskRegistry::default(),
                rate_limits: RateLimitCoordinator::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_txt,
            list_novels,
            get_novel_detail,
            delete_novel,
            save_model_profile,
            delete_model_profile,
            list_model_profiles,
            test_model_profile,
            diagnose_model_profile,
            estimate_job_cost,
            list_ai_logs,
            clear_ai_logs,
            get_token_usage_stats,
            get_app_settings,
            save_app_settings,
            save_selected_profile_id,
            get_novel_settings,
            save_novel_settings,
            list_chapter_batches,
            update_canon_assets,
            update_chapter_title,
            start_analysis,
            start_rewrite,
            rewrite_single_chapter,
            terminate_single_chapter_rewrite,
            start_analyze_rewrite_batch,
            start_analyze_rewrite_all,
            pause_analyze_rewrite_all,
            terminate_analyze_rewrite_all,
            get_job,
            list_auto_run_recoveries,
            save_chapter_rewrite_edit,
            restore_chapter_rewrite_edit,
            restore_single_chapter_rewrite,
            export_novel,
            open_github_url,
            open_github_release_url,
            check_for_updates,
            download_latest_update,
            take_update_install_result,
            record_frontend_error
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Yuri Rewrite");
}

fn restore_auto_run_controls(
    conn: &Connection,
) -> Result<HashMap<String, AutoRunControl>, Box<dyn std::error::Error>> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE auto_run_checkpoints SET status = 'paused', pause_reason = CASE WHEN trim(pause_reason) = '' THEN '检测到软件上次运行时任务未正常结束。' ELSE pause_reason END, updated_at = ?1 WHERE status IN ('running', 'pausing', 'pause_requested', 'terminating', 'terminate_requested')",
        params![now],
    )?;
    conn.execute(
        "UPDATE jobs SET status = 'paused', message = '检测到上次未完成的一键任务，可点击继续处理当前批次的未完成分片。', updated_at = ?1 WHERE job_type = 'auto' AND status IN ('running', 'pausing', 'terminating') AND id IN (SELECT job_id FROM auto_run_checkpoints WHERE job_id IS NOT NULL)",
        params![now],
    )?;
    conn.execute(
        "UPDATE jobs SET status = 'failed', message = '旧版本任务在软件关闭时中断，缺少恢复检查点，无法继续。', updated_at = ?1 WHERE job_type = 'auto' AND status IN ('running', 'pausing', 'terminating') AND id NOT IN (SELECT job_id FROM auto_run_checkpoints WHERE job_id IS NOT NULL)",
        params![now],
    )?;

    let mut stmt = conn.prepare(
        "SELECT novel_id, start_batch_index, next_batch_index, job_id, status, profile_ids FROM auto_run_checkpoints",
    )?;
    let rows = stmt.query_map([], |row| {
        let profile_json: String = row.get(5)?;
        let profile_ids = serde_json::from_str::<Vec<String>>(&profile_json)
            .unwrap_or_default()
            .into_iter()
            .collect::<HashSet<_>>();
        Ok((
            row.get::<_, String>(0)?,
            AutoRunControl {
                start_batch_index: row.get(1)?,
                completed_batches: row.get(2)?,
                job_id: row.get(3)?,
                status: row.get(4)?,
                profile_ids,
                recoverable: true,
            },
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn restore_orphaned_rewrite_statuses(
    conn: &Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "UPDATE chapters
         SET rewrite_status = CASE
             WHEN rewrite_text IS NOT NULL AND trim(rewrite_text) != '' THEN 'completed'
             ELSE 'failed'
         END
         WHERE rewrite_status = 'running'
           AND novel_id NOT IN (
               SELECT novel_id FROM auto_run_checkpoints WHERE status = 'paused'
           )",
        [],
    )?;
    Ok(())
}

fn restore_orphaned_rewrite_status_for_chapter(
    conn: &Connection,
    chapter_id: &str,
) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE chapters
         SET rewrite_status = 'completed'
         WHERE id = ?1
           AND rewrite_status = 'running'
           AND rewrite_text IS NOT NULL
           AND trim(rewrite_text) != ''
           AND novel_id NOT IN (
               SELECT novel_id FROM auto_run_checkpoints WHERE status = 'paused'
           )",
        params![chapter_id],
    )? > 0)
}

fn cleanup_deletion_trash(data_dir: &Path) {
    let trash_dir = data_dir.join("deletion-trash");
    if trash_dir.exists() {
        let _ = fs::remove_dir_all(&trash_dir);
    }
}

async fn analyze_chapters_for_auto(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    checkpoint_batch_index: Option<i64>,
) -> Result<(), String> {
    mark_empty_source_chapters_skipped(state, chapters)?;
    let chapters = chapters
        .iter()
        .filter(|chapter| chapter_has_source_body(chapter))
        .cloned()
        .collect::<Vec<_>>();
    if chapters.is_empty() {
        ensure_name_mapping_asset_if_settings_available(state, novel_id, profile, api_key).await?;
        return Ok(());
    }
    let staged_chapter_ids = if let Some(batch_index) = checkpoint_batch_index {
        load_staged_chapter_ids(state, novel_id, batch_index, "analysis")?
    } else {
        HashSet::new()
    };
    for chapter in &chapters {
        if !staged_chapter_ids.contains(&chapter.id) {
            set_chapter_status(state, &chapter.id, "analysis_status", "running")?;
        }
    }

    let rewrite_parallelism = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_rewrite_parallelism(&conn)?
    };
    services::analysis::analyze_and_save(
        state,
        novel_id,
        profile,
        api_key,
        &chapters,
        rewrite_parallelism,
        checkpoint_batch_index,
    )
    .await
    .inspect_err(|error| {
        if error != AUTO_RUN_PAUSED
            && error != AUTO_RUN_TERMINATED
            && !is_recoverable_model_format_error(error)
        {
            let _ = mark_chapters_analysis_failed(state, &chapters);
        }
    })?;
    ensure_name_mapping_asset_if_settings_available(state, novel_id, profile, api_key).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn analyze_batch_with_parallelism(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    all_chapters: &[Chapter],
    chapters: &[Chapter],
    rewrite_parallelism: usize,
    checkpoint_batch_index: Option<i64>,
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    let analysis_identity_context = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_novel_settings(&conn, novel_id)?
            .map(|settings| build_analysis_identity_context(&settings))
            .unwrap_or_default()
    };
    let rewrite_parallelism = state
        .rate_limits
        .effective_parallelism(rewrite_parallelism, &[profile])?;
    let shard_work = build_contiguous_shard_work(all_chapters, chapters, rewrite_parallelism);
    let shard_total = shard_work.len();
    let batch_label = format_batch_label(chapters);
    let staged = checkpoint_batch_index
        .map(|batch_index| load_staged_outputs(state, novel_id, batch_index, "analysis"))
        .transpose()?
        .unwrap_or_default();
    set_auto_progress_shard_total(
        state,
        novel_id,
        "analysis",
        shard_total,
        all_chapters.len(),
        completed_chapter_ids_before_resume(all_chapters, chapters),
    )?;
    let tasks = stream::iter(shard_work.into_iter().enumerate().map(|(idx, work)| {
        let shard = work.chapters.clone();
        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let readonly_context = build_readonly_adjacent_context(&work, &staged, "analysis");
        let context = format_shard_context_with_neighbors(
            idx,
            shard_total,
            rewrite_parallelism,
            &batch_label,
            &shard,
            &readonly_context,
        );
        let prompt =
            build_batch_analysis_prompt_with_identity(&shard, &context, &analysis_identity_context);
        let client = state.client.clone();
        let rate_limiter = state.rate_limits.clone();
        let profile_for_task = profile.clone();
        let api_key = api_key.to_string();
        async move {
            let output = match report_auto_shard_started(
                state,
                novel_id,
                "analysis",
                idx,
                shard_total,
                &shard,
            ) {
                Ok(()) => {
                    generate_text(
                        &client,
                        Some(rate_limiter),
                        &profile_for_task,
                        &api_key,
                        SYSTEM_ANALYSIS_EXPERT,
                        &prompt,
                        true,
                    )
                    .await
                }
                Err(error) => Err(error),
            };
            (idx, shard_label, context, shard, output)
        }
    }))
    .buffer_unordered(rewrite_parallelism);
    futures_util::pin_mut!(tasks);

    let mut parsed_by_shard = Vec::new();
    loop {
        let result = tokio::select! {
            result = tasks.next() => result,
            _ = tokio::time::sleep(Duration::from_millis(300)) => {
                if let Some(status) = requested_auto_run_stop(state, novel_id)? {
                    return Err(status);
                }
                continue;
            }
        };
        let Some(result) = result else {
            break;
        };
        let (idx, shard_label, context, shard, output) = result;
        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次分析",
                    Some(&shard_label),
                    "success",
                    &format_model_log_content(&output, profile, None),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let parsed = match parse_analysis_model_output(&output, &shard) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次分析解析",
                            Some(&shard_label),
                            "error",
                            &error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;
                        match retry_analysis_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &shard,
                            &context,
                            &shard_label,
                            &error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(parsed) => parsed,
                            Err(retry_error) => {
                                return Err(format!("{}：{}", shard_label, retry_error));
                            }
                        }
                    }
                };
                if let Some(batch_index) = checkpoint_batch_index {
                    stage_analysis_shard(
                        state,
                        novel_id,
                        batch_index,
                        &shard,
                        &parsed,
                    )?;
                }
                report_auto_shard_completed(state, novel_id, idx, &shard)?;
                parsed_by_shard.push((idx, parsed));
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次分析",
                    Some(&shard_label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("{}：{}", shard_label, error));
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn retry_analysis_shard_after_parse_error(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    shard_context: &str,
    shard_label: &str,
    parse_error: &str,
    bad_output: &str,
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    if parse_error.contains("模型输出因达到长度上限被截断") {
        return retry_truncated_analysis_shard_by_splitting(
            state,
            novel_id,
            profile,
            api_key,
            shard,
            shard_context,
            shard_label,
        )
        .await;
    }

    let retry_context = format!(
        "{}\n\n只输出当前输入级一致性资产 JSON 对象；不要输出 Markdown、解释、空内容或 chapters 数组；JSON 字符串内换行必须写成 \\n。",
        shard_context.trim()
    );
    let identity_context = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_novel_settings(&conn, novel_id)?
            .map(|settings| build_analysis_identity_context(&settings))
            .unwrap_or_default()
    };
    let base_prompt =
        build_batch_analysis_prompt_with_identity(shard, retry_context.trim(), &identity_context);
    let prompt = format!(
        "{}\n\n上一次无法解析的输出如下，仅供你避开格式错误，不要照抄：\n{}",
        base_prompt,
        truncate_text(bad_output, 12_000)
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_ANALYSIS_JSON_REPAIR,
        &prompt,
        true,
    )
    .await;

    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次分析重试",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, None),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_analysis_model_output(&output, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次分析重试解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    if error.contains("模型输出因达到长度上限被截断") {
                        retry_truncated_analysis_shard_by_splitting(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            shard,
                            shard_context,
                            shard_label,
                        )
                        .await
                    } else {
                        Err(format!(
                            "分析输出格式多次修复后仍无法解析：{}。任务可以暂停后手动继续，已完成分片会保留，仅重新尝试未完成分片；如果频繁出现，请更换 JSON 输出更稳定的模型、提高并发以缩小单个分片，或缩小处理范围。",
                            error
                        ))
                    }
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次分析重试",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!(
                "分析输出格式修复重试调用失败：{}。任务可以暂停后手动继续，已完成分片会保留，仅重新尝试未完成分片；如果频繁出现，请更换 JSON 输出更稳定的模型、提高并发以缩小单个分片，或缩小处理范围。",
                error
            ))
        }
    }
}

fn parse_analysis_model_output(
    output: &ModelOutput,
    chapters: &[Chapter],
) -> Result<Vec<ParsedChapterAnalysis>, String> {
    if let Some(error) = model_output_truncation_error(&output.raw_response) {
        return Err(error);
    }
    parse_batch_analysis_output(&output.text, chapters)
}

#[allow(clippy::too_many_arguments)]
fn retry_truncated_analysis_shard_by_splitting<'a>(
    state: &'a State<'a, AppState>,
    novel_id: &'a str,
    profile: &'a ModelProfile,
    api_key: &'a str,
    shard: &'a [Chapter],
    shard_context: &'a str,
    shard_label: &'a str,
) -> BoxFuture<'a, Result<Vec<ParsedChapterAnalysis>, String>> {
    async move {
        if shard.len() <= 1 {
            return Err(format!(
                "分析输出在 64K 上限下仍被截断，且当前已是单章分片（{}）。请缩短该章原文、关闭思考或更换输出上限更高的模型。",
                shard_label
            ));
        }

        let midpoint = shard.len().div_ceil(2);
        let parts = [&shard[..midpoint], &shard[midpoint..]];
        let identity_context = {
            let conn = state.conn.lock().map_err(to_string)?;
            load_novel_settings(&conn, novel_id)?
                .map(|settings| build_analysis_identity_context(&settings))
                .unwrap_or_default()
        };
        let mut merged = Vec::with_capacity(shard.len());

        for (part_index, part) in parts.into_iter().filter(|part| !part.is_empty()).enumerate() {
            if let Some(status) = requested_auto_run_stop(state, novel_id)? {
                return Err(status);
            }
            let part_label = format!(
                "{} · 长度恢复 {}/2 · {}",
                shard_label,
                part_index + 1,
                format_batch_label(part)
            );
            let part_context = format!(
                "{}\n\n长度恢复说明：原分片在保留思考并提高到 64K 输出上限后仍被截断；当前只分析上述范围中的这个连续子分片。",
                shard_context.trim()
            );
            let prompt =
                build_batch_analysis_prompt_with_identity(part, &part_context, &identity_context);
            let output = generate_text(
                &state.client,
                Some(state.rate_limits.clone()),
                profile,
                api_key,
                SYSTEM_ANALYSIS_EXPERT,
                &prompt,
                true,
            )
            .await?;
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次分析拆分重试",
                Some(&part_label),
                "success",
                &format_model_log_content(&output, profile, None),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;

            let parsed = match parse_analysis_model_output(&output, part) {
                Ok(parsed) => parsed,
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次分析拆分重试解析",
                        Some(&part_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    retry_analysis_shard_after_parse_error(
                        state,
                        novel_id,
                        profile,
                        api_key,
                        part,
                        &part_context,
                        &part_label,
                        &error,
                        &output.text,
                    )
                    .await?
                }
            };
            merged.extend(parsed);
        }

        Ok(merged)
    }
    .boxed()
}

fn save_parsed_analyses(
    state: &State<'_, AppState>,
    novel_id: &str,
    chapters: &[Chapter],
    analyses: Vec<ParsedChapterAnalysis>,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    for chapter in chapters {
        tx.execute(
            "UPDATE chapters SET analysis_json = NULL, analysis_status = 'completed' WHERE id = ?1",
            params![chapter.id],
        )
        .map_err(to_string)?;
    }
    for analysis in analyses {
        tx.execute(
            "UPDATE chapters SET analysis_json = ?1 WHERE id = ?2",
            params![analysis.json, analysis.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    merge_analysis_into_canon_assets(&conn, novel_id).map_err(to_string)?;
    Ok(())
}

fn load_staged_chapter_ids(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    phase: &str,
) -> Result<HashSet<String>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT chapter_id FROM auto_run_shard_outputs
             WHERE novel_id = ?1 AND batch_index = ?2 AND phase = ?3",
        )
        .map_err(to_string)?;
    let ids = stmt
        .query_map(params![novel_id, batch_index, phase], |row| row.get(0))
        .map_err(to_string)?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(to_string)?;
    Ok(ids)
}

fn chapters_without_staged_outputs(
    chapters: &[Chapter],
    staged_chapter_ids: &HashSet<String>,
) -> Vec<Chapter> {
    chapters
        .iter()
        .filter(|chapter| !staged_chapter_ids.contains(&chapter.id))
        .cloned()
        .collect()
}

fn completed_chapter_ids_before_resume(
    all_chapters: &[Chapter],
    target_chapters: &[Chapter],
) -> HashSet<String> {
    let target_ids = target_chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect::<HashSet<_>>();
    all_chapters
        .iter()
        .filter(|chapter| !target_ids.contains(chapter.id.as_str()))
        .map(|chapter| chapter.id.clone())
        .collect()
}

fn stage_analysis_shard(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    chapters: &[Chapter],
    analyses: &[ParsedChapterAnalysis],
) -> Result<(), String> {
    let analysis_by_id = analyses
        .iter()
        .map(|analysis| (analysis.id.as_str(), analysis.json.as_str()))
        .collect::<HashMap<_, _>>();
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    for chapter in chapters {
        tx.execute(
            "INSERT INTO auto_run_shard_outputs (
                novel_id, batch_index, phase, chapter_id, chapter_index, title, content, created_at
             ) VALUES (?1, ?2, 'analysis', ?3, ?4, NULL, ?5, ?6)
             ON CONFLICT(novel_id, batch_index, phase, chapter_id) DO UPDATE SET
                chapter_index = excluded.chapter_index,
                content = excluded.content,
                created_at = excluded.created_at",
            params![
                novel_id,
                batch_index,
                chapter.id,
                chapter.index,
                analysis_by_id.get(chapter.id.as_str()).copied(),
                now
            ],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)
}

fn apply_staged_analyses(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    chapters: &[Chapter],
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let staged = {
        let mut stmt = conn
            .prepare(
                "SELECT chapter_id, content FROM auto_run_shard_outputs
                 WHERE novel_id = ?1 AND batch_index = ?2 AND phase = 'analysis'",
            )
            .map_err(to_string)?;
        let rows = stmt
            .query_map(params![novel_id, batch_index], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(to_string)?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(to_string)?;
        rows
    };
    if chapters.iter().any(|chapter| !staged.contains_key(&chapter.id)) {
        return Err("分析分片恢复数据不完整，未写入章节结果。".to_string());
    }

    let tx = conn.transaction().map_err(to_string)?;
    for chapter in chapters {
        tx.execute(
            "UPDATE chapters SET analysis_json = ?1, analysis_status = 'completed' WHERE id = ?2",
            params![staged.get(&chapter.id).cloned().flatten(), chapter.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    merge_analysis_into_canon_assets(&conn, novel_id).map_err(to_string)
}

async fn ensure_name_mapping_asset(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    settings: &NovelSettings,
) -> Result<(), String> {
    let existing_content = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_asset_content(&conn, novel_id, "姓名映射表")?
    };
    let mut mappings = parse_name_mapping_entries(existing_content.as_deref().unwrap_or(""));
    let required_names = required_feminized_name_sources(settings);
    if required_names.is_empty() {
        return Ok(());
    }

    if !settings.rewritten_protagonist_name.trim().is_empty() {
        upsert_name_mapping_entry(
            &mut mappings,
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
        );
    }

    let missing_sources = required_names
        .iter()
        .filter(|source| {
            !mappings
                .iter()
                .any(|entry| entry.source == **source && !entry.target.trim().is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();

    if !missing_sources.is_empty() {
        match generate_name_mapping_entries(
            state,
            novel_id,
            profile,
            api_key,
            settings,
            &missing_sources,
        )
        .await
        {
            Ok(generated) => {
                for entry in generated {
                    upsert_name_mapping_entry(&mut mappings, &entry.source, &entry.target);
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "姓名映射生成",
                    Some("姓名映射表"),
                    "error",
                    &format!("AI 姓名映射生成失败，已使用本地兜底规则：{}", error),
                    None,
                    None,
                )?;
            }
        }
    }

    for source in required_names {
        if !mappings
            .iter()
            .any(|entry| entry.source == source && !entry.target.trim().is_empty())
        {
            let target = fallback_feminized_name(&source);
            upsert_name_mapping_entry(&mut mappings, &source, &target);
        }
    }

    let content = build_name_mapping_asset_content(settings, mappings)?;
    let conn = state.conn.lock().map_err(to_string)?;
    upsert_canon_asset(
        &conn,
        novel_id,
        "姓名映射表",
        &content,
        &Utc::now().to_rfc3339(),
    )
    .map_err(to_string)?;
    Ok(())
}

async fn ensure_name_mapping_asset_if_settings_available(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
) -> Result<bool, String> {
    let settings = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_novel_settings(&conn, novel_id)?
    };
    let Some(settings) = settings else {
        return Ok(false);
    };
    if settings.protagonist_name.trim().is_empty()
        || settings.bust.trim().is_empty()
        || settings.body_type.trim().is_empty()
        || settings.rewrite_mode.trim().is_empty()
    {
        return Ok(false);
    }
    ensure_name_mapping_asset(state, novel_id, profile, api_key, &settings).await?;
    Ok(true)
}

async fn generate_name_mapping_entries(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    settings: &NovelSettings,
    sources: &[String],
) -> Result<Vec<NameMappingEntry>, String> {
    let prompt = build_name_mapping_prompt(settings, sources);
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_NAME_MAPPING_EXPERT,
        &prompt,
        true,
    )
    .await?;
    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "姓名映射生成",
        Some("姓名映射表"),
        "success",
        &format_model_log_content(&output, profile, None),
        output.reasoning.as_deref(),
        Some(&output.raw_response),
    )?;
    parse_generated_name_mapping_entries(&output.text, sources)
}

fn build_name_mapping_prompt(settings: &NovelSettings, sources: &[String]) -> String {
    let forced = if settings.rewritten_protagonist_name.trim().is_empty() {
        "无".to_string()
    } else {
        format!(
            "{} -> {}",
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim()
        )
    };
    format!(
        r#"请为以下中文小说人物姓名生成固定的女性化姓名映射。

输出 JSON 结构必须是：
{{
  "names": [
    {{ "source": "原姓名", "target": "女性化姓名" }}
  ]
}}

要求：
1. 每个输入姓名都必须输出一条映射。
2. target 必须是中文姓名，不能为空，不能与 source 完全相同。
3. 优先保留姓氏，名字部分使用同音或近音的女性化字。
4. 若存在强制映射，必须逐字使用强制 target。
5. 只输出 JSON，不要解释、不要 Markdown。

强制映射：
{}

待生成姓名：
{}"#,
        forced,
        sources.join("\n")
    )
}

fn parse_generated_name_mapping_entries(
    output: &str,
    expected_sources: &[String],
) -> Result<Vec<NameMappingEntry>, String> {
    let value = parse_jsonish_value(output)?;
    let items = value
        .get("names")
        .or_else(|| value.get("mappings"))
        .or_else(|| value.get("name_mapping"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "姓名映射 JSON 缺少 names 数组。".to_string())?;
    let expected = expected_sources
        .iter()
        .map(|source| source.trim().to_string())
        .collect::<HashSet<_>>();
    let mut parsed = Vec::new();
    for item in items {
        let source = item
            .get("source")
            .or_else(|| item.get("original"))
            .or_else(|| item.get("from"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let target = item
            .get("target")
            .or_else(|| item.get("rewritten"))
            .or_else(|| item.get("to"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if source.is_empty() || target.is_empty() || source == target || !expected.contains(source)
        {
            continue;
        }
        parsed.push(NameMappingEntry {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
    if parsed.is_empty() {
        return Err("姓名映射 JSON 中没有可用映射。".to_string());
    }
    Ok(parsed)
}

fn required_feminized_name_sources(settings: &NovelSettings) -> Vec<String> {
    let mut names = Vec::new();
    push_unique_name(&mut names, settings.protagonist_name.trim());
    for name in settings.protagonist_aliases.lines() {
        push_unique_name(&mut names, name.trim());
    }
    for name in settings.additional_feminize_names.lines() {
        push_unique_name(&mut names, name.trim());
    }
    names
}

fn push_unique_name(names: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !names.iter().any(|existing| existing == name) {
        names.push(name.to_string());
    }
}

fn parse_name_mapping_entries(content: &str) -> Vec<NameMappingEntry> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    let Ok(value) = parse_jsonish_value(content) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    if let Some(protagonist) = value
        .get("protagonist")
        .and_then(serde_json::Value::as_object)
    {
        let source = protagonist
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let target = protagonist
            .get("target")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if !source.is_empty() && !target.is_empty() {
            entries.push(NameMappingEntry {
                source: source.to_string(),
                target: target.to_string(),
            });
        }
    }
    if let Some(items) = value.get("names").and_then(serde_json::Value::as_array) {
        for item in items {
            let source = item
                .get("source")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            let target = item
                .get("target")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim();
            if !source.is_empty() && !target.is_empty() {
                upsert_name_mapping_entry(&mut entries, source, target);
            }
        }
    }
    entries
}

fn build_name_mapping_asset_content(
    settings: &NovelSettings,
    mut mappings: Vec<NameMappingEntry>,
) -> Result<String, String> {
    if !settings.rewritten_protagonist_name.trim().is_empty() {
        upsert_name_mapping_entry(
            &mut mappings,
            settings.protagonist_name.trim(),
            settings.rewritten_protagonist_name.trim(),
        );
    }
    let protagonist = mappings
        .iter()
        .find(|entry| entry.source == settings.protagonist_name.trim())
        .cloned();
    mappings.sort_by(|left, right| left.source.cmp(&right.source));
    mappings.dedup_by(|left, right| left.source == right.source);
    let asset = NameMappingAsset {
        version: 1,
        protagonist,
        names: mappings,
    };
    serde_json::to_string_pretty(&asset).map_err(to_string)
}

fn upsert_name_mapping_entry(entries: &mut Vec<NameMappingEntry>, source: &str, target: &str) {
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return;
    }
    if let Some(entry) = entries.iter_mut().find(|entry| entry.source == source) {
        entry.target = target.to_string();
    } else {
        entries.push(NameMappingEntry {
            source: source.to_string(),
            target: target.to_string(),
        });
    }
}

fn fallback_feminized_name(source: &str) -> String {
    let mut chars = source.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return "妍".to_string();
    }
    if chars.len() == 1 {
        return feminized_char(chars[0]).unwrap_or('妍').to_string();
    }
    let mut changed = false;
    for ch in chars.iter_mut().skip(1) {
        if let Some(next) = feminized_char(*ch) {
            *ch = next;
            changed = true;
        }
    }
    if !changed || chars.iter().collect::<String>() == source {
        if let Some(last) = chars.last_mut() {
            *last = '妍';
        }
    }
    chars.iter().collect()
}

fn feminized_char(ch: char) -> Option<char> {
    match ch {
        '炎' | '岩' | '言' | '焱' | '彦' => Some('妍'),
        '旺' | '望' | '王' => Some('婉'),
        '磊' | '雷' => Some('蕾'),
        '强' => Some('蔷'),
        '刚' | '钢' => Some('婉'),
        '伟' | '威' => Some('薇'),
        '勇' => Some('咏'),
        '龙' => Some('珑'),
        '虎' => Some('琥'),
        '峰' | '锋' => Some('枫'),
        '阳' => Some('漾'),
        '明' => Some('茗'),
        '杰' => Some('洁'),
        '豪' | '昊' => Some('皓'),
        '宇' => Some('羽'),
        '轩' => Some('萱'),
        '飞' => Some('霏'),
        '凡' => Some('樊'),
        '尘' => Some('晨'),
        '三' => Some('姗'),
        _ => None,
    }
}

async fn rewrite_chapters_for_auto(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    batch_id: &str,
    checkpoint_batch_index: Option<i64>,
) -> Result<(), String> {
    let (
        all_chapters,
        settings,
        core_prompt,
        review_enabled,
        review_profile_id,
        rewrite_parallelism,
    ) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, novel_id)?;
        (
            load_chapters_for_batch(&conn, novel_id, batch_id)?,
            settings,
            load_core_prompt(&conn)?,
            load_review_enabled(&conn)?,
            load_review_profile_id(&conn)?,
            load_rewrite_parallelism(&conn)?,
        )
    };
    mark_empty_source_chapters_skipped(state, &all_chapters)?;
    let chapters = all_chapters
        .iter()
        .filter(|chapter| {
            chapter_has_source_body(chapter) && chapter.analysis_status == "completed"
        })
        .cloned()
        .collect::<Vec<_>>();
    if chapters.is_empty() {
        if all_chapters.iter().any(chapter_has_source_body) {
            return Err("当前批次没有已完成分析的内容。".to_string());
        }
        return Ok(());
    }

    let (review_profile, review_api_key) =
        load_review_profile_for_run(state, profile, review_enabled, review_profile_id.as_deref())?;
    ensure_name_mapping_asset(state, novel_id, profile, api_key, &settings).await?;
    let canon_assets = {
        let conn = state.conn.lock().map_err(to_string)?;
        load_canon_assets(&conn, novel_id)?
    };
    let canon_text = build_relevant_canon_text(&canon_assets, &chapters, &settings);
    let staged_chapter_ids = if let Some(batch_index) = checkpoint_batch_index {
        load_staged_chapter_ids(state, novel_id, batch_index, "rewrite")?
    } else {
        HashSet::new()
    };
    for chapter in &chapters {
        if !staged_chapter_ids.contains(&chapter.id) {
            set_chapter_status(state, &chapter.id, "rewrite_status", "running")?;
        }
    }

    services::rewrite::rewrite_and_save(
        state,
        services::rewrite::RewriteRunContext {
            novel_id,
            profile,
            api_key,
            chapters: &chapters,
            canon_text: &canon_text,
            settings: &settings,
            core_prompt: &core_prompt,
            review_enabled,
            review_profile: review_profile.as_ref(),
            review_api_key: review_api_key.as_deref(),
            parallelism: rewrite_parallelism,
            checkpoint_batch_index,
        },
    )
    .await
    .inspect_err(|error| {
        if error != AUTO_RUN_PAUSED
            && error != AUTO_RUN_TERMINATED
            && !is_recoverable_model_format_error(error)
        {
            let _ = mark_chapters_rewrite_failed(state, &chapters);
        }
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn rewrite_batch_with_parallelism(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    all_chapters: &[Chapter],
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    review_enabled: bool,
    review_profile: Option<&ModelProfile>,
    review_api_key: Option<&str>,
    rewrite_parallelism: usize,
    checkpoint_batch_index: Option<i64>,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let effective_profiles = match review_profile {
        Some(review_profile) => vec![profile, review_profile],
        None => vec![profile],
    };
    let rewrite_parallelism = state
        .rate_limits
        .effective_parallelism(rewrite_parallelism, &effective_profiles)?;
    if review_enabled {
        services::review::run_review_pipeline(
            state,
            services::review::ReviewPipelineContext {
                novel_id,
                rewrite_profile: profile,
                rewrite_api_key: api_key,
                review_profile: review_profile.unwrap_or(profile),
                review_api_key: review_api_key.unwrap_or(api_key),
                all_chapters,
                chapters,
                canon_text,
                settings,
                core_prompt,
                parallelism: rewrite_parallelism,
                checkpoint_batch_index,
            },
        )
        .await
    } else {
        generate_rewrite_shards(
            state,
            novel_id,
            profile,
            api_key,
            all_chapters,
            chapters,
            canon_text,
            settings,
            core_prompt,
            false,
            rewrite_parallelism,
            checkpoint_batch_index,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn generate_rewrite_shards(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    all_chapters: &[Chapter],
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    review_enabled: bool,
    rewrite_parallelism: usize,
    checkpoint_batch_index: Option<i64>,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let shard_work = build_contiguous_shard_work(all_chapters, chapters, rewrite_parallelism);
    let shard_total = shard_work.len();
    let batch_label = format_batch_label(chapters);
    let staged = checkpoint_batch_index
        .map(|batch_index| load_staged_outputs(state, novel_id, batch_index, "rewrite"))
        .transpose()?
        .unwrap_or_default();
    set_auto_progress_shard_total(
        state,
        novel_id,
        "rewrite",
        shard_total,
        all_chapters.len(),
        completed_chapter_ids_before_resume(all_chapters, chapters),
    )?;
    let tasks = stream::iter(shard_work.into_iter().enumerate().map(|(idx, work)| {
        let shard = work.chapters.clone();
        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let readonly_context = build_readonly_adjacent_context(&work, &staged, "rewrite");
        let context = format_shard_context_with_neighbors(
            idx,
            shard_total,
            rewrite_parallelism,
            &batch_label,
            &shard,
            &readonly_context,
        );
        async move {
            report_auto_shard_started(
                state,
                novel_id,
                "rewrite",
                idx,
                shard_total,
                &shard,
            )?;
            let parsed = generate_single_rewrite_shard(
                state,
                novel_id,
                profile,
                api_key,
                &shard,
                canon_text,
                settings,
                core_prompt,
                &context,
                &readonly_context,
                &shard_label,
                review_enabled,
            )
            .await;
            Ok::<_, String>((idx, shard_label, shard, parsed))
        }
    }))
    .buffer_unordered(rewrite_parallelism);
    futures_util::pin_mut!(tasks);

    let mut parsed_by_shard = Vec::new();
    loop {
        let result = tokio::select! {
            result = tasks.next() => result,
            _ = tokio::time::sleep(Duration::from_millis(300)) => {
                if let Some(status) = requested_auto_run_stop(state, novel_id)? {
                    return Err(status);
                }
                continue;
            }
        };
        let Some(result) = result else {
            break;
        };
        let (idx, shard_label, shard, parsed) = result?;
        match parsed {
            Ok(parsed) => {
                if let Some(batch_index) = checkpoint_batch_index {
                    stage_rewrite_shard(state, novel_id, batch_index, &parsed)?;
                }
                report_auto_shard_completed(state, novel_id, idx, &shard)?;
                parsed_by_shard.push((idx, parsed))
            }
            Err(error) => {
                return Err(format!("{}：{}", shard_label, error));
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn generate_single_rewrite_shard(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    readonly_adjacent_context: &str,
    shard_label: &str,
    review_enabled: bool,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let shard_canon_text = build_relevant_canon_text_from_text(canon_text, shard, settings);
    let prompt = build_batch_rewrite_prompt_with_context(
        shard,
        &shard_canon_text,
        settings,
        core_prompt,
        shard_context,
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_REWRITE_EXPERT,
        &prompt,
        false,
    )
    .await;
    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(review_enabled)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_rewrite_model_output(&output, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次改写解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    match retry_rewrite_shard_after_parse_error(
                        state,
                        novel_id,
                        profile,
                        api_key,
                        shard,
                        &shard_canon_text,
                        settings,
                        core_prompt,
                        shard_context,
                        shard_label,
                        review_enabled,
                        &error,
                        &output.text,
                    )
                    .await
                    {
                        Ok(parsed) => Ok(parsed),
                        Err(retry_error) => {
                            recover_rewrite_shard_by_subdivision(
                                state,
                                novel_id,
                                profile,
                                api_key,
                                shard,
                                &shard_canon_text,
                                settings,
                                core_prompt,
                                readonly_adjacent_context,
                                shard_label,
                                review_enabled,
                                &retry_error,
                            )
                            .await
                        }
                    }
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn recover_rewrite_shard_by_subdivision(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    readonly_adjacent_context: &str,
    shard_label: &str,
    review_enabled: bool,
    original_error: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    report_auto_shard_phase_for_chapters(state, novel_id, shard, "revision")?;
    let Some((left, right)) = split_chapters_for_rewrite_recovery(shard) else {
        return Err(original_error.to_string());
    };

    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "批次改写自动细分",
        Some(shard_label),
        "running",
        &format!(
            "原分片解析重试后仍失败，开始自动细分为更小分片重写。原错误：{}",
            original_error
        ),
        None,
        None,
    )?;

    let mut pending = std::collections::VecDeque::from([
        (format!("{} · 自动细分 1", shard_label), left),
        (format!("{} · 自动细分 2", shard_label), right),
    ]);
    let mut parsed = Vec::new();

    while let Some((label, subshard)) = pending.pop_front() {
        if let Some(status) = requested_auto_run_stop(state, novel_id)? {
            return Err(status);
        }

        let batch_label = format_batch_label(&subshard);
        let context = format_shard_context_with_neighbors(
            0,
            1,
            1,
            &batch_label,
            &subshard,
            readonly_adjacent_context,
        );
        let subshard_canon_text =
            build_relevant_canon_text_from_text(canon_text, &subshard, settings);
        let prompt = build_batch_rewrite_prompt_with_context(
            &subshard,
            &subshard_canon_text,
            settings,
            core_prompt,
            &context,
        );
        let output = generate_text(
            &state.client,
            Some(state.rate_limits.clone()),
            profile,
            api_key,
            SYSTEM_REWRITE_FORMAT_REPAIR,
            &prompt,
            false,
        )
        .await;

        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写自动细分",
                    Some(&label),
                    "success",
                    &format_model_log_content(&output, profile, Some(review_enabled)),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;

                match parse_rewrite_model_output(&output, &subshard) {
                    Ok(mut subparsed) => parsed.append(&mut subparsed),
                    Err(parse_error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次改写自动细分解析",
                            Some(&label),
                            "error",
                            &parse_error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;

                        match retry_rewrite_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &subshard,
                            &subshard_canon_text,
                            settings,
                            core_prompt,
                            &context,
                            &label,
                            review_enabled,
                            &parse_error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(mut retried) => parsed.append(&mut retried),
                            Err(retry_error) => {
                                if let Some((left, right)) =
                                    split_chapters_for_rewrite_recovery(&subshard)
                                {
                                    pending.push_front((format!("{} · 继续细分 2", label), right));
                                    pending.push_front((format!("{} · 继续细分 1", label), left));
                                } else {
                                    return Err(format!(
                                        "自动细分到单章后仍无法解析：{}",
                                        retry_error
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次改写自动细分",
                    Some(&label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("自动细分改写调用失败：{}", error));
            }
        }
    }

    parsed.sort_by_key(|rewrite| rewrite.index);
    if parsed.len() == shard.len() {
        Ok(parsed)
    } else {
        Err(format!(
            "自动细分后章节数量不匹配：期望 {} 章，得到 {} 章。",
            shard.len(),
            parsed.len()
        ))
    }
}

fn split_chapters_for_rewrite_recovery(
    chapters: &[Chapter],
) -> Option<(Vec<Chapter>, Vec<Chapter>)> {
    if chapters.len() <= 1 {
        return None;
    }
    let mid = chapters.len().div_ceil(2);
    Some((chapters[..mid].to_vec(), chapters[mid..].to_vec()))
}

#[allow(clippy::too_many_arguments)]
async fn generate_reviewed_rewrite_pipeline(
    state: &State<'_, AppState>,
    novel_id: &str,
    rewrite_profile: &ModelProfile,
    rewrite_api_key: &str,
    review_profile: &ModelProfile,
    review_api_key: &str,
    all_chapters: &[Chapter],
    chapters: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    rewrite_parallelism: usize,
    checkpoint_batch_index: Option<i64>,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let shard_work = build_contiguous_shard_work(all_chapters, chapters, rewrite_parallelism);
    let shard_total = shard_work.len();
    let batch_label = format_batch_label(chapters);
    let staged = checkpoint_batch_index
        .map(|batch_index| load_staged_outputs(state, novel_id, batch_index, "rewrite"))
        .transpose()?
        .unwrap_or_default();
    set_auto_progress_shard_total(
        state,
        novel_id,
        "rewrite",
        shard_total,
        all_chapters.len(),
        completed_chapter_ids_before_resume(all_chapters, chapters),
    )?;
    let tasks = stream::iter(shard_work.into_iter().enumerate().map(|(idx, work)| {
        let shard = work.chapters.clone();
        let shard_label = format_shard_label(&batch_label, idx, shard_total, &shard);
        let readonly_context = build_readonly_adjacent_context(&work, &staged, "rewrite");
        let context = format_shard_context_with_neighbors(
            idx,
            shard_total,
            rewrite_parallelism,
            &batch_label,
            &shard,
            &readonly_context,
        );
        async move {
            report_auto_shard_started(
                state,
                novel_id,
                "rewrite",
                idx,
                shard_total,
                &shard,
            )?;
            let rewrite_shard = generate_single_rewrite_shard(
                state,
                novel_id,
                rewrite_profile,
                rewrite_api_key,
                &shard,
                canon_text,
                settings,
                core_prompt,
                &context,
                &readonly_context,
                &shard_label,
                true,
            )
            .await
            .map_err(|error| format!("{}：{}", shard_label, error))?;
            report_auto_shard_phase(state, novel_id, idx, "review")?;
            let reviewed = review_rewrite_shard_strict(
                state,
                novel_id,
                rewrite_profile,
                rewrite_api_key,
                review_profile,
                review_api_key,
                &shard,
                rewrite_shard,
                canon_text,
                settings,
                core_prompt,
                &context,
                &shard_label,
                idx,
            )
            .await?;
            Ok::<_, String>((idx, shard_label, shard, reviewed))
        }
    }))
    .buffer_unordered(rewrite_parallelism);
    futures_util::pin_mut!(tasks);

    let mut parsed_by_shard = Vec::new();
    loop {
        let result = tokio::select! {
            result = tasks.next() => result,
            _ = tokio::time::sleep(Duration::from_millis(300)) => {
                if let Some(status) = requested_auto_run_stop(state, novel_id)? {
                    return Err(status);
                }
                continue;
            }
        };
        let Some(result) = result else {
            break;
        };
        match result {
            Ok((idx, _, shard, parsed)) => {
                if let Some(batch_index) = checkpoint_batch_index {
                    stage_rewrite_shard(state, novel_id, batch_index, &parsed)?;
                }
                report_auto_shard_completed(state, novel_id, idx, &shard)?;
                parsed_by_shard.push((idx, parsed))
            }
            Err(error) => {
                return Err(error);
            }
        }
    }

    parsed_by_shard.sort_by_key(|(idx, _)| *idx);
    Ok(parsed_by_shard
        .into_iter()
        .flat_map(|(_, parsed)| parsed)
        .collect())
}

#[allow(clippy::too_many_arguments)]
async fn review_rewrite_shard_strict(
    state: &State<'_, AppState>,
    novel_id: &str,
    rewrite_profile: &ModelProfile,
    rewrite_api_key: &str,
    review_profile: &ModelProfile,
    review_api_key: &str,
    shard: &[Chapter],
    rewrite_shard: Vec<ParsedChapterRewrite>,
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    progress_shard_index: usize,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let first_decision = review_shard_decision(
        state,
        novel_id,
        review_profile,
        review_api_key,
        shard,
        &rewrite_shard,
        settings,
        core_prompt,
        canon_text,
        shard_context,
        shard_label,
        "批次审查决策",
        SYSTEM_REVIEW_DECISION_EXPERT,
    )
    .await?;
    if first_decision.approved {
        return Ok(rewrite_shard);
    }
    report_auto_shard_phase(state, novel_id, progress_shard_index, "revision")?;
    append_ai_log(
        state,
        Some(novel_id),
        &review_profile.id,
        "批次审查打回",
        Some(shard_label),
        "warning",
        &review_issues_text(&first_decision.issues),
        None,
        None,
    )?;
    let revised = services::repair::repair_reviewed_shard(
        state,
        services::repair::ReviewRepairContext {
            novel_id,
            profile: rewrite_profile,
            api_key: rewrite_api_key,
            shard,
            rewrites: &rewrite_shard,
            canon_text,
            settings,
            core_prompt,
            shard_context,
            shard_label,
            decision: &first_decision,
        },
    )
    .await?;
    let second_label = format!("{} · 第二次审查", shard_label);
    report_auto_shard_phase(state, novel_id, progress_shard_index, "review")?;
    let second_decision = review_revised_shard(
        state,
        novel_id,
        review_profile,
        review_api_key,
        shard,
        &revised,
        settings,
        core_prompt,
        canon_text,
        shard_context,
        &second_label,
    )
    .await?;
    if second_decision.approved {
        return Ok(revised);
    }
    report_auto_shard_phase(state, novel_id, progress_shard_index, "revision")?;
    append_ai_log(
        state,
        Some(novel_id),
        &review_profile.id,
        "批次审查二次打回",
        Some(shard_label),
        "warning",
        &review_issues_text(&second_decision.issues),
        None,
        None,
    )?;
    let second_revised = services::repair::repair_reviewed_shard(
        state,
        services::repair::ReviewRepairContext {
            novel_id,
            profile: rewrite_profile,
            api_key: rewrite_api_key,
            shard,
            rewrites: &revised,
            canon_text,
            settings,
            core_prompt,
            shard_context,
            shard_label,
            decision: &second_decision,
        },
    )
    .await?;
    let third_label = format!("{} · 第三次审查", shard_label);
    report_auto_shard_phase(state, novel_id, progress_shard_index, "final_review")?;
    let third_decision = review_revised_shard(
        state,
        novel_id,
        review_profile,
        review_api_key,
        shard,
        &second_revised,
        settings,
        core_prompt,
        canon_text,
        shard_context,
        &third_label,
    )
    .await?;
    if third_decision.approved {
        return Ok(second_revised);
    }
    let warning_log_result =
        append_review_warning_file(state, novel_id, shard_label, &third_decision);
    append_ai_log(
        state,
        Some(novel_id),
        &review_profile.id,
        "批次审查三次未通过",
        Some(shard_label),
        "warning",
        &format!(
            "第三次审查仍未通过，已保存第二次修复后的完整分片并继续处理后续分片。\n警告日志：{}\n\n第三次审查问题：\n{}",
            warning_log_result,
            review_issues_text(&third_decision.issues)
        ),
        None,
        None,
    )?;
    Ok(second_revised)
}

#[allow(clippy::too_many_arguments)]
async fn review_shard_decision(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    core_prompt: &str,
    canon_text: &str,
    shard_context: &str,
    shard_label: &str,
    log_action: &str,
    system_prompt: &str,
) -> Result<ReviewDecision, String> {
    let prompt = build_batch_review_decision_prompt_with_context(
        shard,
        rewrites,
        settings,
        core_prompt,
        canon_text,
        shard_context,
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        system_prompt,
        &prompt,
        true,
    )
    .await;
    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                log_action,
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(true)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_review_decision_output(&output.text, settings) {
                Ok(decision) => services::validation::validate_review_decision(
                    state,
                    decision,
                    services::validation::ReviewValidationContext {
                        novel_id,
                        profile_id: &profile.id,
                        shard_label,
                        shard,
                        rewrites,
                        settings,
                    },
                ),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        &format!("{}解析", log_action),
                        Some(shard_label),
                        "warning",
                        &format!(
                            "审查决策 JSON 解析失败，开始格式修复重试：{}\n\n原始输出：\n{}",
                            error, output.text
                        ),
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    let repair_prompt =
                        build_review_decision_json_repair_prompt(&output.text, &error, settings);
                    let repair_output = generate_text(
                        &state.client,
                        Some(state.rate_limits.clone()),
                        profile,
                        api_key,
                        SYSTEM_REVIEW_JSON_REPAIR,
                        &repair_prompt,
                        true,
                    )
                    .await;
                    match repair_output {
                        Ok(repair_output) => {
                            append_ai_log(
                                state,
                                Some(novel_id),
                                &profile.id,
                                "审查决策格式修复",
                                Some(shard_label),
                                "success",
                                &format_model_log_content(&repair_output, profile, Some(true)),
                                repair_output.reasoning.as_deref(),
                                Some(&repair_output.raw_response),
                            )?;
                            match parse_review_decision_output(&repair_output.text, settings) {
                                Ok(decision) => services::validation::validate_review_decision(
                                    state,
                                    decision,
                                    services::validation::ReviewValidationContext {
                                        novel_id,
                                        profile_id: &profile.id,
                                        shard_label,
                                        shard,
                                        rewrites,
                                        settings,
                                    },
                                ),
                                Err(repair_error) => {
                                    append_ai_log(
                                        state,
                                        Some(novel_id),
                                        &profile.id,
                                        "审查决策格式修复解析",
                                        Some(shard_label),
                                        "error",
                                        &format!(
                                            "格式修复重试后仍无法解析：{}\n\n修复输出：\n{}",
                                            repair_error, repair_output.text
                                        ),
                                        repair_output.reasoning.as_deref(),
                                        Some(&repair_output.raw_response),
                                    )?;
                                    Err(review_decision_parse_error_message(
                                        shard_label,
                                        &format!(
                                            "{}；格式修复重试后仍失败：{}",
                                            error, repair_error
                                        ),
                                    ))
                                }
                            }
                        }
                        Err(repair_error) => {
                            append_ai_log(
                                state,
                                Some(novel_id),
                                &profile.id,
                                "审查决策格式修复",
                                Some(shard_label),
                                "error",
                                &repair_error,
                                None,
                                None,
                            )?;
                            Err(review_decision_parse_error_message(
                                shard_label,
                                &format!("{}；格式修复重试调用失败：{}", error, repair_error),
                            ))
                        }
                    }
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                log_action,
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("{}：{}", shard_label, error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_review_decision(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile_id: &str,
    shard_label: &str,
    decision: ReviewDecision,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> Result<ReviewDecision, String> {
    let (decision, filtered_issues) =
        filter_review_decision_against_rewrites(decision, shard, rewrites, settings);
    let (decision, deterministic_issues) =
        merge_deterministic_protagonist_residue_issues(decision, shard, rewrites, settings);
    if !filtered_issues.is_empty() {
        append_ai_log(
            state,
            Some(novel_id),
            profile_id,
            "批次审查误判过滤",
            Some(shard_label),
            "warning",
            &format!(
                "以下问题经本地证据校验后确认不构成 blocking（包括改写稿中不存在的性别残留引用，或仅删除了可确定的作者更新提示/非正文附言），已忽略：\n{}",
                review_issues_text(&filtered_issues)
            ),
            None,
            None,
        )?;
    }
    if !deterministic_issues.is_empty() {
        append_ai_log(
            state,
            Some(novel_id),
            profile_id,
            "本地主角残留扫描",
            Some(shard_label),
            "warning",
            &format!(
                "本地扫描发现改写稿仍包含主角原名或派生称呼残留，将作为 blocking 问题进入修复流程：\n{}",
                review_issues_text(&deterministic_issues)
            ),
            None,
            None,
        )?;
    }
    Ok(decision)
}

fn review_decision_parse_error_message(shard_label: &str, error: &str) -> String {
    format!(
        "{}：审查决策无法解析：{}。可以手动重试当前任务；如果频繁出现，请为复检选择 JSON 输出更稳定的模型，或降低并发后再试。",
        shard_label, error
    )
}

fn build_review_decision_json_repair_prompt(
    invalid_output: &str,
    parse_error: &str,
    settings: &NovelSettings,
) -> String {
    format!(
        r#"上一次审查决策输出不是合法 JSON，解析错误：{}

请只修复 JSON 格式，不要重新审查正文，不要新增或删除审查问题，不要改变 approved / summary / issues 的语义。
如果原输出中有中文引号、未转义英文引号、真实换行、尾逗号或 Markdown 包裹，请修复为合法 JSON。

必须输出一个 JSON 对象，格式如下：
{{
  "approved": false,
  "summary": "一句话总体判断",
  "issues": [
    {{
      "chapter_indexes": [1],
      "scope": "chapter",
      "category": "gender_residue",
      "severity": "blocking",
      "problem": "具体问题",
      "required_fix": "必须如何修改"
    }}
  ]
}}

如果原输出语义是完全合格，则输出：
{{
  "approved": true,
  "summary": "合格",
  "issues": []
}}

字段要求：
- approved 必须是布尔值。
- issues 必须是数组。
- issue.severity 必须是 "blocking"。
- chapter_indexes 使用 marker index；如果原输出只有 chapter_index，请转为 chapter_indexes 数组。
- 主角原名：{}
- 主角改写名：{}

只输出修复后的 JSON，不要 Markdown，不要解释。

待修复原输出：
{}"#,
        parse_error,
        settings.protagonist_name.trim(),
        settings.rewritten_protagonist_name.trim(),
        truncate_text(invalid_output, 12_000)
    )
}

fn build_batch_review_decision_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    core_prompt: &str,
    canon_text: &str,
    shard_context: &str,
) -> String {
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    let review_constraints = build_compact_review_constraints(settings, core_prompt, canon_text);
    format!(
        r#"【JSON 输出硬性格式】
- 必须只输出一个合法 JSON 对象，不要 Markdown，不要解释，不要代码块，不要前后缀。
- JSON 字符串内的换行必须写成 \n，英文双引号必须写成 \"，不得输出未转义控制字符。
- 必须报告所有 blocking 问题，不得因为字段长度限制而省略问题。

请以“审查专家”身份判断改写稿是否合格。只列会导致打回的 blocking 问题，不直接改写正文。

Blocking 清单：
- 主角或指定性转角色在改写稿中仍有明确男性姓名、代词、身份、称谓、身体特征或社会角色残留。
- 未指定性转的角色被误改性别、亲属关系、称谓或代词。
- 主角与男性角色共同被复数指代，或群体中包含任一未指定性转的男性成员时，改写稿却使用“她们”；此类混合性别群体必须使用“他们”或准确的群体称呼。只有确认全员女性时才使用“她们”。
- 当前改写稿缺句、重复、串章、空正文、额外章节、marker/章节边界错误，或破坏原文事件顺序、因果、战力、伏笔、人物动机。
- 外貌、能力状态、关系推进、核心设定或高级设定出现实质矛盾。
- 主角改名后，改写稿仍保留“同名、原名、旧名、以旧名某字为名、名字含义”等暴露旧主角姓名或与新姓名矛盾的表达。
- 标题只有在明确出现主角原名，或明确描述主角男性身份、男性称谓、男性身体状态时才算问题；标题编号与 marker index 不一致不是问题。

排除项：
- 每个问题必须引用“待审查改写稿”中仍存在的实际文字；只出现在原文中的证据不得列入 issues。
- 不要把仅与主角原名共享单字的未指定 NPC 当成主角残留。例如主角“石昊”改为“石念昔”时，未被指定或映射的 NPC“秦昊”仍应保留，不是 blocking。
- “这家伙”“这个家伙”“家伙”“熊孩子”“孩子”“吃货”“小鬼”等中性昵称本身不是男性残留。只有同处证据明确出现“少年”“男孩”“男子”“公子”“少爷”“小子”“他”等男性指代且确实指向主角，才是 blocking。
- 原文未明确性别或性别模糊的动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物，改写稿保留原文人称代词和称谓不是问题。
- 群体成员性别不明时，保留原文“他们”或使用中性群体称呼不是问题；不得仅因群体中包含女性主角就要求改成“她们”。
- 原文中明显不属于小说正文的作者更新提示、求票求收藏、简短勘误、作者与读者互动、装饰分隔线和孤立乱码允许删除，不得按缺句或内容缺失打回。完本感言、卷末后记、正式后记、番外和实际剧情正文不在此排除项内；无法确定时必须按正文严格审查。
- 不要输出通过项、优点、确认事项、建议性润色或“无需修改”的内容。

问题字段长度：
- 不限制 issues 数量；同一章存在多个不同 blocking 问题时必须分别列出。
- 同一章同类问题重复出现时可以合并为一条，但 problem 必须包含最关键的当前改写稿短引用。
- summary 最多 80 字。
- problem 最多 120 字，只写当前改写稿中的短证据和为什么 blocking。
- required_fix 最多 120 字，只写必须如何修复。

只输出合法 JSON，不要 Markdown，不要解释。格式：
{{
  "approved": false,
  "summary": "一句话总体判断",
  "issues": [
    {{
      "chapter_indexes": [1],
      "scope": "chapter",
      "category": "gender_residue",
      "severity": "blocking",
      "problem": "具体问题",
      "required_fix": "必须如何修改"
    }}
  ]
}}

如果完全合格：
{{
  "approved": true,
  "summary": "合格",
  "issues": []
}}

`chapter_indexes` 使用 marker 内部 index 定位，不代表标题章节编号。`scope` 只能是 `chapter` 或 `cross_chapter`；跨章连续性、边界、缺失、重复或串章问题使用 `cross_chapter`。兼容旧格式时可使用单个 `chapter_index`。所有 issues 的 severity 必须为 `blocking`；没有实际阻断问题时必须返回 approved=true 和空 issues。

{}

处理范围约束：
{}

原文章节：
{}

待审查改写稿：
{}

再次确认：只输出合法 JSON 对象；不要 Markdown、解释或代码块；字符串内换行写成 \n，英文双引号写成 \"；不得因字段长度限制漏报 blocking 问题。"#,
        review_constraints,
        shard_context,
        build_batch_chapter_text(chapters, false),
        build_batch_rewrite_text(chapters, rewrites)
    )
}

fn build_compact_review_constraints(
    settings: &NovelSettings,
    core_prompt: &str,
    canon_text: &str,
) -> String {
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "按姓名映射表或同音近音规则生成，并保持一致"
    } else {
        settings.rewritten_protagonist_name.trim()
    };
    let additional_names = if settings.additional_feminize_names.trim().is_empty() {
        "无"
    } else {
        settings.additional_feminize_names.trim()
    };
    let advanced_settings = if settings.advanced_settings.trim().is_empty() {
        "无".to_string()
    } else {
        truncate_text(settings.advanced_settings.trim(), 2_000)
    };
    let core_prompt = if core_prompt.trim().is_empty() {
        "无".to_string()
    } else {
        truncate_text(core_prompt.trim(), 2_000)
    };
    let canon_summary = compact_review_canon(canon_text);
    format!(
        "复检约束摘要：\n- 主角原名：{}\n- 主角改写名：{}\n- 其他指定女性化姓名：{}\n- 身材/体型：{} / {}\n- 模式：{}\n- 高级设定：{}\n- 核心设定：{}\n\n相关一致性资料：\n{}\n\n只判断 blocking：主角/指定角色明确男性残留、主角改名后的同名/旧名/以旧名某字为名等姓名逻辑矛盾、未指定角色误改性别、混合性别群体或含男性成员的群体被误称为“她们”、逻辑/边界/缺句/重复/串章、外貌关系或核心设定实质矛盾。标题默认保留；marker index 不是标题编号。仅与主角原名共享单字的未指定 NPC 不得当作主角残留。性别不明的动物、灵兽等非人生物保留原文代词可通过。明显的作者更新提示、求票互动、简短勘误、分隔线和孤立乱码可删除；完本感言、卷末后记、正式后记、番外和剧情正文不可删除。只有确认全员女性时才使用“她们”；群体含男性成员时使用“他们”或准确群体称呼。",
        settings.protagonist_name.trim(),
        rewritten_name,
        additional_names,
        settings.bust,
        settings.body_type,
        rewrite_mode_label(&settings.rewrite_mode),
        advanced_settings,
        core_prompt,
        canon_summary
    )
}

fn compact_review_canon(canon_text: &str) -> String {
    let mut selected = Vec::new();
    let mut current_name = "";
    let mut current_lines = Vec::new();
    let flush = |name: &str, lines: &mut Vec<&str>, output: &mut Vec<String>| {
        if matches!(name, "姓名映射表" | "人物卡" | "人物关系") && !lines.is_empty() {
            output.push(format!(
                "## {}\n{}",
                name,
                truncate_text(&lines.join("\n"), 1_200)
            ));
        }
        lines.clear();
    };
    for line in canon_text.lines() {
        if let Some(name) = line.trim().strip_prefix("## ") {
            flush(current_name, &mut current_lines, &mut selected);
            current_name = name.trim();
        } else if !line.trim().is_empty() {
            current_lines.push(line);
        }
    }
    flush(current_name, &mut current_lines, &mut selected);
    if selected.is_empty() {
        truncate_text(canon_text, 2_500)
    } else {
        selected.join("\n\n")
    }
}

fn build_batch_revision_prompt_with_context(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    decision: &ReviewDecision,
) -> String {
    let issue_text = if decision.issues.is_empty() {
        "审查专家未给出具体问题，但判定不通过；请全面复查主角女性化、指定角色女性化、未指定角色性别保持、称谓、逻辑和章节边界。".to_string()
    } else {
        decision
            .issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {}", idx + 1, review_issue_text(issue)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let base_prompt = build_batch_rewrite_prompt_with_context(
        chapters,
        canon_text,
        settings,
        core_prompt,
        shard_context,
    );
    format!(
        r#"{}

审查专家已打回上一版改写稿。请你作为原改写专家，根据下面的问题清单重新输出当前分片的完整改写结果。

必须遵守：
1. 不要只局部补丁，必须重新输出当前分片所有章节的完整标题和正文。
2. 保留原章节顺序和所有 `<<<YURI_REWRITE_CHAPTER_START ...>>>` / `<<<YURI_REWRITE_CHAPTER_END ...>>>` marker，marker 的 index 和 id 必须逐字复制。
3. 逐条修复审查问题，同时继续遵守姓名映射、女性化要求、未指定角色性别保持、外貌一致性和原文逻辑。主角与男性共同被指代或群体含男性成员时必须使用“他们”或准确群体称呼，只有确认全员女性时才使用“她们”。
4. 只输出当前分片章节，不要解释、不要 Markdown、不要输出审查意见。

审查打回问题：
{}

上一版改写稿：
{}

{}"#,
        base_prompt,
        issue_text,
        build_batch_rewrite_text(chapters, rewrites),
        rewrite_marker_final_reminder("当前分片章节")
    )
}

#[allow(clippy::too_many_arguments)]
fn build_targeted_revision_prompt(
    target_chapters: &[Chapter],
    target_rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    decision: &ReviewDecision,
    adjacent_context: &str,
) -> String {
    let issue_text = if decision.issues.is_empty() {
        "审查专家未给出具体问题；请只复查目标章节中的主角女性化、姓名映射、未指定角色性别保持、逻辑和章节边界。".to_string()
    } else {
        decision
            .issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {}", idx + 1, review_issue_text(issue)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let rewritten_name = if settings.rewritten_protagonist_name.trim().is_empty() {
        "按姓名映射表或同音近音规则生成，并保持一致"
    } else {
        settings.rewritten_protagonist_name.trim()
    };
    let core_prompt = if core_prompt.trim().is_empty() {
        "无".to_string()
    } else {
        truncate_text(core_prompt.trim(), 1_200)
    };
    let advanced_settings = if settings.advanced_settings.trim().is_empty() {
        "无".to_string()
    } else {
        truncate_text(settings.advanced_settings.trim(), 1_200)
    };
    let shard_context = if shard_context.trim().is_empty() {
        "无".to_string()
    } else {
        shard_context.trim().to_string()
    };
    format!(
        r#"请定向修复审查打回的目标章节。只输出目标章节，禁止输出相邻只读章节或其他章节。

{}

必须遵守：
- 主角原名：{}；主角改写名：{}；其他指定女性化姓名：{}。
- 身材/体型：{} / {}；改写模式：{}。
- 核心设定：{}
- 高级设定：{}
- 保留原章节顺序、原文主线、因果、战力、伏笔、人物动机和目标章节 marker。
- 只修复 blocking 问题，不改动已合格内容；未指定性转角色保持原文性别；主角与男性共同被指代或群体含男性成员时使用“他们”或准确群体称呼，只有全员女性时才使用“她们”；性别不明的动物、灵兽等非人生物保留原文代词可通过。
- 每个目标章节必须完整输出原 `<<<YURI_REWRITE_CHAPTER_START ...>>>` 和 `<<<YURI_REWRITE_CHAPTER_END ...>>>`，marker 的 index 和 id 逐字复制。
- 只输出目标章节的 marker、标题、正文；不要解释、不要 Markdown。

分片约束：
{}

审查打回问题：
{}

相关一致性资料：
{}

相邻章节只读上下文（不得输出）：
{}

目标章节原文：
{}

目标章节当前改写稿：
{}

{}"#,
        rewrite_marker_format_guard("目标章节"),
        settings.protagonist_name.trim(),
        rewritten_name,
        if settings.additional_feminize_names.trim().is_empty() {
            "无"
        } else {
            settings.additional_feminize_names.trim()
        },
        settings.bust,
        settings.body_type,
        rewrite_mode_label(&settings.rewrite_mode),
        core_prompt,
        advanced_settings,
        shard_context,
        issue_text,
        canon_text,
        adjacent_context,
        build_batch_chapter_text(target_chapters, false),
        build_batch_rewrite_text(target_chapters, target_rewrites),
        rewrite_marker_final_reminder("目标章节")
    )
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn review_issue_text(issue: &ReviewIssue) -> String {
    let location = if issue.chapter_indexes.is_empty() {
        "未指定章节".to_string()
    } else {
        format!(
            "分片索引 {}",
            issue
                .chapter_indexes
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join("、")
        )
    };
    format!(
        "{} [{} / {} / {}] {} {}",
        location,
        issue.severity,
        issue.scope,
        issue.category,
        issue.problem.trim(),
        issue.required_fix.trim()
    )
    .trim()
    .to_string()
}

fn review_issues_text(issues: &[ReviewIssue]) -> String {
    issues
        .iter()
        .map(review_issue_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn filter_review_decision_against_rewrites(
    decision: ReviewDecision,
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> (ReviewDecision, Vec<ReviewIssue>) {
    let source_name = settings.protagonist_name.trim();
    let rewrites_by_index = rewrites
        .iter()
        .map(|rewrite| (rewrite.index, rewrite))
        .collect::<HashMap<_, _>>();
    let mut retained = Vec::new();
    let mut filtered = Vec::new();
    for issue in decision.issues {
        let issue_text = format!("{} {}", issue.problem, issue.required_fix);
        let claims_source_name_residue = !source_name.is_empty()
            && issue_text.contains(source_name)
            && contains_any(
                &issue_text,
                &[
                    "原名",
                    "男性姓名",
                    "姓名残留",
                    "仍残留",
                    "残留主角",
                    "没有被替换",
                    "未被替换",
                    "未替换",
                    "未改写",
                ],
            );
        let relevant_rewrites = if issue.chapter_indexes.is_empty() {
            rewrites.iter().collect::<Vec<_>>()
        } else {
            issue
                .chapter_indexes
                .iter()
                .filter_map(|index| rewrites_by_index.get(index).copied())
                .collect::<Vec<_>>()
        };
        let source_name_is_present = relevant_rewrites.iter().any(|rewrite| {
            rewrite.title.contains(source_name) || rewrite.text.contains(source_name)
        });
        let quoted_evidence_is_absent =
            gender_residue_evidence_is_absent_from_rewrites(&issue, &relevant_rewrites);
        let neutral_nickname_false_positive =
            gender_residue_claim_only_targets_neutral_nickname(&issue, &relevant_rewrites);
        let ambiguous_non_human_false_positive =
            gender_residue_claim_targets_ambiguous_non_human(&issue, settings, &relevant_rewrites);
        let droppable_author_note_false_positive =
            missing_content_claim_only_targets_droppable_author_notes(
                &issue,
                chapters,
                &relevant_rewrites,
            );
        if !relevant_rewrites.is_empty()
            && ((claims_source_name_residue && !source_name_is_present)
                || quoted_evidence_is_absent
                || neutral_nickname_false_positive
                || ambiguous_non_human_false_positive
                || droppable_author_note_false_positive)
        {
            filtered.push(issue);
        } else {
            retained.push(issue);
        }
    }
    (
        ReviewDecision {
            approved: retained.is_empty(),
            issues: retained,
        },
        filtered,
    )
}

fn missing_content_claim_only_targets_droppable_author_notes(
    issue: &ReviewIssue,
    chapters: &[Chapter],
    rewrites: &[&ParsedChapterRewrite],
) -> bool {
    let issue_text = format!("{} {}", issue.problem, issue.required_fix);
    let category = issue.category.to_ascii_lowercase();
    let claims_missing_content = contains_any(&category, &["missing", "content", "omission"])
        || contains_any(
            &issue_text,
            &[
                "内容缺失",
                "正文缺失",
                "缺句",
                "遗漏",
                "被删除",
                "删除了",
                "未保留",
                "没有保留",
            ],
        );
    if !claims_missing_content {
        return false;
    }
    let relevant_indexes = if issue.chapter_indexes.is_empty() {
        None
    } else {
        Some(
            issue
                .chapter_indexes
                .iter()
                .copied()
                .collect::<HashSet<_>>(),
        )
    };
    let original_lines = chapters
        .iter()
        .filter(|chapter| {
            relevant_indexes
                .as_ref()
                .is_none_or(|indexes| indexes.contains(&chapter.index))
        })
        .flat_map(|chapter| {
            std::iter::once(chapter.title.as_str()).chain(chapter.original_text.lines())
        })
        .collect::<Vec<_>>();
    let mut evidence = extract_review_quoted_evidence(&issue.problem);
    evidence.extend(
        original_lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| {
                !line.is_empty()
                    && issue_text.contains(*line)
                    && is_obvious_droppable_author_note_line(line)
            })
            .map(str::to_string),
    );
    evidence.sort();
    evidence.dedup();
    if evidence.is_empty() {
        return false;
    }
    let rewrite_text = rewrites
        .iter()
        .map(|rewrite| format!("{}\n{}", rewrite.title, rewrite.text))
        .collect::<Vec<_>>()
        .join("\n");

    let omitted_evidence = evidence
        .iter()
        .filter(|fragment| !rewrite_text.contains(fragment.as_str()))
        .collect::<Vec<_>>();
    !omitted_evidence.is_empty()
        && omitted_evidence.iter().all(|fragment| {
            original_lines.iter().any(|line| {
                line.contains(fragment.as_str())
                    && (is_obvious_droppable_author_note_text(fragment)
                        || is_obvious_droppable_author_note_line(line))
            })
        })
}

fn merge_deterministic_protagonist_residue_issues(
    decision: ReviewDecision,
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> (ReviewDecision, Vec<ReviewIssue>) {
    let mut deterministic_issues =
        detect_protagonist_derived_name_residue(chapters, rewrites, settings);
    deterministic_issues.extend(detect_protagonist_name_logic_inconsistency(
        chapters, rewrites, settings,
    ));
    if deterministic_issues.is_empty() {
        return (decision, Vec::new());
    }

    let mut issues = decision.issues;
    let mut existing_keys = issues.iter().map(review_issue_text).collect::<HashSet<_>>();
    let mut added = Vec::new();
    for issue in deterministic_issues {
        let key = review_issue_text(&issue);
        if existing_keys.contains(&key) {
            continue;
        }
        existing_keys.insert(key);
        added.push(issue.clone());
        issues.push(issue);
    }
    (
        ReviewDecision {
            approved: issues.is_empty(),
            issues,
        },
        added,
    )
}

fn detect_protagonist_derived_name_residue(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> Vec<ReviewIssue> {
    let source_names = protagonist_source_names(settings);
    if source_names.is_empty() {
        return Vec::new();
    }
    let rewrite_by_id = rewrites
        .iter()
        .map(|rewrite| (rewrite.id.as_str(), rewrite))
        .collect::<HashMap<_, _>>();
    let mut issues = Vec::new();
    for chapter in chapters {
        let Some(rewrite) = rewrite_by_id.get(chapter.id.as_str()) else {
            continue;
        };
        let candidates = source_names
            .iter()
            .flat_map(|source_name| {
                protagonist_residue_candidates_from_original(chapter, source_name)
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let rewrite_text = format!("{}\n{}", rewrite.title, rewrite.text);
        let mut residues = candidates
            .into_iter()
            .filter(|candidate| rewrite_text.contains(candidate.as_str()))
            .collect::<Vec<_>>();
        residues.sort_by_key(|candidate| (candidate.chars().count(), candidate.clone()));
        residues.dedup();
        if residues.is_empty() {
            continue;
        }
        issues.push(ReviewIssue {
            chapter_indexes: vec![chapter.index],
            scope: "chapter".to_string(),
            category: "source_name_residue".to_string(),
            severity: "blocking".to_string(),
            problem: format!(
                "分片索引 {} 的改写稿仍残留主角原名或派生称呼：{}。",
                chapter.index,
                residues.join("、")
            ),
            required_fix: format!(
                "这些称呼来自原文主角“{}”，必须按姓名映射或改写名统一女性化；不要保留完整原名、原名派生昵称或男性化称谓。",
                source_names.join("、")
            ),
        });
    }
    issues
}

fn detect_protagonist_name_logic_inconsistency(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
) -> Vec<ReviewIssue> {
    let source_name = settings.protagonist_name.trim();
    let rewritten_name = settings.rewritten_protagonist_name.trim();
    if source_name.is_empty() || rewritten_name.is_empty() {
        return Vec::new();
    }
    let rewrite_by_id = rewrites
        .iter()
        .map(|rewrite| (rewrite.id.as_str(), rewrite))
        .collect::<HashMap<_, _>>();
    let mut issues = Vec::new();
    for chapter in chapters {
        let Some(rewrite) = rewrite_by_id.get(chapter.id.as_str()) else {
            continue;
        };
        let rewrite_text = format!("{}\n{}", rewrite.title, rewrite.text);
        let evidence = protagonist_source_names(settings)
            .iter()
            .flat_map(|source| {
                protagonist_name_logic_conflict_evidence(&rewrite_text, source, rewritten_name)
            })
            .collect::<Vec<_>>();
        if evidence.is_empty() {
            continue;
        }
        issues.push(ReviewIssue {
            chapter_indexes: vec![chapter.index],
            scope: "chapter".to_string(),
            category: "name_logic_inconsistency".to_string(),
            severity: "blocking".to_string(),
            problem: format!(
                "分片索引 {} 的改写稿存在主角改名后的姓名逻辑矛盾：{}。",
                chapter.index,
                evidence.join(" / ")
            ),
            required_fix: format!(
                "主角已从“{}”改写为“{}”，涉及同名、旧名、姓名来源、以旧名某字为名等句子必须同步改写为与新姓名一致的逻辑；不得把仅共享单字的未指定 NPC 当作主角改名。",
                source_name, rewritten_name
            ),
        });
    }
    issues
}

fn protagonist_name_logic_conflict_evidence(
    text: &str,
    source_name: &str,
    rewritten_name: &str,
) -> Vec<String> {
    let legacy_parts = protagonist_legacy_name_parts(source_name);
    if legacy_parts.is_empty() {
        return Vec::new();
    }
    let mut evidence = Vec::new();
    for sentence in split_review_sentences(text) {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !legacy_name_sentence_points_to_protagonist(trimmed, rewritten_name) {
            continue;
        }
        if legacy_parts
            .iter()
            .any(|part| contains_legacy_name_semantic_pattern(trimmed, part.as_str()))
        {
            evidence.push(truncate_text(trimmed, 180));
        }
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn split_review_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            if !current.trim().is_empty() {
                sentences.push(current.trim().to_string());
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

fn legacy_name_sentence_points_to_protagonist(sentence: &str, rewritten_name: &str) -> bool {
    (!rewritten_name.is_empty() && sentence.contains(rewritten_name))
        || contains_any(
            sentence,
            &[
                "她原本",
                "她原来",
                "她本来",
                "她曾",
                "她旧",
                "她的原名",
                "她的旧名",
                "主角",
                "女主",
                "原名",
                "旧名",
                "同名",
            ],
        )
}

fn contains_legacy_name_semantic_pattern(sentence: &str, legacy_part: &str) -> bool {
    if legacy_part.is_empty() {
        return false;
    }
    [
        format!("以{legacy_part}为名"),
        format!("以“{legacy_part}”为名"),
        format!("以‘{legacy_part}’为名"),
        format!("{legacy_part}为名"),
        format!("名中带{legacy_part}"),
        format!("名字里有{legacy_part}"),
        format!("名字中有{legacy_part}"),
        format!("名里有{legacy_part}"),
        format!("名叫{legacy_part}"),
    ]
    .iter()
    .any(|pattern| sentence.contains(pattern))
        || ((sentence.contains("同名")
            || sentence.contains("原名")
            || sentence.contains("旧名")
            || sentence.contains("本名"))
            && sentence.contains(legacy_part))
}

fn protagonist_residue_candidates_from_original(
    chapter: &Chapter,
    source_name: &str,
) -> Vec<String> {
    let original_text = format!("{}\n{}", chapter.title, chapter.original_text);
    let mut candidates = protagonist_derived_name_candidates(source_name)
        .into_iter()
        .filter(|candidate| original_text.contains(candidate.as_str()))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| (candidate.chars().count(), candidate.clone()));
    candidates.dedup();
    candidates
}

fn protagonist_legacy_name_parts(source_name: &str) -> Vec<String> {
    let source_name = source_name.trim();
    let chars = source_name.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut parts = vec![source_name.to_string()];
    if chars.len() >= 2 {
        let given = chars[1..].iter().collect::<String>();
        let last = chars[chars.len() - 1].to_string();
        parts.push(given);
        parts.push(last);
    }
    parts.sort_by_key(|part| (part.chars().count(), part.clone()));
    parts.dedup();
    parts
}

fn protagonist_derived_name_candidates(source_name: &str) -> Vec<String> {
    let source_name = source_name.trim();
    let chars = source_name.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut candidates = vec![source_name.to_string()];
    if chars.len() < 2 {
        return candidates;
    }

    let given = chars[1..].iter().collect::<String>();
    let last = chars[chars.len() - 1].to_string();
    let name_parts = if given == last {
        vec![given]
    } else {
        vec![given, last]
    };
    let prefixes = ["小", "老", "大", "阿"];
    let suffixes = [
        "哥", "哥哥", "兄", "兄弟", "弟", "弟弟", "叔", "伯", "爷", "少", "少爷", "公子", "兄台",
        "老弟", "贤弟", "道兄", "道友",
    ];

    for part in &name_parts {
        for prefix in prefixes {
            candidates.push(format!("{prefix}{part}"));
        }
        for suffix in suffixes {
            candidates.push(format!("{part}{suffix}"));
        }
    }
    for suffix in suffixes {
        candidates.push(format!("{source_name}{suffix}"));
    }

    candidates.sort_by_key(|candidate| (candidate.chars().count(), candidate.clone()));
    candidates.dedup();
    candidates
}

fn gender_residue_claim_targets_ambiguous_non_human(
    issue: &ReviewIssue,
    settings: &NovelSettings,
    rewrites: &[&ParsedChapterRewrite],
) -> bool {
    let issue_text = format!("{} {}", issue.problem, issue.required_fix);
    if !is_gender_residue_claim(&issue_text, &issue.category) {
        return false;
    }
    if references_target_protagonist(&issue_text, settings) {
        return false;
    }
    if !contains_non_human_reference(&issue_text)
        || !contains_any(&issue_text, &["代词", "人称", "称谓", "他", "她", "它"])
    {
        return false;
    }
    let evidence = extract_review_quoted_evidence(&issue.problem);
    if evidence.is_empty() {
        return true;
    }
    let rewrite_text = rewrites
        .iter()
        .map(|rewrite| format!("{}\n{}", rewrite.title, rewrite.text))
        .collect::<Vec<_>>()
        .join("\n");
    evidence.iter().any(|fragment| {
        rewrite_text.contains(fragment.as_str())
            && contains_any(fragment, &["他", "她", "它", "牠"])
            && !contains_explicit_male_identity_reference(fragment)
    })
}

fn gender_residue_claim_only_targets_neutral_nickname(
    issue: &ReviewIssue,
    rewrites: &[&ParsedChapterRewrite],
) -> bool {
    let issue_text = format!("{} {}", issue.problem, issue.required_fix);
    if !is_gender_residue_claim(&issue_text, &issue.category) {
        return false;
    }
    let evidence = extract_review_quoted_evidence(&issue.problem);
    if evidence.is_empty() {
        return false;
    }
    let rewrite_text = rewrites
        .iter()
        .map(|rewrite| format!("{}\n{}", rewrite.title, rewrite.text))
        .collect::<Vec<_>>()
        .join("\n");
    let present_evidence = evidence
        .iter()
        .filter(|fragment| rewrite_text.contains(fragment.as_str()))
        .collect::<Vec<_>>();
    if present_evidence.is_empty() {
        return false;
    }
    let mentions_neutral_nickname = present_evidence
        .iter()
        .any(|fragment| contains_neutral_protagonist_nickname(fragment))
        || contains_neutral_protagonist_nickname(&issue_text);
    mentions_neutral_nickname
        && !present_evidence
            .iter()
            .any(|fragment| contains_explicit_male_protagonist_reference(fragment))
}

fn gender_residue_evidence_is_absent_from_rewrites(
    issue: &ReviewIssue,
    rewrites: &[&ParsedChapterRewrite],
) -> bool {
    let issue_text = format!("{} {}", issue.problem, issue.required_fix);
    if !is_gender_residue_claim(&issue_text, &issue.category) {
        return false;
    }
    let evidence = extract_review_quoted_evidence(&issue.problem);
    if evidence.is_empty() {
        return false;
    }
    let rewrite_text = rewrites
        .iter()
        .map(|rewrite| format!("{}\n{}", rewrite.title, rewrite.text))
        .collect::<Vec<_>>()
        .join("\n");
    !evidence
        .iter()
        .any(|fragment| rewrite_text.contains(fragment))
}

fn is_gender_residue_claim(text: &str, category: &str) -> bool {
    category.contains("gender")
        || contains_any(
            text,
            &[
                "男性残留",
                "男性化",
                "男性姓名",
                "男性代词",
                "男性称谓",
                "男性身份",
                "男孩",
                "少年",
                "小子",
                "他",
                "残留",
                "未改写",
                "未修改",
                "没有修改",
                "没有被修改",
                "没有被替换",
                "仍为",
                "仍是",
                "仍然是",
            ],
        )
}

fn references_target_protagonist(text: &str, settings: &NovelSettings) -> bool {
    let source_name = settings.protagonist_name.trim();
    let rewritten_name = settings.rewritten_protagonist_name.trim();
    contains_any(text, &["主角", "女主", "男主"])
        || (!source_name.is_empty() && text.contains(source_name))
        || (!rewritten_name.is_empty() && text.contains(rewritten_name))
        || settings
            .protagonist_aliases
            .lines()
            .any(|alias| !alias.trim().is_empty() && text.contains(alias.trim()))
}

fn protagonist_source_names(settings: &NovelSettings) -> Vec<String> {
    let mut names = Vec::new();
    push_unique_name(&mut names, settings.protagonist_name.trim());
    for alias in settings.protagonist_aliases.lines() {
        push_unique_name(&mut names, alias.trim());
    }
    names
}

fn contains_non_human_reference(text: &str) -> bool {
    contains_any(
        text,
        &[
            "非人",
            "非人生物",
            "动物",
            "灵兽",
            "妖兽",
            "凶兽",
            "神兽",
            "魔兽",
            "异兽",
            "古兽",
            "蛮兽",
            "荒兽",
            "器灵",
            "兽",
            "鸟",
            "鱼",
            "蛇",
            "龙",
            "虎",
            "狼",
            "猴",
            "猿",
            "熊",
            "牛",
            "马",
            "鹏",
            "蛟",
            "狐",
            "雀",
            "龟",
        ],
    )
}

fn contains_neutral_protagonist_nickname(text: &str) -> bool {
    contains_any(
        text,
        &[
            "这个家伙",
            "这家伙",
            "那个家伙",
            "那家伙",
            "家伙",
            "熊孩子",
            "孩子",
            "吃货",
            "小鬼",
        ],
    )
}

fn contains_explicit_male_protagonist_reference(text: &str) -> bool {
    contains_any(
        text,
        &[
            "少年",
            "男孩",
            "男子",
            "男人",
            "男性",
            "男儿",
            "男主",
            "公子",
            "少爷",
            "少主",
            "父亲",
            "兄弟",
            "哥哥",
            "弟弟",
            "小子",
            "汉子",
            "爷们",
            "郎君",
            "少年郎",
            "他",
            "他的",
            "他是",
        ],
    )
}

fn contains_explicit_male_identity_reference(text: &str) -> bool {
    contains_any(
        text,
        &[
            "雄性",
            "公兽",
            "雄兽",
            "雄鸟",
            "公鸟",
            "雄龙",
            "公龙",
            "少年",
            "男孩",
            "男子",
            "男人",
            "男性",
            "男儿",
            "男主",
            "公子",
            "少爷",
            "少主",
            "父亲",
            "兄弟",
            "哥哥",
            "弟弟",
            "小子",
            "汉子",
            "爷们",
            "郎君",
            "少年郎",
        ],
    )
}

fn extract_review_quoted_evidence(text: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    let quote_pairs = [('‘', '’'), ('“', '”'), ('\'', '\''), ('"', '"')];
    for (open, close) in quote_pairs {
        let mut start = None;
        for (idx, ch) in text.char_indices() {
            if ch == open && start.is_none() {
                start = Some(idx + ch.len_utf8());
            } else if ch == close {
                if let Some(start_idx) = start.take() {
                    if start_idx <= idx {
                        let fragment = text[start_idx..idx].trim();
                        if is_meaningful_review_evidence(fragment) {
                            fragments.push(fragment.to_string());
                        }
                    }
                } else if open == close {
                    start = Some(idx + ch.len_utf8());
                }
            }
        }
    }
    fragments.sort();
    fragments.dedup();
    fragments
}

fn is_meaningful_review_evidence(fragment: &str) -> bool {
    if fragment.chars().count() < 2 {
        return false;
    }
    !matches!(
        fragment,
        "blocking"
            | "chapter"
            | "gender_residue"
            | "scope"
            | "category"
            | "severity"
            | "problem"
            | "required_fix"
    )
}

fn extract_review_issue_indexes(text: &str) -> Vec<i64> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"(?:分片索引|chapter_index)\s*[:：=]?\s*(\d+)")
            .expect("valid review issue index regex")
    });
    let mut indexes = pattern
        .captures_iter(text)
        .filter_map(|capture| capture.get(1)?.as_str().parse::<i64>().ok())
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    indexes.dedup();
    indexes
}

fn is_non_actionable_review_issue(
    problem: &str,
    required_fix: &str,
    settings: &NovelSettings,
) -> bool {
    let text = format!("{} {}", problem.trim(), required_fix.trim());
    let text = text.trim();
    if text.is_empty() {
        return true;
    }

    let title_index_false_positive = text.contains("标题")
        && text.contains("索引")
        && contains_any(
            text,
            &[
                "编号",
                "章号",
                "数字",
                "不匹配",
                "不对应",
                "矛盾",
                "统一",
                "重编号",
                "补零",
            ],
        );
    if title_index_false_positive {
        return true;
    }

    let title_only_issue = text.contains("标题") && !text.contains("正文");
    if title_only_issue {
        let original_name = settings.protagonist_name.trim();
        let rewritten_name = settings.rewritten_protagonist_name.trim();
        let identifies_protagonist = text.contains("主角")
            || (!original_name.is_empty() && text.contains(original_name))
            || (!rewritten_name.is_empty() && text.contains(rewritten_name));
        let structural_problem = contains_any(
            text,
            &[
                "标题缺失",
                "标题为空",
                "空标题",
                "标题重复",
                "串章",
                "边界",
                "截断",
                "乱码",
                "不完整",
            ],
        );
        if !identifies_protagonist && !structural_problem {
            return true;
        }
    }

    let explicitly_compliant = contains_any(
        text,
        &[
            "符合规则",
            "符合要求",
            "正确保留",
            "确认保留",
            "无需修改",
            "不需要修改",
            "各项通过",
            "基本通过",
            "基本合格",
            "整体合格",
            "全部合格",
            "审查通过",
            "审查点基本通过",
            "保持良好",
            "表现良好",
            "文风自然",
            "章节完整",
            "修改正确",
            "替换正确",
            "完整性均符合",
            "正确且一致",
            "均已正确",
            "未见残留",
            "未发现问题",
            "未发现阻断",
            "不存在阻断",
            "无阻断问题",
            "没有阻断问题",
            "符合原文",
            "符合情节",
            "符合设定",
            "补充适当",
            "确认即可",
            "保持即可",
            "维持原样",
        ],
    );
    let defect_text = text
        .replace("无需修改", "")
        .replace("不需要修改", "")
        .replace("无需修正", "")
        .replace("不需要修正", "")
        .replace("无需替换", "")
        .replace("不需要替换", "");
    let has_actual_defect = contains_any(
        &defect_text,
        &[
            "关键错误",
            "存在错误",
            "违反",
            "不符合",
            "未遵守",
            "误改",
            "缺失",
            "缺少",
            "遗漏",
            "不一致",
            "矛盾",
            "冲突",
            "断裂",
            "错乱",
            "空正文",
            "不完整",
            "截断",
            "未女性化",
            "没有女性化",
            "仍为男性",
            "仍使用男性",
            "错误地",
            "误将",
            "未改",
            "未替换",
            "破坏",
            "突然跳跃",
            "突然重置",
            "必须修改",
            "需要修改",
            "需要修正",
            "必须修正",
            "应当修改",
            "应修改",
            "需修改",
            "应当修正",
            "应修正",
            "需修正",
            "应当替换",
            "应替换",
            "需替换",
        ],
    );

    explicitly_compliant && !has_actual_defect
}

fn parse_review_decision_output(
    output: &str,
    settings: &NovelSettings,
) -> Result<ReviewDecision, String> {
    let value = parse_jsonish_value(output)?;
    let approved = value
        .get("approved")
        .or_else(|| value.get("pass"))
        .or_else(|| value.get("passed"))
        .and_then(|value| {
            value.as_bool().or_else(|| {
                value.as_str().map(|text| {
                    matches!(
                        text.trim().to_ascii_lowercase().as_str(),
                        "true" | "yes" | "pass" | "passed" | "approved" | "ok"
                    )
                })
            })
        });
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|summary| !summary.is_empty());
    let mut issues = Vec::new();
    let mut raw_issue_count = 0usize;
    if let Some(array) = value.get("issues").and_then(serde_json::Value::as_array) {
        for item in array {
            if let Some(text) = item.as_str() {
                raw_issue_count += 1;
                if !is_non_actionable_review_issue(text, "", settings) {
                    let chapter_indexes = extract_review_issue_indexes(text);
                    issues.push(ReviewIssue {
                        scope: if chapter_indexes.is_empty() {
                            "shard".to_string()
                        } else {
                            "chapter".to_string()
                        },
                        chapter_indexes,
                        category: "legacy".to_string(),
                        severity: "blocking".to_string(),
                        problem: text.trim().to_string(),
                        required_fix: String::new(),
                    });
                }
            } else if item.is_object() {
                raw_issue_count += 1;
                let mut chapter_indexes = item
                    .get("chapter_indexes")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_i64)
                    .collect::<Vec<_>>();
                if let Some(chapter_index) = item
                    .get("chapter_index")
                    .and_then(serde_json::Value::as_i64)
                {
                    chapter_indexes.push(chapter_index);
                }
                chapter_indexes.sort_unstable();
                chapter_indexes.dedup();
                let problem = item
                    .get("problem")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let required_fix = item
                    .get("required_fix")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let severity = item
                    .get("severity")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("blocking");
                let scope = item
                    .get("scope")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(if chapter_indexes.is_empty() {
                        "shard"
                    } else {
                        "chapter"
                    });
                let category = item
                    .get("category")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("general");
                if is_non_actionable_review_issue(problem, required_fix, settings) {
                    continue;
                }
                if !problem.trim().is_empty() || !required_fix.trim().is_empty() {
                    issues.push(ReviewIssue {
                        chapter_indexes,
                        scope: scope.trim().to_ascii_lowercase(),
                        category: category.trim().to_ascii_lowercase(),
                        severity: severity.trim().to_ascii_lowercase(),
                        problem: problem.trim().to_string(),
                        required_fix: required_fix.trim().to_string(),
                    });
                }
            }
        }
    }
    if issues.is_empty() {
        if let Some(summary) = summary {
            if !matches!(summary, "合格" | "通过")
                && !is_non_actionable_review_issue(summary, "", settings)
            {
                issues.push(ReviewIssue {
                    chapter_indexes: Vec::new(),
                    scope: "shard".to_string(),
                    category: "summary".to_string(),
                    severity: "blocking".to_string(),
                    problem: format!("总体判断：{}", summary),
                    required_fix: String::new(),
                });
            }
        }
    }
    if issues.is_empty() && approved == Some(false) && raw_issue_count == 0 {
        issues.push(ReviewIssue {
            chapter_indexes: Vec::new(),
            scope: "shard".to_string(),
            category: "summary".to_string(),
            severity: "blocking".to_string(),
            problem: "总体判断：审查模型判定不合格，但未提供具体可执行问题。".to_string(),
            required_fix: String::new(),
        });
    }
    let approved = issues.is_empty();
    Ok(ReviewDecision { approved, issues })
}

#[derive(Debug, PartialEq, Eq)]
enum RevisionPlan {
    Targeted(Vec<i64>),
    Full(String),
}

fn plan_review_revision(shard: &[Chapter], decision: &ReviewDecision) -> RevisionPlan {
    let shard_indexes = shard
        .iter()
        .map(|chapter| chapter.index)
        .collect::<HashSet<_>>();
    let mut target_indexes = HashSet::new();
    for issue in &decision.issues {
        let issue_text = format!("{} {}", issue.problem, issue.required_fix);
        let category = issue.category.to_ascii_lowercase();
        let scope = issue.scope.to_ascii_lowercase();
        let crosses_chapters = scope != "chapter"
            || contains_any(
                &category,
                &[
                    "cross",
                    "boundary",
                    "continuity",
                    "missing",
                    "duplicate",
                    "order",
                ],
            )
            || contains_any(
                &issue_text,
                &[
                    "跨章",
                    "连续性",
                    "章节边界",
                    "章节缺失",
                    "缺少章节",
                    "章节重复",
                    "重复章节",
                    "串章",
                    "额外章节",
                    "章节顺序",
                    "空正文",
                ],
            );
        if crosses_chapters {
            return RevisionPlan::Full(format!(
                "问题涉及跨章一致性或章节结构：{}",
                review_issue_text(issue)
            ));
        }
        if issue.chapter_indexes.is_empty() {
            return RevisionPlan::Full(format!(
                "审查问题未提供可定位的分片索引：{}",
                review_issue_text(issue)
            ));
        }
        if issue
            .chapter_indexes
            .iter()
            .any(|index| !shard_indexes.contains(index))
        {
            return RevisionPlan::Full(format!(
                "审查问题包含当前分片之外的索引：{}",
                review_issue_text(issue)
            ));
        }
        target_indexes.extend(issue.chapter_indexes.iter().copied());
    }

    if target_indexes.is_empty() {
        return RevisionPlan::Full("审查未提供可执行的目标章节。".to_string());
    }
    if target_indexes.len() * 2 > shard.len() {
        return RevisionPlan::Full(format!(
            "目标章节 {} 个，超过当前分片 {} 章的一半。",
            target_indexes.len(),
            shard.len()
        ));
    }
    let mut ordered = shard
        .iter()
        .filter(|chapter| target_indexes.contains(&chapter.index))
        .map(|chapter| chapter.index)
        .collect::<Vec<_>>();
    ordered.dedup();
    RevisionPlan::Targeted(ordered)
}

fn build_targeted_revision_context(
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    target_indexes: &HashSet<i64>,
) -> String {
    let mut context = Vec::new();
    let rewrite_by_id = rewrites
        .iter()
        .map(|rewrite| (rewrite.id.as_str(), rewrite))
        .collect::<HashMap<_, _>>();
    let mut included = HashSet::new();
    for (position, chapter) in shard.iter().enumerate() {
        if !target_indexes.contains(&chapter.index) {
            continue;
        }
        for neighbor in [position.checked_sub(1), position.checked_add(1)]
            .into_iter()
            .flatten()
            .filter(|neighbor| *neighbor < shard.len())
        {
            let neighbor_chapter = &shard[neighbor];
            if target_indexes.contains(&neighbor_chapter.index)
                || !included.insert(neighbor_chapter.id.clone())
            {
                continue;
            }
            if let Some(rewrite) = rewrite_by_id.get(neighbor_chapter.id.as_str()) {
                context.push(format!(
                    "分片索引 {} · 标题：{}\n原文摘要：{}\n当前改写摘要：{}",
                    neighbor_chapter.index,
                    rewrite.title,
                    truncate_text(neighbor_chapter.original_text.trim(), 80),
                    truncate_text(rewrite.text.trim(), 100)
                ));
            }
        }
    }
    if context.is_empty() {
        "无相邻章节。".to_string()
    } else {
        context.join("\n\n")
    }
}

fn merge_targeted_rewrites(
    rewrites: &[ParsedChapterRewrite],
    targeted: Vec<ParsedChapterRewrite>,
    target_indexes: &[i64],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let expected = target_indexes.iter().copied().collect::<HashSet<_>>();
    let mut replacements = HashMap::new();
    for rewrite in targeted {
        if !expected.contains(&rewrite.index) {
            return Err(format!("定向重写返回了非目标分片索引 {}。", rewrite.index));
        }
        if replacements.insert(rewrite.id.clone(), rewrite).is_some() {
            return Err("定向重写返回了重复章节。".to_string());
        }
    }
    if replacements.len() != expected.len() {
        return Err(format!(
            "定向重写章节数量不匹配：期望 {} 章，得到 {} 章。",
            expected.len(),
            replacements.len()
        ));
    }
    let mut merged = Vec::with_capacity(rewrites.len());
    for rewrite in rewrites {
        if expected.contains(&rewrite.index) {
            let replacement = replacements
                .remove(&rewrite.id)
                .ok_or_else(|| format!("定向重写缺少章节 ID：{}。", rewrite.id))?;
            if replacement.index != rewrite.index {
                return Err(format!("定向重写章节 ID 与索引不匹配：{}。", rewrite.id));
            }
            merged.push(replacement);
        } else {
            merged.push(rewrite.clone());
        }
    }
    Ok(merged)
}

fn validate_targeted_rewrite_markers(
    output: &str,
    target_chapters: &[Chapter],
) -> Result<(), String> {
    static MARKER_PATTERN: OnceLock<Regex> = OnceLock::new();
    let marker_pattern = MARKER_PATTERN.get_or_init(|| {
        Regex::new(r"<<<YURI_REWRITE_CHAPTER_(START|END)\s+index=(\d+)\s+id=([^>\s]+)>>>")
            .expect("valid targeted rewrite marker regex")
    });
    let expected = target_chapters
        .iter()
        .map(|chapter| (chapter.index, chapter.id.as_str()))
        .collect::<HashSet<_>>();
    let mut starts = HashSet::new();
    let mut ends = HashSet::new();
    for capture in marker_pattern.captures_iter(output) {
        let kind = capture.get(1).map(|value| value.as_str()).unwrap_or("");
        let index = capture
            .get(2)
            .and_then(|value| value.as_str().parse::<i64>().ok())
            .ok_or_else(|| "定向重写返回了无效 marker 索引。".to_string())?;
        let id = capture.get(3).map(|value| value.as_str()).unwrap_or("");
        if !expected.contains(&(index, id)) {
            return Err(format!(
                "定向重写返回了非目标或 ID 不匹配的 marker：index={} id={}。",
                index, id
            ));
        }
        let inserted = if kind == "START" {
            starts.insert((index, id))
        } else {
            ends.insert((index, id))
        };
        if !inserted {
            return Err(format!(
                "定向重写返回了重复的 {} marker：index={} id={}。",
                kind, index, id
            ));
        }
    }
    if starts.len() != expected.len() || ends.len() != expected.len() {
        return Err(format!(
            "定向重写 marker 不完整：期望 {} 组，得到 {} 个开始 marker 和 {} 个结束 marker。",
            expected.len(),
            starts.len(),
            ends.len()
        ));
    }
    if starts != expected || ends != expected {
        return Err("定向重写 marker 与目标章节不一致。".to_string());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn revise_rewrite_shard_after_review(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    decision: &ReviewDecision,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let target_indexes = match plan_review_revision(shard, decision) {
        RevisionPlan::Targeted(indexes) => indexes,
        RevisionPlan::Full(reason) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次审查修复规划",
                Some(shard_label),
                "running",
                &format!("修复方式：整分片重写。原因：{}", reason),
                None,
                None,
            )?;
            return revise_full_rewrite_shard_after_review(
                state,
                novel_id,
                profile,
                api_key,
                shard,
                rewrites,
                canon_text,
                settings,
                core_prompt,
                shard_context,
                shard_label,
                decision,
            )
            .await;
        }
    };

    let target_set = target_indexes.iter().copied().collect::<HashSet<_>>();
    let target_chapters = shard
        .iter()
        .filter(|chapter| target_set.contains(&chapter.index))
        .cloned()
        .collect::<Vec<_>>();
    let target_rewrites = rewrites
        .iter()
        .filter(|rewrite| target_set.contains(&rewrite.index))
        .cloned()
        .collect::<Vec<_>>();
    let target_issues = ReviewDecision {
        approved: false,
        issues: decision
            .issues
            .iter()
            .filter(|issue| {
                issue
                    .chapter_indexes
                    .iter()
                    .any(|index| target_set.contains(index))
            })
            .cloned()
            .collect(),
    };
    let adjacent_context = build_targeted_revision_context(shard, rewrites, &target_set);
    let targeted_canon_text =
        build_relevant_canon_text_from_text(canon_text, &target_chapters, settings);
    let prompt = build_targeted_revision_prompt(
        &target_chapters,
        &target_rewrites,
        &targeted_canon_text,
        settings,
        core_prompt,
        shard_context,
        &target_issues,
        &adjacent_context,
    );
    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "批次审查修复规划",
        Some(shard_label),
        "running",
        &format!(
            "修复方式：定向章节重写。目标章节数：{}。分片索引：{}。",
            target_indexes.len(),
            target_indexes
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join("、")
        ),
        None,
        None,
    )?;
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_TARGETED_REVISION_EXPERT,
        &prompt,
        false,
    )
    .await;
    let targeted_result = match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次定向打回重写",
                Some(shard_label),
                "success",
                &format!(
                    "修复方式：定向章节重写；目标章节数：{}。\n{}",
                    target_indexes.len(),
                    format_model_log_content(&output, profile, Some(true))
                ),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            validate_targeted_rewrite_markers(&output.text, &target_chapters)
                .and_then(|_| parse_rewrite_model_output(&output, &target_chapters))
                .and_then(|parsed| merge_targeted_rewrites(rewrites, parsed, &target_indexes))
        }
        Err(error) => Err(error),
    };
    match targeted_result {
        Ok(merged) => Ok(merged),
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次定向打回重写回退",
                Some(shard_label),
                "warning",
                &format!(
                    "定向重写无法可靠解析或合并，回退整分片重写。原因：{}",
                    error
                ),
                None,
                None,
            )?;
            revise_full_rewrite_shard_after_review(
                state,
                novel_id,
                profile,
                api_key,
                shard,
                rewrites,
                canon_text,
                settings,
                core_prompt,
                shard_context,
                shard_label,
                decision,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn revise_full_rewrite_shard_after_review(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    decision: &ReviewDecision,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let prompt = build_batch_revision_prompt_with_context(
        shard,
        rewrites,
        canon_text,
        settings,
        core_prompt,
        shard_context,
        decision,
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_REVIEW_REVISION_EXPERT,
        &prompt,
        false,
    )
    .await;
    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(true)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_rewrite_model_output(&output, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次打回重写解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    match retry_revision_shard_after_parse_error(
                        state,
                        novel_id,
                        profile,
                        api_key,
                        shard,
                        rewrites,
                        canon_text,
                        settings,
                        core_prompt,
                        shard_context,
                        shard_label,
                        decision,
                        &error,
                        &output.text,
                    )
                    .await
                    {
                        Ok(parsed) => Ok(parsed),
                        Err(retry_error) => {
                            recover_revision_shard_by_subdivision(
                                state,
                                novel_id,
                                profile,
                                api_key,
                                shard,
                                rewrites,
                                canon_text,
                                settings,
                                core_prompt,
                                shard_label,
                                decision,
                                &retry_error,
                            )
                            .await
                        }
                    }
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("审查打回后重写失败：{}", error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn review_revised_shard(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    settings: &NovelSettings,
    core_prompt: &str,
    canon_text: &str,
    shard_context: &str,
    shard_label: &str,
) -> Result<ReviewDecision, String> {
    review_shard_decision(
        state,
        novel_id,
        profile,
        api_key,
        shard,
        rewrites,
        settings,
        core_prompt,
        canon_text,
        shard_context,
        shard_label,
        "批次审查复判",
        SYSTEM_REVIEW_FINAL_EXPERT,
    )
    .await
}

fn append_review_warning_file(
    state: &AppState,
    novel_id: &str,
    shard_label: &str,
    third_decision: &ReviewDecision,
) -> String {
    let novel_title = state
        .conn
        .lock()
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
                params![novel_id],
                row_to_novel,
            )
            .ok()
            .map(|novel| novel.title)
        })
        .unwrap_or_else(|| novel_id.to_string());
    append_review_warning_file_for_title(
        &state.app_dir,
        &state.data_dir,
        &novel_title,
        shard_label,
        third_decision,
    )
}

fn append_review_warning_file_for_title(
    app_dir: &Path,
    data_dir: &Path,
    novel_title: &str,
    shard_label: &str,
    third_decision: &ReviewDecision,
) -> String {
    let [root_path, fallback_path] = review_warning_file_paths(app_dir, data_dir, novel_title);
    let content = format!(
        "\n===== {} =====\n小说：{}\n分片：{}\n结果：第三次审查仍未通过，程序已保存第二次重写稿并继续处理后续分片。\n\n第三次审查问题：\n{}\n",
        Utc::now().to_rfc3339(),
        novel_title,
        shard_label,
        format_review_issues(&third_decision.issues)
    );

    match append_text_file(&root_path, &content) {
        Ok(()) => root_path.to_string_lossy().to_string(),
        Err(root_error) => match append_text_file(&fallback_path, &content) {
            Ok(()) => format!(
                "{}（写入软件根目录失败，已改写入应用数据目录：{}）",
                fallback_path.to_string_lossy(),
                root_error
            ),
            Err(fallback_error) => format!(
                "警告日志写入失败；软件根目录错误：{}；应用数据目录错误：{}",
                root_error, fallback_error
            ),
        },
    }
}

pub(crate) fn review_warning_file_paths(
    app_dir: &Path,
    data_dir: &Path,
    novel_title: &str,
) -> [PathBuf; 2] {
    let file_name = format!("{}_审查警告.log", sanitize_file_name(novel_title));
    [app_dir.join(&file_name), data_dir.join(file_name)]
}

fn append_text_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(to_string)?;
    file.write_all(content.as_bytes()).map_err(to_string)
}

fn format_review_issues(issues: &[ReviewIssue]) -> String {
    if issues.is_empty() {
        "未提供具体问题。".to_string()
    } else {
        issues
            .iter()
            .enumerate()
            .map(|(idx, issue)| format!("{}. {}", idx + 1, review_issue_text(issue)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn parse_rewrite_model_output(
    output: &ModelOutput,
    chapters: &[Chapter],
) -> Result<Vec<ParsedChapterRewrite>, String> {
    if let Some(error) = model_output_truncation_error(&output.raw_response) {
        return Err(error);
    }
    parse_batch_rewrite_output(&output.text, chapters)
}

#[allow(clippy::too_many_arguments)]
async fn retry_revision_shard_after_parse_error(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    decision: &ReviewDecision,
    _parse_error: &str,
    bad_output: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let retry_context = format!(
        "{}\n\n必须重新输出当前分片的全部章节，并完整保留每章开始和结束标记。",
        shard_context.trim()
    );
    let base_prompt = build_batch_revision_prompt_with_context(
        shard,
        rewrites,
        canon_text,
        settings,
        core_prompt,
        retry_context.trim(),
        decision,
    );
    let prompt = format!(
        "{}\n\n上一次无法解析的审查打回重写输出如下，仅用于识别格式错误，不要照抄残缺正文或错误边界：\n{}",
        base_prompt,
        truncate_text(bad_output, 12_000)
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_REVIEW_REVISION_REPAIR,
        &prompt,
        false,
    )
    .await;

    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写重试",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(true)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_rewrite_model_output(&output, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次打回重写重试解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    Err(format!(
                        "审查打回重写解析失败后已自动重试，但重试输出仍无法解析：{}",
                        error
                    ))
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次打回重写重试",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("审查打回重写解析失败后自动重试也失败：{}", error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn recover_revision_shard_by_subdivision(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_label: &str,
    decision: &ReviewDecision,
    original_error: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let Some(split) = split_revision_for_recovery(shard, rewrites) else {
        return Err(format!(
            "审查打回重写自动细分到单章后仍无法解析：{}",
            original_error
        ));
    };
    let (left_chapters, left_rewrites) = split.left;
    let (right_chapters, right_rewrites) = split.right;

    append_ai_log(
        state,
        Some(novel_id),
        &profile.id,
        "批次打回重写自动细分",
        Some(shard_label),
        "running",
        &format!(
            "审查打回重写解析重试后仍失败，开始自动细分为更小分片。原错误：{}",
            original_error
        ),
        None,
        None,
    )?;

    let mut pending = std::collections::VecDeque::from([
        (
            format!("{} · 审查打回自动细分 1", shard_label),
            left_chapters,
            left_rewrites,
        ),
        (
            format!("{} · 审查打回自动细分 2", shard_label),
            right_chapters,
            right_rewrites,
        ),
    ]);
    let mut parsed = Vec::new();

    while let Some((label, subshard, subrewrites)) = pending.pop_front() {
        if let Some(status) = requested_auto_run_stop(state, novel_id)? {
            return Err(status);
        }

        let batch_label = format_batch_label(&subshard);
        let context = format_shard_context(0, 1, 1, &batch_label, &subshard);
        let prompt = build_batch_revision_prompt_with_context(
            &subshard,
            &subrewrites,
            canon_text,
            settings,
            core_prompt,
            &context,
            decision,
        );
        let output = generate_text(
            &state.client,
            Some(state.rate_limits.clone()),
            profile,
            api_key,
            SYSTEM_REVIEW_REVISION_EXPERT,
            &prompt,
            false,
        )
        .await;

        match output {
            Ok(output) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次打回重写自动细分",
                    Some(&label),
                    "success",
                    &format_model_log_content(&output, profile, Some(true)),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                match parse_rewrite_model_output(&output, &subshard) {
                    Ok(mut subparsed) => parsed.append(&mut subparsed),
                    Err(parse_error) => {
                        append_ai_log(
                            state,
                            Some(novel_id),
                            &profile.id,
                            "批次打回重写自动细分解析",
                            Some(&label),
                            "error",
                            &parse_error,
                            output.reasoning.as_deref(),
                            Some(&output.raw_response),
                        )?;
                        match retry_revision_shard_after_parse_error(
                            state,
                            novel_id,
                            profile,
                            api_key,
                            &subshard,
                            &subrewrites,
                            canon_text,
                            settings,
                            core_prompt,
                            &context,
                            &label,
                            decision,
                            &parse_error,
                            &output.text,
                        )
                        .await
                        {
                            Ok(mut retried) => parsed.append(&mut retried),
                            Err(retry_error) => {
                                if let Some(split) =
                                    split_revision_for_recovery(&subshard, &subrewrites)
                                {
                                    let (left_chapters, left_rewrites) = split.left;
                                    let (right_chapters, right_rewrites) = split.right;
                                    pending.push_front((
                                        format!("{} · 继续细分 2", label),
                                        right_chapters,
                                        right_rewrites,
                                    ));
                                    pending.push_front((
                                        format!("{} · 继续细分 1", label),
                                        left_chapters,
                                        left_rewrites,
                                    ));
                                } else {
                                    return Err(format!(
                                        "审查打回重写自动细分到单章后仍无法解析：{}",
                                        retry_error
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                append_ai_log(
                    state,
                    Some(novel_id),
                    &profile.id,
                    "批次打回重写自动细分",
                    Some(&label),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                return Err(format!("审查打回自动细分调用失败：{}", error));
            }
        }
    }

    parsed.sort_by_key(|rewrite| rewrite.index);
    if parsed.len() == shard.len() {
        Ok(parsed)
    } else {
        Err(format!(
            "审查打回自动细分后章节数量不匹配：期望 {} 章，得到 {} 章。",
            shard.len(),
            parsed.len()
        ))
    }
}

struct RevisionRecoverySplit {
    left: (Vec<Chapter>, Vec<ParsedChapterRewrite>),
    right: (Vec<Chapter>, Vec<ParsedChapterRewrite>),
}

fn split_revision_for_recovery(
    chapters: &[Chapter],
    rewrites: &[ParsedChapterRewrite],
) -> Option<RevisionRecoverySplit> {
    if chapters.len() <= 1 || chapters.len() != rewrites.len() {
        return None;
    }
    let mid = chapters.len().div_ceil(2);
    Some(RevisionRecoverySplit {
        left: (chapters[..mid].to_vec(), rewrites[..mid].to_vec()),
        right: (chapters[mid..].to_vec(), rewrites[mid..].to_vec()),
    })
}

#[allow(clippy::too_many_arguments)]
async fn retry_rewrite_shard_after_parse_error(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    shard: &[Chapter],
    canon_text: &str,
    settings: &NovelSettings,
    core_prompt: &str,
    shard_context: &str,
    shard_label: &str,
    review_enabled: bool,
    _parse_error: &str,
    bad_output: &str,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    let retry_context = format!(
        "{}\n\n请完全重新输出当前分片，只输出当前分片要求的章节。每章必须包含原样章节开始标记、改写后标题、非空正文和原样章节结束标记。正文不能留空，不能输出当前分片外章节。",
        shard_context.trim()
    );
    let base_prompt = build_batch_rewrite_prompt_with_context(
        shard,
        canon_text,
        settings,
        core_prompt,
        retry_context.trim(),
    );
    let prompt = format!(
        "{}\n\n上一次无法解析的输出如下，仅供你避开格式错误，不要照抄空正文或错误边界：\n{}",
        base_prompt,
        truncate_text(bad_output, 12_000)
    );
    let output = generate_text(
        &state.client,
        Some(state.rate_limits.clone()),
        profile,
        api_key,
        SYSTEM_REWRITE_FORMAT_REPAIR,
        &prompt,
        false,
    )
    .await;

    match output {
        Ok(output) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写重试",
                Some(shard_label),
                "success",
                &format_model_log_content(&output, profile, Some(review_enabled)),
                output.reasoning.as_deref(),
                Some(&output.raw_response),
            )?;
            match parse_rewrite_model_output(&output, shard) {
                Ok(parsed) => Ok(parsed),
                Err(error) => {
                    append_ai_log(
                        state,
                        Some(novel_id),
                        &profile.id,
                        "批次改写重试解析",
                        Some(shard_label),
                        "error",
                        &error,
                        output.reasoning.as_deref(),
                        Some(&output.raw_response),
                    )?;
                    Err(format!(
                        "解析失败后已自动重试，但重试输出仍无法解析：{}",
                        error
                    ))
                }
            }
        }
        Err(error) => {
            append_ai_log(
                state,
                Some(novel_id),
                &profile.id,
                "批次改写重试",
                Some(shard_label),
                "error",
                &error,
                None,
                None,
            )?;
            Err(format!("解析失败后自动重试也失败：{}", error))
        }
    }
}

#[allow(dead_code)]
async fn start_rewrite_legacy(
    novel_id: String,
    profile_id: String,
    batch_id: String,
    state: State<'_, AppState>,
) -> Result<Job, String> {
    let profile = load_model_profile(&state, &profile_id)?;
    let api_key = read_stored_api_key(&state, &profile.id)?;
    let (chapters, canon_assets, settings, core_prompt) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let settings = require_novel_settings(&conn, &novel_id)?;
        let chapters = load_chapters_for_batch(&conn, &novel_id, &batch_id)?
            .into_iter()
            .filter(|chapter| chapter.analysis_status == "completed")
            .collect::<Vec<_>>();
        (
            chapters,
            load_canon_assets(&conn, &novel_id)?,
            settings,
            load_core_prompt(&conn)?,
        )
    };
    if chapters.is_empty() {
        return Err("当前批次没有已完成分析的内容，请先分析该批次。".to_string());
    }
    let total = chapters.len() as i64;
    let mut job = create_job(&state, &novel_id, "rewrite", total)?;
    let canon_text = build_compact_canon_text(&canon_assets);

    for chapter in chapters {
        update_job(
            &state,
            &job.id,
            "running",
            chapter.index,
            &format!("正在改写 {}", chapter.title),
        )?;
        set_chapter_status(&state, &chapter.id, "rewrite_status", "running")?;
        let prompt =
            build_rewrite_prompt_with_settings(&chapter, &canon_text, &settings, &core_prompt);
        match generate_text(
            &state.client,
            Some(state.rate_limits.clone()),
            &profile,
            &api_key,
            SYSTEM_REWRITE_EXPERT,
            &prompt,
            false,
        )
        .await
        {
            Ok(output) => {
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节改写",
                    Some(&chapter.title),
                    "success",
                    &format_model_log_content(&output, &profile, None),
                    output.reasoning.as_deref(),
                    Some(&output.raw_response),
                )?;
                let conn = state.conn.lock().map_err(to_string)?;
                conn.execute(
                    "DELETE FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
                    params![chapter.id],
                )
                .map_err(to_string)?;
                conn.execute(
            "UPDATE chapters SET rewrite_text = ?1, ai_rewrite_text = ?1, rewrite_edited_at = NULL, rewrite_status = 'completed' WHERE id = ?2",
                    params![output.text.trim(), chapter.id],
                )
                .map_err(to_string)?;
            }
            Err(error) => {
                append_ai_log(
                    &state,
                    Some(&novel_id),
                    &profile.id,
                    "章节改写",
                    Some(&chapter.title),
                    "error",
                    &error,
                    None,
                    None,
                )?;
                set_chapter_status(&state, &chapter.id, "rewrite_status", "failed")?;
                update_job(&state, &job.id, "failed", chapter.index, &error)?;
                job = get_job(job.id.clone(), state)?;
                return Ok(job);
            }
        }
    }

    update_job(&state, &job.id, "completed", total, "改写完成")?;
    get_job(job.id, state)
}

fn seed_canon_assets(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    for kind in ["姓名映射表", "人物卡", "人物关系", "地点", "伏笔", "术语表"] {
        conn.execute(
            "INSERT OR IGNORE INTO canon_assets (novel_id, kind, content, updated_at) VALUES (?1, ?2, '', ?3)",
            params![novel_id, kind, now],
        )?;
    }
    Ok(())
}

fn create_chapter_batches(
    conn: &Connection,
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    detected_chapters: bool,
    chapter_batch_size: usize,
) -> Result<(), String> {
    let batch_size = if detected_chapters {
        normalize_chapter_batch_size(chapter_batch_size)
    } else {
        1
    };
    let batch_dir = data_dir.join("chapter_batches").join(novel_id);
    fs::create_dir_all(&batch_dir).map_err(to_string)?;
    let now = Utc::now().to_rfc3339();

    for (idx, chunk) in chapters.chunks(batch_size).enumerate() {
        let first = chunk.first().ok_or_else(|| "批次内容为空。".to_string())?;
        let last = chunk.last().ok_or_else(|| "批次内容为空。".to_string())?;
        let batch_index = (idx + 1) as i64;
        let label = if detected_chapters {
            format!("{}-{}章", first.index, last.index)
        } else {
            format!("第{}批（约10万字）", batch_index)
        };
        let file_path = batch_dir.join(format!("batch-{batch_index:03}.txt"));
        let body = chunk
            .iter()
            .map(|chapter| format!("{}\n\n{}", chapter.title, chapter.original_text))
            .collect::<Vec<_>>()
            .join("\n\n");
        fs::write(&file_path, body).map_err(to_string)?;
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                novel_id,
                batch_index,
                label,
                first.index,
                last.index,
                file_path.to_string_lossy().to_string(),
                now
            ],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

fn chapter_has_source_body(chapter: &Chapter) -> bool {
    !chapter.original_text.trim().is_empty()
}

fn mark_empty_source_chapters_skipped(
    state: &State<'_, AppState>,
    chapters: &[Chapter],
) -> Result<(), String> {
    let empty_chapters = chapters
        .iter()
        .filter(|chapter| !chapter_has_source_body(chapter))
        .collect::<Vec<_>>();
    if empty_chapters.is_empty() {
        return Ok(());
    }
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    for chapter in empty_chapters {
        tx.execute(
            "UPDATE chapters SET analysis_json = NULL, analysis_status = 'completed', rewrite_text = NULL, ai_rewrite_text = NULL, rewrite_edited_at = NULL, rewrite_status = 'completed' WHERE id = ?1",
            params![chapter.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)
}

fn load_chapter_batches(conn: &Connection, novel_id: &str) -> Result<Vec<ChapterBatch>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at FROM chapter_batches WHERE novel_id = ?1 ORDER BY batch_index",
        )
        .map_err(to_string)?;
    let batches = stmt
        .query_map(params![novel_id], row_to_chapter_batch)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(batches)
}

fn load_novel_settings(conn: &Connection, novel_id: &str) -> Result<Option<NovelSettings>, String> {
    let result = conn.query_row(
        "SELECT novel_id, protagonist_name, protagonist_aliases, rewritten_protagonist_name, additional_feminize_names, bust, body_type, rewrite_mode, advanced_settings, updated_at FROM novel_settings WHERE novel_id = ?1",
        params![novel_id],
        row_to_novel_settings,
    );
    match result {
        Ok(settings) => Ok(Some(settings)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(to_string(error)),
    }
}

fn require_novel_settings(conn: &Connection, novel_id: &str) -> Result<NovelSettings, String> {
    let settings =
        load_novel_settings(conn, novel_id)?.ok_or_else(|| "请先填写设定".to_string())?;
    if settings.protagonist_name.trim().is_empty()
        || settings.bust.trim().is_empty()
        || settings.body_type.trim().is_empty()
        || settings.rewrite_mode.trim().is_empty()
    {
        return Err("请先填写设定".to_string());
    }
    Ok(settings)
}

fn load_canon_assets(conn: &Connection, novel_id: &str) -> Result<Vec<CanonAsset>, String> {
    let mut stmt = conn
        .prepare("SELECT novel_id, kind, content, updated_at FROM canon_assets WHERE novel_id = ?1 ORDER BY kind")
        .map_err(to_string)?;
    let assets = stmt
        .query_map(params![novel_id], |row| {
            Ok(CanonAsset {
                novel_id: row.get(0)?,
                kind: row.get(1)?,
                content: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(assets)
}

fn load_canon_asset_content(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
) -> Result<Option<String>, String> {
    match conn.query_row(
        "SELECT content FROM canon_assets WHERE novel_id = ?1 AND kind = ?2",
        params![novel_id, kind],
        |row| row.get::<_, String>(0),
    ) {
        Ok(content) => Ok(Some(content)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(to_string(error)),
    }
}

fn load_model_profile(
    state: &State<'_, AppState>,
    profile_id: &str,
) -> Result<ModelProfile, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    conn.query_row(
        "SELECT id, name, provider, base_url, model, temperature, top_p, thinking_mode,
                prompt_obfuscation_enabled, updated_at, api_key
         FROM model_profiles WHERE id = ?1",
        params![profile_id],
        |row| {
            let id: String = row.get(0)?;
            let db_api_key: Option<String> = row.get(10)?;
            let storage = api_key_storage_from_values(&id, db_api_key.as_deref());
            Ok(ModelProfile {
                has_api_key: storage != ApiKeyStorage::None,
                api_key_storage: storage.as_str().to_string(),
                id,
                name: row.get(1)?,
                provider: row.get(2)?,
                base_url: row.get(3)?,
                model: row.get(4)?,
                temperature: row.get(5)?,
                top_p: row.get(6)?,
                thinking_mode: row.get(7)?,
                prompt_obfuscation_enabled: row.get(8)?,
                updated_at: row.get(9)?,
            })
        },
    )
    .map_err(to_string)
}

fn mark_chapters_rewrite_failed(
    state: &State<'_, AppState>,
    chapters: &[Chapter],
) -> Result<(), String> {
    for chapter in chapters {
        set_chapter_status(state, &chapter.id, "rewrite_status", "failed")?;
    }
    Ok(())
}

fn save_parsed_rewrites(
    state: &State<'_, AppState>,
    rewrites: Vec<ParsedChapterRewrite>,
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    for rewrite in rewrites {
        tx.execute(
            "DELETE FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
            params![rewrite.id],
        )
        .map_err(to_string)?;
        tx.execute(
            "UPDATE chapters SET title = ?1, rewrite_text = ?2, ai_rewrite_text = ?2, rewrite_edited_at = NULL, rewrite_status = 'completed' WHERE id = ?3",
            params![rewrite.title.trim(), rewrite.text.trim(), rewrite.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)?;
    Ok(())
}

fn stage_rewrite_shard(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    rewrites: &[ParsedChapterRewrite],
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    for rewrite in rewrites {
        tx.execute(
            "INSERT INTO auto_run_shard_outputs (
                novel_id, batch_index, phase, chapter_id, chapter_index, title, content, created_at
             ) VALUES (?1, ?2, 'rewrite', ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(novel_id, batch_index, phase, chapter_id) DO UPDATE SET
                chapter_index = excluded.chapter_index,
                title = excluded.title,
                content = excluded.content,
                created_at = excluded.created_at",
            params![
                novel_id,
                batch_index,
                rewrite.id,
                rewrite.index,
                rewrite.title.trim(),
                rewrite.text.trim(),
                now
            ],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)
}

fn apply_staged_rewrites(
    state: &State<'_, AppState>,
    novel_id: &str,
    batch_index: i64,
    chapters: &[Chapter],
) -> Result<(), String> {
    let mut conn = state.conn.lock().map_err(to_string)?;
    let staged = {
        let mut stmt = conn
            .prepare(
                "SELECT chapter_id, title, content FROM auto_run_shard_outputs
                 WHERE novel_id = ?1 AND batch_index = ?2 AND phase = 'rewrite'",
            )
            .map_err(to_string)?;
        let rows = stmt
            .query_map(params![novel_id, batch_index], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ),
                ))
            })
            .map_err(to_string)?
            .collect::<Result<HashMap<_, _>, _>>()
            .map_err(to_string)?;
        rows
    };
    if chapters.iter().any(|chapter| {
        staged
            .get(&chapter.id)
            .is_none_or(|(title, content)| title.is_none() || content.is_none())
    }) {
        return Err("改写分片恢复数据不完整，未写入章节结果。".to_string());
    }

    let tx = conn.transaction().map_err(to_string)?;
    for chapter in chapters {
        let (title, content) = staged
            .get(&chapter.id)
            .expect("validated staged rewrite");
        tx.execute(
            "DELETE FROM chapter_rewrite_snapshots WHERE chapter_id = ?1",
            params![chapter.id],
        )
        .map_err(to_string)?;
        tx.execute(
            "UPDATE chapters SET title = ?1, rewrite_text = ?2, ai_rewrite_text = ?2,
                rewrite_edited_at = NULL, rewrite_status = 'completed' WHERE id = ?3",
            params![title, content, chapter.id],
        )
        .map_err(to_string)?;
    }
    tx.commit().map_err(to_string)
}

fn mark_chapters_analysis_failed(
    state: &State<'_, AppState>,
    chapters: &[Chapter],
) -> Result<(), String> {
    for chapter in chapters {
        set_chapter_status(state, &chapter.id, "analysis_status", "failed")?;
    }
    Ok(())
}

#[allow(dead_code)]
fn merge_analysis_into_canon_assets(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let rows = stmt
        .query_map(params![novel_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let analyses = rows
        .iter()
        .map(|(title, analysis_json)| format!("## {}\n{}", title, analysis_json))
        .collect::<Vec<_>>()
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    upsert_canon_asset(
        conn,
        novel_id,
        "AI分析汇总",
        &compact_analysis_asset("AI分析汇总", &analyses),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "人物卡",
        &compact_analysis_asset("人物卡", &collect_analysis_field(&rows, "characters")),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "人物关系",
        &compact_analysis_asset("人物关系", &collect_analysis_field(&rows, "relationships")),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "地点",
        &compact_analysis_asset("地点", &collect_analysis_field(&rows, "locations")),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "伏笔",
        &compact_analysis_asset("伏笔", &collect_analysis_field(&rows, "foreshadowing")),
        &now,
    )?;
    upsert_canon_asset(
        conn,
        novel_id,
        "术语表",
        &compact_analysis_asset("术语表", &collect_analysis_terms(&rows)),
        &now,
    )?;
    Ok(())
}

fn upsert_canon_asset(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
    content: &str,
    updated_at: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(novel_id, kind) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
        "#,
        params![novel_id, kind, content, updated_at],
    )?;
    Ok(())
}

fn collect_analysis_field(rows: &[(String, String)], field: &str) -> String {
    rows.iter()
        .filter_map(|(title, analysis_json)| {
            let value = serde_json::from_str::<serde_json::Value>(analysis_json).ok()?;
            let text = json_field_to_text(value.get(field)?);
            if text.trim().is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", title, text))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn compact_analysis_asset(kind: &str, content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("## ") {
            lines.push(trimmed.to_string());
            continue;
        }
        let key = normalize_analysis_entry(trimmed);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    truncate_analysis_asset_at_line(kind, &lines.join("\n"))
}

fn normalize_analysis_entry(text: &str) -> String {
    text.chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '-' | '，' | ',' | '。' | '.' | '；' | ';' | '：' | ':' | '、'
                )
        })
        .collect()
}

fn analysis_asset_char_limit(kind: &str) -> usize {
    match kind {
        "AI分析汇总" => 6_000,
        "人物卡" => 7_000,
        "人物关系" => 5_000,
        "术语表" => 4_000,
        "伏笔" => 5_000,
        "地点" => 3_000,
        _ => 3_000,
    }
}

fn truncate_analysis_asset_at_line(kind: &str, content: &str) -> String {
    let limit = analysis_asset_char_limit(kind);
    if content.chars().count() <= limit {
        return content.to_string();
    }

    let note = format!("[{}已去重并达到长度上限，后续低相关条目已省略]", kind);
    let mut output = String::new();
    for line in content.lines() {
        let projected = output.chars().count() + line.chars().count() + 1 + note.chars().count();
        if projected > limit {
            break;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line);
    }
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(&note);
    output
}

fn collect_analysis_terms(rows: &[(String, String)]) -> String {
    rows.iter()
        .filter_map(|(title, analysis_json)| {
            let value = serde_json::from_str::<serde_json::Value>(analysis_json).ok()?;
            let mut sections = Vec::new();
            if let Some(text) = value
                .get("terms")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("原文术语：\n{}", text));
            }
            if let Some(text) = value
                .get("names")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("原文姓名与称谓：\n{}", text));
            }
            if let Some(text) = value
                .get("name_feminization_map")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("姓名女性化映射：\n{}", text));
            }
            if let Some(text) = value
                .get("rewrite_notes")
                .map(json_field_to_text)
                .filter(|text| !text.trim().is_empty())
            {
                sections.push(format!("改写注意事项：\n{}", text));
            }
            if sections.is_empty() {
                None
            } else {
                Some(format!("## {}\n{}", title, sections.join("\n\n")))
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn json_field_to_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(json_field_to_text)
            .filter(|text| !text.trim().is_empty())
            .map(|text| format!("- {}", text))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    }
}

fn fill_empty_canon_assets_from_analysis(
    conn: &Connection,
    novel_id: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let rows = stmt
        .query_map(params![novel_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(());
    }

    let analyses = rows
        .iter()
        .map(|(title, analysis_json)| format!("## {}\n{}", title, analysis_json))
        .collect::<Vec<_>>()
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "AI分析汇总",
        &compact_analysis_asset("AI分析汇总", &analyses),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "人物卡",
        &compact_analysis_asset("人物卡", &collect_analysis_field(&rows, "characters")),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "人物关系",
        &compact_analysis_asset("人物关系", &collect_analysis_field(&rows, "relationships")),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "地点",
        &compact_analysis_asset("地点", &collect_analysis_field(&rows, "locations")),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "伏笔",
        &compact_analysis_asset("伏笔", &collect_analysis_field(&rows, "foreshadowing")),
        &now,
    )?;
    upsert_empty_canon_asset(
        conn,
        novel_id,
        "术语表",
        &compact_analysis_asset("术语表", &collect_analysis_terms(&rows)),
        &now,
    )?;
    Ok(())
}

fn upsert_empty_canon_asset(
    conn: &Connection,
    novel_id: &str,
    kind: &str,
    content: &str,
    updated_at: &str,
) -> rusqlite::Result<()> {
    if content.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(novel_id, kind) DO UPDATE SET
            content = CASE
                WHEN trim(canon_assets.content) = '' THEN excluded.content
                ELSE canon_assets.content
            END,
            updated_at = CASE
                WHEN trim(canon_assets.content) = '' THEN excluded.updated_at
                ELSE canon_assets.updated_at
            END
        "#,
        params![novel_id, kind, content, updated_at],
    )?;
    Ok(())
}

#[allow(dead_code)]
fn merge_analysis_into_canon(conn: &Connection, novel_id: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT title, analysis_json FROM chapters WHERE novel_id = ?1 AND analysis_json IS NOT NULL ORDER BY chapter_index",
    )?;
    let analyses = stmt
        .query_map(params![novel_id], |row| {
            Ok(format!(
                "## {}\n{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .join("\n\n");
    let now = Utc::now().to_rfc3339();
    conn.execute(
        r#"
        INSERT INTO canon_assets (novel_id, kind, content, updated_at)
        VALUES (?1, 'AI分析汇总', ?2, ?3)
        ON CONFLICT(novel_id, kind) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
        "#,
        params![novel_id, analyses, now],
    )?;
    Ok(())
}

fn prepare_auto_run(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile_ids: HashSet<String>,
    requested_start_batch_index: i64,
) -> Result<(i64, i64), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get(novel_id) {
        if control.status == "running" || control.status == "pause_requested" {
            return Err("一键分析改写正在运行，请先暂停或终止当前任务。".to_string());
        }
    }
    let paused_control = runs
        .get(novel_id)
        .filter(|control| control.status == "paused");
    let start_batch_index = paused_control
        .map(|control| control.start_batch_index)
        .unwrap_or(requested_start_batch_index);
    let resume_from = paused_control
        .map(|control| control.completed_batches)
        .unwrap_or(start_batch_index);
    runs.insert(
        novel_id.to_string(),
        AutoRunControl {
            status: "running".to_string(),
            start_batch_index,
            completed_batches: resume_from,
            job_id: None,
            profile_ids,
            recoverable: true,
        },
    );
    let control = runs.get(novel_id).cloned().expect("inserted auto run");
    drop(runs);
    persist_auto_run_checkpoint(state, novel_id, &control, "", None, None)?;
    Ok((resume_from, start_batch_index))
}

fn register_auto_run_job(
    state: &State<'_, AppState>,
    novel_id: &str,
    job_id: &str,
    completed_batches: i64,
    start_batch_index: i64,
) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    let control = runs
        .entry(novel_id.to_string())
        .or_insert_with(|| AutoRunControl {
            status: "running".to_string(),
            start_batch_index,
            completed_batches,
            job_id: None,
            profile_ids: HashSet::new(),
            recoverable: true,
        });
    control.status = "running".to_string();
    control.start_batch_index = start_batch_index;
    control.completed_batches = completed_batches;
    control.job_id = Some(job_id.to_string());
    let control = control.clone();
    drop(runs);
    persist_auto_run_checkpoint(state, novel_id, &control, "", None, None)?;
    Ok(())
}

fn set_auto_run_completed(
    state: &State<'_, AppState>,
    novel_id: &str,
    completed_batches: i64,
) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(novel_id) {
        control.completed_batches = completed_batches;
        let control = control.clone();
        drop(runs);
        persist_auto_run_checkpoint(
            state,
            novel_id,
            &control,
            "",
            Some("export"),
            Some(completed_batches),
        )?;
        let conn = state.conn.lock().map_err(to_string)?;
        conn.execute(
            "DELETE FROM auto_run_shard_outputs WHERE novel_id = ?1 AND batch_index = ?2",
            params![novel_id, completed_batches],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

fn persist_auto_run_checkpoint(
    state: &State<'_, AppState>,
    novel_id: &str,
    control: &AutoRunControl,
    pause_reason: &str,
    phase: Option<&str>,
    batch_index: Option<i64>,
) -> Result<(), String> {
    if !control.recoverable {
        return Ok(());
    }
    let profile_ids =
        serde_json::to_string(&control.profile_ids.iter().cloned().collect::<Vec<_>>())
            .map_err(to_string)?;
    let now = Utc::now().to_rfc3339();
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        r#"
        INSERT INTO auto_run_checkpoints (
            novel_id, start_batch_index, next_batch_index, job_id, status,
            pause_reason, phase, batch_index, profile_ids, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
        ON CONFLICT(novel_id) DO UPDATE SET
            start_batch_index = excluded.start_batch_index,
            next_batch_index = excluded.next_batch_index,
            job_id = excluded.job_id,
            status = excluded.status,
            pause_reason = CASE
                WHEN excluded.status = 'running' THEN excluded.pause_reason
                WHEN excluded.pause_reason = '' THEN auto_run_checkpoints.pause_reason
                ELSE excluded.pause_reason
            END,
            phase = COALESCE(excluded.phase, auto_run_checkpoints.phase),
            batch_index = COALESCE(excluded.batch_index, auto_run_checkpoints.batch_index),
            profile_ids = excluded.profile_ids,
            updated_at = excluded.updated_at
        "#,
        params![
            novel_id,
            control.start_batch_index,
            control.completed_batches,
            control.job_id,
            control.status,
            pause_reason,
            phase,
            batch_index,
            profile_ids,
            now
        ],
    )
    .map_err(to_string)?;
    Ok(())
}

fn update_auto_run_checkpoint_phase(
    state: &State<'_, AppState>,
    novel_id: &str,
    phase: &str,
    batch_index: i64,
) -> Result<(), String> {
    let control = state
        .auto_runs
        .lock()
        .map_err(to_string)?
        .get(novel_id)
        .cloned()
        .ok_or_else(|| "当前一键任务状态不存在。".to_string())?;
    persist_auto_run_checkpoint(
        state,
        novel_id,
        &control,
        "",
        Some(phase),
        Some(batch_index),
    )
}

fn requested_auto_run_stop(
    state: &State<'_, AppState>,
    novel_id: &str,
) -> Result<Option<String>, String> {
    let runs = state.auto_runs.lock().map_err(to_string)?;
    Ok(runs.get(novel_id).and_then(|control| {
        if control.status == "pause_requested" {
            Some(AUTO_RUN_PAUSED.to_string())
        } else if control.status == "terminate_requested" {
            Some(AUTO_RUN_TERMINATED.to_string())
        } else {
            None
        }
    }))
}

fn request_auto_run_stop(
    state: &State<'_, AppState>,
    novel_id: &str,
    status: &str,
) -> Result<Job, String> {
    let (job_id, completed_batches, start_batch_index, message, job_status, terminate_paused_run) = {
        let mut runs = state.auto_runs.lock().map_err(to_string)?;
        let control = runs
            .get_mut(novel_id)
            .ok_or_else(|| "当前没有正在运行的一键分析改写任务。".to_string())?;
        let terminate_paused_run = should_terminate_paused_run(&control.status, status);
        control.status = status.to_string();
        let job_id = control
            .job_id
            .clone()
            .ok_or_else(|| "当前一键任务尚未创建进度记录。".to_string())?;
        let message = if status == "terminate_requested" {
            "正在终止一键分析改写，当前未输出批次将不会保存。"
        } else {
            "正在暂停一键分析改写，已完成分片会保留，继续时仅处理未完成分片。"
        };
        let job_status = if status == "terminate_requested" {
            "terminating"
        } else {
            "pausing"
        };
        (
            job_id,
            control.completed_batches,
            control.start_batch_index,
            message.to_string(),
            job_status.to_string(),
            terminate_paused_run,
        )
    };
    if !terminate_paused_run {
        let control = state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .get(novel_id)
            .cloned()
            .ok_or_else(|| "当前一键任务状态不存在。".to_string())?;
        persist_auto_run_checkpoint(state, novel_id, &control, &message, None, None)?;
    }
    if terminate_paused_run {
        let message = "一键分析改写已终止。下次点击将从头开始新的执行。";
        update_job(
            state,
            &job_id,
            "terminated",
            completed_batches.saturating_sub(start_batch_index),
            message,
        )?;
        clear_auto_run(state, novel_id)?;
        return load_job(state, &job_id);
    }
    update_job(
        state,
        &job_id,
        &job_status,
        completed_batches.saturating_sub(start_batch_index),
        &message,
    )?;
    load_job(state, &job_id)
}

fn finish_stopped_auto_run(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    status_marker: &str,
) -> Result<Job, String> {
    let completed_in_range = completed_batches.saturating_sub(start_batch_index);
    if status_marker == AUTO_RUN_TERMINATED {
        let message = "一键分析改写已终止。下次点击将从头开始新的执行。";
        update_job(state, &job.id, "terminated", completed_in_range, message)?;
        emit_job_progress(app, &job, "terminated", completed_in_range, message);
        clear_auto_run(state, &job.novel_id)?;
    } else {
        let message = format!(
            "一键分析改写已暂停。继续后将处理第 {} 批的未完成分片。",
            completed_batches + 1
        );
        update_job(state, &job.id, "paused", completed_in_range, &message)?;
        emit_job_progress(app, &job, "paused", completed_in_range, &message);
        let mut runs = state.auto_runs.lock().map_err(to_string)?;
        if let Some(control) = runs.get_mut(&job.novel_id) {
            control.status = "paused".to_string();
            control.completed_batches = completed_batches;
            control.job_id = Some(job.id.clone());
            let control = control.clone();
            drop(runs);
            persist_auto_run_checkpoint(state, &job.novel_id, &control, &message, None, None)?;
        }
    }
    load_job(state, &job.id)
}

fn pause_auto_run_after_rate_limit(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    error: &str,
) -> Result<Job, String> {
    let completed_in_range = completed_batches.saturating_sub(start_batch_index);
    let message = format!(
        "服务商限流重试已耗尽，任务已暂停。请降低并发、等待额度恢复或更换模型后点击继续；已完成分片已保留，将继续处理第 {} 批的未完成分片。\n\n{}",
        completed_batches + 1,
        error
    );
    update_job(state, &job.id, "paused", completed_in_range, &message)?;
    emit_job_progress(app, &job, "paused", completed_in_range, &message);
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(&job.novel_id) {
        control.status = "paused".to_string();
        control.completed_batches = completed_batches;
        control.job_id = Some(job.id.clone());
        let control = control.clone();
        drop(runs);
        persist_auto_run_checkpoint(state, &job.novel_id, &control, &message, None, None)?;
    }
    load_job(state, &job.id)
}

fn pause_auto_run_after_network_error(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    error: &str,
) -> Result<Job, String> {
    let completed_in_range = completed_batches.saturating_sub(start_batch_index);
    let message = format!(
        "网络连接异常，任务已暂停。请检查网络、代理或服务商连接状态后点击继续；已完成分片已保留，将继续处理第 {} 批的未完成分片。\n\n{}",
        completed_batches + 1,
        error
    );
    update_job(state, &job.id, "paused", completed_in_range, &message)?;
    emit_job_progress(app, &job, "paused", completed_in_range, &message);
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(&job.novel_id) {
        control.status = "paused".to_string();
        control.completed_batches = completed_batches;
        control.job_id = Some(job.id.clone());
        let control = control.clone();
        drop(runs);
        persist_auto_run_checkpoint(state, &job.novel_id, &control, &message, None, None)?;
    }
    load_job(state, &job.id)
}

fn pause_auto_run_after_temporary_gateway_error(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    error: &str,
) -> Result<Job, String> {
    let completed_in_range = completed_batches.saturating_sub(start_batch_index);
    let message = format!(
        "模型服务或反向代理暂时不可用，任务已暂停。可以调整并发或模型后点击继续；已完成分片已保留，将继续处理第 {} 批的未完成分片。\n\n{}",
        completed_batches + 1,
        error
    );
    update_job(state, &job.id, "paused", completed_in_range, &message)?;
    emit_job_progress(app, &job, "paused", completed_in_range, &message);
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(&job.novel_id) {
        control.status = "paused".to_string();
        control.completed_batches = completed_batches;
        control.job_id = Some(job.id.clone());
        let control = control.clone();
        drop(runs);
        persist_auto_run_checkpoint(state, &job.novel_id, &control, &message, None, None)?;
    }
    load_job(state, &job.id)
}

fn pause_auto_run_after_model_format_error(
    state: &State<'_, AppState>,
    app: &AppHandle,
    job: Job,
    completed_batches: i64,
    start_batch_index: i64,
    error: &str,
) -> Result<Job, String> {
    let completed_in_range = completed_batches.saturating_sub(start_batch_index);
    let message = format!(
        "模型输出格式多次修复后仍无法解析，任务已暂停。已完成分片已保留，可以点击继续处理第 {} 批的未完成分片；如果频繁出现，请更换 JSON 输出更稳定的模型、提高并发以缩小单个分片，或缩小处理范围。\n\n{}",
        completed_batches + 1,
        error
    );
    update_job(state, &job.id, "paused", completed_in_range, &message)?;
    emit_job_progress(app, &job, "paused", completed_in_range, &message);
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    if let Some(control) = runs.get_mut(&job.novel_id) {
        control.status = "paused".to_string();
        control.completed_batches = completed_batches;
        control.job_id = Some(job.id.clone());
        let control = control.clone();
        drop(runs);
        persist_auto_run_checkpoint(state, &job.novel_id, &control, &message, None, None)?;
    }
    load_job(state, &job.id)
}

fn clear_auto_run(state: &State<'_, AppState>, novel_id: &str) -> Result<(), String> {
    let mut runs = state.auto_runs.lock().map_err(to_string)?;
    runs.remove(novel_id);
    drop(runs);
    state
        .auto_run_progress
        .lock()
        .map_err(to_string)?
        .remove(novel_id);
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(
        "DELETE FROM auto_run_checkpoints WHERE novel_id = ?1",
        params![novel_id],
    )
    .map_err(to_string)?;
    Ok(())
}

fn emit_job_progress(
    app: &AppHandle,
    job: &Job,
    status: &str,
    current_chapter: i64,
    message: &str,
) {
    let progress = JobProgress {
        id: job.id.clone(),
        novel_id: job.novel_id.clone(),
        job_type: job.job_type.clone(),
        status: status.to_string(),
        current_chapter,
        total_chapters: job.total_chapters,
        message: message.to_string(),
        phase: None,
        batch_index: None,
        batch_total: None,
        batch_label: None,
        shard_completed: None,
        shard_total: None,
        chapter_completed: None,
        chapter_total: None,
        active_shards: None,
    };
    let _ = app.emit("job-progress", progress);
}

#[cfg(any())]
mod legacy_progress_implementation {
    use super::*;

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

    fn begin_auto_batch_progress(
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
        state.auto_run_progress.lock().map_err(to_string)?.insert(
            novel_id.to_string(),
            AutoRunProgressState {
                phase: Some(phase.to_string()),
                batch_index: Some(batch_index),
                batch_total: Some(batch_total),
                batch_label: Some(batch_label.to_string()),
                ..AutoRunProgressState::default()
            },
        );
        emit_auto_runtime_progress(state, novel_id)
    }

    fn set_auto_progress_shard_total(
        state: &State<'_, AppState>,
        novel_id: &str,
        phase: &str,
        shard_total: usize,
    ) -> Result<(), String> {
        if !state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .contains_key(novel_id)
        {
            return Ok(());
        }
        let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
        let entry = progress.entry(novel_id.to_string()).or_default();
        entry.phase = Some(phase.to_string());
        entry.shard_total = shard_total;
        entry.completed_shards.clear();
        entry.active_shards.clear();
        drop(progress);
        emit_auto_runtime_progress(state, novel_id)
    }

    fn set_auto_progress_phase(
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

    fn report_auto_shard_started(
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

    fn report_auto_shard_phase(
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

    fn report_auto_shard_phase_for_chapters(
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

    fn report_auto_shard_completed(
        state: &State<'_, AppState>,
        novel_id: &str,
        shard_index: usize,
    ) -> Result<(), String> {
        let mut progress = state.auto_run_progress.lock().map_err(to_string)?;
        let Some(entry) = progress.get_mut(novel_id) else {
            return Ok(());
        };
        entry.active_shards.remove(&shard_index);
        entry.completed_shards.insert(shard_index);
        drop(progress);
        emit_auto_runtime_progress(state, novel_id)
    }

    fn emit_auto_runtime_progress(
        state: &State<'_, AppState>,
        novel_id: &str,
    ) -> Result<(), String> {
        let control = state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .get(novel_id)
            .cloned();
        let Some(control) = control else {
            return Ok(());
        };
        let Some(job_id) = control.job_id.as_deref() else {
            return Ok(());
        };
        let progress_state = state
            .auto_run_progress
            .lock()
            .map_err(to_string)?
            .get(novel_id)
            .cloned()
            .unwrap_or_default();
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
        let message = if shard_total > 0 {
            format!(
                "第 {}/{} 批 · {} · 分片已完成 {}/{}",
                batch_index,
                batch_total,
                phase_label(phase),
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
        update_job(
            state,
            &job.id,
            "running",
            control
                .completed_batches
                .saturating_sub(control.start_batch_index),
            &message,
        )?;
        let payload = JobProgress {
            id: job.id,
            novel_id: job.novel_id,
            job_type: job.job_type,
            status: "running".to_string(),
            current_chapter: control
                .completed_batches
                .saturating_sub(control.start_batch_index),
            total_chapters: job.total_chapters,
            message,
            phase: progress_state.phase,
            batch_index: progress_state.batch_index,
            batch_total: progress_state.batch_total,
            batch_label: progress_state.batch_label,
            shard_completed: Some(shard_completed),
            shard_total: Some(shard_total),
            active_shards: Some(progress_state.active_shards.into_values().collect()),
        };
        let _ = state.app.emit("job-progress", payload);
        Ok(())
    }
}

fn set_chapter_status(
    state: &State<'_, AppState>,
    chapter_id: &str,
    column: &str,
    status: &str,
) -> Result<(), String> {
    let sql = match column {
        "analysis_status" => "UPDATE chapters SET analysis_status = ?1 WHERE id = ?2",
        "rewrite_status" => "UPDATE chapters SET rewrite_status = ?1 WHERE id = ?2",
        _ => return Err("invalid chapter status column".to_string()),
    };
    let conn = state.conn.lock().map_err(to_string)?;
    conn.execute(sql, params![status, chapter_id])
        .map_err(to_string)?;
    Ok(())
}

fn read_stored_api_key(state: &State<'_, AppState>, profile_id: &str) -> Result<String, String> {
    if let Ok(api_key) = read_api_key(profile_id) {
        if !api_key.trim().is_empty() {
            let conn = state.conn.lock().map_err(to_string)?;
            conn.execute(
                "UPDATE model_profiles SET api_key = NULL WHERE id = ?1",
                params![profile_id],
            )
            .map_err(to_string)?;
            return Ok(api_key);
        }
    }
    let conn = state.conn.lock().map_err(to_string)?;
    let db_api_key = conn
        .query_row(
            "SELECT api_key FROM model_profiles WHERE id = ?1",
            params![profile_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(to_string)?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未保存 API Key，请填写 API Key 后点击保存。".to_string())?;
    if write_api_key(profile_id, &db_api_key).is_ok() {
        conn.execute(
            "UPDATE model_profiles SET api_key = NULL WHERE id = ?1",
            params![profile_id],
        )
        .map_err(to_string)?;
    }
    Ok(db_api_key)
}

#[cfg(test)]
async fn abort_and_drain_tasks<T: 'static>(tasks: &mut tokio::task::JoinSet<T>) {
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}

fn api_key_storage(conn: &Connection, profile_id: &str) -> ApiKeyStorage {
    let db_api_key = conn
        .query_row(
            "SELECT api_key FROM model_profiles WHERE id = ?1",
            params![profile_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    api_key_storage_from_values(profile_id, db_api_key.as_deref())
}

fn api_key_storage_from_values(profile_id: &str, db_api_key: Option<&str>) -> ApiKeyStorage {
    let system_has_key = read_api_key(profile_id).is_ok_and(|value| !value.trim().is_empty());
    let database_has_key = db_api_key.is_some_and(|value| !value.trim().is_empty());
    classify_api_key_storage(system_has_key, database_has_key)
}

fn row_to_novel(row: &rusqlite::Row<'_>) -> rusqlite::Result<Novel> {
    Ok(Novel {
        id: row.get(0)?,
        title: row.get(1)?,
        source_path: row.get(2)?,
        encoding: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn row_to_chapter(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chapter> {
    Ok(Chapter {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        index: row.get(2)?,
        title: row.get(3)?,
        original_text: row.get(4)?,
        analysis_json: row.get(5)?,
        rewrite_text: row.get(6)?,
        rewrite_edited: row.get(7)?,
        single_rewrite_original_available: row.get(8)?,
        analysis_status: row.get(9)?,
        rewrite_status: row.get(10)?,
    })
}

fn row_to_chapter_batch(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChapterBatch> {
    Ok(ChapterBatch {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        batch_index: row.get(2)?,
        label: row.get(3)?,
        start_chapter: row.get(4)?,
        end_chapter: row.get(5)?,
        file_path: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_to_novel_settings(row: &rusqlite::Row<'_>) -> rusqlite::Result<NovelSettings> {
    Ok(NovelSettings {
        novel_id: row.get(0)?,
        protagonist_name: row.get(1)?,
        protagonist_aliases: row.get(2)?,
        rewritten_protagonist_name: row.get(3)?,
        additional_feminize_names: row.get(4)?,
        bust: row.get(5)?,
        body_type: row.get(6)?,
        rewrite_mode: row.get(7)?,
        advanced_settings: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        job_type: row.get(2)?,
        status: row.get(3)?,
        current_chapter: row.get(4)?,
        total_chapters: row.get(5)?,
        message: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_ai_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiLog> {
    Ok(AiLog {
        id: row.get(0)?,
        novel_id: row.get(1)?,
        profile_id: row.get(2)?,
        action: row.get(3)?,
        chapter_title: row.get(4)?,
        status: row.get(5)?,
        content: row.get(6)?,
        reasoning: row.get(7)?,
        raw_response: row.get(8)?,
        finish_reason: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn diagnosis_check(name: &str, status: &str, message: &str) -> ModelDiagnosisCheck {
    ModelDiagnosisCheck {
        name: name.to_string(),
        status: status.to_string(),
        message: message.to_string(),
    }
}

fn build_model_diagnosis(
    checks: Vec<ModelDiagnosisCheck>,
    recommended_thinking_mode: Option<&str>,
) -> ModelDiagnosis {
    let status = if checks.iter().any(|check| check.status == "failed") {
        "failed"
    } else if checks.iter().any(|check| check.status == "warning") {
        "warning"
    } else {
        "ok"
    };
    ModelDiagnosis {
        status: status.to_string(),
        recommended_thinking_mode: recommended_thinking_mode.map(str::to_string),
        checks,
    }
}

fn append_diagnosis_log(
    state: &State<'_, AppState>,
    profile_id: &str,
    diagnosis: &ModelDiagnosis,
) -> Result<(), String> {
    let content = diagnosis
        .checks
        .iter()
        .map(|check| format!("- {} [{}] {}", check.name, check.status, check.message))
        .collect::<Vec<_>>()
        .join("\n");
    append_ai_log(
        state,
        None,
        profile_id,
        "模型诊断",
        None,
        if diagnosis.status == "failed" {
            "error"
        } else {
            "success"
        },
        &format!("诊断状态：{}\n{}", diagnosis.status, content),
        None,
        None,
    )
}

fn compact_log_line(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        format!("{}...", take_chars(&compact, max_chars))
    }
}

fn format_model_log_content(
    output: &ModelOutput,
    profile: &ModelProfile,
    review_enabled: Option<bool>,
) -> String {
    let review_label = match review_enabled {
        Some(true) => "开启",
        Some(false) => "关闭",
        None => "不适用",
    };
    format!(
        "调用统计：\n- 输入字符数：{}\n- 输出字符数：{}\n- AI 调用耗时：{:.2} 秒\n- 复检：{}\n- 思考模式：{}\n\n{}",
        output.input_chars,
        output.output_chars,
        output.elapsed_ms as f64 / 1000.0,
        review_label,
        profile.thinking_mode,
        output.text.trim()
    )
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut value = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        value.push_str("\n\n[由于上下文限制，本章后续内容已截断。]");
    }
    value
}

fn truncate_text_tail(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }
    let tail = text
        .chars()
        .skip(char_count.saturating_sub(max_chars))
        .collect::<String>();
    format!("[由于上下文限制，本章前文已截断。]\n\n{}", tail)
}

/// Extract the trailing JSON object or array from a text such as reasoning / thinking content.
/// When a model puts all output into reasoning_content and leaves content empty, the actual
/// structured output (review decision, analysis result, etc.) often appears as the last JSON block
/// inside the reasoning text.
fn extract_tailing_json_from_text(text: &str) -> Option<&str> {
    // Find the last candidate '{' and '[' positions.
    let last_brace = text.rfind('{');
    let last_bracket = text.rfind('[');
    // Try brace first (most review/analysis outputs are objects), then bracket.
    let mut candidates = Vec::new();
    if let Some(pos) = last_brace {
        candidates.push(pos);
    }
    if let Some(pos) = last_bracket {
        candidates.push(pos);
    }
    for start in candidates {
        let candidate = &text[start..];
        if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn normalize_jsonish(text: &str) -> String {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

fn parse_jsonish_value(text: &str) -> Result<serde_json::Value, String> {
    if text.trim().is_empty() {
        return Err("AI 返回了空响应正文（模型可能将所有 token 消耗在思维链 reasoning 中，content 字段为空白，且无法从 reasoning 中提取到有效的 JSON）".to_string());
    }

    let normalized = normalize_jsonish(text);
    match serde_json::from_str::<serde_json::Value>(&normalized) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            let repaired = escape_unescaped_json_control_chars(&normalized);
            if repaired != normalized {
                serde_json::from_str::<serde_json::Value>(&repaired).map_err(|second_error| {
                    format!("{}；修复控制字符后仍失败：{}", first_error, second_error)
                })
            } else {
                Err(first_error.to_string())
            }
        }
    }
}

fn escape_unescaped_json_control_chars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            output.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                output.push(ch);
                escaped = true;
            }
            '"' => {
                output.push(ch);
                in_string = !in_string;
            }
            '\n' if in_string => output.push_str("\\n"),
            '\r' if in_string => output.push_str("\\r"),
            '\t' if in_string => output.push_str("\\t"),
            ch if in_string && ch.is_control() => {
                output.push_str(&format!("\\u{:04X}", ch as u32));
            }
            _ => output.push(ch),
        }
    }

    output
}

fn normalize_name_list(input: &str) -> String {
    let mut names = Vec::new();
    for value in input.split(['\n', '\r', ',', '，', '、', ';', '；']) {
        push_unique_name(&mut names, value.trim());
    }
    names.join("\n")
}

fn sanitize_file_name(input: &str) -> String {
    let cleaned = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>();
    if cleaned.trim().is_empty() {
        "novel".to_string()
    } else {
        cleaned
    }
}

fn to_string<E: std::fmt::Display>(error: E) -> String {
    redact_sensitive_text(&error.to_string())
}

fn redact_sensitive_text(text: &str) -> String {
    static QUERY_SECRET_RE: OnceLock<Regex> = OnceLock::new();
    static BEARER_RE: OnceLock<Regex> = OnceLock::new();
    let query_secret_re = QUERY_SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)([?&](?:key|api_key|access_token|token)=)[^&\s]+")
            .expect("valid secret query regex")
    });
    let bearer_re = BEARER_RE.get_or_init(|| {
        Regex::new(r"(?i)(authorization:\s*bearer\s+)[^\s,;]+").expect("valid bearer regex")
    });
    let redacted = query_secret_re.replace_all(text, "${1}[REDACTED]");
    bearer_re
        .replace_all(&redacted, "${1}[REDACTED]")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

    fn sample_chapter(index: i64, title: &str, original_text: &str) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: "novel-1".to_string(),
            index,
            title: title.to_string(),
            original_text: original_text.to_string(),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "completed".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    fn sample_novel_settings() -> NovelSettings {
        NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        }
    }

    fn sample_review_issue(
        chapter_indexes: Vec<i64>,
        scope: &str,
        category: &str,
        problem: &str,
    ) -> ReviewIssue {
        ReviewIssue {
            chapter_indexes,
            scope: scope.to_string(),
            category: category.to_string(),
            severity: "blocking".to_string(),
            problem: problem.to_string(),
            required_fix: "按要求修复。".to_string(),
        }
    }

    #[test]
    fn network_disconnect_errors_are_recoverable_but_timeouts_are_not() {
        assert!(is_recoverable_network_error(
            "error sending request for url (https://example.invalid): connection closed before message completed"
        ));
        assert!(is_recoverable_network_error(
            "无法连接模型服务：error trying to connect: tcp connect error"
        ));
        assert!(is_recoverable_network_error(
            "远程主机强迫关闭了一个现有的连接。"
        ));
        assert!(!is_recoverable_network_error(
            "模型请求超时（最长等待 20 分钟），请检查网络或降低单次处理量。"
        ));
        assert!(!is_recoverable_network_error(
            "HTTP 500: {\"error\":\"server overloaded\"}"
        ));
    }

    #[tokio::test]
    async fn aborting_parallel_tasks_waits_until_tasks_are_drained() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let mut tasks = tokio::task::JoinSet::new();
        let task_flag = dropped.clone();
        tasks.spawn(async move {
            let _flag = DropFlag(task_flag);
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        tokio::task::yield_now().await;

        abort_and_drain_tasks(&mut tasks).await;

        assert!(tasks.is_empty());
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn dropping_pipeline_futures_cancels_in_flight_shards() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let task_flag = dropped.clone();
        let mut tasks = FuturesUnordered::new();
        tasks.push(
            async move {
                let _flag = DropFlag(task_flag);
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            .boxed(),
        );
        tokio::select! {
            _ = tasks.next() => panic!("pipeline task unexpectedly completed"),
            _ = tokio::time::sleep(Duration::from_millis(10)) => {}
        }

        drop(tasks);

        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn completed_shard_enters_review_before_slowest_draft_finishes() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = FuturesUnordered::new();
        let fast_events = events.clone();
        tasks.push(
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                fast_events
                    .lock()
                    .expect("lock fast events")
                    .push("fast-review-start");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            .boxed(),
        );
        let slow_events = events.clone();
        tasks.push(
            async move {
                tokio::time::sleep(Duration::from_millis(80)).await;
                slow_events
                    .lock()
                    .expect("lock slow events")
                    .push("slow-draft-finished");
            }
            .boxed(),
        );

        while tasks.next().await.is_some() {}

        assert_eq!(
            *events.lock().expect("lock final events"),
            vec!["fast-review-start", "slow-draft-finished"]
        );
    }

    #[test]
    fn batch_rewrite_markers_round_trip() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            "{}\n标题：第一章\n正文：\n改写一\n{}\n\n{}\n标题：第二章\n正文：\n改写二\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0]),
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert_eq!(parsed[0].index, 1);
        assert_eq!(parsed[0].title, "第一章");
        assert_eq!(parsed[0].text, "改写一");
        assert_eq!(parsed[1].id, "chapter-2");
        assert_eq!(parsed[1].index, 2);
        assert_eq!(parsed[1].title, "第二章");
        assert_eq!(parsed[1].text, "改写二");
    }

    #[test]
    fn batch_rewrite_parser_extracts_rewritten_title() {
        let chapters = vec![sample_chapter(1, "第一章 男儿志", "原文一")];
        let output = format!(
            "{}\n标题：第一章 少女志\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "第一章 少女志");
        assert_eq!(parsed[0].text, "改写一");
    }

    #[test]
    fn batch_rewrite_parser_removes_dangling_format_labels() {
        let chapter = sample_chapter(1, "第一章", "原文一");
        let output = format!(
            "{}\n标题：第一章\n正文：\n改写正文\n\n标题：\n{}",
            chapter_start_marker(&chapter),
            chapter_end_marker(&chapter)
        );

        let parsed = parse_batch_rewrite_output(&output, std::slice::from_ref(&chapter))
            .expect("dangling title label should be removed");

        assert_eq!(parsed[0].text, "改写正文");
    }

    #[test]
    fn batch_rewrite_parser_rejects_empty_placeholder_pollution() {
        let chapter = sample_chapter(2, "第二更！", "");
        let output = format!(
            "{}\n第二更！\n正文：\n\n标题：\n{}",
            chapter_start_marker(&chapter),
            chapter_end_marker(&chapter)
        );

        let error = parse_batch_rewrite_output(&output, std::slice::from_ref(&chapter))
            .expect_err("placeholder-only output must stay invalid");

        assert!(error.contains("改写正文为空"));
    }

    #[test]
    fn batch_rewrite_parser_accepts_marker_with_wrong_id_when_index_matches() {
        let chapters = vec![sample_chapter(4, "第四章", "原文四")];
        let output = "<<<YURI_REWRITE_CHAPTER_START index=4 id=model-made-up-id>>>\n标题：第四章\n正文：\n改写四\n<<<YURI_REWRITE_CHAPTER_END index=4 id=model-made-up-id>>>";

        let parsed = parse_batch_rewrite_output(output, &chapters)
            .expect("index-matched marker should recover from wrong id");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].index, 4);
        assert_eq!(parsed[0].text, "改写四");
    }

    #[test]
    fn batch_rewrite_parser_recovers_markerless_title_body_output() {
        let chapters = vec![
            sample_chapter(4, "第四章", "原文四"),
            sample_chapter(5, "第五章", "原文五"),
            sample_chapter(6, "第六章", "原文六"),
        ];
        let output = "标题：第四章\n正文：\n改写四\n\n标题：第五章\n正文：\n改写五\n\n标题：第六章\n正文：\n改写六";

        let parsed = parse_batch_rewrite_output(output, &chapters)
            .expect("title/body output should be used as fallback");

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].index, 4);
        assert_eq!(parsed[1].text, "改写五");
        assert_eq!(parsed[2].index, 6);
    }

    #[test]
    fn batch_rewrite_parser_ignores_non_marker_intro_before_first_marker() {
        let chapters = vec![sample_chapter(4, "第四章", "原文四")];
        let output = format!(
            "好的，以下是当前分片。\n\n{}\n标题：第四章\n正文：\n改写四\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );

        let parsed = parse_batch_rewrite_output(&output, &chapters)
            .expect("non-marker intro should not break marker parsing");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "改写四");
    }

    #[test]
    fn batch_rewrite_parser_rejects_missing_or_out_of_order_markers() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let missing_second = format!(
            "{}\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_rewrite_output(&missing_second, &chapters).is_err());

        let out_of_order = format!(
            "{}\n正文：\n改写二\n{}\n\n{}\n正文：\n改写一\n{}",
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1]),
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_rewrite_output(&out_of_order, &chapters).is_err());
    }

    #[test]
    fn batch_rewrite_parser_accepts_missing_end_marker_when_boundary_is_clear() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let missing_first_end = format!(
            "{}\n正文：\n改写一\n\n{}\n正文：\n改写二\n{}",
            chapter_start_marker(&chapters[0]),
            chapter_start_marker(&chapters[1]),
            chapter_end_marker(&chapters[1])
        );
        let parsed = parse_batch_rewrite_output(&missing_first_end, &chapters)
            .expect("next start marker is enough to recover missing end marker");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].text, "改写一");
        assert_eq!(parsed[1].text, "改写二");

        let missing_last_end = format!(
            "{}\n正文：\n改写一\n{}\n\n{}\n正文：\n改写二",
            chapter_start_marker(&chapters[0]),
            chapter_end_marker(&chapters[0]),
            chapter_start_marker(&chapters[1])
        );
        let parsed = parse_batch_rewrite_output(&missing_last_end, &chapters)
            .expect("final non-empty block is enough to recover missing final end marker");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].text, "改写二");
    }

    #[test]
    fn batch_rewrite_parser_ignores_extra_unexpected_chapters_after_expected_shard() {
        let expected = vec![
            sample_chapter(25, "第二十五章", "原文二十五"),
            sample_chapter(26, "第二十六章", "原文二十六"),
            sample_chapter(27, "第二十七章", "原文二十七"),
        ];
        let extra = [
            sample_chapter(28, "第二十八章", "原文二十八"),
            sample_chapter(29, "第二十九章", "原文二十九"),
            sample_chapter(30, "第三十章", "原文三十"),
        ];
        let mut output = String::new();
        for chapter in expected.iter().chain(extra.iter()) {
            output.push_str(&format!(
                "{}\n标题：{}\n正文：\n改写{}\n{}\n\n",
                chapter_start_marker(chapter),
                chapter.title,
                chapter.index,
                chapter_end_marker(chapter)
            ));
        }

        let parsed = parse_batch_rewrite_output(&output, &expected)
            .expect("extra unexpected chapter markers should be ignored after expected shard");

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].index, 25);
        assert_eq!(parsed[2].index, 27);
    }

    #[test]
    fn batch_analysis_markers_round_trip() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            "{}\n{{\"outline\":\"大纲一\",\"characters\":[\"萧炎\"],\"relationships\":[],\"locations\":[],\"foreshadowing\":[],\"terms\":[],\"names\":[\"萧炎\"]}}\n{}\n\n{}\n{{\"outline\":\"大纲二\",\"characters\":[\"药老\"],\"relationships\":[],\"locations\":[],\"foreshadowing\":[],\"terms\":[],\"names\":[\"药老\"]}}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            analysis_chapter_end_marker(&chapters[0]),
            analysis_chapter_start_marker(&chapters[1]),
            analysis_chapter_end_marker(&chapters[1])
        );

        let parsed = parse_batch_analysis_output(&output, &chapters).expect("valid batch output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("大纲一"));
        assert_eq!(parsed[1].id, "chapter-2");
        assert!(parsed[1].json.contains("大纲二"));
    }

    #[test]
    fn batch_analysis_json_output_round_trip_without_markers() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = format!(
            r#"{{
  "chapters": [
    {{
      "index": 1,
      "id": "{}",
      "title": "第一章",
      "analysis": {{
        "outline": "大纲一",
        "characters": ["萧炎"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["萧炎"]
      }}
    }},
    {{
      "index": 2,
      "id": "{}",
      "title": "第二章",
      "analysis": {{
        "outline": "大纲二",
        "characters": ["药老"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["药老"]
      }}
    }}
  ]
}}"#,
            chapters[0].id, chapters[1].id
        );

        let parsed = parse_batch_analysis_output(&output, &chapters).expect("valid json output");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("大纲一"));
        assert_eq!(parsed[1].id, "chapter-2");
        assert!(parsed[1].json.contains("大纲二"));
    }

    #[test]
    fn batch_analysis_json_output_accepts_batch_level_assets() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let output = r#"{
  "batch": {"start_index": 1, "end_index": 2, "chapter_count": 2},
  "outline": ["萧炎进入大厅并遇见药老。"],
  "characters": ["萧炎：少年。", "药老：神秘人物。"],
  "relationships": ["萧炎与药老建立联系。"],
  "locations": ["大厅"],
  "foreshadowing": ["药老身份仍有悬念。"],
  "terms": ["斗气"],
  "names": ["萧炎", "药老"]
}"#;

        let parsed =
            parse_batch_analysis_output(output, &chapters).expect("valid batch-level output");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "chapter-1");
        assert!(parsed[0].json.contains("萧炎进入大厅"));
        assert!(parsed[0].json.contains("斗气"));
    }

    #[test]
    fn batch_analysis_json_output_repairs_control_chars_inside_strings() {
        let chapters = vec![sample_chapter(1, "第一章", "原文一")];
        let output = format!(
            r#"{{
  "chapters": [
    {{
      "index": 1,
      "id": "{}",
      "title": "第一章",
      "analysis": {{
        "outline": "第一行
第二行",
        "characters": ["萧炎	少年"],
        "relationships": [],
        "locations": [],
        "foreshadowing": [],
        "terms": [],
        "names": ["萧炎"]
      }}
    }}
  ]
}}"#,
            chapters[0].id
        );

        let parsed = parse_batch_analysis_output(&output, &chapters)
            .expect("control characters inside JSON strings should be repaired");

        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].json.contains("\\n"));
        assert!(parsed[0].json.contains("\\t"));
    }

    #[test]
    fn batch_analysis_parser_rejects_missing_out_of_order_or_invalid_json() {
        let chapters = vec![
            sample_chapter(1, "第一章", "原文一"),
            sample_chapter(2, "第二章", "原文二"),
        ];
        let valid_first = "{\"outline\":\"大纲一\"}";
        let valid_second = "{\"outline\":\"大纲二\"}";
        let missing_second = format!(
            "{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            valid_first,
            analysis_chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_analysis_output(&missing_second, &chapters).is_err());

        let out_of_order = format!(
            "{}\n{}\n{}\n\n{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[1]),
            valid_second,
            analysis_chapter_end_marker(&chapters[1]),
            analysis_chapter_start_marker(&chapters[0]),
            valid_first,
            analysis_chapter_end_marker(&chapters[0])
        );
        assert!(parse_batch_analysis_output(&out_of_order, &chapters).is_err());

        let invalid_json = format!(
            "{}\nnot-json\n{}\n\n{}\n{}\n{}",
            analysis_chapter_start_marker(&chapters[0]),
            analysis_chapter_end_marker(&chapters[0]),
            analysis_chapter_start_marker(&chapters[1]),
            valid_second,
            analysis_chapter_end_marker(&chapters[1])
        );
        assert!(parse_batch_analysis_output(&invalid_json, &chapters).is_err());
    }

    #[test]
    fn export_body_contains_only_completed_rewrites() {
        let mut completed = sample_chapter(1, "第一章", "不应导出的原文一");
        completed.rewrite_status = "completed".to_string();
        completed.rewrite_text = Some("已改写正文一".to_string());

        let mut pending = sample_chapter(2, "第二章", "不应导出的原文二");
        pending.rewrite_text = Some("未完成改写也不导出".to_string());

        let body =
            build_rewritten_export_body(&[completed, pending]).expect("has completed rewrite");

        assert!(body.contains("第一章"));
        assert!(body.contains("已改写正文一"));
        assert!(!body.contains("第二章"));
        assert!(!body.contains("不应导出的原文"));
        assert!(!body.contains("未完成改写也不导出"));
    }

    #[test]
    fn chinese_batch_label_formats_common_batch_indices() {
        assert_eq!(chinese_batch_label(1), "第一批");
        assert_eq!(chinese_batch_label(2), "第二批");
        assert_eq!(chinese_batch_label(10), "第十批");
        assert_eq!(chinese_batch_label(12), "第十二批");
        assert_eq!(chinese_batch_label(30), "第三十批");
    }

    #[test]
    fn analysis_prompt_does_not_include_rewrite_instructions() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let prompt = build_batch_analysis_prompt(&[chapter]);

        for forbidden in ["百合", "改写", "女性化", "代词替换", "双女主"] {
            assert!(
                !prompt.contains(forbidden),
                "prompt contains forbidden term: {forbidden}"
            );
        }
    }

    #[test]
    fn analysis_identity_context_links_protagonist_aliases_without_rewrite_rules() {
        let mut settings = sample_novel_settings();
        settings.protagonist_aliases = "炎儿\n岩枭".to_string();
        let identity = build_analysis_identity_context(&settings);
        let chapter = sample_chapter(1, "第一章", "炎儿以岩枭之名进入大厅。");
        let prompt = build_batch_analysis_prompt_with_identity(&[chapter], "", &identity);

        assert!(prompt.contains("炎儿"));
        assert!(prompt.contains("岩枭"));
        assert!(prompt.contains("归属于同一人物"));
        for forbidden in ["百合", "女性化", "双女主", "改写后姓名"] {
            assert!(
                !prompt.contains(forbidden),
                "prompt contains forbidden term: {forbidden}"
            );
        }
    }

    #[test]
    fn protagonist_aliases_join_name_mapping_and_rewrite_context() {
        let mut settings = sample_novel_settings();
        settings.protagonist_aliases = "炎儿\n岩枭".to_string();
        let required = required_feminized_name_sources(&settings);
        assert_eq!(required, vec!["萧炎", "炎儿", "岩枭"]);

        let prompt = build_rewrite_settings_prompt(&settings);
        assert!(prompt.contains("主角原文别名：炎儿、岩枭"));
        assert!(prompt.contains("每个别名都必须按一致性资产中的固定映射同步女性化"));
    }

    #[test]
    fn draft_based_single_rewrite_prompt_keeps_draft_primary_and_original_reference_only() {
        let settings = sample_novel_settings();
        let mut chapter = sample_chapter(2, "第二章", "原文事件顺序。");
        chapter.rewrite_text = Some("当前改写稿正文。".to_string());
        let prompt = build_single_chapter_rewrite_from_draft_prompt(
            &chapter,
            "## 人物卡\n萧炎 -> 萧妍",
            &settings,
            "保持简洁文风",
            "前一章已完成改写摘要。",
            "加强情绪互动",
        );

        assert!(prompt.contains("当前改写稿是本次修改的主要底稿"));
        assert!(prompt.contains("不能抛弃现稿、退回原文重新生成"));
        assert!(prompt.contains("当前改写稿正文"));
        assert!(prompt.contains("原文仅用于核对事实"));
        assert!(prompt.contains("前一章已完成改写摘要"));
        assert!(prompt.contains("加强情绪互动"));
    }

    #[test]
    fn app_review_setting_defaults_off_and_can_be_enabled() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        assert!(!load_review_enabled(&conn).expect("load default review setting"));
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load default parallelism"),
            10
        );
        assert_eq!(
            load_chapter_batch_size(&conn).expect("load default batch size"),
            30
        );

        save_review_enabled(&conn, true).expect("enable review");
        assert!(load_review_enabled(&conn).expect("load enabled review setting"));

        save_review_enabled(&conn, false).expect("disable review");
        assert!(!load_review_enabled(&conn).expect("load disabled review setting"));

        save_rewrite_parallelism(&conn, 10).expect("save parallelism");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load parallelism"),
            10
        );
        save_rewrite_parallelism(&conn, 2).expect("normalize invalid parallelism");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load normalized parallelism"),
            10
        );
        save_chapter_batch_size(&conn, 100).expect("save batch size");
        save_rewrite_parallelism(&conn, 50).expect("save high parallelism");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("load high parallelism"),
            50
        );
        save_chapter_batch_size(&conn, 30).expect("lower batch size");
        assert_eq!(
            load_rewrite_parallelism(&conn).expect("clamp high parallelism"),
            10
        );
    }

    #[test]
    fn startup_restores_incomplete_auto_run_as_paused() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO jobs (id, novel_id, job_type, status, current_chapter, total_chapters, message, created_at, updated_at) VALUES ('job-1', 'novel-1', 'auto', 'running', 2, 10, '运行中', 'now', 'now')",
            [],
        )
        .expect("insert job");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (novel_id, start_batch_index, next_batch_index, job_id, status, pause_reason, phase, batch_index, profile_ids, created_at, updated_at) VALUES ('novel-1', 0, 2, 'job-1', 'running', '', 'rewrite', 3, '[\"profile-1\"]', 'now', 'now')",
            [],
        )
        .expect("insert checkpoint");

        let restored = restore_auto_run_controls(&conn).expect("restore controls");
        let control = restored.get("novel-1").expect("restored novel");
        assert_eq!(control.status, "paused");
        assert_eq!(control.completed_batches, 2);
        assert!(control.profile_ids.contains("profile-1"));
        let status: String = conn
            .query_row("SELECT status FROM jobs WHERE id = 'job-1'", [], |row| {
                row.get(0)
            })
            .expect("load job");
        assert_eq!(status, "paused");
    }

    #[test]
    fn startup_restores_only_orphaned_running_rewrite_statuses() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");
        for novel_id in ["orphan", "paused"] {
            conn.execute(
                "INSERT INTO novels (
                    id, title, source_path, encoding, status, created_at
                 ) VALUES (?1, '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
                params![novel_id],
            )
            .expect("insert novel");
        }
        conn.execute(
            "INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text, rewrite_text,
                analysis_status, rewrite_status
             ) VALUES
                ('with-draft', 'orphan', 1, '第一章', '原文', '已有改写稿', 'completed', 'running'),
                ('without-draft', 'orphan', 2, '第二章', '原文', NULL, 'completed', 'running'),
                ('paused-draft', 'paused', 1, '第一章', '原文', '旧改写稿', 'completed', 'running')",
            [],
        )
        .expect("insert chapters");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (
                novel_id, start_batch_index, next_batch_index, status, pause_reason,
                phase, batch_index, profile_ids, created_at, updated_at
             ) VALUES (
                'paused', 0, 0, 'paused', '测试暂停', 'rewrite', 0, '[]', 'now', 'now'
             )",
            [],
        )
        .expect("insert paused checkpoint");

        restore_orphaned_rewrite_statuses(&conn).expect("restore statuses");

        let status = |chapter_id: &str| {
            conn.query_row(
                "SELECT rewrite_status FROM chapters WHERE id = ?1",
                params![chapter_id],
                |row| row.get::<_, String>(0),
            )
            .expect("load status")
        };
        assert_eq!(status("with-draft"), "completed");
        assert_eq!(status("without-draft"), "failed");
        assert_eq!(status("paused-draft"), "running");
    }

    #[test]
    fn single_chapter_entry_restores_orphaned_running_draft() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");
        conn.execute(
            "INSERT INTO novels (
                id, title, source_path, encoding, status, created_at
             ) VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text, rewrite_text,
                analysis_status, rewrite_status
             ) VALUES (
                'chapter-1', 'novel-1', 1, '第一章', '原文', '已有改写稿',
                'completed', 'running'
             )",
            [],
        )
        .expect("insert chapter");

        assert!(restore_orphaned_rewrite_status_for_chapter(&conn, "chapter-1")
            .expect("restore chapter"));
        let status: String = conn
            .query_row(
                "SELECT rewrite_status FROM chapters WHERE id = 'chapter-1'",
                [],
                |row| row.get(0),
            )
            .expect("load status");
        assert_eq!(status, "completed");
    }

    #[test]
    fn core_prompt_can_be_saved_loaded_and_cleared() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_db(&conn).expect("init db");

        assert_eq!(
            load_core_prompt(&conn).expect("load default core prompt"),
            ""
        );

        save_core_prompt(&conn, "  文风克制，动作描写细腻。  ").expect("save core prompt");
        assert_eq!(
            load_core_prompt(&conn).expect("load saved core prompt"),
            "文风克制，动作描写细腻。"
        );

        save_core_prompt(&conn, "   ").expect("clear core prompt");
        assert_eq!(
            load_core_prompt(&conn).expect("load cleared core prompt"),
            ""
        );
    }

    #[test]
    fn review_warning_file_appends_per_novel() {
        let temp_dir = env::temp_dir().join(format!("yuri-rewrite-warning-{}", Uuid::new_v4()));
        let app_dir = temp_dir.join("app");
        let data_dir = temp_dir.join("data");
        let third_decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![1],
                "chapter",
                "core_prompt",
                "第三次仍未满足核心设定",
            )],
        };

        let first_path = append_review_warning_file_for_title(
            &app_dir,
            &data_dir,
            "测试:小说",
            "第1-3章",
            &third_decision,
        );
        let second_path = append_review_warning_file_for_title(
            &app_dir,
            &data_dir,
            "测试:小说",
            "第4-6章",
            &third_decision,
        );

        assert_eq!(first_path, second_path);
        let content = fs::read_to_string(&first_path).expect("read warning log");
        assert!(content.contains("测试:小说"));
        assert!(content.contains("第1-3章"));
        assert!(content.contains("第4-6章"));
        assert!(content.contains("已保存第二次重写稿并继续处理后续分片"));
        assert!(content.contains("第三次审查问题"));
        assert!(content.contains("第三次仍未满足核心设定"));
        assert!(!content.contains("第二次审查问题"));
        assert!(!content.contains("第二次仍缺少外貌描写"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn model_diagnosis_status_uses_worst_check() {
        let ok = build_model_diagnosis(vec![diagnosis_check("连接", "ok", "ok")], None);
        assert_eq!(ok.status, "ok");

        let warning = build_model_diagnosis(
            vec![
                diagnosis_check("连接", "ok", "ok"),
                diagnosis_check("JSON", "warning", "unstable"),
            ],
            Some("auto"),
        );
        assert_eq!(warning.status, "warning");
        assert_eq!(warning.recommended_thinking_mode.as_deref(), Some("auto"));

        let failed = build_model_diagnosis(
            vec![
                diagnosis_check("连接", "warning", "slow"),
                diagnosis_check("API Key", "failed", "bad key"),
            ],
            None,
        );
        assert_eq!(failed.status, "failed");
    }

    #[test]
    fn rewrite_recovery_split_halves_large_failed_shards() {
        let chapters = (1..=10)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();

        let (left, right) = split_chapters_for_rewrite_recovery(&chapters).expect("split ten");
        assert_eq!(left.len(), 5);
        assert_eq!(right.len(), 5);
        assert_eq!(left[0].index, 1);
        assert_eq!(right[0].index, 6);

        let (left, right) =
            split_chapters_for_rewrite_recovery(&chapters[..3]).expect("split three");
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 1);
        assert!(split_chapters_for_rewrite_recovery(&chapters[..1]).is_none());
    }

    #[test]
    fn rewrite_parser_rejects_provider_length_truncation_before_marker_parsing() {
        let chapter = sample_chapter(1, "第一章", "原文");
        let output = ModelOutput {
            text: format!(
                "{}\n标题：第一章\n正文：\n完整正文\n{}",
                chapter_start_marker(&chapter),
                chapter_end_marker(&chapter)
            ),
            reasoning: None,
            raw_response: json!({
                "choices": [{"finish_reason": "length"}]
            })
            .to_string(),
            input_chars: 10,
            output_chars: 10,
            elapsed_ms: 10,
            retried_without_thinking: false,
        };

        let error = parse_rewrite_model_output(&output, &[chapter])
            .expect_err("truncated output must not be saved");
        assert!(error.contains("达到长度上限被截断"));
    }

    #[test]
    fn review_revision_recovery_keeps_chapters_and_previous_rewrites_aligned() {
        let chapters = (1..=5)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();
        let rewrites = chapters
            .iter()
            .map(|chapter| ParsedChapterRewrite {
                id: chapter.id.clone(),
                index: chapter.index,
                title: chapter.title.clone(),
                text: format!("改写正文 {}", chapter.index),
            })
            .collect::<Vec<_>>();

        let split = split_revision_for_recovery(&chapters, &rewrites).expect("split revisions");
        let (left_chapters, left_rewrites) = split.left;
        let (right_chapters, right_rewrites) = split.right;

        assert_eq!(left_chapters.len(), 3);
        assert_eq!(right_chapters.len(), 2);
        for (chapter, rewrite) in left_chapters
            .iter()
            .zip(left_rewrites.iter())
            .chain(right_chapters.iter().zip(right_rewrites.iter()))
        {
            assert_eq!(chapter.id, rewrite.id);
            assert_eq!(chapter.index, rewrite.index);
        }
    }

    #[test]
    fn canon_assets_are_compacted_before_rewrite_prompt() {
        let huge_content = (0..1_200)
            .map(|index| format!("人物设定行{index}：很长的一致性资产内容。"))
            .collect::<Vec<_>>()
            .join("\n");
        let assets = vec![CanonAsset {
            novel_id: "novel-1".to_string(),
            kind: "AI分析汇总".to_string(),
            content: huge_content.clone(),
            updated_at: "now".to_string(),
        }];

        let compact = build_compact_canon_text(&assets);

        assert!(compact.contains("AI分析汇总"));
        assert!(compact.contains("一致性资产已压缩"));
        assert!(compact.chars().count() < huge_content.chars().count());
        assert!(compact.chars().count() < 4_500);
    }

    #[test]
    fn relevant_canon_selection_keeps_mapping_and_excludes_unrelated_sections() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "小医仙".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let chapter = sample_chapter(1, "第一章", "萧炎与小医仙进入青山镇。");
        let assets = vec![
            CanonAsset {
                novel_id: "novel-1".to_string(),
                kind: "姓名映射表".to_string(),
                content: "萧炎 -> 萧妍\n小医仙 -> 小医仙".to_string(),
                updated_at: "now".to_string(),
            },
            CanonAsset {
                novel_id: "novel-1".to_string(),
                kind: "人物关系".to_string(),
                content:
                    "## 萧炎与小医仙\n二人在青山镇同行。\n\n## 无关路人\n这个角色只在很后面出现。"
                        .to_string(),
                updated_at: "now".to_string(),
            },
        ];

        let canon = build_relevant_canon_text(&assets, &[chapter], &settings);

        assert!(canon.contains("萧炎 -> 萧妍"));
        assert!(canon.contains("二人在青山镇同行"));
        assert!(!canon.contains("无关路人"));
    }

    #[test]
    fn relevant_canon_output_uses_stable_asset_order() {
        let settings = sample_novel_settings();
        let chapter = sample_chapter(1, "第一章", "萧炎进入乌坦城。");
        let assets = vec![
            CanonAsset {
                novel_id: "novel-1".to_string(),
                kind: "术语表".to_string(),
                content: "斗气：修炼体系".to_string(),
                updated_at: "now".to_string(),
            },
            CanonAsset {
                novel_id: "novel-1".to_string(),
                kind: "姓名映射表".to_string(),
                content: "萧炎 -> 萧妍".to_string(),
                updated_at: "now".to_string(),
            },
            CanonAsset {
                novel_id: "novel-1".to_string(),
                kind: "人物关系".to_string(),
                content: "## 萧炎\n萧炎与萧薰儿关系密切。".to_string(),
                updated_at: "now".to_string(),
            },
        ];

        let canon = build_relevant_canon_text(&assets, &[chapter], &settings);

        let mapping_pos = canon.find("## 姓名映射表").expect("mapping first");
        let relation_pos = canon.find("## 人物关系").expect("relationship later");
        assert!(mapping_pos < relation_pos);
    }

    #[test]
    fn batch_prompts_keep_dynamic_scope_after_stable_rules() {
        let settings = sample_novel_settings();
        let chapters = vec![sample_chapter(1, "第一章", "萧炎走进大厅。")];
        let scope = format_shard_context(3, 10, 10, "第1-30章", &chapters);
        let prompt = build_batch_rewrite_prompt_with_context(
            &chapters,
            "## 姓名映射表\n萧炎 -> 萧妍",
            &settings,
            "核心规则",
            &scope,
        );

        assert!(prompt.contains("处理范围约束："));
        assert!(!prompt.contains("并发分片上下文"));
        assert!(!prompt.contains("当前设置的并发请求数"));
        assert!(!prompt.contains("分片 4/10"));
        assert!(
            prompt.find("改写要求").expect("stable rewrite rules")
                < prompt.find("处理范围约束：").expect("dynamic scope")
        );
        assert!(
            prompt.find("一致性资产：").expect("canon section")
                < prompt.find("处理范围约束：").expect("scope section")
        );
        assert!(
            prompt.find("处理范围约束：").expect("scope section")
                < prompt.find("当前输入章节：").expect("chapter input")
        );
    }

    #[test]
    fn compact_analysis_asset_deduplicates_and_limits_length_at_line_boundary() {
        let content = (0..300)
            .map(|index| {
                if index % 2 == 0 {
                    "- 萧炎与小医仙同行。".to_string()
                } else {
                    format!("- 低相关条目{index}：很长的历史分析内容。")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let compact = compact_analysis_asset("地点", &content);

        assert_eq!(compact.matches("萧炎与小医仙同行").count(), 1);
        assert!(compact.contains("已去重并达到长度上限"));
        assert!(compact.chars().count() <= analysis_asset_char_limit("地点"));
    }

    #[test]
    fn rewrite_settings_prompt_includes_selected_rewrite_mode() {
        let strict_settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let mut creative_settings = strict_settings.clone();
        creative_settings.rewrite_mode = "creative".to_string();

        let strict_prompt = build_rewrite_settings_prompt(&strict_settings);
        let creative_prompt = build_rewrite_settings_prompt(&creative_settings);

        assert!(strict_prompt.contains("严谨模式"));
        assert!(strict_prompt.contains("更加忠于原文"));
        assert!(strict_prompt.contains("不对主角添加过多额外女性化描写"));
        assert!(strict_prompt.contains("章节标题原则上保留原标题和原编号"));
        assert!(strict_prompt.contains("只有标题明确出现主角原名"));
        assert!(strict_prompt.contains("看不出主角改写前曾是男性"));
        assert!(strict_prompt.contains("男性化姓名、代词、称谓、身份、身体特征"));
        assert!(strict_prompt.contains("人物外貌特征必须前后一致"));
        assert!(strict_prompt.contains("上一章是金发，下一章不能无理由变成红发"));
        assert!(strict_prompt.contains("人物关系和百合向情绪推进必须连续"));
        assert!(strict_prompt.contains("只允许主角、用户填写的“其他需要女性化的人物姓名”"));
        assert!(strict_prompt.contains("其他未指定人物必须保持原文性别、身份、称谓和人称代词"));
        assert!(strict_prompt.contains("动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物"));
        assert!(strict_prompt.contains("保留原文中的人称代词和称谓"));
        assert!(strict_prompt.contains("群体中包含任何未指定性转的男性成员"));
        assert!(strict_prompt.contains("只有能够确认群体成员全部为女性时才使用“她们”"));
        assert!(strict_prompt.contains("性别构成不明时保留原文“他们”"));
        assert!(strict_prompt.contains("不得因为百合改写目标而把所有重要配角"));
        assert!(strict_prompt.contains("不要因为 NPC 名字与主角原名共享某个字"));
        assert!(strict_prompt.contains("涉及主角姓名来源、同名关系、名字含义、旧名对比"));
        assert!(creative_prompt.contains("创意模式"));
        assert!(creative_prompt.contains("优先级高于普通的“中度再创作”约束"));
        assert!(creative_prompt.contains("每章都能明确感知主角已经从男性变为女性"));
        assert!(creative_prompt.contains("每章至少在关键场景中增加或强化 2-4 处女性化感知点"));
        assert!(creative_prompt.contains("同性亲密感和百合向情绪推进"));
    }

    #[test]
    fn batch_rewrite_prompt_puts_core_prompt_before_rewrite_rules() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let chapters = vec![sample_chapter(1, "第一章", "萧炎走进大厅。")];
        let prompt = build_batch_rewrite_prompt_with_context(
            &chapters,
            "一致性资产",
            &settings,
            "文风克制，动作描写细腻，保留轻小说对白节奏。",
            "",
        );

        let core_pos = prompt
            .find("最高优先级核心设定")
            .expect("core prompt section");
        let rewrite_pos = prompt.find("改写要求").expect("rewrite rules");
        assert!(core_pos < rewrite_pos);
        assert!(prompt.contains("文风克制，动作描写细腻"));
        assert!(prompt.contains("优先级高于本次改写中的其他风格"));
        assert!(prompt.contains("【输出格式硬性要求】"));
        assert!(prompt.contains("每章必须完整复制输入中的 START marker 和 END marker"));
        assert!(prompt.contains("再次确认：只输出当前输入章节的结果"));
        assert!(
            prompt.find("【输出格式硬性要求】").expect("format guard")
                < prompt.find("改写要求").expect("rewrite rules")
        );
        assert!(
            prompt
                .rfind("再次确认：只输出当前输入章节的结果")
                .expect("final marker reminder")
                > prompt.find("当前输入章节").expect("input chapters")
        );
    }

    #[test]
    fn rewrite_settings_prompt_includes_forced_rewritten_protagonist_name() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };

        let prompt = build_rewrite_settings_prompt(&settings);

        assert!(prompt.contains("主角改写后姓名：萧妍"));
        assert!(prompt.contains("强制姓名规则"));
        assert!(prompt.contains("主角姓名必须统一为“萧妍”"));
        assert!(prompt.contains("不得自行改成其他姓名"));
    }

    #[test]
    fn name_mapping_asset_persists_forced_and_generated_names() {
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "林动\n唐三".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let content = build_name_mapping_asset_content(
            &settings,
            vec![
                NameMappingEntry {
                    source: "林动".to_string(),
                    target: "林彤".to_string(),
                },
                NameMappingEntry {
                    source: "唐三".to_string(),
                    target: fallback_feminized_name("唐三"),
                },
            ],
        )
        .expect("valid mapping content");
        let entries = parse_name_mapping_entries(&content);
        let prompt = build_rewrite_settings_prompt(&settings);

        assert!(content.contains("\"protagonist\""));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "萧炎" && entry.target == "萧妍"));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "林动" && entry.target == "林彤"));
        assert!(entries
            .iter()
            .any(|entry| entry.source == "唐三" && entry.target == "唐姗"));
        assert!(prompt.contains("姓名映射表"));
        assert!(prompt.contains("并发分片和后续批次也必须继续使用同一份映射表"));
    }

    #[test]
    fn batch_rewrite_prompt_requires_yuri_and_appearance_consistency() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "萧妍".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "strict".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };

        let prompt = build_batch_rewrite_prompt_with_settings(
            &[chapter],
            "姓名映射表：萧炎 -> 萧妍",
            &settings,
        );

        assert!(prompt.contains("双女主百合叙事"));
        assert!(prompt.contains("清除所有原男性主角痕迹"));
        assert!(prompt.contains("人物外貌特征必须前后一致"));
        assert!(prompt.contains("上一章是金发，下一章不能无理由变成红发"));
        assert!(prompt.contains("百合向关系推进必须承接前文"));
        assert!(prompt.contains("不能突然重置或跳跃"));
        assert!(prompt.contains("其他配角、敌人、长辈、师父、兄弟、父亲、旁观者必须保持原文性别"));
        assert!(prompt.contains("原文男性继续使用男性代词/称谓"));
        assert!(prompt.contains("主角与男性角色共同被指代"));
        assert!(prompt.contains("禁止改成“她们”"));
        assert!(prompt.contains("只有确认全员女性时才能使用“她们”"));
        assert!(prompt.contains("动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物"));
    }

    #[test]
    fn review_prompt_checks_creative_mode_strength() {
        let chapter = sample_chapter(1, "第一章", "萧炎走进大厅。");
        let settings = NovelSettings {
            novel_id: "novel-1".to_string(),
            protagonist_name: "萧炎".to_string(),
            protagonist_aliases: "".to_string(),
            rewritten_protagonist_name: "".to_string(),
            additional_feminize_names: "".to_string(),
            bust: "平胸".to_string(),
            body_type: "少女".to_string(),
            rewrite_mode: "creative".to_string(),
            advanced_settings: "".to_string(),
            updated_at: "now".to_string(),
        };
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "萧妍走进大厅。".to_string(),
        };
        let prompt = build_batch_review_prompt_with_settings(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );
        let decision_prompt = build_batch_review_decision_prompt_with_context(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
            "",
            "姓名映射表：萧炎 -> 萧妍",
            "",
        );

        assert!(prompt.contains("如果当前为创意模式"));
        assert!(prompt.contains("只是替换姓名/代词"));
        assert!(prompt.contains("章节标题原则上必须保留原标题和原编号"));
        assert!(prompt.contains("无法确认指向主角的男性词语都不是标题问题"));
        assert!(prompt.contains("女性外貌、神态、互动距离、称谓变化、百合向情绪张力"));
        assert!(prompt.contains("看不出主角原本是男性"));
        assert!(prompt.contains("人物外貌特征是否前后一致"));
        assert!(prompt.contains("百合向关系推进是否承接前文"));
        assert!(prompt.contains("不能为了强调性别而破坏原文战力"));
        assert!(
            prompt.contains("未指定性转的配角、敌人、长辈、师父、兄弟、父亲、旁观者是否被误改性别")
        );
        assert!(prompt.contains("同一人物在不同章节中的他/她"));
        assert!(prompt.contains("复数群体代词是否符合成员构成"));
        assert!(prompt.contains("若被改成“她们”必须修正"));
        assert!(decision_prompt.contains("Blocking 清单"));
        assert!(decision_prompt.contains("混合性别群体必须使用“他们”"));
        assert!(decision_prompt.contains("不得仅因群体中包含女性主角就要求改成“她们”"));
        assert!(decision_prompt.contains("标题编号与 marker index 不一致不是问题"));
        assert!(decision_prompt.contains("不要输出通过项、优点、确认事项"));
        assert!(decision_prompt.contains("不代表标题章节编号"));
        assert!(decision_prompt.contains("动物、灵兽、妖兽、凶兽、神兽、器灵等非人生物"));
        assert!(decision_prompt.contains("保留原文人称代词和称谓"));
        assert!(decision_prompt.contains("仅与主角原名共享单字的未指定 NPC"));
        assert!(decision_prompt.contains("同名/旧名/以旧名某字为名"));
        assert!(decision_prompt.contains("JSON 输出硬性格式"));
        assert!(decision_prompt.contains("不得因为字段长度限制而省略问题"));
        assert!(decision_prompt.contains("problem 最多 120 字"));
        assert!(decision_prompt.contains("再次确认：只输出合法 JSON 对象"));
    }

    #[test]
    fn review_decision_ignores_title_number_and_marker_index_mismatch() {
        let output = r#"{
          "approved": false,
          "summary": "标题编号与分片索引不一致",
          "issues": [
            {
              "chapter_index": 22,
              "severity": "blocking",
              "problem": "章节标题仍保留原章节号‘第0021章’，与当前分片索引22不对应。",
              "required_fix": "标题应改为第0022章或按分片索引统一。"
            }
          ]
        }"#;

        let decision = parse_review_decision_output(output, &sample_novel_settings())
            .expect("parse review decision");

        assert!(decision.approved);
        assert!(decision.issues.is_empty());
    }

    #[test]
    fn review_decision_discards_compliant_blocking_items_but_keeps_real_defects() {
        let output = r#"{
          "approved": false,
          "summary": "存在一处未指定角色性别误改",
          "issues": [
            {
              "chapter_index": 14,
              "severity": "blocking",
              "problem": "原文‘少年’描述未指定性转的男性敌人，改写稿正确保留，符合规则。",
              "required_fix": "确认保留，无需修改。"
            },
            {
              "chapter_index": 13,
              "severity": "blocking",
              "problem": "主角与男性配角共同组成的群体原应使用‘他们’，改写稿却误改为‘她们’，违反混合性别群体代词规则。",
              "required_fix": "必须修正为‘他们’。"
            },
            {
              "chapter_index": 15,
              "severity": "blocking",
              "problem": "章节边界清晰，无缺句、重复或串章，章节完整。",
              "required_fix": "各项通过。"
            }
          ]
        }"#;

        let decision = parse_review_decision_output(output, &sample_novel_settings())
            .expect("parse review decision");

        assert!(!decision.approved);
        assert_eq!(decision.issues.len(), 1);
        assert_eq!(decision.issues[0].chapter_indexes, vec![13]);
        assert!(review_issue_text(&decision.issues[0]).contains("误改为‘她们’"));
        assert!(review_issue_text(&decision.issues[0]).contains("混合性别群体"));
    }

    #[test]
    fn review_decision_ignores_unnecessary_title_feminization_but_keeps_target_name() {
        let output = r#"{
          "approved": false,
          "issues": [
            {
              "chapter_index": 7,
              "severity": "blocking",
              "problem": "标题‘斗之力，三段！’没有增加女性化意象。",
              "required_fix": "创意模式下应修改标题。"
            },
            {
              "chapter_index": 8,
              "severity": "blocking",
              "problem": "标题仍保留主角原名‘萧炎’。",
              "required_fix": "必须修改为设定姓名‘萧妍’。"
            }
          ]
        }"#;

        let decision = parse_review_decision_output(output, &sample_novel_settings())
            .expect("parse review decision");

        assert!(!decision.approved);
        assert_eq!(decision.issues.len(), 1);
        assert!(review_issue_text(&decision.issues[0]).contains("萧炎"));
    }

    #[test]
    fn review_decision_supports_structured_and_legacy_issue_indexes() {
        let output = r#"{
          "approved": false,
          "issues": [
            {
              "chapter_indexes": [3, 2, 3],
              "scope": "chapter",
              "category": "gender_residue",
              "severity": "blocking",
              "problem": "两章仍有主角男性称谓。",
              "required_fix": "移除男性称谓。"
            },
            {
              "chapter_index": 4,
              "problem": "主角原名仍有残留。",
              "required_fix": "替换为设定姓名。"
            },
            "分片索引 5：主角仍被称为少爷，必须修改。"
          ]
        }"#;

        let decision = parse_review_decision_output(output, &sample_novel_settings())
            .expect("parse mixed review formats");

        assert!(!decision.approved);
        assert_eq!(decision.issues.len(), 3);
        assert_eq!(decision.issues[0].chapter_indexes, vec![2, 3]);
        assert_eq!(decision.issues[0].scope, "chapter");
        assert_eq!(decision.issues[0].category, "gender_residue");
        assert_eq!(decision.issues[1].chapter_indexes, vec![4]);
        assert_eq!(decision.issues[2].chapter_indexes, vec![5]);
    }

    #[test]
    fn review_decision_parse_error_message_suggests_manual_retry() {
        let message = review_decision_parse_error_message(
            "第1-30章 · 分片 1/3",
            "expected value at line 11 column 23",
        );

        assert!(message.contains("审查决策无法解析"));
        assert!(message.contains("可以手动重试当前任务"));
        assert!(message.contains("JSON 输出更稳定的模型"));
    }

    #[test]
    fn review_decision_json_repair_prompt_preserves_original_semantics() {
        let invalid_output = r#"{
  "approved": false,
  "issues": [
    {"chapter_indexes":[1891],"problem":"改写稿中出现了 "少年" 称谓","required_fix":"改为少女"}
  ]
}"#;

        let prompt = build_review_decision_json_repair_prompt(
            invalid_output,
            "expected `,` or `}` at line 3 column 80",
            &sample_novel_settings(),
        );

        assert!(prompt.contains("只修复 JSON 格式"));
        assert!(prompt.contains("不要重新审查正文"));
        assert!(prompt.contains("不要新增或删除审查问题"));
        assert!(prompt.contains("\"chapter_indexes\": [1]"));
        assert!(prompt.contains("expected `,` or `}`"));
        assert!(prompt.contains("改写稿中出现了 \"少年\" 称谓"));
    }

    #[test]
    fn protagonist_residue_scan_detects_original_derived_nicknames() {
        let chapter = sample_chapter(
            368,
            "第0368章 童趣",
            "石昊挥了挥手。小昊，三日后你要进去。小昊姑娘，赶紧闭关吧。昊叔，我们从村后走。",
        );
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text:
                "石念昔挥了挥手。小昊，三日后你要进去。小昊姑娘，赶紧闭关吧。昊叔，我们从村后走。"
                    .to_string(),
        };
        let mut settings = sample_novel_settings();
        settings.protagonist_name = "石昊".to_string();
        settings.rewritten_protagonist_name = "石念昔".to_string();

        let issues = detect_protagonist_derived_name_residue(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );

        assert_eq!(issues.len(), 1);
        let text = review_issue_text(&issues[0]);
        assert!(text.contains("小昊"));
        assert!(text.contains("昊叔"));
        let candidates = protagonist_residue_candidates_from_original(&chapter, "石昊");
        assert!(!candidates.iter().any(|candidate| candidate == "昊"));
    }

    #[test]
    fn protagonist_residue_scan_handles_multi_character_given_names() {
        let chapter = sample_chapter(12, "第十二章", "李火旺回头。小火旺别闹。旺哥今日出门。");
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "李火婉回头。小火旺别闹。旺哥今日出门。".to_string(),
        };
        let mut settings = sample_novel_settings();
        settings.protagonist_name = "李火旺".to_string();
        settings.rewritten_protagonist_name = "李火婉".to_string();

        let issues = detect_protagonist_derived_name_residue(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );

        assert_eq!(issues.len(), 1);
        let text = review_issue_text(&issues[0]);
        assert!(text.contains("小火旺"));
        assert!(text.contains("旺哥"));
    }

    #[test]
    fn protagonist_residue_scan_ignores_bare_last_character_and_unseen_candidates() {
        let chapter = sample_chapter(9, "第九章", "石昊进入昊天塔。");
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "石念昔进入昊天塔。昊哥没有出现在原文，不能凭空当残留。".to_string(),
        };
        let mut settings = sample_novel_settings();
        settings.protagonist_name = "石昊".to_string();
        settings.rewritten_protagonist_name = "石念昔".to_string();

        let issues = detect_protagonist_derived_name_residue(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );

        assert!(issues.is_empty());
    }

    #[test]
    fn protagonist_residue_scan_preserves_npc_with_shared_name_character() {
        let chapter = sample_chapter(
            370,
            "第0370章 双昊",
            "石昊脸上平静。秦昊走来，与石昊一样，以昊为名，只是姓氏不同。",
        );
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "石念昔脸上平静。秦昊走来，与她相似，只是姓氏不同。".to_string(),
        };
        let mut settings = sample_novel_settings();
        settings.protagonist_name = "石昊".to_string();
        settings.rewritten_protagonist_name = "石念昔".to_string();

        let issues = detect_protagonist_derived_name_residue(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );

        assert!(issues.is_empty());
        let candidates = protagonist_residue_candidates_from_original(&chapter, "石昊");
        assert!(!candidates.iter().any(|candidate| candidate == "秦昊"));
    }

    #[test]
    fn protagonist_name_logic_scan_flags_old_name_semantic_conflict() {
        let chapter = sample_chapter(
            370,
            "第0370章 双昊",
            "石昊脸上平静。秦昊走来，与石昊一样，以昊为名，只是姓氏不同。",
        );
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "石念昔脸上平静。秦昊走来，与她原本同名，都曾以昊为名，只是姓氏不同。"
                .to_string(),
        };
        let mut settings = sample_novel_settings();
        settings.protagonist_name = "石昊".to_string();
        settings.rewritten_protagonist_name = "石念昔".to_string();

        let issues = detect_protagonist_name_logic_inconsistency(
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &settings,
        );

        assert_eq!(issues.len(), 1);
        let text = review_issue_text(&issues[0]);
        assert!(text.contains("姓名逻辑矛盾"));
        assert!(text.contains("以昊为名"));
    }

    #[test]
    fn protagonist_residue_scan_merges_with_ai_review_decision() {
        let chapter = sample_chapter(1, "第一章", "萧炎来了。小炎也来了。");
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "萧妍来了。小炎也来了。".to_string(),
        };
        let decision = ReviewDecision {
            approved: true,
            issues: Vec::new(),
        };

        let (decision, added) = merge_deterministic_protagonist_residue_issues(
            decision,
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &sample_novel_settings(),
        );

        assert!(!decision.approved);
        assert_eq!(added.len(), 1);
        assert_eq!(decision.issues[0].chapter_indexes, vec![1]);
        assert!(review_issue_text(&decision.issues[0]).contains("小炎"));
    }

    #[test]
    fn review_decision_filters_hallucinated_source_name_residue() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![27],
                "chapter",
                "gender_residue",
                "第27章中，原文‘石昊眼神清亮’未改写，残留主角男性姓名‘石昊’。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-27".to_string(),
            index: 27,
            title: "第27章".to_string(),
            text: "石林虎将石念昔抛上高空，石念昔眼神清亮。".to_string(),
        }];

        let mut settings = sample_novel_settings();
        settings.protagonist_name = "石昊".to_string();
        settings.rewritten_protagonist_name = "石念昔".to_string();
        let (filtered_decision, filtered_issues) =
            filter_review_decision_against_rewrites(decision, &[], &rewrites, &settings);

        assert!(filtered_decision.approved);
        assert!(filtered_decision.issues.is_empty());
        assert_eq!(filtered_issues.len(), 1);
    }

    #[test]
    fn review_decision_keeps_verified_source_name_residue() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![27],
                "chapter",
                "gender_residue",
                "第27章仍残留主角男性姓名‘萧炎’，必须修改。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-27".to_string(),
            index: 27,
            title: "第27章".to_string(),
            text: "萧炎抬起头。".to_string(),
        }];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(!filtered_decision.approved);
        assert_eq!(filtered_decision.issues.len(), 1);
        assert!(filtered_issues.is_empty());
    }

    #[test]
    fn review_decision_filters_hallucinated_quoted_gender_residue() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![255],
                "chapter",
                "gender_residue",
                "第255章正文中，存在一处指代主角的男性化称谓残留：‘在场中防御的少年也很逆天’。此处少年明显指代正在防御的主角，必须修改为少女。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-255".to_string(),
            index: 255,
            title: "第255章".to_string(),
            text: "每一个人都震惊，在场中防御的少女也很逆天，居然以十大洞天挡住了。".to_string(),
        }];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(filtered_decision.approved);
        assert!(filtered_decision.issues.is_empty());
        assert_eq!(filtered_issues.len(), 1);
    }

    #[test]
    fn review_decision_keeps_quoted_gender_residue_present_in_rewrite() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![255],
                "chapter",
                "gender_residue",
                "第255章正文中，存在一处指代主角的男性化称谓残留：‘在场中防御的少年也很逆天’。必须修改为少女。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-255".to_string(),
            index: 255,
            title: "第255章".to_string(),
            text: "每一个人都震惊，在场中防御的少年也很逆天，居然以十大洞天挡住了。".to_string(),
        }];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(!filtered_decision.approved);
        assert_eq!(filtered_decision.issues.len(), 1);
        assert!(filtered_issues.is_empty());
    }

    #[test]
    fn review_decision_filters_neutral_nickname_style_complaints() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![
                sample_review_issue(
                    vec![254],
                    "chapter",
                    "gender_residue",
                    "第254章中，旁白已有‘这凶残的少女开创了一个又一个另类的纪录’，但前文仍有感叹‘我就知道，这个家伙又要凶残到底了’，其中‘这个家伙’指代主角，保留了中性/偏男性泛指，与整体称为少女的风格不一致。",
                ),
                sample_review_issue(
                    vec![244],
                    "chapter",
                    "gender_residue",
                    "第244章中，‘熊孩子不时与人激战’仍保留中性昵称‘熊孩子’，建议改为石念昔。",
                ),
            ],
        };
        let rewrites = vec![
            ParsedChapterRewrite {
                id: "chapter-254".to_string(),
                index: 254,
                title: "第254章".to_string(),
                text: "旁白写道，这凶残的少女开创了一个又一个另类的纪录。陆地生灵感叹：我就知道，这个家伙又要凶残到底了。".to_string(),
            },
            ParsedChapterRewrite {
                id: "chapter-244".to_string(),
                index: 244,
                title: "第244章".to_string(),
                text: "在前行的路上，熊孩子不时与人激战，笑得很开心。".to_string(),
            },
        ];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(filtered_decision.approved);
        assert!(filtered_decision.issues.is_empty());
        assert_eq!(filtered_issues.len(), 2);
    }

    #[test]
    fn review_decision_keeps_explicit_male_reference_even_near_neutral_nickname() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![
                sample_review_issue(
                    vec![244],
                    "chapter",
                    "gender_residue",
                    "第244章中，‘这个家伙还是少年模样’明确将主角称为少年，必须修改。",
                ),
                sample_review_issue(
                    vec![245],
                    "chapter",
                    "gender_residue",
                    "第245章中，‘他抬头看向众人’仍使用男性代词指代主角，必须修改。",
                ),
            ],
        };
        let rewrites = vec![
            ParsedChapterRewrite {
                id: "chapter-244".to_string(),
                index: 244,
                title: "第244章".to_string(),
                text: "这个家伙还是少年模样，却挡住了众人。".to_string(),
            },
            ParsedChapterRewrite {
                id: "chapter-245".to_string(),
                index: 245,
                title: "第245章".to_string(),
                text: "他抬头看向众人，神情平静。".to_string(),
            },
        ];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(!filtered_decision.approved);
        assert_eq!(filtered_decision.issues.len(), 2);
        assert!(filtered_issues.is_empty());
    }

    #[test]
    fn review_decision_filters_ambiguous_non_human_pronoun_complaints() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![88],
                "chapter",
                "gender_residue",
                "第88章中，灵兽原文性别不明，但改写稿仍称‘它盘踞在石台上’，没有改成女性代词。应改为‘她’以统一女性化。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-88".to_string(),
            index: 88,
            title: "第88章".to_string(),
            text: "那头灵兽沉默不语，它盘踞在石台上，双目发光。".to_string(),
        }];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(filtered_decision.approved);
        assert!(filtered_decision.issues.is_empty());
        assert_eq!(filtered_issues.len(), 1);
    }

    #[test]
    fn review_decision_allows_ambiguous_non_human_original_he_pronoun() {
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![89],
                "chapter",
                "gender_residue",
                "第89章中，妖兽原文性别不明，但改写稿仍称‘他伏在洞口’，没有改成女性代词。应改为‘她’以统一女性化。",
            )],
        };
        let rewrites = vec![ParsedChapterRewrite {
            id: "chapter-89".to_string(),
            index: 89,
            title: "第89章".to_string(),
            text: "那头妖兽没有回应，他伏在洞口，像是在守着什么。".to_string(),
        }];

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            &[],
            &rewrites,
            &sample_novel_settings(),
        );

        assert!(filtered_decision.approved);
        assert!(filtered_decision.issues.is_empty());
        assert_eq!(filtered_issues.len(), 1);
    }

    #[test]
    fn review_decision_filters_missing_droppable_author_notes_only() {
        let chapter = sample_chapter(
            177,
            "第177章 归来",
            "她推门走进院中。\n作者年份勘误\n大家投\nし\n----------",
        );
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "她推门走进院中。".to_string(),
        };
        let decision = ReviewDecision {
            approved: false,
            issues: vec![
                sample_review_issue(
                    vec![177],
                    "chapter",
                    "content_missing",
                    "改写稿删除了原文中的“作者年份勘误”。",
                ),
                sample_review_issue(
                    vec![177],
                    "chapter",
                    "content_missing",
                    "改写稿未保留“大家投”。",
                ),
                sample_review_issue(
                    vec![177],
                    "chapter",
                    "content_missing",
                    "改写稿删除了单独的 し 和“----------”。",
                ),
            ],
        };

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &sample_novel_settings(),
        );

        assert!(filtered_decision.approved);
        assert_eq!(filtered_issues.len(), 3);
    }

    #[test]
    fn review_decision_keeps_missing_story_or_protected_postscript() {
        let chapter = sample_chapter(
            180,
            "第180章 尾声",
            "她推门走进院中，终于见到了等待多年的人。\n完本感言\n感谢一路陪伴。",
        );
        let rewrite = ParsedChapterRewrite {
            id: chapter.id.clone(),
            index: chapter.index,
            title: chapter.title.clone(),
            text: "她推门走进院中。".to_string(),
        };
        let decision = ReviewDecision {
            approved: false,
            issues: vec![
                sample_review_issue(
                    vec![180],
                    "chapter",
                    "content_missing",
                    "改写稿删除了“终于见到了等待多年的人”。",
                ),
                sample_review_issue(
                    vec![180],
                    "chapter",
                    "content_missing",
                    "改写稿删除了“完本感言”。",
                ),
            ],
        };

        let (filtered_decision, filtered_issues) = filter_review_decision_against_rewrites(
            decision,
            std::slice::from_ref(&chapter),
            std::slice::from_ref(&rewrite),
            &sample_novel_settings(),
        );

        assert!(!filtered_decision.approved);
        assert_eq!(filtered_decision.issues.len(), 2);
        assert!(filtered_issues.is_empty());
    }

    #[test]
    fn legacy_issue_does_not_treat_original_title_number_as_marker_index() {
        let output = r#"{
          "approved": false,
          "issues": ["第21章正文仍有主角男性称谓，需要修改。"]
        }"#;

        let decision = parse_review_decision_output(output, &sample_novel_settings())
            .expect("parse legacy issue");

        assert_eq!(decision.issues.len(), 1);
        assert!(decision.issues[0].chapter_indexes.is_empty());
        assert_eq!(decision.issues[0].scope, "shard");
    }

    #[test]
    fn review_revision_targets_only_valid_local_chapters() {
        let chapters = (1..=6)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();
        let decision = ReviewDecision {
            approved: false,
            issues: vec![
                sample_review_issue(vec![2], "chapter", "gender_residue", "第二章有残留。"),
                sample_review_issue(vec![4], "chapter", "name", "第四章姓名错误。"),
            ],
        };

        assert_eq!(
            plan_review_revision(&chapters, &decision),
            RevisionPlan::Targeted(vec![2, 4])
        );
    }

    #[test]
    fn review_revision_falls_back_for_unscoped_cross_chapter_or_excessive_targets() {
        let chapters = (1..=6)
            .map(|index| sample_chapter(index, &format!("第{index}章"), "原文"))
            .collect::<Vec<_>>();
        let cases = [
            ReviewDecision {
                approved: false,
                issues: vec![sample_review_issue(
                    vec![],
                    "shard",
                    "legacy",
                    "问题无法定位。",
                )],
            },
            ReviewDecision {
                approved: false,
                issues: vec![sample_review_issue(
                    vec![2, 3],
                    "cross_chapter",
                    "continuity",
                    "跨章连续性断裂。",
                )],
            },
            ReviewDecision {
                approved: false,
                issues: vec![sample_review_issue(
                    vec![1, 2, 3, 4],
                    "chapter",
                    "gender_residue",
                    "多个章节有残留。",
                )],
            },
            ReviewDecision {
                approved: false,
                issues: vec![sample_review_issue(
                    vec![99],
                    "chapter",
                    "name",
                    "索引越界。",
                )],
            },
        ];

        for decision in cases {
            assert!(matches!(
                plan_review_revision(&chapters, &decision),
                RevisionPlan::Full(_)
            ));
        }
    }

    #[test]
    fn targeted_merge_preserves_unaffected_chapters_byte_for_byte() {
        let originals = (1..=4)
            .map(|index| ParsedChapterRewrite {
                id: format!("chapter-{index}"),
                index,
                title: format!("第{index}章"),
                text: format!("  原样正文 {index}\n第二行  "),
            })
            .collect::<Vec<_>>();
        let replacement = ParsedChapterRewrite {
            id: "chapter-2".to_string(),
            index: 2,
            title: "第二章修复".to_string(),
            text: "修复正文".to_string(),
        };

        let merged = merge_targeted_rewrites(&originals, vec![replacement.clone()], &[2])
            .expect("merge targeted rewrite");

        assert_eq!(merged[1].text, replacement.text);
        assert_eq!(merged[0].text.as_bytes(), originals[0].text.as_bytes());
        assert_eq!(merged[2].text.as_bytes(), originals[2].text.as_bytes());
        assert_eq!(merged[3].text.as_bytes(), originals[3].text.as_bytes());
    }

    #[test]
    fn targeted_marker_validation_rejects_missing_extra_and_wrong_id_markers() {
        let target = sample_chapter(2, "第二章", "原文");
        let extra = sample_chapter(3, "第三章", "原文");
        let valid = format!(
            "{}\n标题：第二章\n正文：\n修复正文\n{}",
            chapter_start_marker(&target),
            chapter_end_marker(&target)
        );
        assert!(validate_targeted_rewrite_markers(&valid, std::slice::from_ref(&target)).is_ok());

        let missing_end = format!(
            "{}\n标题：第二章\n正文：\n修复正文",
            chapter_start_marker(&target)
        );
        assert!(
            validate_targeted_rewrite_markers(&missing_end, std::slice::from_ref(&target)).is_err()
        );

        let with_extra = format!(
            "{}\n{}\n{}\n{}",
            chapter_start_marker(&target),
            chapter_end_marker(&target),
            chapter_start_marker(&extra),
            chapter_end_marker(&extra)
        );
        assert!(
            validate_targeted_rewrite_markers(&with_extra, std::slice::from_ref(&target)).is_err()
        );

        let wrong_id = valid.replace(&target.id, "wrong-id");
        assert!(
            validate_targeted_rewrite_markers(&wrong_id, std::slice::from_ref(&target)).is_err()
        );
    }

    #[test]
    fn targeted_revision_context_contains_read_only_original_and_rewrite_neighbors() {
        let chapters = (1..=4)
            .map(|index| {
                sample_chapter(index, &format!("第{index}章"), &format!("原文内容 {index}"))
            })
            .collect::<Vec<_>>();
        let rewrites = chapters
            .iter()
            .map(|chapter| ParsedChapterRewrite {
                id: chapter.id.clone(),
                index: chapter.index,
                title: chapter.title.clone(),
                text: format!("改写内容 {}", chapter.index),
            })
            .collect::<Vec<_>>();

        let context =
            build_targeted_revision_context(&chapters, &rewrites, &HashSet::from([2_i64]));

        assert!(context.contains("原文内容 1"));
        assert!(context.contains("改写内容 1"));
        assert!(context.contains("原文内容 3"));
        assert!(context.contains("改写内容 3"));
        assert!(!context.contains("改写内容 2"));
    }

    #[test]
    fn targeted_revision_prompt_contains_only_full_target_chapter_body() {
        let chapters = (1..=3)
            .map(|index| {
                sample_chapter(
                    index,
                    &format!("第{index}章"),
                    &format!("原文内容 {index} {}", "很长正文".repeat(20)),
                )
            })
            .collect::<Vec<_>>();
        let rewrites = chapters
            .iter()
            .map(|chapter| ParsedChapterRewrite {
                id: chapter.id.clone(),
                index: chapter.index,
                title: chapter.title.clone(),
                text: format!("改写内容 {} {}", chapter.index, "很长改写".repeat(20)),
            })
            .collect::<Vec<_>>();
        let decision = ReviewDecision {
            approved: false,
            issues: vec![sample_review_issue(
                vec![2],
                "chapter",
                "gender_residue",
                "第二章仍有主角男性称谓。",
            )],
        };
        let adjacent =
            build_targeted_revision_context(&chapters, &rewrites, &HashSet::from([2_i64]));

        let prompt = build_targeted_revision_prompt(
            &chapters[1..2],
            &rewrites[1..2],
            "姓名映射表：萧炎 -> 萧妍",
            &sample_novel_settings(),
            "",
            "",
            &decision,
            &adjacent,
        );

        assert!(prompt.contains("目标章节原文"));
        assert!(prompt.contains("原文内容 2"));
        assert!(prompt.contains("目标章节当前改写稿"));
        assert!(prompt.contains("改写内容 2"));
        assert!(prompt.contains("【输出格式硬性要求】"));
        assert!(prompt.contains("再次确认：只输出目标章节的结果"));
        assert!(prompt.contains("相邻章节只读上下文"));
        assert!(prompt.contains("主角与男性共同被指代或群体含男性成员时使用“他们”"));
        assert!(prompt.contains("只有全员女性时才使用“她们”"));
        assert!(!prompt.contains(&format!("原文内容 1 {}", "很长正文".repeat(20))));
        assert!(!prompt.contains(&format!("原文内容 3 {}", "很长正文".repeat(20))));
        assert!(!prompt.contains(&chapter_start_marker(&chapters[0])));
        assert!(!prompt.contains(&chapter_start_marker(&chapters[2])));
    }

    #[test]
    fn analysis_prompt_tracks_original_gender_pronouns_without_rewrite_rules() {
        let chapter = sample_chapter(1, "第一章", "萧炎和父亲说话，旁边的少女点头。");
        let prompt = build_batch_analysis_prompt(&[chapter]);

        assert!(prompt.contains("原文性别线索"));
        assert!(prompt.contains("原文人称代词"));
        assert!(prompt.contains("性别不明"));
        assert!(!prompt.contains("百合"));
        assert!(!prompt.contains("女性化"));
        assert!(!prompt.contains("代词替换"));
    }

    #[test]
    fn deepseek_detection_covers_official_and_proxy_configs() {
        let mut profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "DeepSeek".to_string(),
            provider: "OpenAI 兼容".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            model: "deepseek-chat".to_string(),
            temperature: 0.7,
            top_p: 1.0,
            thinking_mode: "auto".to_string(),
            prompt_obfuscation_enabled: false,
            has_api_key: true,
            api_key_storage: "system".to_string(),
            updated_at: "now".to_string(),
        };
        assert!(is_deepseek_profile(
            &profile,
            "https://api.deepseek.com/v1",
            "deepseek-chat"
        ));

        profile.base_url = "https://example-proxy.invalid/v1".to_string();
        profile.model = "deepseek-v4-pro".to_string();
        assert!(is_deepseek_profile(
            &profile,
            "https://example-proxy.invalid/v1",
            "deepseek-v4-pro"
        ));

        profile.model = "gpt-4o".to_string();
        assert!(!is_deepseek_profile(
            &profile,
            "https://example-proxy.invalid/v1",
            "gpt-4o"
        ));
    }

    #[test]
    fn thinking_mode_parameters_are_provider_specific() {
        let mut profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "OpenRouter".to_string(),
            provider: "openai-compatible".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "anthropic/claude-sonnet-4".to_string(),
            temperature: 0.7,
            top_p: 1.0,
            thinking_mode: "off".to_string(),
            prompt_obfuscation_enabled: false,
            has_api_key: true,
            api_key_storage: "system".to_string(),
            updated_at: "now".to_string(),
        };

        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["reasoning"]["effort"], "none");

        profile.base_url = "https://api.openai.com/v1".to_string();
        profile.model = "gpt-5.1".to_string();
        profile.thinking_mode = "on".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["reasoning_effort"], "medium");

        profile.base_url = "https://api.deepseek.com/v1".to_string();
        profile.model = "deepseek-v4-pro".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking"]["type"], "disabled");

        profile.base_url = "https://api.moonshot.ai/v1".to_string();
        profile.model = "kimi-k2.5".to_string();
        profile.thinking_mode = "on".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking"]["type"], "enabled");

        profile.base_url = "https://api.siliconflow.cn/v1".to_string();
        profile.model = "Qwen/Qwen3.5-122B-A10B".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["enable_thinking"], false);

        profile.base_url = "https://api.minimax.io/v1".to_string();
        profile.model = "MiniMax-M2.7".to_string();
        let mut payload = json!({});
        assert!(!apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload, json!({}));

        profile.model = "MiniMax-M3".to_string();
        profile.thinking_mode = "on".to_string();
        let mut payload = json!({});
        assert!(apply_openai_compatible_thinking_control(
            &mut payload,
            &profile,
            &profile.base_url,
            &profile.model
        ));
        assert_eq!(payload["thinking"]["type"], "adaptive");

        profile.provider = "gemini".to_string();
        profile.model = "gemini-2.5-flash".to_string();
        profile.thinking_mode = "off".to_string();
        let mut payload = json!({ "generationConfig": {} });
        assert!(apply_gemini_thinking_control(&mut payload, &profile));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );

        profile.model = "gemini-2.5-pro".to_string();
        let mut payload = json!({ "generationConfig": {} });
        assert!(apply_gemini_thinking_control(&mut payload, &profile));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            128
        );

        profile.model = "gemini-3.1-pro-preview".to_string();
        let mut payload = json!({ "generationConfig": {} });
        assert!(apply_gemini_thinking_control(&mut payload, &profile));
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
    }

    #[test]
    fn enabled_prompt_obfuscation_only_softens_configured_phrase() {
        let profile = ModelProfile {
            id: "profile-1".to_string(),
            name: "MiMo".to_string(),
            provider: "openai-compatible".to_string(),
            base_url: "https://api.xiaomimimo.com/v1".to_string(),
            model: "mimo-v2.5-pro".to_string(),
            temperature: 0.7,
            top_p: 1.0,
            thinking_mode: "auto".to_string(),
            prompt_obfuscation_enabled: true,
            has_api_key: true,
            api_key_storage: "system".to_string(),
            updated_at: "now".to_string(),
        };

        let (system, user) = prepare_prompt_for_profile(
            &profile,
            "双女主百合文本",
            "百合向关系、亲密互动暗示、身体描写、体型：萝莉、身材：巨乳、平胸",
        );

        assert!(system.contains("双女主百合文本"));
        assert!(user.contains("百合向关系"));
        assert!(user.contains("亲密互动暗示"));
        assert!(user.contains("身体描写"));
        assert!(user.contains("体型：萝莉"));
        assert!(user.contains("身材：胸围丰满"));
        assert!(user.contains("平胸"));
        assert!(!user.contains("巨乳"));
    }

    #[test]
    fn openai_content_filter_response_is_reported_before_parsing() {
        let value = json!({
            "choices": [{
                "finish_reason": "content_filter",
                "message": {
                    "content": "The request was rejected because it was considered high risk"
                }
            }]
        });

        let error = openai_content_filter_error(&value, "mimo-v2.5-pro").expect("content filter");

        assert!(error.contains("模型内容安全策略拦截"));
        assert!(error.contains("mimo-v2.5-pro"));
        assert!(error.contains("content_filter"));
    }

    #[test]
    fn update_check_parses_release_redirect_url_without_api() {
        let tag =
            release_tag_from_url("https://github.com/3minto1/Yuri-Rewrite/releases/tag/v0.1.2")
                .expect("release tag");

        assert_eq!(tag, "v0.1.2");
        assert_eq!(
            portable_zip_name(&normalize_release_version(&tag)),
            "YuriRewrite-v0.1.2-windows-x64.zip"
        );
        assert!(is_newer_version("0.1.2", "0.1.1"));
        assert!(!is_newer_version("0.1.1", "0.1.1"));
    }

    #[test]
    fn chapter_heading_regex_covers_common_toc_rules() {
        let heading_re = chapter_heading_regex();
        for title in [
            "第1章 限落的天才",
            "正文 第三章：客人",
            "第一话 新的开始",
            "卷五 开源盛典",
            "上卷 山雨",
            "Chapter 1 MyGrandmaIsNB",
            "Section 12",
            "Part 3 - After",
            "Episode 4",
            "No. 5",
            "第1夜 雨中旧案",
            "第2案 无声证词",
            "第3场 天台重逢",
            "第4弹 她的反击",
            "第5折 灯下回身",
            "第6更 月色正好",
            "【特别篇】",
            "=== 第五章 娜儿 ===",
            "===楔子===",
            "===引言===",
            "序言",
            "序幕 神树之下",
            "番外篇 她们后来",
            "===番外 她们后来===",
        ] {
            assert!(
                heading_re.is_match(title),
                "expected heading match: {title}"
            );
        }
    }

    #[test]
    fn chapter_heading_regex_handles_windows_crlf_lines() {
        let text = "第1章 陨落的天才\r\n这里是第一章正文。\r\n第2章 斗气大陆\r\n这里是第二章正文。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第1章 陨落的天才");
        assert_eq!(split.chapters[1].title, "第2章 斗气大陆");
    }

    #[test]
    fn strict_chapter_split_ignores_update_notice_pseudo_headings() {
        let text = "第159章 夜归\n这里是第一段正式剧情。\n第一更！\n第二更！\n第三更！\n第一章，第二章也快了。（未完待续）\n第160章 重逢\n这里是第二段正式剧情。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第159章 夜归");
        assert_eq!(split.chapters[0].original_text, "这里是第一段正式剧情。");
        assert_eq!(split.chapters[1].title, "第160章 重逢");
    }

    #[test]
    fn strict_chapter_split_merges_update_marker_body_back_into_formal_chapter() {
        let story = "宽阔的大船撕裂开空间裂缝，碾压飞入云雾中。雪鹰看向远方，继续追查敌人的踪迹。";
        let text = format!(
            "第九篇 第二十四章 刺入\n第二更到！番茄继续写~~~\n{story}\n第九篇 第二十五章 坠落\n下一章正式剧情。"
        );
        let split = split_chapters("novel-1", &text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第九篇 第二十四章 刺入");
        assert_eq!(split.chapters[0].original_text, story);
        assert_eq!(split.chapters[1].title, "第九篇 第二十五章 坠落");
    }

    #[test]
    fn strict_chapter_split_drops_composite_update_notice_pseudo_headings() {
        let text = "第九篇 第二十五章 坠落\n正式剧情甲。\n第一更！第二更也快了。\n第九篇 第二十六章 救治\n正式剧情乙。\n第二更到！番茄继续写~~~\n第九篇 第二十七章 夏族先辈降临\n正式剧情丙。\n第三更到，第四更快了。\n第九篇 第二十八章 救\n正式剧情丁。\n第一更未完待续。~好搜搜篮色，即可最快阅读后面章节\n第九篇 第二十九章 醒来\n正式剧情戊。\n第二更到，还有第三章\n未完待续。~好搜搜篮色，即可最快阅读后面章节";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec![
                "第九篇 第二十五章 坠落",
                "第九篇 第二十六章 救治",
                "第九篇 第二十七章 夏族先辈降临",
                "第九篇 第二十八章 救",
                "第九篇 第二十九章 醒来",
            ]
        );
        assert!(split.chapters.iter().all(|chapter| {
            !chapter.original_text.contains("更到")
                && !chapter.original_text.contains("继续写")
                && !chapter.original_text.contains("最快阅读")
        }));
        assert_eq!(split.chapters[4].original_text, "正式剧情戊。");
    }

    #[test]
    fn strict_chapter_split_preserves_real_geng_headings_and_special_chapters() {
        let text = "第1更 潮声\n这里是第一段完整剧情，标题中的“更”是这篇小说稳定使用的章节单位。\n第2更 月下\n这里是第二段完整剧情，继续推动人物关系和主要冲突。\n番外 她们后来\n这是独立番外剧情，应当继续保留。\n第九篇结束。\n作者总结这一篇的剧情并引出下一篇。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 4);
        assert_eq!(split.chapters[0].title, "第1更 潮声");
        assert_eq!(split.chapters[1].title, "第2更 月下");
        assert_eq!(split.chapters[2].title, "番外 她们后来");
        assert_eq!(split.chapters[3].title, "第九篇结束。");
    }

    #[test]
    fn author_note_classifier_is_conservative() {
        for line in [
            "第一更！",
            "作者年份勘误",
            "大家投",
            "し",
            "----------",
            "求月票求收藏",
        ] {
            assert!(
                is_obvious_droppable_author_note_line(line),
                "expected droppable author note: {line}"
            );
        }
        for line in ["完本感言", "卷末后记", "后记", "她推门走进院中。"] {
            assert!(
                !is_obvious_droppable_author_note_line(line),
                "expected protected or narrative content: {line}"
            );
        }
    }

    #[test]
    fn strict_chapter_headings_reject_loose_numbers_and_pure_symbols() {
        let heading_re = chapter_heading_regex();
        for title in [
            "1、这就是标题",
            "二十四、我瞎编的标题",
            "（11）我奶常山赵子龙",
            "====================================",
            "=== 起 ===",
        ] {
            assert!(
                !heading_re.is_match(title),
                "expected non-strict heading rejection: {title}"
            );
        }
    }

    #[test]
    fn loose_numbered_headings_are_used_only_without_standard_headings() {
        let text = "001 不应作为章节\n这一行会留在正文里。\n第1章 正式开始\n这里是内容甲\n第2章 继续推进\n这里是内容乙";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第1章 正式开始");
        assert!(split.chapters[0].original_text.contains("这里是内容甲"));
    }

    #[test]
    fn loose_numbered_headings_detect_sequential_numbered_titles() {
        let text = "001 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n002 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n003 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "001 初遇");
        assert_eq!(split.chapters[1].title, "002 再会");
        assert_eq!(split.chapters[2].title, "003 终局");
    }

    #[test]
    fn loose_numbered_headings_allow_punctuation_inside_titles() {
        let text = "1、遇事不决，量子力学\n这里是第一章正文，章节内容足够完整，不是普通列表项。\n2、巨型boss？更兴奋了\n这里是第二章正文，剧情继续推进并保持较长正文。\n3、不要完美，要夸张。\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "1、遇事不决，量子力学");
        assert_eq!(split.chapters[1].title, "2、巨型boss？更兴奋了");
        assert_eq!(split.chapters[2].title, "3、不要完美，要夸张。");
    }

    #[test]
    fn loose_numbered_headings_handle_windows_crlf_lines() {
        let text = "简介：\r\n这里是简介，可能包含主角名字。\r\n1、丧尸娘与不死美人的娇躯\r\n这里是第一章正文，章节内容足够完整，不是普通列表项。\r\n2、身为丧尸怎么能不吃人呢\r\n这里是第二章正文，剧情继续推进并保持较长正文。\r\n3、身材真是极品\r\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 4);
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[2].title, "2、身为丧尸怎么能不吃人呢");
    }

    #[test]
    fn loose_numbered_headings_are_used_when_only_volume_headings_are_strict() {
        let text = "第一卷\n1、丧尸娘与不死美人的娇躯\n这里是第一章正文，主角醒来后确认环境和身份，章节内容足够完整。\n2、身为丧尸怎么能不吃人呢\n这里是第二章正文，剧情继续推进，标题使用纯数字顿号格式。\n3、身材真是极品\n这里是第三章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[1].title, "2、身为丧尸怎么能不吃人呢");
        assert_eq!(split.chapters[2].title, "3、身材真是极品");
    }

    #[test]
    fn loose_numbered_headings_ignore_volume_titles_and_prose_like_round_lines() {
        let text = "简介：\n作为堂堂魔族第一王女，只想每天喝喝小茶，看看风景，平稳度日。\n第1卷.新的归宿？\n1.败啦？\n这里是第一章正文，主角在战场上完成计划，章节内容足够完整。\n2.她似乎在撩我！\n这里是第二章正文，剧情继续推进，纯数字标题后面带有正式标题。\n第二回是手里的甜甜圈从手里滑出去然后手忙脚乱在空中惊险拦截避免落地。\n这是一句正文，不应该被识别成章节标题。\n3.过往\n这里是第三章正文，继续展开人物行动和场景变化。\n第一回魔力链接失败了，喊圣剑过来，西莱特菈说她正在睡觉休息。\n这也是正文里的普通句子。\n上架感言\n上架感言无需说的太多，今晚上架，大家可以提前点一下上架预订。\n4.火祸降罚\n这里是第四章正文，继续展开人物行动和场景变化。\n5.真实实力？\n这里是第五章正文，继续展开人物行动和场景变化。\n6.说到做到\n这里是第六章正文，继续展开人物行动和场景变化。\n7.懂了！\n这里是第七章正文，继续展开人物行动和场景变化。\n8.商讨\n这里是第八章正文，继续展开人物行动和场景变化。\n9.如何处置？\n这里是第九章正文，继续展开人物行动和场景变化。\n10.寒意\n这里是第十章正文，继续展开人物行动和场景变化。\n11.找到啦！\n这里是第十一章正文，继续展开人物行动和场景变化。\n12.湖与鱼\n这里是第十二章正文，继续展开人物行动和场景变化。\n第2卷.雪落花飞月盈时\n1.新卷开头\n这里是第二卷第一章正文，卷内编号重新开始但仍然是章节。\n2.雪落花飞\n这里是第二卷第二章正文，继续展开人物行动和场景变化。\n2023.7.3——2025.6.30\n这里是日期记录，不应该被识别成章节标题。\n0.说明\n这里是番外或插图说明，也可能需要保留为可处理章节。\n1.神明的邀约\n这里是附录第一章正文，编号再次重置也应该保留。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(split.chapters.len(), 17, "{titles:?}");
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1.败啦？");
        assert_eq!(split.chapters[2].title, "2.她似乎在撩我！");
        assert!(split.chapters[2]
            .original_text
            .contains("第二回是手里的甜甜圈"));
        assert!(split.chapters[3]
            .original_text
            .contains("第一回魔力链接失败了"));
        assert!(split.chapters[3]
            .original_text
            .contains("上架感言无需说的太多"));
        assert_eq!(split.chapters[13].title, "1.新卷开头");
        assert!(split.chapters[14].original_text.contains("2023.7.3"));
        assert_eq!(split.chapters[15].title, "0.说明");
        assert_eq!(split.chapters[16].title, "1.神明的邀约");
    }

    #[test]
    fn loose_numbered_headings_override_intro_volume_and_prose_like_special_matches() {
        let text = "简介：\n【变身+丧尸娘+系统+末世+自恋向】\n第一卷\n1、丧尸娘与不死美人的娇躯\n这里是第一章正文，主角醒来后确认环境和身份，章节内容足够完整。\n2、身为丧尸怎么能不吃人呢\n这里是第二章正文，剧情继续推进，标题使用纯数字顿号格式。\n上架感言\n看到评论区有读者在关心作者的精神状态，作者表示自己状态很好。\n3、身材真是极品\n这里是第三章正文，继续展开人物行动和场景变化。\n　　话一落音，只见手机屏幕上的内容就投影在了空气中，分辨率精细无比。\n4、比人类更加高等的生物\n这里是第四章正文，继续展开人物行动和场景变化。\n5、一分饱也算饱\n这里是第五章正文，继续展开人物行动和场景变化。\n6、真长舌妇\n这里是第六章正文，继续展开人物行动和场景变化。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        let titles = split
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(split.chapters.len(), 7, "{titles:?}");
        assert_eq!(split.chapters[0].title, "简介：");
        assert!(split.chapters[0].original_text.contains("丧尸娘"));
        assert_eq!(split.chapters[1].title, "1、丧尸娘与不死美人的娇躯");
        assert_eq!(split.chapters[2].title, "2、身为丧尸怎么能不吃人呢");
        assert!(split.chapters[2].original_text.contains("上架感言"));
        assert!(split.chapters[3].original_text.contains("话一落音"));
        assert_eq!(split.chapters[4].title, "4、比人类更加高等的生物");
    }

    #[test]
    fn loose_numbered_headings_allow_small_numbering_typos() {
        let text = "1、开局\n这里是第一章正文，章节内容足够完整，不是普通编号列表。\n2、推进\n这里是第二章正文，剧情继续推进并保持较长正文。\n3、转折\n这里是第三章正文，继续展开人物行动和场景变化。\n3、编号笔误\n这里是第四章正文，但是标题编号误写成了三，仍然应作为章节。\n5、收束\n这里是第五章正文，前文线索继续回收并形成完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 5);
        assert_eq!(split.chapters[3].title, "3、编号笔误");
        assert_eq!(split.chapters[4].title, "5、收束");
    }

    #[test]
    fn loose_numbered_headings_allow_single_digit_typo_inside_long_run() {
        let mut text = String::new();
        for idx in 1..=112 {
            let shown = if idx == 109 { 9 } else { idx };
            text.push_str(&format!("{shown}、第{idx}章标题\n"));
            text.push_str("这里是章节正文，内容长度足够说明它不是普通列表项，剧情持续推进。\n");
        }
        let split = split_chapters("novel-1", &text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 112);
        assert_eq!(split.chapters[108].title, "9、第109章标题");
        assert_eq!(split.chapters[109].title, "110、第110章标题");
    }

    #[test]
    fn loose_numbered_headings_handle_long_run_with_real_world_numbering_glitches() {
        let mut text =
            String::from("简介：\n这里是简介，可能包含主角名字，也应该进入改写流程。\n第一卷\n");
        for idx in 1..=441 {
            let shown = match idx {
                65 => 64,
                109 => 9,
                126 | 135 => continue,
                435 => 434,
                _ => idx,
            };
            text.push_str(&format!("{shown}、第{idx}章标题\n"));
            text.push_str("这里是章节正文，内容长度足够说明它不是普通列表项，剧情持续推进。\n");
        }
        let split = split_chapters("novel-1", &text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters[0].title, "简介：");
        assert_eq!(split.chapters[1].title, "1、第1章标题");
        assert_eq!(split.chapters[65].title, "64、第65章标题");
        assert!(split
            .chapters
            .iter()
            .any(|chapter| chapter.title == "9、第109章标题"));
        assert_eq!(
            split.chapters.last().map(|chapter| chapter.title.as_str()),
            Some("441、第441章标题")
        );
    }

    #[test]
    fn loose_numbered_headings_detect_sequential_chinese_numbered_titles() {
        let text = "一 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n二 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n三 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert_eq!(split.chapters[0].title, "一 初遇");
        assert_eq!(split.chapters[1].title, "二 再会");
        assert_eq!(split.chapters[2].title, "三 终局");
    }

    #[test]
    fn loose_numbered_headings_reject_nonsequential_numbered_lists() {
        let text =
            "1 普通列表项\n这里是内容甲\n3 跳号列表项\n这里是内容乙\n7 又一个列表项\n这里是内容丙";
        let split = split_chapters("novel-1", text);

        assert!(!split.detected_chapters);
        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapters[0].title, "自动分段 1");
    }

    #[test]
    fn loose_numbered_headings_reject_sentence_like_numbered_lines() {
        let text = "一 初遇\n这里是内容甲，主角第一次遇见重要人物，冲突与伏笔同时出现，场景完整展开。\n二 再会\n这里是内容乙，两人重新见面并推动关系变化，正文长度足够说明这不是列表项。\n三 终局\n这里是内容丙，前文线索被回收，章节正文继续推进到完整段落。\n五、六岁的孩子，自然没有什么男女之别，琅玡仍旧只顾着玩闹。\n后面还有普通正文，不能因为句首中文数字和顿号就切出新章节。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 3);
        assert!(split.chapters[2]
            .original_text
            .contains("五、六岁的孩子，自然没有什么男女之别"));
    }

    #[test]
    fn loose_numbered_headings_reject_symbol_separator_lines() {
        let text = "====================================\n正文开头有装饰分隔符。\n1 普通列表项\n这里是内容甲\n2 又一个列表项\n这里是内容乙";
        let split = split_chapters("novel-1", text);

        assert!(!split.detected_chapters);
        assert_eq!(split.chapters.len(), 1);
        assert_eq!(split.chapters[0].title, "自动分段 1");
    }

    #[test]
    fn special_headings_keep_preface_and_interleaved_extra_chapters() {
        let text = "===楔子===\n高大的树木茂密得连阳光也无法透入，这里是开篇内容。\n===第一章 觉醒日===\n第一章正文继续展开，主角正式登场。\n===番外 她们后来===\n番外正文穿插在正常章节之间，也应该作为独立章节保留。\n===第二章 武魂觉醒===\n第二章正文继续推进。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 4);
        assert_eq!(split.chapters[0].title, "===楔子===");
        assert!(split.chapters[0].original_text.contains("开篇内容"));
        assert_eq!(split.chapters[1].title, "===第一章 觉醒日===");
        assert_eq!(split.chapters[2].title, "===番外 她们后来===");
        assert!(split.chapters[2].original_text.contains("番外正文"));
        assert_eq!(split.chapters[3].title, "===第二章 武魂觉醒===");
    }

    #[test]
    fn strict_headings_reject_round_phrase_inside_body() {
        let text = "第415章 挑战\n第一回合的接触，武器便是被击落，这一幕即使看台上的人也怔住了。\n第416章 家传玉片\n真正的下一章正文。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第415章 挑战");
        assert!(split.chapters[0]
            .original_text
            .contains("第一回合的接触，武器便是被击落"));
        assert_eq!(split.chapters[1].title, "第416章 家传玉片");
    }

    #[test]
    fn postscript_heading_is_kept_but_postscript_body_sentence_is_not_split() {
        let text = "第417章 最后的选拔赛\n最后一章正文。\n后记。\n后记写到这里，也该是结束的时候了，可我还是有些舍不得。\n谢谢看到这里的读者。";
        let split = split_chapters("novel-1", text);

        assert!(split.detected_chapters);
        assert_eq!(split.chapters.len(), 2);
        assert_eq!(split.chapters[0].title, "第417章 最后的选拔赛");
        assert_eq!(split.chapters[1].title, "后记。");
        assert!(split.chapters[1]
            .original_text
            .contains("后记写到这里，也该是结束的时候了"));
    }

    #[test]
    fn extract_tailing_json_from_reasoning_content() {
        // Reasoning text with JSON object at the end
        let reasoning = "审查分析：改写稿基本合格。输出JSON。{\n  \"approved\": true,\n  \"summary\": \"通过\"\n}";
        let extracted =
            extract_tailing_json_from_text(reasoning).expect("should extract trailing JSON object");
        let value: serde_json::Value =
            serde_json::from_str(extracted).expect("extracted text must be valid JSON");
        assert_eq!(value["approved"], true);
        assert_eq!(value["summary"], "通过");

        // No valid JSON anywhere should return None.
        let plain = "这是一段普通的思考文字，不包含任何 JSON 结构。";
        assert!(extract_tailing_json_from_text(plain).is_none());

        // JSON array at the end.
        let reasoning_array = "思考中...最终输出：[\n  {\"name\": \"萧炎\", \"gender\": \"male\"},\n  {\"name\": \"萧妍\", \"gender\": \"female\"}\n]";
        let extracted_array = extract_tailing_json_from_text(reasoning_array)
            .expect("should extract trailing JSON array");
        let value_array: serde_json::Value =
            serde_json::from_str(extracted_array).expect("extracted text must be valid JSON");
        assert_eq!(value_array.as_array().unwrap().len(), 2);

        // Empty text.
        assert!(extract_tailing_json_from_text("").is_none());
        assert!(extract_tailing_json_from_text("   ").is_none());
    }

    #[test]
    fn frontend_error_log_redacts_sensitive_values() {
        let entry = format_frontend_error_entry(
            "request failed Authorization: Bearer secret-token",
            Some("https://example.com?api_key=secret-key"),
            Some("at App"),
        );
        assert!(!entry.contains("secret-token"));
        assert!(!entry.contains("secret-key"));
        assert!(entry.contains("[REDACTED]"));
        assert!(entry.contains("at App"));
    }
}
