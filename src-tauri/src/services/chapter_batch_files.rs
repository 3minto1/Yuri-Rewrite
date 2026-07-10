use crate::commands::settings::{load_chapter_batch_size, normalize_chapter_batch_size};
use crate::domain::{Chapter, ChapterBatch};
use crate::repositories::chapters::load_chapters;
use crate::to_string;
use chrono::Utc;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const ORPHAN_GRACE_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);
const REPARSE_POINT_ATTRIBUTE: u32 = 0x0400;

#[derive(Debug)]
pub(crate) struct PreparedChapterBatchGeneration {
    novel_id: String,
    generation_dir: PathBuf,
    batches: Vec<ChapterBatch>,
    retained: bool,
}

impl PreparedChapterBatchGeneration {
    pub(crate) fn novel_id(&self) -> &str {
        &self.novel_id
    }

    pub(crate) fn batches(&self) -> &[ChapterBatch] {
        &self.batches
    }

    pub(crate) fn retain_after_commit(&mut self) {
        self.retained = true;
    }

    pub(crate) fn discard(&mut self) -> Result<(), String> {
        if self.retained || !self.generation_dir.exists() {
            self.retained = true;
            return Ok(());
        }
        remove_managed_directory(
            &chapter_batches_root_from_generation(&self.generation_dir)?,
            &self.generation_dir,
        )?;
        self.retained = true;
        Ok(())
    }
}

impl Drop for PreparedChapterBatchGeneration {
    fn drop(&mut self) {
        if !self.retained && self.generation_dir.exists() {
            if let Ok(root) = chapter_batches_root_from_generation(&self.generation_dir) {
                let _ = remove_managed_directory(&root, &self.generation_dir);
            }
        }
    }
}

#[derive(Clone)]
struct BatchFileSpec {
    id: String,
    batch_index: i64,
    label: String,
    start_chapter: i64,
    end_chapter: i64,
    created_at: String,
    body: String,
}

