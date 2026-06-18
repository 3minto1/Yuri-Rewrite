use crate::domain::{AppState, Chapter, NovelSettings};
use crate::{finalize_review_decision, ParsedChapterRewrite, ReviewDecision};
use tauri::State;

pub(crate) struct ReviewValidationContext<'a> {
    pub(crate) novel_id: &'a str,
    pub(crate) profile_id: &'a str,
    pub(crate) shard_label: &'a str,
    pub(crate) shard: &'a [Chapter],
    pub(crate) rewrites: &'a [ParsedChapterRewrite],
    pub(crate) settings: &'a NovelSettings,
}

pub(crate) fn validate_review_decision(
    state: &State<'_, AppState>,
    decision: ReviewDecision,
    context: ReviewValidationContext<'_>,
) -> Result<ReviewDecision, String> {
    finalize_review_decision(
        state,
        context.novel_id,
        context.profile_id,
        context.shard_label,
        decision,
        context.shard,
        context.rewrites,
        context.settings,
    )
}
