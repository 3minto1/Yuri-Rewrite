import { describe, expect, it } from "vitest";
import { emptyProfile, getModelSuggestions } from "./modelRecommendations";

describe("model recommendations", () => {
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
});
