import type { ProfileDraft } from "../types";

type ModelSuggestion = {
  label: string;
  model: string;
};

type ModelSuggestionGroup = {
  baseTerms: string[];
  modelTerms: string[];
  models: ModelSuggestion[];
};

export const emptyProfile: ProfileDraft = {
  name: "OpenAI 兼容接口",
  provider: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  model: "请填写模型名",
  temperature: 0.7,
  thinking_mode: "auto",
  api_key: ""
};

export const thinkingModeTooltip =
  "建议自动；分析阶段通常关闭更快\n兼容性：OpenAI 推理模型可控；DeepSeek V4 与 Kimi K2.5 支持 thinking 开关；Gemini 2.5 用 thinkingBudget；SiliconFlow 推理模型用 thinking_budget；Claude 原生 API 支持 extended/adaptive thinking；MiniMax/MiMo/Claude 转发取决于服务商，不支持时会自动降级";

const groups: ModelSuggestionGroup[] = [
  {
    baseTerms: ["deepseek"],
    modelTerms: ["deepseek"],
    models: [
      { label: "DeepSeek V4 Pro", model: "deepseek-v4-pro" },
      { label: "DeepSeek V4 Flash", model: "deepseek-v4-flash" }
    ]
  },
  {
    baseTerms: ["volcengine", "volces", "ark.cn-"],
    modelTerms: ["doubao-", "seed-"],
    models: [
      { label: "Doubao Seed 2.0 Pro", model: "doubao-seed-2-0-pro-260215" },
      { label: "Doubao Seed 2.0 Lite", model: "doubao-seed-2-0-lite-260428" },
      { label: "Doubao Seed 2.0 Mini", model: "doubao-seed-2-0-mini-260428" },
      { label: "Doubao Seed 2.0 Code", model: "doubao-seed-2-0-code-preview-260215" },
      { label: "Doubao 1.5 Pro 32K", model: "doubao-1-5-pro-32k-250115" },
      { label: "Doubao 1.5 Pro 256K", model: "doubao-1-5-pro-256k-250115" },
      { label: "Doubao 1.5 Lite 32K", model: "doubao-1-5-lite-32k-250115" },
      { label: "Doubao 1.5 Thinking Pro", model: "doubao-1-5-thinking-pro-250415" },
      { label: "Doubao 1.5 Vision Pro", model: "doubao-1-5-vision-pro-250328" }
    ]
  },
  {
    baseTerms: ["api.openai.com", "openai.azure.com"],
    modelTerms: ["gpt-", "o3", "o4"],
    models: [
      { label: "GPT-5.2", model: "gpt-5.2" },
      { label: "GPT-5.2 Pro", model: "gpt-5.2-pro" },
      { label: "GPT-5.1", model: "gpt-5.1" },
      { label: "GPT-5", model: "gpt-5" },
      { label: "GPT-5 Mini", model: "gpt-5-mini" },
      { label: "GPT-5 Nano", model: "gpt-5-nano" },
      { label: "o3 Pro", model: "o3-pro" },
      { label: "o3", model: "o3" },
      { label: "GPT-4.1", model: "gpt-4.1" },
      { label: "GPT-4.1 Mini", model: "gpt-4.1-mini" },
      { label: "GPT-4o Mini", model: "gpt-4o-mini" }
    ]
  },
  {
    baseTerms: ["bigmodel", "zhipu", "z.ai", "智谱"],
    modelTerms: ["glm-"],
    models: [
      { label: "GLM-5.2", model: "glm-5.2" },
      { label: "GLM-5.1", model: "glm-5.1" },
      { label: "GLM-5", model: "glm-5" },
      { label: "GLM-5 Turbo", model: "glm-5-turbo" },
      { label: "GLM-4.7", model: "glm-4.7" },
      { label: "GLM-4.6", model: "glm-4.6" },
      { label: "GLM-4.5", model: "glm-4.5" },
      { label: "GLM-4.5 Air", model: "glm-4.5-air" },
      { label: "GLM-4 Plus", model: "glm-4-plus" },
      { label: "GLM-4 Flash", model: "glm-4-flash" }
    ]
  },
  {
    baseTerms: ["moonshot", "kimi"],
    modelTerms: ["moonshot", "kimi"],
    models: [
      { label: "Kimi K2.6", model: "kimi-k2.6" },
      { label: "Kimi K2.5", model: "kimi-k2.5" },
      { label: "Moonshot V1 128K", model: "moonshot-v1-128k" },
      { label: "Moonshot V1 32K", model: "moonshot-v1-32k" },
      { label: "Moonshot V1 8K", model: "moonshot-v1-8k" }
    ]
  },
  {
    baseTerms: ["minimax"],
    modelTerms: ["minimax", "m2-her"],
    models: [
      { label: "MiniMax M3", model: "MiniMax-M3" },
      { label: "MiniMax M2.7", model: "MiniMax-M2.7" },
      { label: "MiniMax M2.7 Highspeed", model: "MiniMax-M2.7-highspeed" },
      { label: "MiniMax M2.5", model: "MiniMax-M2.5" },
      { label: "MiniMax M2.5 Highspeed", model: "MiniMax-M2.5-highspeed" },
      { label: "MiniMax M2.1", model: "MiniMax-M2.1" },
      { label: "MiniMax M2.1 Highspeed", model: "MiniMax-M2.1-highspeed" },
      { label: "MiniMax M2", model: "MiniMax-M2" },
      { label: "M2-her", model: "M2-her" }
    ]
  },
  {
    baseTerms: ["xiaomimimo", "mimo.xiaomi", "mimo.mi.com", "mimo"],
    modelTerms: ["mimo-"],
    models: [
      { label: "MiMo V2.5 Pro", model: "mimo-v2.5-pro" },
      { label: "MiMo V2.5", model: "mimo-v2.5" },
      { label: "MiMo V2 Flash", model: "mimo-v2-flash" }
    ]
  },
  {
    baseTerms: ["siliconflow"],
    modelTerms: ["qwen/", "thudm/", "deepseek-ai/", "moonshotai/", "minimaxai/", "zai-org/", "bytedance-seed/", "internlm/", "mistralai/", "openai/"],
    models: [
      { label: "DeepSeek V3.2", model: "deepseek-ai/DeepSeek-V3.2" },
      { label: "DeepSeek V3.2 Exp", model: "deepseek-ai/DeepSeek-V3.2-Exp" },
      { label: "DeepSeek V3.1 Terminus", model: "deepseek-ai/DeepSeek-V3.1-Terminus" },
      { label: "DeepSeek V3.1", model: "deepseek-ai/DeepSeek-V3.1" },
      { label: "DeepSeek R1", model: "deepseek-ai/DeepSeek-R1" },
      { label: "Qwen3.6 27B", model: "Qwen/Qwen3.6-27B" },
      { label: "Qwen3.5 122B A10B", model: "Qwen/Qwen3.5-122B-A10B" },
      { label: "Qwen3.5 35B A3B", model: "Qwen/Qwen3.5-35B-A3B" },
      { label: "Qwen3.5 27B", model: "Qwen/Qwen3.5-27B" },
      { label: "Qwen3 Coder 480B A35B", model: "Qwen/Qwen3-Coder-480B-A35B-Instruct" },
      { label: "Qwen3 Coder 30B A3B", model: "Qwen/Qwen3-Coder-30B-A3B-Instruct" },
      { label: "Kimi K2.6", model: "moonshotai/Kimi-K2.6" },
      { label: "Kimi K2 Instruct 0905", model: "moonshotai/Kimi-K2-Instruct-0905" },
      { label: "GLM-5.1", model: "zai-org/GLM-5.1" },
      { label: "GLM-4.5 Air", model: "zai-org/GLM-4.5-Air" },
      { label: "MiniMax M2.5", model: "MiniMaxAI/MiniMax-M2.5" },
      { label: "MiniMax M2", model: "MiniMaxAI/MiniMax-M2" },
      { label: "GPT OSS 120B", model: "openai/gpt-oss-120b" },
      { label: "Seed OSS 36B Instruct", model: "ByteDance-Seed/Seed-OSS-36B-Instruct" }
    ]
  },
  {
    baseTerms: ["anthropic", "claude"],
    modelTerms: ["claude-"],
    models: [
      { label: "Claude Opus 4.8", model: "claude-opus-4-8" },
      { label: "Claude Sonnet 4.6", model: "claude-sonnet-4-6" },
      { label: "Claude Haiku 4.5", model: "claude-haiku-4-5-20251001" }
    ]
  }
];

export function getModelSuggestions(profile: ProfileDraft): ModelSuggestion[] {
  const baseHint = profile.base_url.toLowerCase();
  const modelHint = profile.model.toLowerCase();
  const baseMatched = groups.find((group) =>
    group.baseTerms.some((term) => baseHint.includes(term))
  );
  if (baseMatched) return baseMatched.models;
  return groups.find((group) =>
    group.modelTerms.some((term) => modelHint.includes(term))
  )?.models ?? [];
}
