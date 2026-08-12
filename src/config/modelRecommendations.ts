import type { ProfileDraft } from "../types";

type ProviderEndpointGroup = {
  id: string;
  baseTerms: string[];
  modelTerms: string[];
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

const groups: ProviderEndpointGroup[] = [
  {
    id: "deepseek",
    baseTerms: ["deepseek"],
    modelTerms: ["deepseek"],
    openaiBaseUrl: "https://api.deepseek.com",
    anthropicBaseUrl: "https://api.deepseek.com/anthropic"
  },
  {
    id: "volcengine",
    baseTerms: ["volcengine", "volces", "ark.cn-"],
    modelTerms: ["doubao-", "seed-"],
    openaiBaseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
    anthropicBaseUrl: "https://ark.cn-beijing.volces.com/api/coding"
  },
  {
    id: "openai",
    baseTerms: ["api.openai.com", "openai.azure.com"],
    modelTerms: ["gpt-", "o3", "o4"],
    openaiBaseUrl: "https://api.openai.com/v1"
  },
  {
    id: "zhipu",
    baseTerms: ["bigmodel", "zhipu", "z.ai", "智谱"],
    modelTerms: ["glm-"],
    openaiBaseUrl: "https://open.bigmodel.cn/api/paas/v4",
    anthropicBaseUrl: "https://open.bigmodel.cn/api/anthropic"
  },
  {
    id: "kimi",
    baseTerms: ["moonshot", "kimi"],
    modelTerms: ["moonshot", "kimi"],
    openaiBaseUrl: "https://api.moonshot.cn/v1",
    anthropicBaseUrl: "https://api.moonshot.cn/anthropic"
  },
  {
    id: "minimax",
    baseTerms: ["minimax"],
    modelTerms: ["minimax", "m2-her"],
    openaiBaseUrl: "https://api.minimaxi.com/v1",
    anthropicBaseUrl: "https://api.minimaxi.com/anthropic"
  },
  {
    id: "mimo",
    baseTerms: ["xiaomimimo", "mimo.xiaomi", "mimo.mi.com", "mimo"],
    modelTerms: ["mimo-"],
    openaiBaseUrl: "https://api.xiaomimimo.com/v1",
    anthropicBaseUrl: "https://api.xiaomimimo.com/anthropic"
  },
  {
    id: "siliconflow",
    baseTerms: ["siliconflow"],
    modelTerms: ["qwen/", "thudm/", "deepseek-ai/", "moonshotai/", "minimaxai/", "zai-org/", "bytedance-seed/", "internlm/", "mistralai/", "openai/"],
    openaiBaseUrl: "https://api.siliconflow.cn/v1",
    anthropicBaseUrl: "https://api.siliconflow.cn"
  },
  {
    id: "claude",
    baseTerms: ["anthropic", "claude"],
    modelTerms: ["claude-"],
    anthropicBaseUrl: "https://api.anthropic.com"
  }
];

function getProviderEndpointGroup(profile: ProfileDraft): ProviderEndpointGroup | undefined {
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
  const group = getProviderEndpointGroup(profile);
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
