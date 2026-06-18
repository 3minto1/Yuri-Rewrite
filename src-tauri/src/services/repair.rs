use crate::domain::{AppState, Chapter, ModelProfile, NovelSettings};
use crate::{revise_rewrite_shard_after_review, ParsedChapterRewrite, ReviewDecision};
use tauri::State;

pub(crate) struct ReviewRepairContext<'a> {
    pub(crate) novel_id: &'a str,
    pub(crate) profile: &'a ModelProfile,
    pub(crate) api_key: &'a str,
    pub(crate) shard: &'a [Chapter],
    pub(crate) rewrites: &'a [ParsedChapterRewrite],
    pub(crate) canon_text: &'a str,
    pub(crate) settings: &'a NovelSettings,
    pub(crate) core_prompt: &'a str,
    pub(crate) shard_context: &'a str,
    pub(crate) shard_label: &'a str,
    pub(crate) decision: &'a ReviewDecision,
}

pub(crate) async fn repair_reviewed_shard(
    state: &State<'_, AppState>,
    context: ReviewRepairContext<'_>,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    revise_rewrite_shard_after_review(
        state,
        context.novel_id,
        context.profile,
        context.api_key,
        context.shard,
        context.rewrites,
        context.canon_text,
        context.settings,
        context.core_prompt,
        context.shard_context,
        context.shard_label,
        context.decision,
    )
    .await
}
