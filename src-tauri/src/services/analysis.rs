use crate::domain::{AppState, Chapter, ModelProfile};
use crate::{analyze_batch_with_parallelism, save_parsed_analyses};
use tauri::State;

pub(crate) async fn analyze_and_save(
    state: &State<'_, AppState>,
    novel_id: &str,
    profile: &ModelProfile,
    api_key: &str,
    chapters: &[Chapter],
    parallelism: usize,
) -> Result<(), String> {
    let parsed =
        analyze_batch_with_parallelism(state, novel_id, profile, api_key, chapters, parallelism)
            .await?;
    save_parsed_analyses(state, novel_id, chapters, parsed)
}
