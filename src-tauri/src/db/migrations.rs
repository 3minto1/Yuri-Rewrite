use crate::credentials::write_api_key;
use rusqlite::{params, Connection};

pub(super) fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!(
                "ALTER TABLE {} ADD COLUMN {} {}",
                table, column, column_type
            ),
            [],
        )?;
    }
    Ok(())
}

pub(super) fn migrate_api_keys_to_keyring(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, api_key FROM model_profiles WHERE api_key IS NOT NULL AND trim(api_key) != ''",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    for (profile_id, api_key) in rows {
        if write_api_key(&profile_id, &api_key).is_ok() {
            conn.execute(
                "UPDATE model_profiles SET api_key = NULL WHERE id = ?1",
                params![profile_id],
            )?;
        }
    }
    Ok(())
}
