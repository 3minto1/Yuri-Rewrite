use crate::credentials::{
    combine_rollback_error, delete_api_key_if_present, restore_api_key_snapshot, snapshot_api_key,
    ApiKeySnapshot,
};
use crate::domain::AppState;
use crate::{review_warning_file_paths, to_string};
use rusqlite::Connection;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::State;
use uuid::Uuid;

const DELETE_LOCAL_DATA_CONFIRMATION: &str = "删除全部本地数据";

#[derive(Debug, Serialize)]
pub(crate) struct LocalDataDeletionResult {
    warnings: Vec<String>,
}

struct CredentialBackup {
    profile_id: String,
    snapshot: ApiKeySnapshot,
}

struct StagedLocalData {
    trash_dir: PathBuf,
    moved_entries: Vec<(PathBuf, PathBuf)>,
}

impl StagedLocalData {
    fn stage(data_dir: &Path, additional_files: &[PathBuf]) -> Result<Self, String> {
        let trash_root = data_dir.join("deletion-trash");
        if trash_root.exists() {
            validate_existing_child(data_dir, &trash_root)?;
            fs::remove_dir_all(&trash_root).map_err(to_string)?;
        }

        let trash_dir = trash_root.join(format!("local-data-reset-{}", Uuid::new_v4()));
        validate_new_child(data_dir, &trash_dir)?;
        let mut candidates = vec![data_dir.join("chapter_batches"), data_dir.join("updates")];
        candidates.extend(
            additional_files
                .iter()
                .filter(|path| path.starts_with(data_dir))
                .cloned(),
        );
        candidates.sort();
        candidates.dedup();
        candidates.retain(|path| path.exists());

        let mut staged = Self {
            trash_dir,
            moved_entries: Vec::new(),
        };
        if !candidates.is_empty() {
            fs::create_dir_all(&staged.trash_dir).map_err(to_string)?;
        }
        for (index, source) in candidates.into_iter().enumerate() {
            validate_existing_child(data_dir, &source)?;
            let destination = staged.trash_dir.join(format!("entry-{index}"));
            if let Err(error) = fs::rename(&source, &destination) {
                let restore_result = staged.restore();
                return Err(combine_rollback_error(
                    format!(
                        "无法暂存待删除的应用数据，数据库尚未修改：{}",
                        to_string(error)
                    ),
                    restore_result,
                    "恢复已暂存文件",
                ));
            }
            staged.moved_entries.push((source, destination));
        }
        Ok(staged)
    }

    fn restore(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (source, destination) in self.moved_entries.iter().rev() {
            if !destination.exists() {
                continue;
            }
            if let Some(parent) = source.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    errors.push(format!("{}：{}", source.display(), to_string(error)));
                    continue;
                }
            }
            if let Err(error) = fs::rename(destination, source) {
                errors.push(format!("{}：{}", source.display(), to_string(error)));
            }
        }
        if errors.is_empty() {
            self.cleanup()
        } else {
            Err(errors.join("；"))
        }
    }

    fn cleanup(&self) -> Result<(), String> {
        if self.trash_dir.exists() {
            let root = self
                .trash_dir
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| "无法确认临时回收目录的应用数据根目录。".to_string())?;
            validate_existing_child(root, &self.trash_dir)?;
            fs::remove_dir_all(&self.trash_dir).map_err(to_string)?;
        }
        Ok(())
    }
}

fn validate_existing_child(root: &Path, target: &Path) -> Result<(), String> {
    let canonical_root = fs::canonicalize(root).map_err(to_string)?;
    let canonical_target = fs::canonicalize(target).map_err(to_string)?;
    if canonical_target == canonical_root || !canonical_target.starts_with(&canonical_root) {
        return Err(format!(
            "拒绝处理应用数据目录之外的路径：{}",
            canonical_target.display()
        ));
    }
    Ok(())
}

