use crate::domain::{AppState, Chapter, ModelProfile, NovelSettings};
use crate::{rewrite_batch_with_parallelism, save_parsed_rewrites};
use tauri::State;

pub(crate) struct RewriteRunContext<'a> {
    pub(crate) novel_id: &'a str,
    pub(crate) profile: &'a ModelProfile,
    pub(crate) api_key: &'a str,
    pub(crate) chapters: &'a [Chapter],
    pub(crate) canon_text: &'a str,
    pub(crate) settings: &'a NovelSettings,
    pub(crate) core_prompt: &'a str,
    pub(crate) review_enabled: bool,
    pub(crate) review_profile: Option<&'a ModelProfile>,
    pub(crate) review_api_key: Option<&'a str>,
    pub(crate) parallelism: usize,
}

pub(crate) async fn rewrite_and_save(
    state: &State<'_, AppState>,
    context: RewriteRunContext<'_>,
) -> Result<(), String> {
    let rewrites = rewrite_batch_with_parallelism(
        state,
        context.novel_id,
        context.profile,
        context.api_key,
        context.chapters,
        context.canon_text,
        context.settings,
        context.core_prompt,
        context.review_enabled,
        context.review_profile,
        context.review_api_key,
        context.parallelism,
    )
    .await?;
    save_parsed_rewrites(state, rewrites)
}
