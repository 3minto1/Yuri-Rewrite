import type {
  AiLog,
  AppSettings,
  CanonAsset,
  Chapter,
  ChapterRule,
  ChapterRulePreview,
  Job,
  JobEstimate,
  ModelDiagnosis,
  ModelProfile,
  ModelProfileInput,
  Novel,
  NovelDetail,
  NovelSettings,
  StoredChapterRule,
  TokenUsageReport,
  UpdateCheckResult
} from "../types";

const now = "2026-06-24T12:00:00+08:00";
const novel: Novel = {
  id: "browser-novel-1",
  title: "浏览器测试小说",
  source_path: "browser-mock.txt",
  encoding: "UTF-8",
  status: "imported",
  created_at: now
};

const chapterTitles = [
  "雪鹰领", "超凡", "离离", "姐弟", "枪法", "修炼", "长风骑士", "决定",
  "五年后，脱胎换骨", "狼吞虎咽", "强大", "太古时代", "进城", "飞雪神枪",
  "大法师的要求", "毁灭山脉", "进入山脉的日子", "偷袭", "生死一刹那", "玄冰枪法"
];

let chapters: Chapter[] = chapterTitles.map((title, offset) => {
  const index = offset + 1;
  const completed = index <= 12;
  return {
    id: `browser-chapter-${index}`,
    novel_id: novel.id,
    index,
    title: `第${index}章 ${title}`,
    original_text: `这是第${index}章的浏览器测试原文。东伯雪鹰正在推进剧情，并与余靖秋交流。`,
    analysis_json: completed ? JSON.stringify({ summary: `第${index}章分析摘要` }) : null,
    rewrite_text: completed
      ? `这是第${index}章的浏览器测试改写稿。东伯雪璎与余靖秋共同推进剧情。`
      : null,
    rewrite_edited: false,
    single_rewrite_original_available: false,
    analysis_status: completed ? "completed" : "pending",
    rewrite_status: completed ? "completed" : "pending"
  };
});

let settings: AppSettings = {
  export_dir: null,
  core_prompt: "保持人物关系、世界观和剧情连续性。",
  review_enabled: true,
  review_profile_id: "browser-profile-deepseek",
  analysis_profile_id: "browser-profile-deepseek",
  selected_profile_id: "browser-profile-deepseek",
  chapter_batch_size: 10,
  rewrite_parallelism: 10
};

let profiles: ModelProfile[] = [
  {
    id: "browser-profile-deepseek",
    name: "DeepSeek 官方",
    provider: "openai-compatible",
    base_url: "https://api.deepseek.com",
    model: "deepseek-v4-pro",
    temperature: 0.7,
    top_p: 1,
    thinking_mode: "auto",
    prompt_obfuscation_enabled: false,
    has_api_key: true,
    api_key_storage: "system",
    updated_at: now
  },
  {
    id: "browser-profile-claude",
    name: "Claude 官方",
    provider: "anthropic",
    base_url: "https://api.anthropic.com",
    model: "claude-opus-4-8",
    temperature: 0.7,
    top_p: 1,
    thinking_mode: "auto",
    prompt_obfuscation_enabled: false,
    has_api_key: true,
    api_key_storage: "system",
    updated_at: now
  }
];

let novelSettings: NovelSettings = {
  novel_id: novel.id,
  protagonist_name: "东伯雪鹰",
  protagonist_aliases: "雪鹰",
  rewritten_protagonist_name: "东伯雪璎",
  additional_feminize_names: "",
  bust: "平胸",
  body_type: "少女",
  rewrite_mode: "strict",
  advanced_settings: "",
  relationship_targets: "[]",
  updated_at: now
};

let canonAssets: CanonAsset[] = [
  { novel_id: novel.id, kind: "人物关系", content: "东伯雪璎与余靖秋互相信任。", updated_at: now },
  { novel_id: novel.id, kind: "人物卡", content: "东伯雪璎：主角，沉稳坚韧。", updated_at: now },
  { novel_id: novel.id, kind: "地点", content: "雪鹰领：故事初期主要地点。", updated_at: now }
];

let chapterRule: StoredChapterRule | null = null;

let logs: AiLog[] = [
  {
    id: "browser-log-1",
    novel_id: novel.id,
    profile_id: "browser-profile-deepseek",
    action: "分析章节",
    chapter_title: chapters[0].title,
    status: "success",
    content: "浏览器测试模式下的分析日志。",
    created_at: now
  }
];

function detail(): NovelDetail {
  if (novel.status === "pending_split") {
    return {
      novel: { ...novel },
      chapters: [],
      canon_assets: [],
      batches: [],
      settings: { ...novelSettings }
    };
  }
  return {
    novel: { ...novel },
    chapters: chapters.map((chapter) => ({ ...chapter })),
    canon_assets: canonAssets.map((asset) => ({ ...asset })),
    batches: [
      {
        id: "browser-batch-1",
        novel_id: novel.id,
        batch_index: 1,
        label: "第1批：1-10章",
        start_chapter: 1,
        end_chapter: 10,
        file_path: "browser/1.txt",
        created_at: now
      },
      {
        id: "browser-batch-2",
        novel_id: novel.id,
        batch_index: 2,
        label: "第2批：11-20章",
        start_chapter: 11,
        end_chapter: 20,
        file_path: "browser/2.txt",
        created_at: now
      }
    ],
    settings: { ...novelSettings }
  };
}

