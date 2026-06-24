use crate::domain::{AppState, Novel, NovelDetail};
use crate::{
    create_chapter_batches, decode_text, fill_empty_canon_assets_from_analysis, load_canon_assets,
    load_chapter_batch_size, load_chapter_batches, load_chapters, load_novel_settings,
    review_warning_file_paths, row_to_novel, to_string,
};
use chrono::Utc;
use rusqlite::params;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;
use uuid::Uuid;

struct StagedNovelFiles {
    trash_dir: PathBuf,
    moved_entries: Vec<(PathBuf, PathBuf)>,
}

impl StagedNovelFiles {
    fn stage(
        data_dir: &Path,
        app_dir: &Path,
        novel_id: &str,
        novel_title: &str,
    ) -> Result<Self, String> {
        let trash_dir =
            data_dir
                .join("deletion-trash")
                .join(format!("{}-{}", novel_id, Uuid::new_v4()));
        let batch_dir = data_dir.join("chapter_batches").join(novel_id);
        let [root_warning, fallback_warning] =
            review_warning_file_paths(app_dir, data_dir, novel_title);
        let mut candidates = vec![
            (batch_dir, trash_dir.join("chapter-batches")),
            (root_warning, trash_dir.join("review-warning-app.log")),
        ];
        if fallback_warning != candidates[1].0 {
            candidates.push((fallback_warning, trash_dir.join("review-warning-data.log")));
        }
        candidates.retain(|(source, _)| source.exists());

        let mut staged = Self {
            trash_dir,
            moved_entries: Vec::new(),
        };
        if !candidates.is_empty() {
            fs::create_dir_all(&staged.trash_dir).map_err(to_string)?;
        }
        for (source, destination) in candidates {
            if let Err(error) = fs::rename(&source, &destination) {
                let restore_error = staged.restore().err();
                let _ = staged.cleanup();
                let mut message = format!(
                    "无法准备删除小说关联文件，数据库未修改：{}",
                    to_string(error)
                );
                if let Some(restore_error) = restore_error {
                    message.push_str(&format!("；已移动文件恢复失败：{}", restore_error));
                }
                return Err(message);
            }
            staged.moved_entries.push((source, destination));
        }
        Ok(staged)
    }

    fn restore(&self) -> Result<(), String> {
        let mut restore_errors = Vec::new();
        for (source, destination) in self.moved_entries.iter().rev() {
            if !destination.exists() {
                continue;
            }
            if let Some(parent) = source.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    restore_errors.push(format!("{}：{}", source.display(), to_string(error)));
                    continue;
                }
            }
            if let Err(error) = fs::rename(destination, source) {
                restore_errors.push(format!("{}：{}", source.display(), to_string(error)));
            }
        }
        if restore_errors.is_empty() {
            self.cleanup()
        } else {
            Err(restore_errors.join("；"))
        }
    }

    fn cleanup(&self) -> Result<(), String> {
        if self.trash_dir.exists() {
            fs::remove_dir_all(&self.trash_dir).map_err(to_string)?;
        }
        Ok(())
    }
}

fn restore_staged_files(staged: &StagedNovelFiles, database_error: String) -> String {
    match staged.restore() {
        Ok(()) => database_error,
        Err(restore_error) => format!(
            "{}；关联文件恢复失败，请检查应用数据目录：{}",
            database_error, restore_error
        ),
    }
}

#[tauri::command]
pub(crate) fn import_txt(file_path: String, state: State<AppState>) -> Result<Novel, String> {
    let bytes = fs::read(&file_path).map_err(to_string)?;
    let (_, encoding) = decode_text(&bytes);
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
        status: "pending_split".to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    let mut conn = state.conn.lock().map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute(
        "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![novel.id, novel.title, novel.source_path, novel.encoding, novel.status, true, novel.created_at],
    )
    .map_err(to_string)?;
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
        let detected_chapters = conn
            .query_row(
                "SELECT detected_chapters FROM novels WHERE id = ?1",
                params![novel.id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(to_string)?;
        create_chapter_batches(
            &conn,
            &state.data_dir,
            &novel.id,
            &chapters,
            detected_chapters,
            load_chapter_batch_size(&conn)?,
        )?;
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
    let novel_title = conn
        .query_row(
            "SELECT title FROM novels WHERE id = ?1",
            params![novel_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(to_string)?;
    let staged_files =
        StagedNovelFiles::stage(&state.data_dir, &state.app_dir, &novel_id, &novel_title)?;
    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(error) => {
            return Err(restore_staged_files(&staged_files, to_string(error)));
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
        tx.execute(
            "DELETE FROM auto_run_checkpoints WHERE novel_id = ?1",
            params![novel_id],
        )
        .map_err(to_string)?;
        tx.execute("DELETE FROM ai_logs WHERE novel_id = ?1", params![novel_id])
            .map_err(to_string)?;
        tx.execute("DELETE FROM novels WHERE id = ?1", params![novel_id])
            .map_err(to_string)?;
        tx.commit().map_err(to_string)?;
        Ok(())
    })();
    if let Err(error) = delete_result {
        return Err(restore_staged_files(&staged_files, error));
    }
    let _ = staged_files.cleanup();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_and_restores_batch_and_review_warning_files() {
        let root = std::env::temp_dir().join(format!("yuri-delete-stage-{}", Uuid::new_v4()));
        let app_dir = root.join("app");
        let data_dir = root.join("data");
        let batch_dir = data_dir.join("chapter_batches").join("novel-1");
        fs::create_dir_all(&batch_dir).expect("create batch directory");
        fs::create_dir_all(&app_dir).expect("create app directory");
        fs::write(batch_dir.join("batch.txt"), "batch").expect("write batch file");
        let [root_warning, fallback_warning] =
            review_warning_file_paths(&app_dir, &data_dir, "测试小说");
        fs::write(&root_warning, "root warning").expect("write root warning");
        fs::write(&fallback_warning, "fallback warning").expect("write fallback warning");

        let staged = StagedNovelFiles::stage(&data_dir, &app_dir, "novel-1", "测试小说")
            .expect("stage files");

        assert!(!batch_dir.exists());
        assert!(!root_warning.exists());
        assert!(!fallback_warning.exists());
        assert_eq!(staged.moved_entries.len(), 3);

        staged.restore().expect("restore files");
        assert!(batch_dir.join("batch.txt").exists());
        assert!(root_warning.exists());
        assert!(fallback_warning.exists());
        assert!(!staged.trash_dir.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_removes_staged_review_warning_files() {
        let root = std::env::temp_dir().join(format!("yuri-delete-cleanup-{}", Uuid::new_v4()));
        let app_dir = root.join("app");
        let data_dir = root.join("data");
        fs::create_dir_all(&app_dir).expect("create app directory");
        let [root_warning, _] = review_warning_file_paths(&app_dir, &data_dir, "测试小说");
        fs::write(&root_warning, "warning").expect("write warning");

        let staged = StagedNovelFiles::stage(&data_dir, &app_dir, "novel-1", "测试小说")
            .expect("stage files");
        staged.cleanup().expect("remove staged files");

        assert!(!root_warning.exists());
        assert!(!staged.trash_dir.exists());
        let _ = fs::remove_dir_all(root);
    }
}
