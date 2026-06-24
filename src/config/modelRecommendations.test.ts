import { describe, expect, it } from "vitest";
import {
  emptyProfile,
  getModelSuggestions,
  getProviderBaseUrl,
  getThinkingModeSupport,
  normalizeThinkingMode
} from "./modelRecommendations";

describe("model recommendations", () => {
  it("uses stable novel-rewrite sampling defaults", () => {
    expect(emptyProfile.temperature).toBe(0.7);
    expect(emptyProfile.top_p).toBe(1);
  });

  it("prefers provider URL matching before model-name matching", () => {
    const suggestions = getModelSuggestions({
      ...emptyProfile,
      base_url: "https://api.deepseek.com/v1",
      model: "gpt-5"
    });
    expect(suggestions[0]?.model).toBe("deepseek-v4-pro");
  });

  it("falls back to model-name matching for compatible proxies", () => {
    const suggestions = getModelSuggestions({
      ...emptyProfile,
      base_url: "https://proxy.example.com/v1",
      model: "glm-5"
    });
    expect(suggestions.some((item) => item.model === "glm-5")).toBe(true);
  });

  it("includes the official Doubao Seed 2.1 models", () => {
    const suggestions = getModelSuggestions({
      ...emptyProfile,
      base_url: "https://ark.cn-beijing.volces.com/api/v3",
      model: ""
    });
    expect(suggestions.slice(0, 2)).toEqual([
      { label: "Doubao Seed 2.1 Pro", model: "doubao-seed-2-1-pro-260628" },
      { label: "Doubao Seed 2.1 Turbo", model: "doubao-seed-2-1-turbo-260628" }
    ]);
  });

  it("maps supported services between OpenAI and Anthropic endpoints", () => {
    const anthropicEndpoints = [
      ["https://api.deepseek.com", "deepseek-v4-pro", "https://api.deepseek.com/anthropic"],
      ["https://open.bigmodel.cn/api/paas/v4", "glm-5.2", "https://open.bigmodel.cn/api/anthropic"],
      ["https://api.moonshot.cn/v1", "kimi-k2.6", "https://api.moonshot.cn/anthropic"],
      ["https://api.minimaxi.com/v1", "MiniMax-M3", "https://api.minimaxi.com/anthropic"],
      ["https://api.xiaomimimo.com/v1", "mimo-v2.5-pro", "https://api.xiaomimimo.com/anthropic"],
      ["https://api.siliconflow.cn/v1", "Qwen/Qwen3.5-27B", "https://api.siliconflow.cn"],
      ["https://api.anthropic.com", "claude-opus-4-8", "https://api.anthropic.com"]
    ];
    for (const [base_url, model, expected] of anthropicEndpoints) {
      expect(getProviderBaseUrl({
        ...emptyProfile,
        base_url,
        model
      }, "anthropic")).toBe(expected);
    }
    expect(getProviderBaseUrl({
      ...emptyProfile,
      provider: "anthropic",
      base_url: "https://open.bigmodel.cn/api/anthropic",
      model: "glm-5.2"
    }, "openai-compatible")).toBe("https://open.bigmodel.cn/api/paas/v4");
    expect(getProviderBaseUrl({
      ...emptyProfile,
      base_url: "https://ark.cn-shanghai.volces.com/api/coding/v3",
      model: "doubao-seed-2-1-pro-260628"
    }, "anthropic")).toBe("https://ark.cn-shanghai.volces.com/api/coding");
    expect(getProviderBaseUrl({
      ...emptyProfile,
      base_url: "https://api.openai.com/v1",
      model: "gpt-5.2"
    }, "anthropic")).toBe("https://api.anthropic.com");
  });

  it("reports provider-specific thinking controls", () => {
    expect(getThinkingModeSupport({
      ...emptyProfile,
      base_url: "https://api.deepseek.com/v1",
      model: "deepseek-v4-pro"
    }).disabledModes).toEqual([]);
    expect(getThinkingModeSupport({
      ...emptyProfile,
      base_url: "https://api.minimax.io/v1",
      model: "MiniMax-M2.7"
    }).disabledModes).toEqual(["off", "on"]);
    expect(getThinkingModeSupport({
      ...emptyProfile,
      provider: "gemini",
      model: "gemini-3.1-pro-preview"
    }).guidance).toMatch(/最低思考级别/);
    expect(getThinkingModeSupport({
      ...emptyProfile,
      base_url: "https://ark.cn-beijing.volces.com/api/v3",
      model: "doubao-seed-2-1-pro-260628"
    }).disabledModes).toEqual([]);
    expect(getThinkingModeSupport({
      ...emptyProfile,
      provider: "anthropic",
      base_url: "https://api.anthropic.com",
      model: "claude-opus-4-8"
    }).disabledModes).toEqual([]);
  });

  it("returns unsupported saved modes to automatic", () => {
    expect(normalizeThinkingMode({
      ...emptyProfile,
      model: "gpt-4.1",
      thinking_mode: "on"
    }).thinking_mode).toBe("auto");
    expect(normalizeThinkingMode({
      ...emptyProfile,
      base_url: "https://api.deepseek.com/v1",
      model: "deepseek-v4-pro",
      thinking_mode: "on"
    }).thinking_mode).toBe("on");
  });
});