fn validate_new_child(root: &Path, target: &Path) -> Result<(), String> {
    let canonical_root = fs::canonicalize(root).map_err(to_string)?;
    let relative_target = target
        .strip_prefix(root)
        .map_err(|_| format!("拒绝创建应用数据目录之外的路径：{}", target.display()))?;
    if relative_target.as_os_str().is_empty()
        || relative_target.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "拒绝创建无效的应用数据子路径：{}",
            target.display()
        ));
    }
    let absolute_target = canonical_root.join(relative_target);
    if !absolute_target.starts_with(&canonical_root) {
        return Err(format!(
            "拒绝创建应用数据目录之外的路径：{}",
            absolute_target.display()
        ));
    }
    Ok(())
}

fn restore_credentials(backups: &[CredentialBackup]) -> Result<(), String> {
    let mut errors = Vec::new();
    for backup in backups.iter().rev() {
        if let Err(error) = restore_api_key_snapshot(&backup.profile_id, &backup.snapshot) {
            errors.push(format!("{}：{}", backup.profile_id, error));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

fn clear_local_database(conn: &mut Connection) -> Result<(), String> {
    conn.pragma_update(None, "secure_delete", "ON")
        .map_err(to_string)?;
    let tx = conn.transaction().map_err(to_string)?;
    tx.execute_batch(
        r#"
        DELETE FROM auto_run_shard_outputs;
        DELETE FROM auto_run_checkpoints;
        DELETE FROM chapter_rewrite_snapshots;
        DELETE FROM chapter_batches;
        DELETE FROM chapter_rules;
        DELETE FROM novel_settings;
        DELETE FROM canon_assets;
        DELETE FROM chapters;
        DELETE FROM jobs;
        DELETE FROM ai_logs;
        DELETE FROM token_usage_records;
        DELETE FROM novels;
        DELETE FROM model_profiles;
        DELETE FROM app_settings;
        "#,
    )
    .map_err(to_string)?;
    tx.commit().map_err(to_string)
}

fn remove_diagnostic_files(paths: &[PathBuf]) -> Vec<String> {
    let mut warnings = Vec::new();
    for path in paths {
        if !path.exists() {
            continue;
        }
        if let Err(error) = fs::remove_file(path) {
            warnings.push(format!("未能删除诊断文件 {}：{}", path.display(), error));
        }
    }
    warnings
}

fn validate_deletion_request(
    confirmation_phrase: &str,
    active_task: bool,
    active_single_rewrite: bool,
    has_auto_run: bool,
) -> Result<(), String> {
    if confirmation_phrase.trim() != DELETE_LOCAL_DATA_CONFIRMATION {
        return Err("确认短语不正确，本地数据未删除。".to_string());
    }
    if active_task || active_single_rewrite || has_auto_run {
        return Err("当前仍有运行中或暂停的一键任务，请先终止任务后再删除本地数据。".to_string());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_local_data(
    confirmation_phrase: String,
    state: State<AppState>,
) -> Result<LocalDataDeletionResult, String> {
    validate_deletion_request(
        &confirmation_phrase,
        state.active_tasks.any_active()?,
        state.single_rewrite_tasks.any_active()?,
        !state.auto_runs.lock().map_err(to_string)?.is_empty(),
    )?;

    let (profile_ids, novel_titles) = {
        let conn = state.conn.lock().map_err(to_string)?;
        let profile_ids = conn
            .prepare("SELECT id FROM model_profiles ORDER BY id")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(to_string)?;
        let novel_titles = conn
            .prepare("SELECT title FROM novels ORDER BY id")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(to_string)?;
        (profile_ids, novel_titles)
    };

    let mut diagnostic_paths = vec![state.app_dir.join("frontend-errors.log")];
    for title in &novel_titles {
        diagnostic_paths.extend(review_warning_file_paths(
            &state.app_dir,
            &state.data_dir,
            title,
        ));
    }
    diagnostic_paths.sort();
    diagnostic_paths.dedup();

    let staged = StagedLocalData::stage(&state.data_dir, &diagnostic_paths)?;
    let mut credential_backups = Vec::new();
    for profile_id in profile_ids {
        let snapshot = match snapshot_api_key(&profile_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Err(combine_rollback_error(
                    format!("无法读取模型 {profile_id} 的系统凭据，本地数据未删除：{error}"),
                    staged.restore(),
                    "恢复已暂存文件",
                ));
            }
        };
        credential_backups.push(CredentialBackup {
            profile_id,
            snapshot,
        });
    }

    for backup in &credential_backups {
        if let Err(error) = delete_api_key_if_present(&backup.profile_id) {
            let credential_error = combine_rollback_error(
                format!(
                    "删除模型 {} 的系统凭据失败，本地数据未删除：{}",
                    backup.profile_id, error
                ),
                restore_credentials(&credential_backups),
                "恢复系统凭据",
            );
            return Err(combine_rollback_error(
                credential_error,
                staged.restore(),
                "恢复已暂存文件",
            ));
        }
    }

    let database_result = match state.conn.lock() {
        Ok(mut conn) => clear_local_database(&mut conn),
        Err(error) => Err(to_string(error)),
    };
    if let Err(error) = database_result {
        let credential_error = combine_rollback_error(
            error,
            restore_credentials(&credential_backups),
            "恢复系统凭据",
        );
        return Err(combine_rollback_error(
            credential_error,
            staged.restore(),
            "恢复已暂存文件",
        ));
    }

    let mut warnings = remove_diagnostic_files(
        &diagnostic_paths
            .into_iter()
            .filter(|path| !path.starts_with(&state.data_dir))
            .collect::<Vec<_>>(),
    );
    match state.auto_runs.lock() {
        Ok(mut runs) => runs.clear(),
        Err(error) => warnings.push(format!("运行时一键任务状态清理失败：{}", to_string(error))),
    }
    match state.auto_run_progress.lock() {
        Ok(mut progress) => progress.clear(),
        Err(error) => warnings.push(format!("运行时任务进度清理失败：{}", to_string(error))),
    }
    if let Err(error) = staged.cleanup() {
        warnings.push(format!(
            "工作数据已从原位置移除，但临时回收目录需在下次启动时继续清理：{error}"
        ));
    }
    match state.conn.lock() {
        Ok(conn) => {
            if let Err(error) = conn.execute_batch("VACUUM") {
                warnings.push(format!(
                    "数据已清空，但数据库文件压缩失败：{}",
                    to_string(error)
                ));
            }
        }
        Err(error) => warnings.push(format!(
            "数据已清空，但无法锁定数据库执行文件压缩：{}",
            to_string(error)
        )),
    }

    Ok(LocalDataDeletionResult { warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    #[test]
    fn rejects_root_and_outside_paths() {
        let root = std::env::temp_dir().join(format!("yuri-local-data-path-{}", Uuid::new_v4()));
        let child = root.join("child");
        let outside = std::env::temp_dir().join(format!("yuri-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&child).expect("create child");
        fs::create_dir_all(&outside).expect("create outside");

        assert!(validate_existing_child(&root, &child).is_ok());
        assert!(validate_existing_child(&root, &root).is_err());
        assert!(validate_existing_child(&root, &outside).is_err());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn requires_exact_confirmation_and_blocks_every_task_state() {
        assert!(validate_deletion_request("删除本地数据", false, false, false).is_err());
        assert!(
            validate_deletion_request(DELETE_LOCAL_DATA_CONFIRMATION, true, false, false).is_err()
        );
        assert!(
            validate_deletion_request(DELETE_LOCAL_DATA_CONFIRMATION, false, true, false).is_err()
        );
        assert!(
            validate_deletion_request(DELETE_LOCAL_DATA_CONFIRMATION, false, false, true).is_err()
        );
        assert!(
            validate_deletion_request(DELETE_LOCAL_DATA_CONFIRMATION, false, false, false).is_ok()
        );
    }

    #[test]
    fn stages_internal_data_but_preserves_exports() {
        let root = std::env::temp_dir().join(format!("yuri-local-data-stage-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("chapter_batches")).expect("create batches");
        fs::create_dir_all(root.join("updates")).expect("create updates");
        fs::create_dir_all(root.join("exports")).expect("create exports");
        fs::write(root.join("chapter_batches").join("batch.txt"), "batch").expect("write batch");
        fs::write(root.join("exports").join("export.txt"), "export").expect("write export");

        let staged = StagedLocalData::stage(&root, &[]).expect("stage data");
        assert!(!root.join("chapter_batches").exists());
        assert!(!root.join("updates").exists());
        assert!(root.join("exports").join("export.txt").exists());
        staged.restore().expect("restore data");
        assert!(root.join("chapter_batches").join("batch.txt").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clears_every_business_table() {
        let mut conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize database");
        conn.execute_batch(
            r#"
            INSERT INTO novels (id, title, source_path, encoding, status, created_at)
            VALUES ('n1', 'N', 'source.txt', 'UTF-8', 'imported', 'now');
            INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text,
                analysis_status, rewrite_status
            ) VALUES ('c1', 'n1', 1, '第一章', '原文', 'completed', 'completed');
            INSERT INTO canon_assets (novel_id, kind, content, updated_at)
            VALUES ('n1', '人物卡', '内容', 'now');
            INSERT INTO model_profiles (
                id, name, provider, base_url, model, temperature, updated_at
            ) VALUES ('p1', 'P', 'openai-compatible', 'https://example.com', 'm', 0.7, 'now');
            INSERT INTO jobs (
                id, novel_id, job_type, status, current_chapter, total_chapters,
                message, created_at, updated_at
            ) VALUES ('j1', 'n1', 'analysis', 'completed', 1, 1, '完成', 'now', 'now');
            INSERT INTO ai_logs (
                id, novel_id, profile_id, action, status, content, created_at
            ) VALUES ('l1', 'n1', 'p1', '分析', 'success', '日志', 'now');
            INSERT INTO token_usage_records (
                id, novel_id, profile_id, profile_name, model,
                input_tokens, output_tokens, created_at
            ) VALUES ('t1', 'n1', 'p1', 'P', 'm', 1, 2, 'now');
            INSERT INTO app_settings (key, value) VALUES ('core_prompt', 'secret');
            INSERT INTO novel_settings (
                novel_id, protagonist_name, additional_feminize_names,
                bust, body_type, updated_at
            ) VALUES ('n1', '主角', '', '平胸', '少女', 'now');
            INSERT INTO chapter_rules (novel_id, rule_json, updated_at)
            VALUES ('n1', '{}', 'now');
            INSERT INTO chapter_batches (
                id, novel_id, batch_index, label, start_chapter,
                end_chapter, file_path, created_at
            ) VALUES ('b1', 'n1', 1, '第1批', 1, 1, 'batch.txt', 'now');
            INSERT INTO chapter_rewrite_snapshots (
                chapter_id, title, rewrite_text, created_at
            ) VALUES ('c1', '第一章', '改写', 'now');
            INSERT INTO auto_run_checkpoints (
                novel_id, start_batch_index, next_batch_index, job_id, status,
                pause_reason, phase, batch_index, profile_ids, created_at, updated_at
            ) VALUES ('n1', 0, 0, 'j1', 'paused', '暂停', 'rewrite', 1, '["p1"]', 'now', 'now');
            INSERT INTO auto_run_shard_outputs (
                novel_id, batch_index, phase, chapter_id, chapter_index,
                title, content, created_at
            ) VALUES ('n1', 1, 'rewrite', 'c1', 1, '第一章', '改写', 'now');
            "#,
        )
        .expect("seed business tables");

        clear_local_database(&mut conn).expect("clear database");
        for table in [
            "auto_run_shard_outputs",
            "auto_run_checkpoints",
            "chapter_rewrite_snapshots",
            "chapter_batches",
            "chapter_rules",
            "novel_settings",
            "canon_assets",
            "chapters",
            "jobs",
            "ai_logs",
            "token_usage_records",
            "novels",
            "model_profiles",
            "app_settings",
        ] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count rows");
            assert_eq!(count, 0, "table {table} should be empty");
        }
    }
}
