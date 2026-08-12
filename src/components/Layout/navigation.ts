export type ActiveView =
  | "workspace"
  | "models"
  | "compare"
  | "rewrite-ab"
  | "novel-settings"
  | "core-settings"
  | "chapter-rules"
  | "logs"
  | "token-stats"
  | "settings";

export type ContextKind = "novel" | "models" | "settings" | "data";

export function contextKindForView(view: ActiveView): ContextKind {
  if (view === "models") return "models";
  if (view === "settings") return "settings";
  if (view === "logs" || view === "token-stats") return "data";
  return "novel";
}
