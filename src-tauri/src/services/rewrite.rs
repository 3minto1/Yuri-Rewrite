use crate::domain::{AppState, Chapter, ModelProfile, NovelSettings};
use crate::{
    apply_staged_rewrites, chapters_without_staged_outputs, load_staged_chapter_ids,
    rewrite_batch_with_parallelism, save_parsed_rewrites,
};
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
    pub(crate) checkpoint_batch_index: Option<i64>,
}

pub(crate) async fn rewrite_and_save(
    state: &State<'_, AppState>,
    context: RewriteRunContext<'_>,
) -> Result<(), String> {
    let pending_chapters = if let Some(batch_index) = context.checkpoint_batch_index {
        let staged =
            load_staged_chapter_ids(state, context.novel_id, batch_index, "rewrite")?;
        chapters_without_staged_outputs(context.chapters, &staged)
    } else {
        context.chapters.to_vec()
    };
    let rewrites = if pending_chapters.is_empty() {
        Vec::new()
    } else {
        rewrite_batch_with_parallelism(
            state,
            context.novel_id,
            context.profile,
            context.api_key,
            context.chapters,
            &pending_chapters,
            context.canon_text,
            context.settings,
            context.core_prompt,
            context.review_enabled,
            context.review_profile,
            context.review_api_key,
            context.parallelism,
            context.checkpoint_batch_index,
        )
        .await?
    };
    if let Some(batch_index) = context.checkpoint_batch_index {
        apply_staged_rewrites(state, context.novel_id, batch_index, context.chapters)
    } else {
        save_parsed_rewrites(state, rewrites)
    }
}
