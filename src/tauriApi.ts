import { invoke } from "@tauri-apps/api/core";
import type {
  AiLog,
  AppSettings,
  AutoRunRecovery,
  CanonAsset,
  CanonAssetInput,
  ExportResult,
  Job,
  JobEstimate,
  ModelDiagnosis,
  ModelProfile,
  ModelProfileInput,
  Novel,
  NovelDetail,
  NovelSettings,
  ProfileDraft,
  TokenUsageReport,
  UpdateCheckResult,
  UpdateDownloadResult,
  UpdateInstallResult
} from "./types";

type CommandMap = {
  list_novels: { args?: undefined; result: Novel[] };
  get_novel_detail: { args: { novelId: string }; result: NovelDetail };
  list_auto_run_recoveries: { args?: undefined; result: AutoRunRecovery[] };
  import_txt: { args: { filePath: string }; result: Novel };
  delete_novel: { args: { novelId: string }; result: void };
  list_model_profiles: { args?: undefined; result: ModelProfile[] };
  save_model_profile: { args: { input: ModelProfileInput }; result: ModelProfile };
  delete_model_profile: { args: { profileId: string }; result: void };
  diagnose_model_profile: { args: { profileId: string }; result: ModelDiagnosis };
  list_ai_logs: { args: { novelId: string | null }; result: AiLog[] };
  clear_ai_logs: { args: { novelId: string | null }; result: void };
  get_token_usage_stats: {
    args: { startDate: string; endDate: string };
    result: TokenUsageReport;
  };
  get_app_settings: { args?: undefined; result: AppSettings };
  save_app_settings: { args: { settings: AppSettings }; result: AppSettings };
  save_selected_profile_id: { args: { profileId: string | null }; result: AppSettings };
  save_novel_settings: {
    args: {
      novelId: string;
      protagonistName: string;
      protagonistAliases?: string;
      rewrittenProtagonistName: string;
      additionalFeminizeNames: string;
      bust: string;
      bodyType: string;
      rewriteMode: "strict" | "creative";
      advancedSettings: string;
    };
    result: NovelSettings;
  };
  estimate_job_cost: {
    args: { novelId: string; batchId: string | null; profileId: string | null };
    result: JobEstimate;
  };
  update_canon_assets: { args: { novelId: string; assets: CanonAssetInput[] }; result: CanonAsset[] };
  save_chapter_rewrite_edit: {
    args: { chapterId: string; rewriteText: string };
    result: import("./types").Chapter;
  };
  restore_chapter_rewrite_edit: {
    args: { chapterId: string };
    result: import("./types").Chapter;
  };
  rewrite_single_chapter: {
    args: {
      novelId: string;
      profileId: string;
      chapterId: string;
      instructions: string;
      sourceMode?: "original" | "rewrite";
    };
    result: import("./types").Chapter;
  };
  terminate_single_chapter_rewrite: {
    args: { novelId: string };
    result: void;
  };
  restore_single_chapter_rewrite: {
    args: { chapterId: string };
    result: import("./types").Chapter;
  };
  start_analysis: { args: { novelId: string; profileId: string; batchId: string }; result: Job };
  start_rewrite: { args: { novelId: string; profileId: string; batchId: string }; result: Job };
  start_analyze_rewrite_batch: {
    args: { novelId: string; profileId: string; batchId: string };
    result: Job;
  };
  start_analyze_rewrite_all: {
    args: { novelId: string; profileId: string; startBatchId: string | null };
    result: Job;
  };
  pause_analyze_rewrite_all: { args: { novelId: string }; result: Job };
  terminate_analyze_rewrite_all: { args: { novelId: string }; result: Job };
  export_novel: { args: { novelId: string; format: "txt" }; result: ExportResult };
  open_github_url: { args?: undefined; result: void };
  open_github_release_url: { args?: undefined; result: void };
  check_for_updates: { args?: undefined; result: UpdateCheckResult };
  download_latest_update: { args?: undefined; result: UpdateDownloadResult };
  take_update_install_result: { args?: undefined; result: UpdateInstallResult | null };
  record_frontend_error: {
    args: { message: string; stack: string | null; componentStack: string | null };
    result: void;
  };
};

export type TauriCommand = keyof CommandMap;

export function invokeCommand<C extends TauriCommand>(
  command: C,
  ...args: CommandMap[C] extends { args: infer A } ? [args: A] : [args?: undefined]
): Promise<CommandMap[C]["result"]> {
  return invoke<CommandMap[C]["result"]>(command, args[0] as Record<string, unknown> | undefined);
}
