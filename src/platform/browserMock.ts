import type {
  AiLog,
  AiLogCursor,
  AiLogDaySummary,
  AiLogSummaryPage,
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
  NameMappingConsistencyReport,
  Novel,
  NovelDetail,
  NovelSettings,
  RewriteAbApplyResult,
  RewriteAbChapterDetail,
  RewriteAbChoice,
  RewriteAbEstimate,
  RewriteAbRunDetail,
  RewriteAbSlot,
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

let localDataDeleted = false;
let rewriteAbRuns: RewriteAbRunDetail[] = [];
const rewriteAbChapterDetails = new Map<string, RewriteAbChapterDetail>();

let settings: AppSettings = {
  export_dir: null,
  core_prompt: "保持人物关系、世界观和剧情连续性。",
  review_enabled: true,
  review_profile_id: "browser-profile-deepseek",
  analysis_profile_id: "browser-profile-deepseek",
  selected_profile_id: "browser-profile-deepseek",
  chapter_batch_size: 10,
  rewrite_parallelism: 10,
  auto_continue_enabled: false
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
  {
    novel_id: novel.id,
    kind: "姓名映射表",
    content: JSON.stringify({
      version: 2,
      protagonist: { source: "东伯雪鹰", target: "东伯雪璎" },
      names: [
        { source: "东伯雪鹰", target: "东伯雪璎" },
        { source: "雪鹰", target: "雪瑛" }
      ],
      managed_sources: ["东伯雪鹰", "雪鹰"]
    }, null, 2),
    updated_at: now
  },
  { novel_id: novel.id, kind: "人物关系", content: "东伯雪璎与余靖秋互相信任。", updated_at: now },
  { novel_id: novel.id, kind: "人物卡", content: "东伯雪璎：主角，沉稳坚韧。", updated_at: now },
  { novel_id: novel.id, kind: "地点", content: "雪鹰领：故事初期主要地点。", updated_at: now }
];

type BrowserNameMappingAsset = {
  version: number;
  protagonist?: { source: string; target: string } | null;
  names: Array<{ source: string; target: string }>;
  managed_sources?: string[];
  legacy_unmanaged_sources?: string[];
};

function browserConfiguredNameSources(settingsValue: NovelSettings): string[] {
  const sources = [
    settingsValue.protagonist_name,
    ...settingsValue.protagonist_aliases.split(/\r?\n/),
    ...settingsValue.additional_feminize_names.split(/\r?\n/)
  ]
    .map((entry) => entry.split(/->|=>|→/, 1)[0].trim())
    .filter(Boolean);
  return [...new Set(sources)];
}

function browserNameMappingAsset(): BrowserNameMappingAsset {
  const content = canonAssets.find((asset) => asset.kind === "姓名映射表")?.content ?? "";
  try {
    const parsed = JSON.parse(content) as BrowserNameMappingAsset;
    return {
      version: Number(parsed.version || 1),
      protagonist: parsed.protagonist ?? null,
      names: Array.isArray(parsed.names) ? parsed.names : [],
      managed_sources: Array.isArray(parsed.managed_sources) ? parsed.managed_sources : [],
      legacy_unmanaged_sources: Array.isArray(parsed.legacy_unmanaged_sources)
        ? parsed.legacy_unmanaged_sources
        : []
    };
  } catch {
    return { version: 1, protagonist: null, names: [], managed_sources: [] };
  }
}

function setBrowserNameMappingAsset(asset: BrowserNameMappingAsset): void {
  const index = canonAssets.findIndex((item) => item.kind === "姓名映射表");
  const next: CanonAsset = {
    novel_id: novel.id,
    kind: "姓名映射表",
    content: JSON.stringify(asset, null, 2),
    updated_at: now
  };
  if (index >= 0) canonAssets[index] = next;
  else canonAssets.unshift(next);
}

function browserNameMappingReport(settingsValue = novelSettings): NameMappingConsistencyReport {
  const asset = browserNameMappingAsset();
  const configured = new Set(browserConfiguredNameSources(settingsValue));
  const managedSources = new Set(asset.version >= 2 ? asset.managed_sources ?? [] : configured);
  const legacySources = new Set(
    asset.version >= 2
      ? asset.legacy_unmanaged_sources ?? []
      : asset.names.filter((entry) => !managedSources.has(entry.source)).map((entry) => entry.source)
  );
  const managed = asset.names.filter((entry) => managedSources.has(entry.source));
  const manual = asset.version >= 2
    ? asset.names.filter((entry) => !managedSources.has(entry.source))
    : [];
  const legacy_unmanaged = asset.names.filter((entry) => legacySources.has(entry.source));
  return {
    managed,
    manual,
    legacy_unmanaged,
    needs_resolution: legacy_unmanaged.length > 0
  };
}

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

function logDateKey(createdAt: string) {
  const date = new Date(createdAt);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function recentLogDays(novelId?: string | null): AiLogDaySummary[] {
  const today = new Date();
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(today);
    date.setDate(today.getDate() - index);
    const key = logDateKey(date.toISOString());
    return {
      date: key,
      count: logs.filter(
        (log) => logDateKey(log.created_at) === key
          && (!novelId || !log.novel_id || log.novel_id === novelId)
      ).length
    };
  });
}

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

