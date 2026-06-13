use crate::domain::{AppState, Novel, NovelDetail};
use crate::{
    create_chapter_batches, decode_text, fill_empty_canon_assets_from_analysis, load_canon_assets,
    load_chapter_batches, load_chapters, load_novel_settings, row_to_novel, seed_canon_assets,
    split_chapters, to_string,
};
use chrono::Utc;
use rusqlite::params;
use std::{fs, path::Path};
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub(crate) fn import_txt(file_path: String, state: State<AppState>) -> Result<Novel, String> {
    let bytes = fs::read(&file_path).map_err(to_string)?;
    let (text, encoding) = decode_text(&bytes);
    let source = Path::new(&file_path);
    let title = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("未命名小说")
        .to_string();
    let novel = Novel {
        id: Uuid::new_v4().to_string(),
        title,
        source_path: file_path,
        encoding,
        status: "imported".to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let split = split_chapters(&novel.id, &text);
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![novel.id, novel.title, novel.source_path, novel.encoding, novel.status, novel.created_at],
    )
    .map_err(to_string)?;

    for chapter in &split.chapters {
        tx.execute(
            "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
            params![chapter.id, chapter.novel_id, chapter.index, chapter.title, chapter.original_text],
        )
        .map_err(to_string)?;
    }

    create_chapter_batches(
        &tx,
        &state.data_dir,
        &novel.id,
        &split.chapters,
        split.detected_chapters,
    )
    .map_err(to_string)?;
    seed_canon_assets(&tx, &novel.id).map_err(to_string)?;
    tx.commit().map_err(to_string)?;
    Ok(novel)
}

#[tauri::command]
pub(crate) fn list_novels(state: State<AppState>) -> Result<Vec<Novel>, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let mut stmt = conn
        .prepare("SELECT id, title, source_path, encoding, status, created_at FROM novels ORDER BY created_at DESC")
        .map_err(to_string)?;
    let rows = stmt
        .query_map([], row_to_novel)
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(rows)
}

#[tauri::command]
pub(crate) fn get_novel_detail(
    novel_id: String,
    state: State<AppState>,
) -> Result<NovelDetail, String> {
    let conn = state.conn.lock().map_err(to_string)?;
    let novel = conn
        .query_row(
            "SELECT id, title, source_path, encoding, status, created_at FROM novels WHERE id = ?1",
            params![novel_id],
            row_to_novel,
        )
        .map_err(to_string)?;
    let chapters = load_chapters(&conn, &novel.id)?;
    if !chapters.is_empty() && load_chapter_batches(&conn, &novel.id)?.is_empty() {
        create_chapter_batches(&conn, &state.data_dir, &novel.id, &chapters, true)?;
    }
    fill_empty_canon_assets_from_analysis(&conn, &novel.id).map_err(to_string)?;
    let canon_assets = load_canon_assets(&conn, &novel.id)?;
    let batches = load_chapter_batches(&conn, &novel.id)?;
    let settings = load_novel_settings(&conn, &novel.id)?;
    Ok(NovelDetail {
        novel,
        chapters,
        canon_assets,
        batches,
        settings,
    })
}

#[tauri::command]
pub(crate) fn delete_novel(novel_id: String, state: State<AppState>) -> Result<(), String> {
    if state.active_tasks.novel_is_active(&novel_id)?
        || state
            .auto_runs
            .lock()
            .map_err(to_string)?
            .contains_key(&novel_id)
    {
        return Err("当前小说有任务正在运行，请先暂停或终止任务后再删除。".to_string());
    }
    let mut conn = state.conn.lock().map_err(to_string)?;
    let batch_dir = state.data_dir.join("chapter_batches").join(&novel_id);
    let trash_root = state.data_dir.join("deletion-trash");
    let trash_dir = trash_root.join(format!("{}-{}", novel_id, Uuid::new_v4()));
    let moved_batch_dir = if batch_dir.exists() {
        fs::create_dir_all(&trash_root).map_err(to_string)?;
        fs::rename(&batch_dir, &trash_dir).map_err(|error| {
            format!(
                "无法准备删除小说内部批次文件，数据库未修改：{}",
                to_string(error)
            )
        })?;
        true
    } else {
        false
    };
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(error) => {
            if moved_batch_dir {
                let _ = fs::rename(&trash_dir, &batch_dir);
            }
            return Err(to_string(error));
        }
    };
    let delete_result = (|| -> Result<(), String> {
        tx.execute(
            "DELETE FROM novel_settings WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        tx.execute(
            "DELETE FROM chapter_batches WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        tx.execute(
            "DELETE FROM chapters WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        tx.execute(
            "DELETE FROM canon_assets WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        tx.execute("DELETE FROM jobs WHERE novel_id = ?1", params![novel_id])
            .map_err(to_string)?;
        tx.execute("DELETE FROM ai_logs WHERE novel_id = ?1", params![novel_id])
            .map_err(to_string)?;
        tx.execute("DELETE FROM novels WHERE id = ?1", params![novel_id])
            .map_err(to_string)?;
        tx.commit().map_err(to_string)?;
        Ok(())
    })();
    if let Err(error) = delete_result {
        if moved_batch_dir {
            let _ = fs::rename(&trash_dir, &batch_dir);
        }
        return Err(error);
    }
    if moved_batch_dir {
        let _ = fs::remove_dir_all(&trash_dir);
    }
    Ok(())
}
