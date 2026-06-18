export type Novel = {
  id: string;
  title: string;
  source_path: string;
  encoding: string;
  status: string;
  created_at: string;
};

export type Chapter = {
  id: string;
  novel_id: string;
  index: number;
  title: string;
  original_text: string;
  analysis_json?: string | null;
  rewrite_text?: string | null;
  rewrite_edited?: boolean;
  analysis_status: string;
  rewrite_status: string;
};

export type CanonAsset = {
  novel_id: string;
  kind: string;
  content: string;
  updated_at: string;
};

export type ChapterBatch = {
  id: string;
  novel_id: string;
  batch_index: number;
  label: string;
  start_chapter: number;
  end_chapter: number;
  file_path: string;
  created_at: string;
};

export type NovelSettings = {
  novel_id: string;
  protagonist_name: string;
  rewritten_protagonist_name: string;
  additional_feminize_names: string;
  bust: string;
  body_type: string;
  rewrite_mode: "strict" | "creative";
  advanced_settings: string;
  updated_at: string;
};

export type NovelDetail = {
  novel: Novel;
  chapters: Chapter[];
  canon_assets: CanonAsset[];
  batches: ChapterBatch[];
  settings?: NovelSettings | null;
};

export type ModelProfile = {
  id: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
  thinking_mode: "auto" | "off" | "on";
  has_api_key: boolean;
  api_key_storage: "system" | "database_fallback" | "none";
  updated_at: string;
};

export type ProfileDraft = {
  id?: string;
  name: string;
  provider: string;
  base_url: string;
  model: string;
  temperature: number;
  thinking_mode: "auto" | "off" | "on";
  api_key: string;
};

export type ModelProfileInput = Omit<ProfileDraft, "api_key"> & { api_key?: string };

export type Job = {
  id: string;
  novel_id: string;
  job_type: string;
  status: string;
  current_chapter: number;
  total_chapters: number;
  message: string;
  phase?: "analysis" | "rewrite" | "review" | "revision" | "final_review" | "export";
  batch_index?: number;
  batch_total?: number;
  batch_label?: string;
  shard_completed?: number;
  shard_total?: number;
  active_shards?: ActiveShardProgress[];
};

export type ActiveShardProgress = {
  index: number;
  total: number;
  start_chapter: number;
  end_chapter: number;
  phase: "analysis" | "rewrite" | "review" | "revision" | "final_review" | "export";
};

export type AutoRunRecovery = {
  novel_id: string;
  start_batch_index: number;
  next_batch_index: number;
  status: string;
  pause_reason: string;
  phase?: string | null;
  batch_index?: number | null;
  profile_ids: string[];
  job?: Job | null;
};

export type AiLog = {
  id: string;
  novel_id?: string | null;
  profile_id: string;
  action: string;
  chapter_title?: string | null;
  status: string;
  content: string;
  reasoning?: string | null;
  raw_response?: string | null;
  created_at: string;
};

export type AppSettings = {
  export_dir?: string | null;
  core_prompt?: string;
  review_enabled?: boolean;
  review_profile_id?: string | null;
  selected_profile_id?: string | null;
  chapter_batch_size?: 30 | 50 | 100;
  rewrite_parallelism?: 1 | 3 | 6 | 10 | 25 | 50;
};

export type UpdateCheckResult = {
  current_version: string;
  latest_version: string;
  latest_tag: string;
  is_latest: boolean;
  release_url: string;
  asset_name: string;
  asset_download_url: string;
};

export type UpdateDownloadResult = {
  path: string;
  version: string;
};

export type JobEstimate = {
  novel_chapters: number;
  novel_chars: number;
  novel_batches: number;
  selected_batch_chapters: number;
  selected_batch_chars: number;
  parallelism: number;
  review_enabled: boolean;
  current_batch_requests: number;
  full_run_requests: number;
  average_call_seconds?: number | null;
  estimated_current_batch_seconds?: number | null;
  estimated_full_run_seconds?: number | null;
  recent_success_calls: number;
  recent_failed_calls: number;
  average_input_chars?: number | null;
  average_output_chars?: number | null;
};

export type DiagnosisStatus = "ok" | "warning" | "failed";

export type ModelDiagnosis = {
  status: DiagnosisStatus;
  recommended_thinking_mode?: "auto" | "off" | "on" | null;
  checks: Array<{
    name: string;
    status: DiagnosisStatus;
    message: string;
  }>;
};

export type NovelSettingsDraft = {
  protagonist_name: string;
  rewritten_protagonist_name: string;
  additional_feminize_names: string;
  bust: string;
  body_type: string;
  rewrite_mode: "strict" | "creative";
  advanced_settings: string;
};

export type ExportResult = { path: string };
export type CanonAssetInput = Pick<CanonAsset, "kind" | "content">;
export type AutoRunState = "idle" | "running" | "paused" | "stopping";