function mockChapterRulePreview(): ChapterRulePreview {
  const previewTitles = [
    "第一章 雪鹰领",
    "第二章 超凡",
    "第三章 离离",
    "第四章 姐弟",
    "第五章 枪法",
    "第六章 修炼"
  ];
  return {
    total_chapters: previewTitles.length,
    chapters: previewTitles.map((title, index) => ({ index: index + 1, title })),
    can_apply: true,
    message: "浏览器测试模式：预览已生成。"
  };
}

function applyMockSplit(rule?: ChapterRule) {
  if (rule) {
    chapterRule = { novel_id: novel.id, rule, updated_at: now };
  }
  novel.status = "imported";
  chapters = chapterTitles.map((title, offset) => {
    const index = offset + 1;
    return {
      id: `browser-chapter-${index}`,
      novel_id: novel.id,
      index,
      title: `第${index}章 ${title}`,
      original_text: `这是第${index}章的浏览器测试原文。东伯雪鹰正在推进剧情，并与余靖秋交流。`,
      analysis_json: null,
      rewrite_text: null,
      rewrite_edited: false,
      single_rewrite_original_available: false,
      analysis_status: "pending",
      rewrite_status: "pending"
    };
  });
  canonAssets = [
    { novel_id: novel.id, kind: "人物关系", content: "", updated_at: now },
    { novel_id: novel.id, kind: "人物卡", content: "", updated_at: now },
    { novel_id: novel.id, kind: "地点", content: "", updated_at: now }
  ];
}

function estimate(): JobEstimate {
  return {
    novel_chapters: chapters.length,
    novel_chars: chapters.reduce((sum, chapter) => sum + chapter.original_text.length, 0),
    novel_batches: 2,
    selected_batch_chapters: 10,
    selected_batch_chars: 23_825,
    parallelism: settings.rewrite_parallelism ?? 10,
    review_enabled: settings.review_enabled ?? true,
    current_batch_requests: settings.review_enabled ? 70 : 20,
    full_run_requests: settings.review_enabled ? 140 : 40,
    average_call_seconds: 52,
    estimated_current_batch_seconds: settings.review_enabled ? 364 : 104,
    estimated_full_run_seconds: settings.review_enabled ? 728 : 208,
    recent_success_calls: 18,
    recent_failed_calls: 1,
    average_input_chars: 12_400,
    average_output_chars: 4_800
  };
}

function completedJob(jobType: string): Job {
  return {
    id: `browser-job-${Date.now()}`,
    novel_id: novel.id,
    job_type: jobType,
    status: "completed",
    current_chapter: 10,
    total_chapters: 10,
    message: "浏览器测试模式：任务已模拟完成。"
  };
}

function updateChapter(chapterId: string, update: Partial<Chapter>): Chapter {
  const index = chapters.findIndex((chapter) => chapter.id === chapterId);
  if (index < 0) throw new Error("浏览器测试章节不存在。");
  chapters[index] = { ...chapters[index], ...update };
  return { ...chapters[index] };
}

