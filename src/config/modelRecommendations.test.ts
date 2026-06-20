import { describe, expect, it } from "vitest";
import {
  emptyProfile,
  getModelSuggestions,
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
