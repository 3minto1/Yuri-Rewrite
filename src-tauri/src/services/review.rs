use crate::domain::{AppState, Chapter, ModelProfile, NovelSettings};
use crate::{generate_reviewed_rewrite_pipeline, ParsedChapterRewrite};
use tauri::State;

pub(crate) struct ReviewPipelineContext<'a> {
    pub(crate) novel_id: &'a str,
    pub(crate) rewrite_profile: &'a ModelProfile,
    pub(crate) rewrite_api_key: &'a str,
    pub(crate) review_profile: &'a ModelProfile,
    pub(crate) review_api_key: &'a str,
    pub(crate) all_chapters: &'a [Chapter],
    pub(crate) chapters: &'a [Chapter],
    pub(crate) canon_text: &'a str,
    pub(crate) settings: &'a NovelSettings,
    pub(crate) core_prompt: &'a str,
    pub(crate) parallelism: usize,
    pub(crate) checkpoint_batch_index: Option<i64>,
}

pub(crate) async fn run_review_pipeline(
    state: &State<'_, AppState>,
    context: ReviewPipelineContext<'_>,
) -> Result<Vec<ParsedChapterRewrite>, String> {
    generate_reviewed_rewrite_pipeline(
        state,
        context.novel_id,
        context.rewrite_profile,
        context.rewrite_api_key,
        context.review_profile,
        context.review_api_key,
        context.all_chapters,
        context.chapters,
        context.canon_text,
        context.settings,
        context.core_prompt,
        context.parallelism,
        context.checkpoint_batch_index,
    )
    .await
}
