use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::ApiError;
use crate::pot::{NewPot, StoredPot};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pots (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    kind TEXT NOT NULL,
    target_sats INTEGER NOT NULL,
    deadline_unix INTEGER NOT NULL,
    destination TEXT NOT NULL,
    organizer_pubkey TEXT,
    status TEXT NOT NULL CHECK (status = 'open'),
    created_at_unix INTEGER NOT NULL
);

-- No proof column and no insert path. Counting a proof is money-path work.
CREATE TABLE IF NOT EXISTS contributions (
    id TEXT PRIMARY KEY,
    pot_id TEXT NOT NULL REFERENCES pots(id),
    created_at_unix INTEGER NOT NULL
);
";

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let connection = Connection::open(path).map_err(|err| err.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| err.to_string())?;
    connection
        .execute_batch(SCHEMA)
        .map_err(|err| err.to_string())?;
    Ok(connection)
}

pub fn insert_pot(
    connection: &Connection,
    id: &str,
    pot: &NewPot,
    created_at_unix: i64,
) -> Result<StoredPot, ApiError> {
    connection
        .execute(
            "INSERT INTO pots (
                id, title, kind, target_sats, deadline_unix, destination,
                organizer_pubkey, status, created_at_unix
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8)",
            params![
                id,
                pot.title,
                pot.kind,
                i64::try_from(pot.target_sats).map_err(|_| ApiError::Database)?,
                pot.deadline_unix,
                pot.destination,
                pot.organizer_pubkey,
                created_at_unix,
            ],
        )
        .map_err(|err| {
            tracing::error!(error = %err, "insert pot failed");
            ApiError::Database
        })?;
    get_pot(connection, id)?.ok_or(ApiError::Database)
}

pub fn list_pots(connection: &Connection) -> Result<Vec<StoredPot>, ApiError> {
    let mut statement = connection
        .prepare(&format!(
            "{POT_SELECT} ORDER BY created_at_unix DESC, id ASC"
        ))
        .map_err(db_err)?;
    let rows = statement
        .query_map([], map_pot)
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

pub fn get_pot(connection: &Connection, id: &str) -> Result<Option<StoredPot>, ApiError> {
    if id.len() > 64 {
        return Ok(None);
    }
    let mut statement = connection
        .prepare(&format!("{POT_SELECT} WHERE p.id = ?1"))
        .map_err(db_err)?;
    let mut rows = statement.query(params![id]).map_err(db_err)?;
    match rows.next().map_err(db_err)? {
        Some(row) => Ok(Some(map_pot(row).map_err(db_err)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
fn contributions_have_no_proof_column(connection: &Connection) -> Result<bool, ApiError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('contributions')
             WHERE name IN ('proof', 'secret', 'witness', 'key')",
            [],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    Ok(count == 0)
}

const POT_SELECT: &str = "
SELECT
    p.id,
    p.title,
    p.kind,
    p.target_sats,
    p.deadline_unix,
    p.destination,
    p.organizer_pubkey,
    p.status,
    p.created_at_unix,
    (SELECT COUNT(*) FROM contributions c WHERE c.pot_id = p.id) AS contribution_count
FROM pots p
";

fn map_pot(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredPot> {
    let target: i64 = row.get(3)?;
    let target_sats = u64::try_from(target).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Integer, Box::new(err))
    })?;
    Ok(StoredPot {
        id: row.get(0)?,
        title: row.get(1)?,
        kind: row.get(2)?,
        target_sats,
        deadline_unix: row.get(4)?,
        destination: row.get(5)?,
        organizer_pubkey: row.get(6)?,
        status: row.get(7)?,
        created_at_unix: row.get(8)?,
        contribution_count: row.get(9)?,
    })
}

fn db_err(err: rusqlite::Error) -> ApiError {
    tracing::error!(error = %err, "pot store query failed");
    ApiError::Database
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pot::NewPot;

    #[test]
    fn round_trip_stays_open_and_stores_no_proofs() {
        let path = std::env::temp_dir().join(format!("vaca-db-{}.sqlite", uuid::Uuid::new_v4()));
        let connection = open(&path).expect("open");
        assert!(contributions_have_no_proof_column(&connection).unwrap());
        let pot = NewPot {
            title: "Cause".to_string(),
            kind: "cause".to_string(),
            target_sats: 100_000,
            deadline_unix: 1_800_000_000,
            destination: "lno1qq".to_string(),
            organizer_pubkey: None,
        };
        let stored = insert_pot(&connection, "pot-1", &pot, 1_700_000_000).expect("insert");
        assert_eq!(stored.status, "open");
        assert_eq!(stored.contribution_count, 0);
        assert_eq!(list_pots(&connection).unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
