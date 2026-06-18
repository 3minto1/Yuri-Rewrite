use super::migrations;
use rusqlite::Connection;

pub(crate) fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS novels (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source_path TEXT NOT NULL,
            encoding TEXT NOT NULL,
            status TEXT NOT NULL,
            detected_chapters INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chapters (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            chapter_index INTEGER NOT NULL,
            title TEXT NOT NULL,
            original_text TEXT NOT NULL,
            analysis_json TEXT,
            rewrite_text TEXT,
            ai_rewrite_text TEXT,
            rewrite_edited_at TEXT,
            analysis_status TEXT NOT NULL,
            rewrite_status TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS canon_assets (
            novel_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY(novel_id, kind)
        );

        CREATE TABLE IF NOT EXISTS model_profiles (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            provider TEXT NOT NULL,
            base_url TEXT NOT NULL,
            model TEXT NOT NULL,
            temperature REAL NOT NULL,
            thinking_mode TEXT NOT NULL DEFAULT 'auto',
            api_key TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            current_chapter INTEGER NOT NULL,
            total_chapters INTEGER NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_logs (
            id TEXT PRIMARY KEY,
            novel_id TEXT,
            profile_id TEXT NOT NULL,
            action TEXT NOT NULL,
            chapter_title TEXT,
            status TEXT NOT NULL,
            content TEXT NOT NULL,
            reasoning TEXT,
            raw_response TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS novel_settings (
            novel_id TEXT PRIMARY KEY,
            protagonist_name TEXT NOT NULL,
            rewritten_protagonist_name TEXT NOT NULL DEFAULT '',
            additional_feminize_names TEXT NOT NULL,
            bust TEXT NOT NULL,
            body_type TEXT NOT NULL,
            rewrite_mode TEXT NOT NULL DEFAULT 'strict',
            advanced_settings TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS chapter_batches (
            id TEXT PRIMARY KEY,
            novel_id TEXT NOT NULL,
            batch_index INTEGER NOT NULL,
            label TEXT NOT NULL,
            start_chapter INTEGER NOT NULL,
            end_chapter INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS auto_run_checkpoints (
            novel_id TEXT PRIMARY KEY,
            start_batch_index INTEGER NOT NULL,
            next_batch_index INTEGER NOT NULL,
            job_id TEXT,
            status TEXT NOT NULL,
            pause_reason TEXT NOT NULL DEFAULT '',
            phase TEXT,
            batch_index INTEGER,
            profile_ids TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY(novel_id) REFERENCES novels(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS auto_run_shard_outputs (
            novel_id TEXT NOT NULL,
            batch_index INTEGER NOT NULL,
            phase TEXT NOT NULL,
            chapter_id TEXT NOT NULL,
            chapter_index INTEGER NOT NULL,
            title TEXT,
            content TEXT,
            created_at TEXT NOT NULL,
            PRIMARY KEY(novel_id, batch_index, phase, chapter_id),
            FOREIGN KEY(novel_id) REFERENCES auto_run_checkpoints(novel_id) ON DELETE CASCADE,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_chapters_novel ON chapters(novel_id, chapter_index);
        CREATE INDEX IF NOT EXISTS idx_jobs_novel ON jobs(novel_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_logs_created ON ai_logs(created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_logs_novel ON ai_logs(novel_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_logs_profile_created ON ai_logs(profile_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_chapter_batches_novel ON chapter_batches(novel_id, batch_index);
        CREATE INDEX IF NOT EXISTS idx_auto_run_shard_outputs_phase
            ON auto_run_shard_outputs(novel_id, batch_index, phase, chapter_index);
        "#,
    )?;
    migrations::ensure_column(conn, "model_profiles", "api_key", "TEXT")?;
    migrations::ensure_column(
        conn,
        "model_profiles",
        "thinking_mode",
        "TEXT NOT NULL DEFAULT 'auto'",
    )?;
    migrations::ensure_column(conn, "ai_logs", "reasoning", "TEXT")?;
    migrations::ensure_column(conn, "ai_logs", "raw_response", "TEXT")?;
    migrations::ensure_column(conn, "chapters", "ai_rewrite_text", "TEXT")?;
    migrations::ensure_column(conn, "chapters", "rewrite_edited_at", "TEXT")?;
    migrations::ensure_column(
        conn,
        "novels",
        "detected_chapters",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    conn.execute(
        "UPDATE novels SET detected_chapters = 0 WHERE EXISTS (
            SELECT 1 FROM chapter_batches
            WHERE chapter_batches.novel_id = novels.id
              AND chapter_batches.label LIKE '%约10万字%'
        )",
        [],
    )?;
    conn.execute(
        "UPDATE chapters SET ai_rewrite_text = rewrite_text WHERE ai_rewrite_text IS NULL AND rewrite_text IS NOT NULL AND trim(rewrite_text) != ''",
        [],
    )?;
    migrations::ensure_column(
        conn,
        "novel_settings",
        "rewritten_protagonist_name",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    migrations::ensure_column(
        conn,
        "novel_settings",
        "advanced_settings",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    migrations::ensure_column(
        conn,
        "novel_settings",
        "rewrite_mode",
        "TEXT NOT NULL DEFAULT 'strict'",
    )?;
    migrations::migrate_api_keys_to_keyring(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_existing_rewrite_as_ai_baseline() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE chapters (
                id TEXT PRIMARY KEY,
                novel_id TEXT NOT NULL,
                chapter_index INTEGER NOT NULL,
                title TEXT NOT NULL,
                original_text TEXT NOT NULL,
                analysis_json TEXT,
                rewrite_text TEXT,
                analysis_status TEXT NOT NULL,
                rewrite_status TEXT NOT NULL
            );
            INSERT INTO chapters VALUES (
                'chapter-1', 'novel-1', 1, '第一章', '原文', NULL, '已有改写',
                'completed', 'completed'
            );
            "#,
        )
        .expect("seed old schema");

        init_db(&conn).expect("migrate schema");

        let (baseline, edited_at): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT ai_rewrite_text, rewrite_edited_at FROM chapters WHERE id = 'chapter-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load migrated chapter");
        assert_eq!(baseline.as_deref(), Some("已有改写"));
        assert!(edited_at.is_none());
    }

    #[test]
    fn creates_auto_run_checkpoint_table() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (novel_id, start_batch_index, next_batch_index, status, profile_ids, created_at, updated_at) VALUES ('novel-1', 0, 2, 'paused', '[\"profile-1\"]', 'now', 'now')",
            [],
        )
        .expect("insert checkpoint");
        let next: i64 = conn
            .query_row(
                "SELECT next_batch_index FROM auto_run_checkpoints WHERE novel_id = 'novel-1'",
                [],
                |row| row.get(0),
            )
            .expect("load checkpoint");
        assert_eq!(next, 2);
    }

    #[test]
    fn keeps_partial_auto_run_shard_outputs_until_checkpoint_cleanup() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, created_at)
             VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO chapters (
                id, novel_id, chapter_index, title, original_text,
                analysis_status, rewrite_status
             ) VALUES ('chapter-1', 'novel-1', 1, '第一章', '正文', 'pending', 'pending')",
            [],
        )
        .expect("insert chapter");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (
                novel_id, start_batch_index, next_batch_index, status,
                profile_ids, created_at, updated_at
             ) VALUES ('novel-1', 0, 0, 'paused', '[]', 'now', 'now')",
            [],
        )
        .expect("insert checkpoint");
        conn.execute(
            "INSERT INTO auto_run_shard_outputs (
                novel_id, batch_index, phase, chapter_id, chapter_index, content, created_at
             ) VALUES ('novel-1', 1, 'analysis', 'chapter-1', 1, '{\"summary\":\"完成\"}', 'now')",
            [],
        )
        .expect("stage shard output");

        init_db(&conn).expect("reopen schema");
        let staged_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_run_shard_outputs", [], |row| {
                row.get(0)
            })
            .expect("count staged outputs");
        assert_eq!(staged_count, 1);

        conn.execute(
            "DELETE FROM auto_run_checkpoints WHERE novel_id = 'novel-1'",
            [],
        )
        .expect("delete checkpoint");
        let staged_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_run_shard_outputs", [], |row| {
                row.get(0)
            })
            .expect("count staged outputs after cleanup");
        assert_eq!(staged_count, 0);
    }

    #[test]
    fn backfills_legacy_character_split_novels() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE novels (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                source_path TEXT NOT NULL,
                encoding TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE chapter_batches (
                id TEXT PRIMARY KEY,
                novel_id TEXT NOT NULL,
                batch_index INTEGER NOT NULL,
                label TEXT NOT NULL,
                start_chapter INTEGER NOT NULL,
                end_chapter INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            INSERT INTO novels VALUES ('chapter-novel', '章节小说', 'a.txt', 'UTF-8', 'imported', 'now');
            INSERT INTO novels VALUES ('text-novel', '长文本', 'b.txt', 'UTF-8', 'imported', 'now');
            INSERT INTO chapter_batches VALUES ('batch-1', 'chapter-novel', 1, '1-30章', 1, 30, 'a', 'now');
            INSERT INTO chapter_batches VALUES ('batch-2', 'text-novel', 1, '第1批（约10万字）', 1, 1, 'b', 'now');
            "#,
        )
        .expect("seed old schema");

        init_db(&conn).expect("migrate schema");

        let chapter_detected: bool = conn
            .query_row(
                "SELECT detected_chapters FROM novels WHERE id = 'chapter-novel'",
                [],
                |row| row.get(0),
            )
            .expect("load chapter novel flag");
        let text_detected: bool = conn
            .query_row(
                "SELECT detected_chapters FROM novels WHERE id = 'text-novel'",
                [],
                |row| row.get(0),
            )
            .expect("load text novel flag");
        assert!(chapter_detected);
        assert!(!text_detected);
    }

    #[test]
    fn deleting_novel_cascades_auto_run_checkpoint() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO novels (id, title, source_path, encoding, status, created_at) VALUES ('novel-1', '测试', 'a.txt', 'UTF-8', 'imported', 'now')",
            [],
        )
        .expect("insert novel");
        conn.execute(
            "INSERT INTO auto_run_checkpoints (novel_id, start_batch_index, next_batch_index, status, profile_ids, created_at, updated_at) VALUES ('novel-1', 0, 0, 'paused', '[]', 'now', 'now')",
            [],
        )
        .expect("insert checkpoint");

        conn.execute("DELETE FROM novels WHERE id = 'novel-1'", [])
            .expect("delete novel");

        let checkpoint_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_run_checkpoints", [], |row| {
                row.get(0)
            })
            .expect("count checkpoints");
        assert_eq!(checkpoint_count, 0);
    }
}
