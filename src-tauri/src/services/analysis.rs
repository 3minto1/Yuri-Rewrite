use crate::domain::{AppState, Chapter, ModelProfile};
use crate::{
    analyze_batch_with_parallelism, apply_staged_analyses, chapters_without_staged_outputs,
    load_staged_chapter_ids, save_parsed_analyses,
};
use tauri::State;

pub(crate) async fn analyze_and_save(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    parallelism: usize,
    checkpoint_batch_index: Option<i64>,
) -> Result<(), String> {
    let pending_chapters = if let Some(batch_index) = checkpoint_batch_index {
        let staged = load_staged_chapter_ids(state, novel_id, batch_index, "analysis")?;
        chapters_without_staged_outputs(chapters, &staged)
    } else {
        chapters.to_vec()
    };
    let parsed = if pending_chapters.is_empty() {
        Vec::new()
    } else {
        analyze_batch_with_parallelism(
            state,
            novel_id,
            profile,
            api_key,
            chapters,
            &pending_chapters,
            parallelism,
            checkpoint_batch_index,
        )
        .await?
    };
    if let Some(batch_index) = checkpoint_batch_index {
        apply_staged_analyses(state, novel_id, batch_index, chapters)
    } else {
        save_parsed_analyses(state, novel_id, chapters, parsed)
    }
}
