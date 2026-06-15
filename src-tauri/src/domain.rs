use crate::task_control::{ActiveTaskRegistry, AutoRunControl};
use reqwest::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

pub(crate) struct AppState {
    pub(crate) conn: Mutex<Connection>,
    pub(crate) client: Client,
    pub(crate) data_dir: PathBuf,
    pub(crate) app_dir: PathBuf,
    pub(crate) auto_runs: Mutex<HashMap<String, AutoRunControl>>,
    pub(crate) active_tasks: ActiveTaskRegistry,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Novel {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source_path: String,
    pub(crate) encoding: String,
    pub(crate) status: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Chapter {
    pub(crate) id: String,
    pub(crate) novel_id: String,
    pub(crate) index: i64,
    pub(crate) title: String,
    pub(crate) original_text: String,
    pub(crate) analysis_json: Option<String>,
    pub(crate) rewrite_text: Option<String>,
    pub(crate) analysis_status: String,
    pub(crate) rewrite_status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct CanonAsset {
    pub(crate) novel_id: String,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct NovelDetail {
    pub(crate) novel: Novel,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) canon_assets: Vec<CanonAsset>,
    pub(crate) batches: Vec<ChapterBatch>,
    pub(crate) settings: Option<NovelSettings>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ChapterBatch {
    pub(crate) id: String,
    pub(crate) novel_id: String,
    pub(crate) batch_index: i64,
    pub(crate) label: String,
    pub(crate) start_chapter: i64,
    pub(crate) end_chapter: i64,
    pub(crate) file_path: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct NovelSettings {
    pub(crate) novel_id: String,
    pub(crate) protagonist_name: String,
    pub(crate) rewritten_protagonist_name: String,
    pub(crate) additional_feminize_names: String,
    pub(crate) bust: String,
    pub(crate) body_type: String,
    pub(crate) rewrite_mode: String,
    pub(crate) advanced_settings: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct NameMappingEntry {
    pub(crate) source: String,
    pub(crate) target: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct NameMappingAsset {
    pub(crate) version: i64,
    pub(crate) protagonist: Option<NameMappingEntry>,
    pub(crate) names: Vec<NameMappingEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ModelProfile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) temperature: f64,
    pub(crate) thinking_mode: String,
    pub(crate) has_api_key: bool,
    pub(crate) api_key_storage: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelProfileInput {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) temperature: f64,
    pub(crate) thinking_mode: Option<String>,
    pub(crate) api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ModelTestResult {
    pub(crate) ok: bool,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Job {
    pub(crate) id: String,
    pub(crate) novel_id: String,
    pub(crate) job_type: String,
    pub(crate) status: String,
    pub(crate) current_chapter: i64,
    pub(crate) total_chapters: i64,
    pub(crate) message: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct JobProgress {
    pub(crate) id: String,
    pub(crate) novel_id: String,
    pub(crate) job_type: String,
    pub(crate) status: String,
    pub(crate) current_chapter: i64,
    pub(crate) total_chapters: i64,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AiLog {
    pub(crate) id: String,
    pub(crate) novel_id: Option<String>,
    pub(crate) profile_id: String,
    pub(crate) action: String,
    pub(crate) chapter_title: Option<String>,
    pub(crate) status: String,
    pub(crate) content: String,
    pub(crate) reasoning: Option<String>,
    pub(crate) raw_response: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AppSettings {
    pub(crate) export_dir: Option<String>,
    #[serde(default)]
    pub(crate) core_prompt: String,
    #[serde(default)]
    pub(crate) review_enabled: bool,
    #[serde(default)]
    pub(crate) review_profile_id: Option<String>,
    #[serde(default = "crate::default_rewrite_parallelism")]
    pub(crate) rewrite_parallelism: usize,
}

pub(crate) struct ModelOutput {
    pub(crate) text: String,
    pub(crate) reasoning: Option<String>,
    pub(crate) raw_response: String,
    pub(crate) input_chars: usize,
    pub(crate) output_chars: usize,
    pub(crate) elapsed_ms: u128,
    pub(crate) retried_without_thinking: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct JobEstimate {
    pub(crate) novel_chapters: usize,
    pub(crate) novel_chars: usize,
    pub(crate) novel_batches: usize,
    pub(crate) selected_batch_chapters: usize,
    pub(crate) selected_batch_chars: usize,
    pub(crate) parallelism: usize,
    pub(crate) review_enabled: bool,
    pub(crate) current_batch_requests: usize,
    pub(crate) full_run_requests: usize,
    pub(crate) average_call_seconds: Option<f64>,
    pub(crate) estimated_current_batch_seconds: Option<f64>,
    pub(crate) estimated_full_run_seconds: Option<f64>,
    pub(crate) recent_success_calls: usize,
    pub(crate) recent_failed_calls: usize,
    pub(crate) average_input_chars: Option<usize>,
    pub(crate) average_output_chars: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelDiagnosis {
    pub(crate) status: String,
    pub(crate) recommended_thinking_mode: Option<String>,
    pub(crate) checks: Vec<ModelDiagnosisCheck>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ModelDiagnosisCheck {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ExportResult {
    pub(crate) path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct UpdateCheckResult {
    pub(crate) current_version: String,
    pub(crate) latest_version: String,
    pub(crate) latest_tag: String,
    pub(crate) is_latest: bool,
    pub(crate) release_url: String,
    pub(crate) asset_name: String,
    pub(crate) asset_download_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct UpdateDownloadResult {
    pub(crate) path: String,
    pub(crate) version: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CanonAssetInput {
    pub(crate) kind: String,
    pub(crate) content: String,
}

pub(crate) struct SplitResult {
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) detected_chapters: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedChapterRewrite {
    pub(crate) id: String,
    pub(crate) index: i64,
    pub(crate) title: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedChapterAnalysis {
    pub(crate) id: String,
    pub(crate) json: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewIssue {
    pub(crate) chapter_indexes: Vec<i64>,
    pub(crate) scope: String,
    pub(crate) category: String,
    pub(crate) severity: String,
    pub(crate) problem: String,
    pub(crate) required_fix: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReviewDecision {
    pub(crate) approved: bool,
    pub(crate) issues: Vec<ReviewIssue>,
}
