use crate::domain::Chapter;
use crate::{row_to_chapter, row_to_chapter_batch, to_string};
use rusqlite::{params, Connection};

pub(crate) fn load_chapters(conn: &Connection, novel_id: &str) -> Result<Vec<Chapter>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, rewrite_edited_at IS NOT NULL, analysis_status, rewrite_status FROM chapters WHERE novel_id = ?1 ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let chapters = stmt
        .query_map(params![novel_id], row_to_chapter)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}

pub(crate) fn load_chapters_for_batch(
    conn: &Connection,
    novel_id: &str,
    batch_id: &str,
) -> Result<Vec<Chapter>, String> {
    let batch = conn
        .query_row(
            "SELECT id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at FROM chapter_batches WHERE id = ?1 AND novel_id = ?2",
            params![batch_id, novel_id],
            row_to_chapter_batch,
        )
        .map_err(to_string)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, chapter_index, title, original_text, analysis_json, rewrite_text, rewrite_edited_at IS NOT NULL, analysis_status, rewrite_status FROM chapters WHERE novel_id = ?1 AND chapter_index BETWEEN ?2 AND ?3 ORDER BY chapter_index",
        )
        .map_err(to_string)?;
    let chapters = stmt
        .query_map(
            params![novel_id, batch.start_chapter, batch.end_chapter],
            row_to_chapter,
        )
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(chapters)
}
