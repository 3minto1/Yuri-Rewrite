import type { ModelProfile } from "../types";

export function modelProfileDisplayName(profile: Pick<ModelProfile, "name" | "model">) {
  return profile.name.trim() || profile.model.trim() || "未命名模型配置";
}

export function modelProfileTitle(profile: Pick<ModelProfile, "name" | "model" | "base_url">) {
  const displayName = modelProfileDisplayName(profile);
  const model = profile.model.trim();
  const baseUrl = profile.base_url.trim();
  return [
    displayName,
    model && model !== displayName ? `模型名：${model}` : "",
    baseUrl ? `Base URL：${baseUrl}` : ""
  ].filter(Boolean).join("\n");
}
