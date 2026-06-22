use super::migrations;
use crate::repositories::logs::extract_token_usage;
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
            top_p REAL NOT NULL DEFAULT 1.0,
            thinking_mode TEXT NOT NULL DEFAULT 'auto',
            prompt_obfuscation_enabled INTEGER NOT NULL DEFAULT 0,
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
            finish_reason TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS token_usage_records (
            id TEXT PRIMARY KEY,
            novel_id TEXT,
            profile_id TEXT NOT NULL,
            profile_name TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS novel_settings (
            novel_id TEXT PRIMARY KEY,
            protagonist_name TEXT NOT NULL,
            protagonist_aliases TEXT NOT NULL DEFAULT '',
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

        CREATE TABLE IF NOT EXISTS chapter_rewrite_snapshots (
            chapter_id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            rewrite_text TEXT NOT NULL,
            ai_rewrite_text TEXT,
            rewrite_edited_at TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY(chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
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
        CREATE INDEX IF NOT EXISTS idx_token_usage_created ON token_usage_records(created_at);
        CREATE INDEX IF NOT EXISTS idx_token_usage_profile_created
            ON token_usage_records(profile_id, created_at);
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
    migrations::ensure_column(conn, "model_profiles", "top_p", "REAL NOT NULL DEFAULT 1.0")?;
    migrations::ensure_column(
        conn,
        "model_profiles",
        "prompt_obfuscation_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    migrations::ensure_column(conn, "ai_logs", "reasoning", "TEXT")?;
    migrations::ensure_column(conn, "ai_logs", "raw_response", "TEXT")?;
    migrations::ensure_column(conn, "ai_logs", "finish_reason", "TEXT")?;
    migrations::ensure_column(conn, "ai_logs", "input_tokens", "INTEGER")?;
    migrations::ensure_column(conn, "ai_logs", "output_tokens", "INTEGER")?;
    backfill_ai_log_token_usage(conn)?;
    backfill_token_usage_records(conn)?;
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
        "protagonist_aliases",
        "TEXT NOT NULL DEFAULT ''",
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

fn backfill_ai_log_token_usage(conn: &Connection) -> rusqlite::Result<()> {
    let already_completed = conn
        .query_row(
            "SELECT 1 FROM app_settings WHERE key = 'token_usage_backfill_v1'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if already_completed {
        return Ok(());
    }
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, raw_response FROM ai_logs
             WHERE input_tokens IS NULL AND output_tokens IS NULL AND raw_response IS NOT NULL",
        )?;
        let mapped = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    for (id, raw_response) in rows {
        if let Some((input_tokens, output_tokens)) = extract_token_usage(Some(&raw_response)) {
            conn.execute(
                "UPDATE ai_logs SET input_tokens = ?1, output_tokens = ?2 WHERE id = ?3",
                rusqlite::params![input_tokens as i64, output_tokens as i64, id],
            )?;
        }
    }
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('token_usage_backfill_v1', 'completed')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    Ok(())
}

fn backfill_token_usage_records(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO token_usage_records (
            id, novel_id, profile_id, profile_name, model,
            input_tokens, output_tokens, created_at
         )
         SELECT logs.id,
                logs.novel_id,
                logs.profile_id,
                COALESCE(profiles.name, logs.profile_id),
                COALESCE(profiles.model, logs.profile_id),
                MAX(COALESCE(logs.input_tokens, 0), 0),
                MAX(COALESCE(logs.output_tokens, 0), 0),
                logs.created_at
         FROM ai_logs AS logs
         LEFT JOIN model_profiles AS profiles ON profiles.id = logs.profile_id
         WHERE logs.input_tokens IS NOT NULL OR logs.output_tokens IS NOT NULL",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_adds_default_top_p_to_existing_profiles() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE model_profiles (
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
            INSERT INTO model_profiles VALUES (
                'profile-1', '测试模型', 'openai-compatible', 'https://example.com/v1',
                'model-a', 0.7, 'auto', NULL, 'now'
            );
            "#,
        )
        .expect("seed previous model profile schema");

        init_db(&conn).expect("migrate schema");

        let top_p: f64 = conn
            .query_row(
                "SELECT top_p FROM model_profiles WHERE id = 'profile-1'",
                [],
                |row| row.get(0),
            )
            .expect("load top p");
        assert_eq!(top_p, 1.0);
    }

    #[test]
    fn migration_disables_prompt_obfuscation_for_existing_profiles() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE model_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                provider TEXT NOT NULL,
                base_url TEXT NOT NULL,
                model TEXT NOT NULL,
                temperature REAL NOT NULL,
                top_p REAL NOT NULL DEFAULT 1.0,
                thinking_mode TEXT NOT NULL DEFAULT 'auto',
                api_key TEXT,
                updated_at TEXT NOT NULL
            );
            INSERT INTO model_profiles (
                id, name, provider, base_url, model, temperature, top_p,
                thinking_mode, api_key, updated_at
            ) VALUES (
                'profile-1', '测试模型', 'openai-compatible', 'https://example.com/v1',
                'model-a', 0.7, 1.0, 'auto', NULL, 'now'
            );
            "#,
        )
        .expect("seed previous model profile schema");

        init_db(&conn).expect("migrate schema");

        let enabled: bool = conn
            .query_row(
                "SELECT prompt_obfuscation_enabled FROM model_profiles WHERE id = 'profile-1'",
                [],
                |row| row.get(0),
            )
            .expect("load prompt obfuscation setting");
        assert!(!enabled);
    }

    #[test]
    fn migration_adds_empty_protagonist_aliases_to_existing_settings() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE novel_settings (
                novel_id TEXT PRIMARY KEY,
                protagonist_name TEXT NOT NULL,
                rewritten_protagonist_name TEXT NOT NULL DEFAULT '',
                additional_feminize_names TEXT NOT NULL,
                bust TEXT NOT NULL,
                body_type TEXT NOT NULL,
                rewrite_mode TEXT NOT NULL DEFAULT 'strict',
                advanced_settings TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL
            );
            INSERT INTO novel_settings VALUES (
                'novel-1', '萧炎', '萧妍', '', '平胸', '少女', 'strict', '', 'now'
            );
            "#,
        )
        .expect("seed previous settings schema");

        init_db(&conn).expect("migrate schema");

        let aliases: String = conn
            .query_row(
                "SELECT protagonist_aliases FROM novel_settings WHERE novel_id = 'novel-1'",
                [],
                |row| row.get(0),
            )
            .expect("load aliases");
        assert!(aliases.is_empty());
    }

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
    fn migration_backfills_token_usage_from_existing_raw_responses() {
        let conn = Connection::open_in_memory().expect("open database");
        conn.execute_batch(
            r#"
            CREATE TABLE ai_logs (
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
            INSERT INTO ai_logs (
                id, novel_id, profile_id, action, chapter_title, status,
                content, raw_response, created_at
            ) VALUES (
                'log-1', NULL, 'profile-1', '分析', NULL, 'success', 'ok',
                '{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":321,"completion_tokens":54}}',
                '2026-06-19T00:00:00Z'
            );
            "#,
        )
        .expect("seed legacy logs");
        init_db(&conn).expect("migrate schema");
        let usage: (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT input_tokens, output_tokens FROM ai_logs WHERE id = 'log-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load token usage");
        assert_eq!(usage, (Some(321), Some(54)));
    }

    #[test]
    fn migration_copies_existing_log_usage_into_independent_records() {
        let conn = Connection::open_in_memory().expect("open database");
        init_db(&conn).expect("initialize schema");
        conn.execute(
            "INSERT INTO model_profiles (
                id, name, provider, base_url, model, temperature, thinking_mode, updated_at
             ) VALUES (
                'profile-1', '历史模型', 'openai-compatible', 'https://example.com',
                'model-a', 0.7, 'auto', 'now'
             )",
            [],
        )
        .expect("insert profile");
        conn.execute(
            "INSERT INTO ai_logs (
                id, profile_id, action, status, content,
                input_tokens, output_tokens, created_at
             ) VALUES (
                'log-1', 'profile-1', '分析', 'success', 'ok',
                120, 45, '2026-06-22T10:00:00+08:00'
             )",
            [],
        )
        .expect("insert legacy log");

        init_db(&conn).expect("backfill independent usage");

        let usage: (String, String, i64, i64) = conn
            .query_row(
                "SELECT profile_name, model, input_tokens, output_tokens
                 FROM token_usage_records WHERE id = 'log-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("load migrated usage");
        assert_eq!(usage, ("历史模型".to_string(), "model-a".to_string(), 120, 45));
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