function mockChapterRulePreview(splitLongChapters = false): ChapterRulePreview {
  const baseTitles = [
    "第一章 雪鹰领",
    "第二章 超凡",
    "第三章 离离",
    "第四章 姐弟",
    "第五章 枪法",
    "第六章 修炼"
  ];
  const previewTitles = splitLongChapters
    ? baseTitles.flatMap((title, index) => index === 0 ? [`${title}（1）`, `${title}（2）`] : [title])
    : baseTitles;
  return {
    total_chapters: previewTitles.length,
    chapters: previewTitles.map((title, index) => ({ index: index + 1, title })),
    long_chapters: splitLongChapters
      ? []
      : [{ index: 1, title: baseTitles[0], char_count: 5200 }],
    can_apply: true,
    message: "浏览器测试模式：预览已生成。"
  };
}

function applyMockSplit(rule?: ChapterRule, splitLongChapters = false) {
  if (rule) {
    chapterRule = { novel_id: novel.id, rule, updated_at: now };
  }
  novel.status = "imported";
  const titles = splitLongChapters
    ? chapterTitles.flatMap((title, index) => index === 0 ? [`${title}（1）`, `${title}（2）`] : [title])
    : chapterTitles;
  chapters = titles.map((title, offset) => {
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

function createRewriteAbRun(profileIds: string[], reviewEnabled: boolean, batchId: string): RewriteAbRunDetail {
  const batch = detail().batches.find((item) => item.id === batchId) ?? detail().batches[0];
  const scopedChapters = chapters.filter((chapter) => chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter);
  const models = profileIds.map((profileId, index) => {
    const profile = profiles.find((item) => item.id === profileId)!;
    return {
      slot: String.fromCharCode(65 + index) as RewriteAbSlot,
      profile_id: profile.id,
      profile_name: profile.name,
      provider: profile.provider,
      model: profile.model
    };
  });
  const runId = `browser-ab-${Date.now()}`;
  const run: RewriteAbRunDetail = {
    id: runId,
    novel_id: novel.id,
    batch_id: batch.id,
    batch_label: batch.label,
    batch_fingerprint: scopedChapters.map((chapter) => chapter.id).join(":"),
    status: "ready",
    review_enabled: reviewEnabled,
    model_count: models.length,
    chapter_count: scopedChapters.length,
    completed_candidates: scopedChapters.length * models.length,
    total_candidates: scopedChapters.length * models.length,
    selected_chapters: 0,
    created_at: now,
    updated_at: now,
    models,
    chapters: scopedChapters.map((chapter) => ({
      chapter_id: chapter.id,
      chapter_index: chapter.index,
      title: chapter.title,
      selected_slot: null,
      candidate_statuses: Object.fromEntries(models.map((model) => [model.slot, "completed"]))
    }))
  };
  for (const chapter of scopedChapters) {
    rewriteAbChapterDetails.set(`${runId}:${chapter.id}`, {
      run_id: runId,
      chapter_id: chapter.id,
      chapter_index: chapter.index,
      original_title: chapter.title,
      original_text: chapter.original_text,
      baseline_title: chapter.title,
      baseline_rewrite_text: chapter.rewrite_text,
      selected_slot: null,
      candidates: models.map((model) => ({
        slot: model.slot,
        profile_id: model.profile_id,
        profile_name: model.profile_name,
        model: model.model,
        status: "completed",
        title: chapter.title,
        rewrite_text: `${model.slot} 候选：这是第${chapter.index}章由${model.profile_name}生成的浏览器测试改写稿。东伯雪璎与余靖秋共同推进剧情。`,
        review_summary: reviewEnabled ? "浏览器模拟复检通过。" : null,
        error: null
      }))
    });
  }
  return run;
}

function cloneRewriteAbRun(run: RewriteAbRunDetail): RewriteAbRunDetail {
  return structuredClone(run);
}

export async function invokeBrowserMock(
  command: string,
  args?: Record<string, unknown>
): Promise<unknown> {
  switch (command) {
    case "list_novels":
      return localDataDeleted ? [] : [{ ...novel }];
    case "get_novel_detail":
      if (localDataDeleted) throw new Error("浏览器测试数据已删除。");
      return detail();
    case "get_novel_batch_update": {
      if (localDataDeleted) throw new Error("浏览器测试数据已删除。");
      const batch = detail().batches.find(
        (item) => item.id === args?.batchId && item.novel_id === args?.novelId
      );
      if (!batch) throw new Error("未找到需要刷新的小说批次。");
      return {
        novel_id: novel.id,
        batch_id: batch.id,
        batch_index: batch.batch_index,
        chapters: chapters
          .filter((chapter) => chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter)
          .map((chapter) => ({ ...chapter })),
        canon_assets: canonAssets.map((asset) => ({ ...asset }))
      };
    }
    case "get_chapter_rule":
      return chapterRule ? { ...chapterRule, rule: { ...chapterRule.rule } } : null;
    case "preview_chapter_rule":
      return mockChapterRulePreview(Boolean(args?.splitLongChapters));
    case "save_chapter_rule_and_split": {
      const rule = args?.rule as ChapterRule;
      applyMockSplit(rule, Boolean(args?.splitLongChapters));
      return { ...chapterRule!, rule: { ...chapterRule!.rule } };
    }
    case "split_novel_with_builtin_rule":
      applyMockSplit(undefined, Boolean(args?.splitLongChapters));
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
    case "set_auto_continue_enabled":
      settings = { ...settings, auto_continue_enabled: Boolean(args?.enabled) };
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
    case "delete_local_data":
      if (String(args?.confirmationPhrase ?? "").trim() !== "删除全部本地数据") {
        throw new Error("确认短语不正确，本地数据未删除。");
      }
      localDataDeleted = true;
      chapters = [];
      profiles = [];
      logs = [];
      settings = {
        export_dir: null,
        core_prompt: "",
        review_enabled: true,
        review_profile_id: null,
        analysis_profile_id: null,
        selected_profile_id: null,
        chapter_batch_size: 30,
        rewrite_parallelism: 10,
        auto_continue_enabled: false
      };
      canonAssets = [];
      chapterRule = null;
      rewriteAbRuns = [];
      rewriteAbChapterDetails.clear();
      return { warnings: [] };
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
    case "list_ai_log_days":
      return recentLogDays(typeof args?.novelId === "string" ? args.novelId : null);
    case "list_ai_log_summaries_by_date": {
      const cursor = args?.cursor as AiLogCursor | null | undefined;
      const limit = Number(args?.limit ?? 50);
      if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
        throw new Error("日志分页数量必须在 1 到 100 之间。");
      }
      const novelId = typeof args?.novelId === "string" ? args.novelId : null;
      const allMatches = logs
        .filter((log) => logDateKey(log.created_at) === args?.date)
        .filter((log) => !novelId || !log.novel_id || log.novel_id === novelId)
        .sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at) || right.id.localeCompare(left.id));
      const cursorTime = cursor ? Date.parse(cursor.created_at) : 0;
      const matches = allMatches.filter((log) => {
        if (!cursor) return true;
        const createdAt = Date.parse(log.created_at);
        return createdAt < cursorTime || (createdAt === cursorTime && log.id < cursor.id);
      });
      const pageItems = matches.slice(0, limit);
      const hasMore = matches.length > limit;
      return {
        items: pageItems.map((log) => ({
          id: log.id,
          novel_id: log.novel_id,
          profile_id: log.profile_id,
          action: log.action,
          chapter_title: log.chapter_title,
          status: log.status,
          preview: (log.content || log.reasoning || log.raw_response || "无正文内容。").slice(0, 180),
          finish_reason: log.finish_reason,
          created_at: log.created_at
        })),
        next_cursor: hasMore && pageItems.length > 0
          ? { created_at: pageItems[pageItems.length - 1].created_at, id: pageItems[pageItems.length - 1].id }
          : null,
        total: allMatches.length
      } satisfies AiLogSummaryPage;
    }
    case "get_ai_log_detail": {
      const detail = logs.find((log) => log.id === args?.logId);
      if (!detail) throw new Error("未找到 AI 日志详情。");
      return { ...detail };
    }
    case "list_ai_logs_by_date":
      return logs
        .filter((log) => logDateKey(log.created_at) === args?.date)
        .map((log) => ({ ...log }));
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
    case "delete_token_usage_for_model":
      return 1;
    case "save_novel_settings":
      {
      const previousSettings = novelSettings;
      const nextSettings: NovelSettings = {
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
      const mapping = browserNameMappingAsset();
      const previousManaged = new Set(
        mapping.version >= 2
          ? mapping.managed_sources ?? []
          : browserConfiguredNameSources(previousSettings)
      );
      const nextManaged = browserConfiguredNameSources(nextSettings);
      const nextManagedSet = new Set(nextManaged);
      const legacySources = new Set(
        mapping.version >= 2
          ? mapping.legacy_unmanaged_sources ?? []
          : mapping.names
              .filter((entry) => !previousManaged.has(entry.source))
              .map((entry) => entry.source)
      );
      for (const source of nextManaged) legacySources.delete(source);
      const names = mapping.names.filter(
        (entry) => !previousManaged.has(entry.source) || nextManagedSet.has(entry.source)
      );
      const upsert = (source: string, target: string) => {
        if (!source || !target) return;
        const existing = names.find((entry) => entry.source === source);
        if (existing) existing.target = target;
        else names.push({ source, target });
      };
      upsert(nextSettings.protagonist_name, nextSettings.rewritten_protagonist_name);
      for (const line of [
        ...nextSettings.protagonist_aliases.split(/\r?\n/),
        ...nextSettings.additional_feminize_names.split(/\r?\n/)
      ]) {
        const [source, target] = line.split(/->|=>|→/).map((value) => value.trim());
        if (target) upsert(source, target);
      }
      novelSettings = nextSettings;
      const protagonist = names.find((entry) => entry.source === nextSettings.protagonist_name) ?? null;
      setBrowserNameMappingAsset({
        version: 2,
        protagonist,
        names,
        managed_sources: nextManaged,
        legacy_unmanaged_sources: [...legacySources]
      });
      return { ...novelSettings };
      }
    case "estimate_job_cost":
      return estimate();
    case "estimate_rewrite_ab": {
      const modelCount = (args?.profileIds as string[]).length;
      const batch = detail().batches.find((item) => item.id === args?.batchId) ?? detail().batches[0];
      const scopedChapters = chapters.filter((chapter) => chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter);
      const chapterCount = scopedChapters.length;
      const fingerprint = scopedChapters.map((chapter) => chapter.id).join(":");
      const existingRun = rewriteAbRuns.find((run) => run.novel_id === novel.id && run.batch_fingerprint === fingerprint);
      const parallelism = Math.max(1, settings.rewrite_parallelism ?? 10);
      const shardCount = Math.min(chapterCount, parallelism);
      const estimatedRequests = shardCount * modelCount * (args?.reviewEnabled ? 2 : 1);
      return {
        chapter_count: chapterCount,
        model_count: modelCount,
        shard_count: shardCount,
        estimated_requests: estimatedRequests,
        estimated_seconds: Math.ceil(estimatedRequests / parallelism) * 12,
        average_call_seconds: 12,
        recent_success_calls: 18,
        existing_run_id: existingRun?.id ?? null
      } satisfies RewriteAbEstimate;
    }
    case "start_rewrite_ab": {
      const profileIds = args?.profileIds as string[];
      const replaceRunId = String(args?.replaceRunId ?? "");
      if (replaceRunId) {
        rewriteAbRuns = rewriteAbRuns.filter((run) => run.id !== replaceRunId);
        for (const key of rewriteAbChapterDetails.keys()) if (key.startsWith(`${replaceRunId}:`)) rewriteAbChapterDetails.delete(key);
      }
      const run = createRewriteAbRun(profileIds, Boolean(args?.reviewEnabled), String(args?.batchId));
      rewriteAbRuns.unshift(run);
      return cloneRewriteAbRun(run);
    }
    case "list_rewrite_ab_runs":
      return rewriteAbRuns.map(({ models: _models, chapters: _chapters, ...summary }) => ({ ...summary }));
    case "get_rewrite_ab_run": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      return cloneRewriteAbRun(run);
    }
    case "get_rewrite_ab_chapter": {
      const value = rewriteAbChapterDetails.get(`${args?.runId}:${args?.chapterId}`);
      if (!value) throw new Error("A/B 候选章节不存在。");
      return structuredClone(value);
    }
    case "save_rewrite_ab_choices": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      if (args?.replaceAll) {
        for (const summary of run.chapters) summary.selected_slot = null;
        for (const summary of run.chapters) {
          const chapterDetail = rewriteAbChapterDetails.get(`${run.id}:${summary.chapter_id}`);
          if (chapterDetail) chapterDetail.selected_slot = null;
        }
      }
      for (const choice of args?.choices as RewriteAbChoice[]) {
        const summary = run.chapters.find((item) => item.chapter_id === choice.chapter_id);
        const chapterDetail = rewriteAbChapterDetails.get(`${run.id}:${choice.chapter_id}`);
        if (summary) summary.selected_slot = choice.slot;
        if (chapterDetail) chapterDetail.selected_slot = choice.slot;
      }
      run.selected_chapters = run.chapters.filter((item) => item.selected_slot).length;
      run.updated_at = now;
      return cloneRewriteAbRun(run);
    }
    case "retry_rewrite_ab": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      run.status = "ready";
      run.completed_candidates = run.total_candidates;
      return cloneRewriteAbRun(run);
    }
    case "terminate_rewrite_ab": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      run.status = "partial";
      return cloneRewriteAbRun(run);
    }
    case "apply_rewrite_ab_choices": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      if (run.chapters.some((item) => !item.selected_slot)) throw new Error("请先为每章选择一个候选。");
      const updated = run.chapters.map((item) => {
        const detailRow = rewriteAbChapterDetails.get(`${run.id}:${item.chapter_id}`)!;
        const candidate = detailRow.candidates.find((value) => value.slot === item.selected_slot)!;
        return updateChapter(item.chapter_id, { title: candidate.title ?? item.title, rewrite_text: candidate.rewrite_text, rewrite_status: "completed", rewrite_edited: false });
      });
      run.status = "applied";
      return { status: "applied", conflict_chapter_ids: [], chapters: updated } satisfies RewriteAbApplyResult;
    }
    case "restore_rewrite_ab_baseline": {
      const run = rewriteAbRuns.find((item) => item.id === args?.runId);
      if (!run) throw new Error("A/B 实验不存在。");
      const updated = run.chapters.map((item) => {
        const detailRow = rewriteAbChapterDetails.get(`${run.id}:${item.chapter_id}`)!;
        return updateChapter(item.chapter_id, { title: detailRow.baseline_title ?? item.title, rewrite_text: detailRow.baseline_rewrite_text, rewrite_status: detailRow.baseline_rewrite_text ? "completed" : "pending", rewrite_edited: false });
      });
      run.status = "ready";
      return { status: "restored", conflict_chapter_ids: [], chapters: updated } satisfies RewriteAbApplyResult;
    }
    case "delete_rewrite_ab_run": {
      const runId = String(args?.runId);
      rewriteAbRuns = rewriteAbRuns.filter((run) => run.id !== runId);
      for (const key of rewriteAbChapterDetails.keys()) if (key.startsWith(`${runId}:`)) rewriteAbChapterDetails.delete(key);
      return undefined;
    }
    case "update_canon_assets":
      canonAssets = (args?.assets as Array<{ kind: string; content: string }>).map((asset) => ({
        novel_id: novel.id,
        kind: asset.kind,
        content: asset.content,
        updated_at: now
      }));
      return canonAssets.map((asset) => ({ ...asset }));
    case "inspect_name_mapping_consistency":
      return browserNameMappingReport();
    case "resolve_name_mapping_consistency": {
      const report = browserNameMappingReport();
      const legacy = new Set(report.legacy_unmanaged.map((entry) => entry.source));
      const remove = new Set((args?.removeSources as string[] | undefined) ?? []);
      const keep = new Set((args?.keepAsManualSources as string[] | undefined) ?? []);
      for (const source of [...remove, ...keep]) {
        if (!legacy.has(source)) throw new Error("只能处理检查结果中列出的旧版未归属映射。");
      }
      if ([...legacy].some((source) => !remove.has(source) && !keep.has(source))) {
        throw new Error("请为每一条旧版未归属映射选择删除或保留。");
      }
      const asset = browserNameMappingAsset();
      const names = asset.names.filter((entry) => !remove.has(entry.source));
      setBrowserNameMappingAsset({
        version: 2,
        protagonist: names.find((entry) => entry.source === novelSettings.protagonist_name) ?? null,
        names,
        managed_sources: browserConfiguredNameSources(novelSettings),
        legacy_unmanaged_sources: []
      });
      return browserNameMappingReport();
    }
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
      localDataDeleted = false;
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