export async function invokeBrowserMock(
  command: string,
  args?: Record<string, unknown>
): Promise<unknown> {
  switch (command) {
    case "list_novels":
      return [{ ...novel }];
    case "get_novel_detail":
      return detail();
    case "get_chapter_rule":
      return chapterRule ? { ...chapterRule, rule: { ...chapterRule.rule } } : null;
    case "preview_chapter_rule":
      return mockChapterRulePreview();
    case "save_chapter_rule_and_split": {
      const rule = args?.rule as ChapterRule;
      applyMockSplit(rule);
      return { ...chapterRule!, rule: { ...chapterRule!.rule } };
    }
    case "split_novel_with_builtin_rule":
      applyMockSplit();
      return undefined;
    case "list_auto_run_recoveries":
      return [];
    case "list_model_profiles":
      return profiles.map((profile) => ({ ...profile }));
    case "get_app_settings":
      return { ...settings };
    case "save_app_settings":
      settings = { ...settings, ...(args?.settings as AppSettings) };
      return { ...settings };
    case "save_selected_profile_id":
      settings = { ...settings, selected_profile_id: (args?.profileId as string | null) ?? null };
      return { ...settings };
    case "save_model_profile": {
      const input = args?.input as ModelProfileInput;
      const id = input.id ?? `browser-profile-${profiles.length + 1}`;
      const saved: ModelProfile = {
        ...input,
        id,
        thinking_mode: input.thinking_mode ?? "auto",
        has_api_key: true,
        api_key_storage: "system",
        updated_at: now
      };
      profiles = [saved, ...profiles.filter((profile) => profile.id !== id)];
      return { ...saved };
    }
    case "delete_model_profile":
      profiles = profiles.filter((profile) => profile.id !== args?.profileId);
      return undefined;
    case "diagnose_model_profile":
      return {
        status: "ok",
        recommended_thinking_mode: null,
        checks: [
          { name: "API Key", status: "ok", message: "浏览器测试凭据可用。" },
          { name: "普通响应", status: "ok", message: "浏览器测试响应正常。" },
          { name: "JSON 输出", status: "ok", message: "浏览器测试 JSON 正常。" }
        ]
      } satisfies ModelDiagnosis;
    case "list_ai_logs":
      return logs.map((log) => ({ ...log }));
    case "clear_ai_logs":
      logs = [];
      return undefined;
    case "get_token_usage_stats":
      return {
        start_date: String(args?.startDate ?? "2026-05-26"),
        end_date: String(args?.endDate ?? "2026-06-24"),
        requests: 19,
        input_tokens: 235_600,
        output_tokens: 91_200,
        models: [{
          profile_id: profiles[0]?.id ?? "browser-profile",
          profile_name: profiles[0]?.name ?? "浏览器模型",
          model: profiles[0]?.model ?? "mock-model",
          requests: 19,
          input_tokens: 235_600,
          output_tokens: 91_200,
          days: [{ date: "2026-06-24", requests: 19, input_tokens: 235_600, output_tokens: 91_200 }]
        }]
      } satisfies TokenUsageReport;
    case "save_novel_settings":
      novelSettings = {
        novel_id: novel.id,
        protagonist_name: String(args?.protagonistName ?? ""),
        protagonist_aliases: String(args?.protagonistAliases ?? ""),
        rewritten_protagonist_name: String(args?.rewrittenProtagonistName ?? ""),
        additional_feminize_names: String(args?.additionalFeminizeNames ?? ""),
        bust: String(args?.bust ?? "平胸"),
        body_type: String(args?.bodyType ?? "少女"),
        rewrite_mode: args?.rewriteMode === "creative" ? "creative" : "strict",
        advanced_settings: String(args?.advancedSettings ?? ""),
        relationship_targets: String(args?.relationshipTargets ?? "[]"),
        updated_at: now
      };
      return { ...novelSettings };
    case "estimate_job_cost":
      return estimate();
    case "update_canon_assets":
      canonAssets = (args?.assets as Array<{ kind: string; content: string }>).map((asset) => ({
        novel_id: novel.id,
        kind: asset.kind,
        content: asset.content,
        updated_at: now
      }));
      return canonAssets.map((asset) => ({ ...asset }));
    case "update_chapter_title": {
      const title = String(args?.title ?? "").trim();
      if (!title) throw new Error("章节名称不能为空。");
      return updateChapter(String(args?.chapterId), { title });
    }
    case "save_chapter_rewrite_edit":
      return updateChapter(String(args?.chapterId), {
        rewrite_text: String(args?.rewriteText ?? ""),
        rewrite_edited: true
      });
    case "restore_chapter_rewrite_edit":
      return updateChapter(String(args?.chapterId), { rewrite_edited: false });
    case "rewrite_single_chapter":
      return updateChapter(String(args?.chapterId), {
        rewrite_text: "浏览器测试模式生成的单章改写稿。",
        rewrite_status: "completed",
        single_rewrite_original_available: true
      });
    case "restore_single_chapter_rewrite":
      return updateChapter(String(args?.chapterId), {
        rewrite_text: "恢复后的浏览器测试初稿。",
        rewrite_status: "completed",
        single_rewrite_original_available: false
      });
    case "terminate_single_chapter_rewrite":
      return undefined;
    case "start_analysis":
      return completedJob("analysis");
    case "start_rewrite":
      return completedJob("rewrite");
    case "start_analyze_rewrite_batch":
      return completedJob("auto_batch");
    case "start_analyze_rewrite_all":
      return completedJob("auto");
    case "pause_analyze_rewrite_all":
      return { ...completedJob("auto"), status: "paused", message: "浏览器测试任务已暂停。" };
    case "terminate_analyze_rewrite_all":
      return { ...completedJob("auto"), status: "terminated", message: "浏览器测试任务已终止。" };
    case "export_novel":
      return { path: "C:\\BrowserMock\\浏览器测试小说-改写稿.txt" };
    case "import_txt":
      novel.status = "pending_split";
      chapters = [];
      canonAssets = [];
      return { ...novel };
    case "delete_novel":
      return undefined;
    case "open_github_url":
    case "open_github_release_url":
    case "record_frontend_error":
      return undefined;
    case "check_for_updates":
      return {
        current_version: "0.3.15",
        latest_version: "0.3.15",
        latest_tag: "v0.3.15",
        is_latest: true,
        release_url: "https://github.com/3minto1/Yuri-Rewrite/releases/latest",
        asset_name: "",
        asset_download_url: ""
      } satisfies UpdateCheckResult;
    case "take_update_install_result":
      return null;
    case "download_latest_update":
      return {
        path: "C:\\BrowserMock\\YuriRewrite-latest.zip",
        version: "0.3.15",
        install_started: false,
        manual_install_required: true,
        message: "浏览器测试模式不会下载安装包。"
      };
    default:
      throw new Error(`浏览器测试模式尚未实现命令：${command}`);
  }
}