pub(crate) fn prepare_chapter_batch_generation(
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    detected_chapters: bool,
    chapter_batch_size: usize,
) -> Result<PreparedChapterBatchGeneration, String> {
    let batch_size = if detected_chapters {
        normalize_chapter_batch_size(chapter_batch_size)
    } else {
        1
    };
    let now = Utc::now().to_rfc3339();
    let specs = chapters
        .chunks(batch_size)
        .enumerate()
        .map(|(index, chunk)| {
            let first = chunk.first().ok_or_else(|| "批次内容为空。".to_string())?;
            let last = chunk.last().ok_or_else(|| "批次内容为空。".to_string())?;
            let batch_index = (index + 1) as i64;
            Ok(BatchFileSpec {
                id: Uuid::new_v4().to_string(),
                batch_index,
                label: if detected_chapters {
                    format!("{}-{}章", first.index, last.index)
                } else {
                    format!("第{batch_index}批（约10万字）")
                },
                start_chapter: first.index,
                end_chapter: last.index,
                created_at: now.clone(),
                body: chapter_batch_body(chunk),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    prepare_generation_from_specs(data_dir, novel_id, specs)
}

fn prepare_generation_for_existing_batches(
    data_dir: &Path,
    novel_id: &str,
    chapters: &[Chapter],
    batches: &[ChapterBatch],
) -> Result<PreparedChapterBatchGeneration, String> {
    let specs = batches
        .iter()
        .map(|batch| {
            let chunk = chapters
                .iter()
                .filter(|chapter| {
                    chapter.index >= batch.start_chapter && chapter.index <= batch.end_chapter
                })
                .cloned()
                .collect::<Vec<_>>();
            if chunk.is_empty() {
                return Err(format!(
                    "批次「{}」引用的章节范围 {}-{} 不存在，无法修复批次文件。",
                    batch.label, batch.start_chapter, batch.end_chapter
                ));
            }
            Ok(BatchFileSpec {
                id: batch.id.clone(),
                batch_index: batch.batch_index,
                label: batch.label.clone(),
                start_chapter: batch.start_chapter,
                end_chapter: batch.end_chapter,
                created_at: batch.created_at.clone(),
                body: chapter_batch_body(&chunk),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    prepare_generation_from_specs(data_dir, novel_id, specs)
}

fn prepare_generation_from_specs(
    data_dir: &Path,
    novel_id: &str,
    specs: Vec<BatchFileSpec>,
) -> Result<PreparedChapterBatchGeneration, String> {
    validate_single_path_component(novel_id, "小说 ID")?;
    let root = data_dir.join("chapter_batches");
    fs::create_dir_all(&root).map_err(to_string)?;
    validate_managed_path(&root, &root, true)?;
    let novel_dir = root.join(novel_id);
    if novel_dir.exists() {
        validate_managed_path(&root, &novel_dir, false)?;
    } else {
        fs::create_dir(&novel_dir).map_err(to_string)?;
        validate_managed_path(&root, &novel_dir, false)?;
    }

    let generation_id = Uuid::new_v4().to_string();
    let temporary_dir = novel_dir.join(format!(".generation-{generation_id}.tmp"));
    let generation_dir = novel_dir.join(format!("generation-{generation_id}"));
    validate_managed_path(&root, &temporary_dir, false)?;
    validate_managed_path(&root, &generation_dir, false)?;
    fs::create_dir(&temporary_dir).map_err(to_string)?;

    let write_result = (|| {
        let mut batches = Vec::with_capacity(specs.len());
        let mut seen_indexes = HashSet::new();
        for spec in specs {
            if spec.batch_index <= 0 || !seen_indexes.insert(spec.batch_index) {
                return Err("批次序号必须为不重复的正整数。".to_string());
            }
            let file_name = format!("batch-{:03}.txt", spec.batch_index);
            let temporary_file = temporary_dir.join(&file_name);
            write_and_verify_file(&temporary_file, spec.body.as_bytes())?;
            batches.push(ChapterBatch {
                id: spec.id,
                novel_id: novel_id.to_string(),
                batch_index: spec.batch_index,
                label: spec.label,
                start_chapter: spec.start_chapter,
                end_chapter: spec.end_chapter,
                file_path: generation_dir.join(file_name).to_string_lossy().to_string(),
                created_at: spec.created_at,
            });
        }
        fs::rename(&temporary_dir, &generation_dir).map_err(to_string)?;
        validate_managed_path(&root, &generation_dir, false)?;
        for batch in &batches {
            let path = PathBuf::from(&batch.file_path);
            if !path.is_file() {
                return Err(format!("批次文件写入后不存在：{}", path.display()));
            }
        }
        Ok(batches)
    })();

    match write_result {
        Ok(batches) => Ok(PreparedChapterBatchGeneration {
            novel_id: novel_id.to_string(),
            generation_dir,
            batches,
            retained: false,
        }),
        Err(error) => {
            if temporary_dir.exists() {
                let _ = remove_managed_directory(&root, &temporary_dir);
            }
            if generation_dir.exists() {
                let _ = remove_managed_directory(&root, &generation_dir);
            }
            Err(error)
        }
    }
}

fn chapter_batch_body(chapters: &[Chapter]) -> String {
    chapters
        .iter()
        .map(|chapter| format!("{}\n\n{}", chapter.title, chapter.original_text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn write_and_verify_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(to_string)?;
    file.write_all(bytes).map_err(to_string)?;
    file.sync_all().map_err(to_string)?;
    drop(file);

    let mut stored = Vec::with_capacity(bytes.len());
    File::open(path)
        .map_err(to_string)?
        .read_to_end(&mut stored)
        .map_err(to_string)?;
    let expected = Sha256::digest(bytes);
    let actual = Sha256::digest(&stored);
    if expected != actual {
        return Err(format!("批次文件写入校验失败：{}", path.display()));
    }
    Ok(())
}

pub(crate) fn replace_chapter_batch_rows(
    conn: &Connection,
    prepared: &PreparedChapterBatchGeneration,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM chapter_batches WHERE novel_id = ?1",
        params![prepared.novel_id],
    )
    .map_err(to_string)?;
    insert_chapter_batch_rows(conn, prepared.batches())
}

pub(crate) fn insert_chapter_batch_rows(
    conn: &Connection,
    batches: &[ChapterBatch],
) -> Result<(), String> {
    for batch in batches {
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                batch.id,
                batch.novel_id,
                batch.batch_index,
                batch.label,
                batch.start_chapter,
                batch.end_chapter,
                batch.file_path,
                batch.created_at
            ],
        )
        .map_err(to_string)?;
    }
    Ok(())
}

pub(crate) fn discard_prepared_generations(
    prepared: &mut [PreparedChapterBatchGeneration],
    primary_error: String,
) -> String {
    let cleanup_errors = prepared
        .iter_mut()
        .filter_map(|item| item.discard().err())
        .collect::<Vec<_>>();
    if cleanup_errors.is_empty() {
        primary_error
    } else {
        format!(
            "{primary_error}；新批次代次清理失败：{}",
            cleanup_errors.join("；")
        )
    }
}

pub(crate) fn repair_chapter_batches_for_novel(
    conn: &mut Connection,
    data_dir: &Path,
    novel_id: &str,
) -> Result<bool, String> {
    let chapters = load_chapters(conn, novel_id)?;
    if chapters.is_empty() {
        return Ok(false);
    }
    let existing = load_batch_rows(conn, novel_id)?;
    let root = data_dir.join("chapter_batches");
    fs::create_dir_all(&root).map_err(to_string)?;
    let mut missing = existing.is_empty();
    for batch in &existing {
        let path = PathBuf::from(&batch.file_path);
        validate_managed_path(&root, &path, false)?;
        missing |= !path.is_file();
    }
    if !missing {
        return Ok(false);
    }

    let mut prepared = if existing.is_empty() {
        let detected = conn
            .query_row(
                "SELECT detected_chapters FROM novels WHERE id = ?1",
                params![novel_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(to_string)?;
        prepare_chapter_batch_generation(
            data_dir,
            novel_id,
            &chapters,
            detected,
            load_chapter_batch_size(conn)?,
        )?
    } else {
        prepare_generation_for_existing_batches(data_dir, novel_id, &chapters, &existing)?
    };

    let database_result = (|| {
        let tx = conn.transaction().map_err(to_string)?;
        if existing.is_empty() {
            replace_chapter_batch_rows(&tx, &prepared)?;
        } else {
            for batch in prepared.batches() {
                tx.execute(
                    "UPDATE chapter_batches SET file_path = ?1 WHERE id = ?2 AND novel_id = ?3",
                    params![batch.file_path, batch.id, novel_id],
                )
                .map_err(to_string)?;
            }
        }
        tx.commit().map_err(to_string)
    })();
    if let Err(error) = database_result {
        return Err(discard_prepared_generations(
            std::slice::from_mut(&mut prepared),
            error,
        ));
    }
    prepared.retain_after_commit();
    cleanup_stale_batch_files_for_novel(conn, data_dir, novel_id)?;
    Ok(true)
}

pub(crate) fn maintain_chapter_batch_files(
    conn: &mut Connection,
    data_dir: &Path,
) -> Result<(), String> {
    let novel_ids = {
        let mut stmt = conn
            .prepare(
                "SELECT id FROM novels
                 WHERE EXISTS (SELECT 1 FROM chapters WHERE chapters.novel_id = novels.id)
                 ORDER BY created_at",
            )
            .map_err(to_string)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        rows
    };
    for novel_id in novel_ids {
        repair_chapter_batches_for_novel(conn, data_dir, &novel_id)?;
    }
    cleanup_orphaned_chapter_batch_files(conn, data_dir, ORPHAN_GRACE_PERIOD)
}

pub(crate) fn cleanup_unreferenced_batch_files_for_novel(
    conn: &Connection,
    data_dir: &Path,
    novel_id: &str,
    minimum_age: Duration,
) -> Result<(), String> {
    validate_single_path_component(novel_id, "小说 ID")?;
    let root = data_dir.join("chapter_batches");
    if !root.exists() {
        return Ok(());
    }
    let novel_dir = root.join(novel_id);
    if !novel_dir.exists() {
        return Ok(());
    }
    validate_managed_path(&root, &novel_dir, false)?;
    let referenced = referenced_batch_paths(conn, Some(novel_id), &root)?;
    cleanup_novel_directory(&root, &novel_dir, &referenced, minimum_age)
}

pub(crate) fn cleanup_stale_batch_files_for_novel(
    conn: &Connection,
    data_dir: &Path,
    novel_id: &str,
) -> Result<(), String> {
    cleanup_unreferenced_batch_files_for_novel(conn, data_dir, novel_id, ORPHAN_GRACE_PERIOD)
}

fn cleanup_orphaned_chapter_batch_files(
    conn: &Connection,
    data_dir: &Path,
    minimum_age: Duration,
) -> Result<(), String> {
    let root = data_dir.join("chapter_batches");
    if !root.exists() {
        cleanup_legacy_rebuild_root(data_dir, minimum_age)?;
        return Ok(());
    }
    validate_managed_path(&root, &root, true)?;
    let referenced = referenced_batch_paths(conn, None, &root)?;
    for entry in fs::read_dir(&root).map_err(to_string)? {
        let entry = entry.map_err(to_string)?;
        let novel_dir = entry.path();
        if is_reparse_point(&fs::symlink_metadata(&novel_dir).map_err(to_string)?) {
            return Err(format!(
                "批次根目录包含不安全的符号链接或目录联接：{}",
                novel_dir.display()
            ));
        }
        if !entry.file_type().map_err(to_string)?.is_dir() {
            continue;
        }
        validate_managed_path(&root, &novel_dir, false)?;
        cleanup_novel_directory(&root, &novel_dir, &referenced, minimum_age)?;
        if fs::read_dir(&novel_dir)
            .map_err(to_string)?
            .next()
            .is_none()
            && is_old_enough(&novel_dir, minimum_age)
        {
            fs::remove_dir(&novel_dir).map_err(to_string)?;
        }
    }
    cleanup_legacy_rebuild_root(data_dir, minimum_age)
}

fn referenced_batch_paths(
    conn: &Connection,
    novel_id: Option<&str>,
    root: &Path,
) -> Result<HashSet<PathBuf>, String> {
    let paths = if let Some(novel_id) = novel_id {
        let mut stmt = conn
            .prepare("SELECT file_path FROM chapter_batches WHERE novel_id = ?1")
            .map_err(to_string)?;
        let rows = stmt
            .query_map(params![novel_id], |row| row.get::<_, String>(0))
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        rows
    } else {
        let mut stmt = conn
            .prepare("SELECT file_path FROM chapter_batches")
            .map_err(to_string)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(to_string)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(to_string)?;
        rows
    };
    paths
        .into_iter()
        .map(PathBuf::from)
        .map(|path| {
            validate_managed_path(root, &path, false)?;
            Ok(fs::canonicalize(&path).unwrap_or(path))
        })
        .collect()
}

fn cleanup_novel_directory(
    root: &Path,
    novel_dir: &Path,
    referenced: &HashSet<PathBuf>,
    minimum_age: Duration,
) -> Result<(), String> {
    for entry in fs::read_dir(novel_dir).map_err(to_string)? {
        let entry = entry.map_err(to_string)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type().map_err(to_string)?;
        if is_reparse_point(&fs::symlink_metadata(&path).map_err(to_string)?) {
            if is_generation_directory_name(&name) || is_legacy_batch_file_name(&name) {
                return Err(format!(
                    "批次目录包含不安全的符号链接或目录联接：{}",
                    path.display()
                ));
            }
            continue;
        }
        if file_type.is_dir() && is_generation_directory_name(&name) {
            validate_managed_path(root, &path, false)?;
            let comparison_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let referenced_here = referenced
                .iter()
                .any(|file| file.starts_with(&comparison_path));
            if !referenced_here && is_old_enough(&path, minimum_age) {
                remove_managed_directory(root, &path)?;
            }
        } else if file_type.is_file()
            && is_legacy_batch_file_name(&name)
            && !referenced.contains(&fs::canonicalize(&path).unwrap_or_else(|_| path.clone()))
            && is_old_enough(&path, minimum_age)
        {
            validate_managed_path(root, &path, false)?;
            fs::remove_file(&path).map_err(to_string)?;
        }
    }
    Ok(())
}

fn is_generation_directory_name(name: &str) -> bool {
    if let Some(id) = name.strip_prefix("generation-") {
        return is_canonical_uuid(id);
    }
    name.strip_prefix(".generation-")
        .and_then(|value| value.strip_suffix(".tmp"))
        .is_some_and(is_canonical_uuid)
}

fn is_canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value)
        .ok()
        .is_some_and(|uuid| uuid.hyphenated().to_string() == value)
}

fn is_legacy_batch_file_name(name: &str) -> bool {
    name.strip_prefix("batch-")
        .and_then(|value| value.strip_suffix(".txt"))
        .is_some_and(|index| !index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit()))
}

fn cleanup_legacy_rebuild_root(data_dir: &Path, minimum_age: Duration) -> Result<(), String> {
    let root = data_dir.join("chapter-batches-rebuild");
    if !root.exists() {
        return Ok(());
    }
    validate_managed_path(data_dir, &root, false)?;
    for entry in fs::read_dir(&root).map_err(to_string)? {
        let path = entry.map_err(to_string)?.path();
        validate_managed_path(data_dir, &path, false)?;
        if path.is_dir() && is_old_enough(&path, minimum_age) {
            remove_managed_directory(data_dir, &path)?;
        }
    }
    if fs::read_dir(&root).map_err(to_string)?.next().is_none() {
        fs::remove_dir(root).map_err(to_string)?;
    }
    Ok(())
}

fn load_batch_rows(conn: &Connection, novel_id: &str) -> Result<Vec<ChapterBatch>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at
             FROM chapter_batches WHERE novel_id = ?1 ORDER BY batch_index",
        )
        .map_err(to_string)?;
    let rows = stmt
        .query_map(params![novel_id], |row| {
            Ok(ChapterBatch {
                id: row.get(0)?,
                novel_id: row.get(1)?,
                batch_index: row.get(2)?,
                label: row.get(3)?,
                start_chapter: row.get(4)?,
                end_chapter: row.get(5)?,
                file_path: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(to_string)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_string)?;
    Ok(rows)
}

fn is_old_enough(path: &Path, minimum_age: Duration) -> bool {
    if minimum_age.is_zero() {
        return true;
    }
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= minimum_age)
}

fn validate_single_path_component(value: &str, label: &str) -> Result<(), String> {
    let mut components = Path::new(value).components();
    let valid = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !value.trim().is_empty();
    if valid {
        Ok(())
    } else {
        Err(format!("{label} 包含不安全的路径字符。"))
    }
}

fn validate_managed_path(root: &Path, target: &Path, allow_root: bool) -> Result<(), String> {
    if !root.exists() {
        return Err(format!("受控批次目录不存在：{}", root.display()));
    }
    let lexical_target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        return Err(format!("拒绝处理相对批次路径：{}", target.display()));
    };
    if !(lexical_target.starts_with(root) || lexical_target == root) {
        return Err(format!(
            "拒绝处理受控目录外的批次路径：{}",
            target.display()
        ));
    }
    if lexical_target == root && !allow_root {
        return Err("拒绝把批次根目录作为操作目标。".to_string());
    }

    reject_reparse_points(root, &lexical_target)?;
    let canonical_root = fs::canonicalize(root).map_err(to_string)?;
    let existing_ancestor = nearest_existing_ancestor(&lexical_target)
        .ok_or_else(|| format!("无法定位批次路径的现有父目录：{}", target.display()))?;
    let canonical_ancestor = fs::canonicalize(existing_ancestor).map_err(to_string)?;
    if !(canonical_ancestor.starts_with(&canonical_root) || canonical_ancestor == canonical_root) {
        return Err(format!(
            "批次路径经解析后逃逸受控目录：{}",
            target.display()
        ));
    }
    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn reject_reparse_points(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target.strip_prefix(root).map_err(to_string)?;
    let mut current = root.to_path_buf();
    if is_reparse_point(&fs::symlink_metadata(&current).map_err(to_string)?) {
        return Err(format!(
            "受控批次根目录不能是符号链接或目录联接：{}",
            root.display()
        ));
    }
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(format!("批次路径包含不安全组件：{}", target.display()));
        }
        current.push(component.as_os_str());
        if current.exists() && is_reparse_point(&fs::symlink_metadata(&current).map_err(to_string)?)
        {
            return Err(format!(
                "批次路径不能经过符号链接或目录联接：{}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & REPARSE_POINT_ATTRIBUTE != 0
    }
    #[cfg(not(windows))]
    {
        let _ = REPARSE_POINT_ATTRIBUTE;
        false
    }
}

fn remove_managed_directory(root: &Path, target: &Path) -> Result<(), String> {
    validate_managed_path(root, target, false)?;
    if target.exists() {
        fs::remove_dir_all(target).map_err(to_string)?;
    }
    Ok(())
}

fn chapter_batches_root_from_generation(generation_dir: &Path) -> Result<PathBuf, String> {
    generation_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "批次代次目录结构无效。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("yuri-batch-generation-{}", Uuid::new_v4()))
    }

    fn chapter(novel_id: &str, index: i64) -> Chapter {
        Chapter {
            id: format!("chapter-{index}"),
            novel_id: novel_id.to_string(),
            index,
            title: format!("第{index}章"),
            original_text: format!("正文{index}"),
            analysis_json: None,
            rewrite_text: None,
            rewrite_edited: false,
            single_rewrite_original_available: false,
            analysis_status: "pending".to_string(),
            rewrite_status: "pending".to_string(),
        }
    }

    fn seed_novel(conn: &Connection, novel_id: &str, chapters: &[Chapter]) {
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, detected_chapters, created_at)
             VALUES (?1, '测试', '', 'UTF-8', 'imported', 1, 'now')",
            params![novel_id],
        )
        .expect("insert novel");
        for chapter in chapters {
            conn.execute(
                "INSERT INTO chapters (id, novel_id, chapter_index, title, original_text, analysis_status, rewrite_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 'pending')",
                params![chapter.id, chapter.novel_id, chapter.index, chapter.title, chapter.original_text],
            )
            .expect("insert chapter");
        }
    }

    #[test]
    fn prepares_verified_files_in_an_immutable_generation() {
        let root = temp_dir();
        let chapters = vec![chapter("novel-1", 1), chapter("novel-1", 2)];
        let mut prepared = prepare_chapter_batch_generation(&root, "novel-1", &chapters, true, 30)
            .expect("prepare generation");
        assert_eq!(prepared.batches().len(), 1);
        let path = PathBuf::from(&prepared.batches()[0].file_path);
        assert!(path.is_file());
        assert!(path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().starts_with("generation-")));
        assert_eq!(
            fs::read_to_string(path).expect("read batch"),
            "第1章\n\n正文1\n\n第2章\n\n正文2"
        );
        prepared.discard().expect("discard generation");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_referenced_files_are_rebuilt_into_a_new_generation() {
        let root = temp_dir();
        let mut conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        let chapters = vec![chapter("novel-1", 1), chapter("novel-1", 2)];
        seed_novel(&conn, "novel-1", &chapters);
        let missing = root
            .join("chapter_batches")
            .join("novel-1")
            .join("legacy-missing.txt");
        fs::create_dir_all(missing.parent().expect("missing parent")).expect("create parent");
        conn.execute(
            "INSERT INTO chapter_batches (id, novel_id, batch_index, label, start_chapter, end_chapter, file_path, created_at)
             VALUES ('batch-1', 'novel-1', 1, '1-2章', 1, 2, ?1, 'now')",
            params![missing.to_string_lossy().to_string()],
        )
        .expect("insert batch");

        assert!(
            repair_chapter_batches_for_novel(&mut conn, &root, "novel-1").expect("repair batches")
        );
        let repaired: String = conn
            .query_row(
                "SELECT file_path FROM chapter_batches WHERE id = 'batch-1'",
                [],
                |row| row.get(0),
            )
            .expect("load repaired path");
        assert!(Path::new(&repaired).is_file());
        assert!(repaired.contains("generation-"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cleanup_removes_only_unreferenced_generations_and_legacy_files() {
        let root = temp_dir();
        let conn = Connection::open_in_memory().expect("open db");
        init_db(&conn).expect("init db");
        let chapters = vec![chapter("novel-1", 1)];
        seed_novel(&conn, "novel-1", &chapters);
        let mut kept = prepare_chapter_batch_generation(&root, "novel-1", &chapters, true, 30)
            .expect("prepare kept");
        insert_chapter_batch_rows(&conn, kept.batches()).expect("insert kept rows");
        kept.retain_after_commit();
        let orphan = root
            .join("chapter_batches")
            .join("novel-1")
            .join(format!("generation-{}", Uuid::new_v4()));
        fs::create_dir_all(&orphan).expect("create orphan");
        fs::write(orphan.join("batch-001.txt"), "orphan").expect("write orphan");
        let similarly_named = root
            .join("chapter_batches")
            .join("novel-1")
            .join("generation-not-a-uuid");
        fs::create_dir_all(&similarly_named).expect("create similarly named directory");
        let legacy = root
            .join("chapter_batches")
            .join("novel-1")
            .join("batch-001.txt");
        fs::write(&legacy, "legacy").expect("write legacy");

        cleanup_unreferenced_batch_files_for_novel(&conn, &root, "novel-1", Duration::ZERO)
            .expect("cleanup orphan files");
        assert!(!orphan.exists());
        assert!(!legacy.exists());
        assert!(similarly_named.exists());
        assert!(Path::new(&kept.batches()[0].file_path).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_novel_id_is_rejected_before_file_creation() {
        let root = temp_dir();
        let error = prepare_chapter_batch_generation(
            &root,
            "../outside",
            &[chapter("novel-1", 1)],
            true,
            30,
        )
        .expect_err("unsafe path should fail");
        assert!(error.contains("不安全"));
        assert!(!root.join("outside").exists());
        let _ = fs::remove_dir_all(root);
    }
}
