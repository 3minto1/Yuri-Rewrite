import type { NovelSettingsDraft } from "../../types";
import { parseAdditionalNameMappings, serializeAdditionalNameMappings } from "./additionalNameMappings";
import { parseRelationshipTargets, serializeRelationshipTargets } from "./relationshipTargets";

export function normalizeNovelSettingsDraft(draft: NovelSettingsDraft) {
  return {
    protagonist_name: draft.protagonist_name.trim(),
    protagonist_aliases: draft.protagonist_aliases.trim(),
    rewritten_protagonist_name: draft.rewritten_protagonist_name.trim(),
    additional_feminize_names: serializeAdditionalNameMappings(parseAdditionalNameMappings(draft.additional_feminize_names)),
    bust: draft.bust.trim(),
    body_type: draft.body_type.trim(),
    rewrite_mode: draft.rewrite_mode,
    advanced_settings: draft.advanced_settings.trim(),
    relationship_targets: serializeRelationshipTargets(parseRelationshipTargets(draft.relationship_targets))
  };
}

export function novelSettingsDraftsEqual(left: NovelSettingsDraft, right: NovelSettingsDraft) {
  return JSON.stringify(normalizeNovelSettingsDraft(left)) === JSON.stringify(normalizeNovelSettingsDraft(right));
}
