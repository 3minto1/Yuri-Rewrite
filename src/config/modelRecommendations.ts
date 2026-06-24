import type { ProfileDraft } from "../types";

type ModelSuggestion = {
  label: string;
  model: string;
};

type ModelSuggestionGroup = {
  id: string;
  baseTerms: string[];
  modelTerms: string[];
  models: ModelSuggestion[];
  openaiBaseUrl?: string;
  anthropicBaseUrl?: string;
};

export type ThinkingModeSupport = {
  disabledModes: Array<"off" | "on">;
  guidance: string;
};

export const emptyProfile: ProfileDraft = {
  name: "OpenAI 兼容接口",
  provider: "openai-compatible",
  base_url: "https://api.openai.com/v1",
  model: "请填写模型名",
  temperature: 0.7,
  top_p: 1,
  thinking_mode: "auto",
  prompt_obfuscation_enabled: false,
  api_key: ""
};

const groups: ModelSuggestionGroup[] = [
  {
    id: "deepseek",
    baseTerms: ["deepseek"],
    modelTerms: ["deepseek"],
    openaiBaseUrl: "https://api.deepseek.com",
    anthropicBaseUrl: "https://api.deepseek.com/anthropic",
    models: [
      { label: "DeepSeek V4 Pro", model: "deepseek-v4-pro" },
      { label: "DeepSeek V4 Flash", model: "deepseek-v4-flash" }
    ]
  },
  {
    id: "volcengine",
    baseTerms: ["volcengine", "volces", "ark.cn-"],
    modelTerms: ["doubao-", "seed-"],
    openaiBaseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
    anthropicBaseUrl: "https://ark.cn-beijing.volces.com/api/coding",
    models: [
      { label: "Doubao Seed 2.1 Pro", model: "doubao-seed-2-1-pro-260628" },
      { label: "Doubao Seed 2.1 Turbo", model: "doubao-seed-2-1-turbo-260628" },
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
    id: "openai",
    baseTerms: ["api.openai.com", "openai.azure.com"],
    modelTerms: ["gpt-", "o3", "o4"],
    openaiBaseUrl: "https://api.openai.com/v1",
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
    id: "zhipu",
    baseTerms: ["bigmodel", "zhipu", "z.ai", "智谱"],
    modelTerms: ["glm-"],
    openaiBaseUrl: "https://open.bigmodel.cn/api/paas/v4",
    anthropicBaseUrl: "https://open.bigmodel.cn/api/anthropic",
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
    id: "kimi",
    baseTerms: ["moonshot", "kimi"],
    modelTerms: ["moonshot", "kimi"],
    openaiBaseUrl: "https://api.moonshot.cn/v1",
    anthropicBaseUrl: "https://api.moonshot.cn/anthropic",
    models: [
      { label: "Kimi K2.6", model: "kimi-k2.6" },
      { label: "Kimi K2.5", model: "kimi-k2.5" },
      { label: "Moonshot V1 128K", model: "moonshot-v1-128k" },
      { label: "Moonshot V1 32K", model: "moonshot-v1-32k" },
      { label: "Moonshot V1 8K", model: "moonshot-v1-8k" }
    ]
  },
  {
    id: "minimax",
    baseTerms: ["minimax"],
    modelTerms: ["minimax", "m2-her"],
    openaiBaseUrl: "https://api.minimaxi.com/v1",
    anthropicBaseUrl: "https://api.minimaxi.com/anthropic",
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
    id: "mimo",
    baseTerms: ["xiaomimimo", "mimo.xiaomi", "mimo.mi.com", "mimo"],
    modelTerms: ["mimo-"],
    openaiBaseUrl: "https://api.xiaomimimo.com/v1",
    anthropicBaseUrl: "https://api.xiaomimimo.com/anthropic",
    models: [
      { label: "MiMo V2.5 Pro", model: "mimo-v2.5-pro" },
      { label: "MiMo V2.5", model: "mimo-v2.5" },
      { label: "MiMo V2 Flash", model: "mimo-v2-flash" }
    ]
  },
  {
    id: "siliconflow",
    baseTerms: ["siliconflow"],
    modelTerms: ["qwen/", "thudm/", "deepseek-ai/", "moonshotai/", "minimaxai/", "zai-org/", "bytedance-seed/", "internlm/", "mistralai/", "openai/"],
    openaiBaseUrl: "https://api.siliconflow.cn/v1",
    anthropicBaseUrl: "https://api.siliconflow.cn",
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
    id: "claude",
    baseTerms: ["anthropic", "claude"],
    modelTerms: ["claude-"],
    anthropicBaseUrl: "https://api.anthropic.com",
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

function getSuggestionGroup(profile: ProfileDraft): ModelSuggestionGroup | undefined {
  const baseHint = profile.base_url.toLowerCase();
  const modelHint = profile.model.toLowerCase();
  return groups.find((group) =>
    group.baseTerms.some((term) => baseHint.includes(term))
  ) ?? groups.find((group) =>
    group.modelTerms.some((term) => modelHint.includes(term))
  );
}

export function getProviderBaseUrl(
  profile: ProfileDraft,
  provider: string
): string {
  if (provider === "gemini") {
    return "https://generativelanguage.googleapis.com/v1beta";
  }
  const group = getSuggestionGroup(profile);
  if (provider === "anthropic") {
    if (group?.id === "volcengine") {
      try {
        return `${new URL(profile.base_url).origin}/api/coding`;
      } catch {
        return group.anthropicBaseUrl ?? "https://api.anthropic.com";
      }
    }
    return group?.anthropicBaseUrl ?? "https://api.anthropic.com";
  }
  if (provider === "openai-compatible") {
    if (group?.id === "volcengine") {
      try {
        return `${new URL(profile.base_url).origin}/api/coding/v3`;
      } catch {
        return group.openaiBaseUrl ?? "https://api.openai.com/v1";
      }
    }
    return group?.openaiBaseUrl ?? "https://api.openai.com/v1";
  }
  return profile.base_url;
}

function includesAny(value: string, terms: string[]) {
  return terms.some((term) => value.includes(term));
}

function isSiliconFlowToggleModel(model: string) {
  return [
    "deepseek-ai/deepseek-v3.2",
    "deepseek-ai/deepseek-v3.1-terminus",
    "qwen/qwen3.5-122b-a10b",
    "qwen/qwen3.5-35b-a3b",
    "qwen/qwen3.5-27b"
  ].includes(model);
}

export function getThinkingModeSupport(profile: ProfileDraft): ThinkingModeSupport {
  const base = profile.base_url.trim().toLowerCase();
  const model = profile.model.trim().toLowerCase();
  const provider = profile.provider.trim().toLowerCase();

  if (provider === "gemini") {
    if (model.includes("2.5-pro")) {
      return {
        disabledModes: [],
        guidance: "Gemini 2.5 Pro 始终会思考。自动使用模型默认动态预算；“关闭”会降到官方允许的最低预算 128，不能完全关闭；“开启”使用动态思考。"
      };
    }
    if (model.includes("2.5")) {
      return {
        disabledModes: [],
        guidance: "Gemini 2.5 使用 thinkingBudget。自动不附加参数；支持关闭思考和开启动态思考，但具体可用范围仍取决于所选 2.5 型号。"
      };
    }
    if (model.includes("gemini-3")) {
      return {
        disabledModes: [],
        guidance: "Gemini 3 使用 thinkingLevel。自动采用模型默认级别；“关闭”会改为最低思考级别，不能保证完全不思考；“开启”使用 high。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "当前 Gemini 型号未确认支持可控思考参数。请选择“自动”，程序不会额外发送 thinkingConfig。"
    };
  }

  if (base.includes("siliconflow")) {
    if (isSiliconFlowToggleModel(model)) {
      return {
        disabledModes: [],
        guidance: "SiliconFlow 官方为该模型提供 enable_thinking 开关。自动不附加参数；关闭或开启会发送对应布尔值。"
      };
    }
    if (includesAny(model, ["deepseek-r1", "minimax", "kimi-k2", "gpt-oss"])) {
      return {
        disabledModes: ["off", "on"],
        guidance: "该 SiliconFlow 模型会自行决定或固定使用推理，官方未为此推荐型号提供可靠的开关。请选择“自动”。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "SiliconFlow 只对部分模型提供 enable_thinking。当前型号不在已确认支持列表中，请使用“自动”。"
    };
  }

  if (includesAny(base, ["api.deepseek.com"]) || model.startsWith("deepseek-v4")) {
    return {
      disabledModes: [],
      guidance: "DeepSeek V4 支持 Thinking / Non-Thinking 双模式。自动使用服务商默认行为；关闭或开启会发送官方 thinking.type 参数。"
    };
  }

  if (includesAny(base, ["volcengine", "volces", "ark.cn-"])) {
    if (/doubao-seed-2-[01](?:-|$)/.test(model.replace(/\./g, "-"))) {
      return {
        disabledModes: [],
        guidance: "豆包 Seed 2.0 / 2.1 支持通过 thinking.type 开启或关闭深度思考。自动不覆盖接入点或模型的默认设置。"
      };
    }
    if (model.includes("thinking")) {
      return {
        disabledModes: ["off", "on"],
        guidance: "该旧版豆包 Thinking 型号并非可切换双模式模型。为避免发送不兼容参数，请使用“自动”。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "当前豆包型号未提供可控思考开关。请选择“自动”，程序不会附加 thinking 参数。"
    };
  }

  if (includesAny(base, ["bigmodel", "zhipu", "z.ai"]) || model.startsWith("glm-")) {
    if (/^glm-(?:[5-9]|4\.(?:[5-9]|[1-9]\d))/.test(model)) {
      return {
        disabledModes: [],
        guidance: "GLM 4.5 及以上支持 thinking.type 开关。自动保留模型默认行为；关闭或开启会发送官方参数。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "该 GLM 型号早于 4.5，未确认支持 thinking.type。请选择“自动”。"
    };
  }

  if (includesAny(base, ["moonshot", "kimi"]) || model.startsWith("kimi-")) {
    if (model.startsWith("kimi-k2.5") || model.startsWith("kimi-k2.6")) {
      return {
        disabledModes: [],
        guidance: "Kimi K2.5 / K2.6 支持 thinking.type 开关。自动采用默认开启；关闭或开启会显式发送官方参数。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "Moonshot V1 等当前型号不支持本应用的思考模式开关。请选择“自动”。"
    };
  }

  if (base.includes("minimax") || model.includes("minimax") || model.startsWith("m2-")) {
    if (model.includes("minimax-m3")) {
      return {
        disabledModes: [],
        guidance: "MiniMax M3 支持关闭或 Adaptive Thinking。自动使用服务商默认开启；开启会发送 adaptive，关闭会发送 disabled。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "MiniMax M2.x 的 thinking 无法关闭，发送 disabled 也不会生效。请选择“自动”，由模型固定启用思考。"
    };
  }

  if (includesAny(base, ["xiaomimimo", "mimo.mi.com", "mimo"]) || model.startsWith("mimo-")) {
    return {
      disabledModes: [],
      guidance: "小米 MiMo 的推荐型号支持 thinking.type 开关。自动使用模型默认行为；关闭或开启会发送官方参数。"
    };
  }

  if (base.includes("api.openai.com") || base.includes("openai.azure.com")) {
    if (/^(?:gpt-5|o[134])/.test(model)) {
      return {
        disabledModes: [],
        guidance: "OpenAI 推理模型支持 reasoning_effort。自动使用模型默认推理强度；关闭使用 none，开启使用 medium；个别型号不接受某档位时会自动移除参数重试。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "GPT-4.1、GPT-4o 等非推理型号不支持 reasoning_effort。请选择“自动”。"
    };
  }

  if (provider === "anthropic" && model.startsWith("claude-")) {
    if (
      model.startsWith("claude-opus-4-8")
      || model.startsWith("claude-opus-4-7")
      || model.startsWith("claude-opus-4-6")
      || model.startsWith("claude-sonnet-4-6")
    ) {
      return {
        disabledModes: [],
        guidance: "Claude 原生 Messages API 支持 Adaptive Thinking。自动不附加参数；关闭不启用思考；开启发送 adaptive thinking 和 high effort。"
      };
    }
    if (model.startsWith("claude-haiku-4-5")) {
      return {
        disabledModes: [],
        guidance: "Claude Haiku 4.5 支持 Extended Thinking。自动或关闭不附加思考参数；开启会使用受限思考预算。"
      };
    }
    return {
      disabledModes: ["off", "on"],
      guidance: "当前 Claude 型号未确认支持本应用的思考参数，请使用“自动”。"
    };
  }

  if (base.includes("anthropic") || model.startsWith("claude-")) {
    return {
      disabledModes: ["off", "on"],
      guidance: "当前配置不是 Anthropic Messages Provider。通过 OpenAI 兼容转发调用 Claude 时，请使用“自动”，避免发送不兼容的原生思考参数。"
    };
  }

  return {
    disabledModes: ["off", "on"],
    guidance: "当前兼容接口未确认支持哪种思考参数。建议使用“自动”；程序不会额外发送参数，避免接口兼容错误。"
  };
}

export function normalizeThinkingMode(profile: ProfileDraft): ProfileDraft {
  const support = getThinkingModeSupport(profile);
  if (
    profile.thinking_mode !== "auto"
    && support.disabledModes.includes(profile.thinking_mode)
  ) {
    return { ...profile, thinking_mode: "auto" };
  }
  return profile;
}
