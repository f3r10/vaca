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
    created_at_unix INTEGER NOT NULL,
    -- Copied from VACA_MINT_URL when the pot is created. NULL is an older row
    -- that startup has not filled yet. Proof checks use this value.
    mint_url TEXT
);

-- One row per checked proof. `proof` is cashu proof JSON for a later melt.
-- The route does not return that JSON. An old database gets the columns in migrate.
CREATE TABLE IF NOT EXISTS contributions (
    id TEXT PRIMARY KEY,
    pot_id TEXT NOT NULL REFERENCES pots(id),
    created_at_unix INTEGER NOT NULL,
    amount_sats INTEGER NOT NULL DEFAULT 0,
    secret TEXT NOT NULL DEFAULT '',
    proof TEXT NOT NULL DEFAULT ''
);
";

pub struct StoredProof {
    pub id: String,
    pub proof: String,
}

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
    migrate(&connection)?;
    Ok(connection)
}

/// Adds columns that `CREATE TABLE IF NOT EXISTS` leaves missing on an old file.
fn migrate(connection: &Connection) -> Result<(), String> {
    if !has_column(connection, "amount_sats")? {
        connection
            .execute(
                "ALTER TABLE contributions ADD COLUMN amount_sats INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|err| err.to_string())?;
    }
    if !has_column(connection, "secret")? {
        connection
            .execute(
                "ALTER TABLE contributions ADD COLUMN secret TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|err| err.to_string())?;
    }
    if !has_column(connection, "proof")? {
        connection
            .execute(
                "ALTER TABLE contributions ADD COLUMN proof TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|err| err.to_string())?;
    }
    if !has_pot_column(connection, "mint_url")? {
        connection
            .execute("ALTER TABLE pots ADD COLUMN mint_url TEXT", [])
            .map_err(|err| err.to_string())?;
    }
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS contributions_secret ON contributions(secret)",
            [],
        )
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn has_pot_column(connection: &Connection, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare("SELECT 1 FROM pragma_table_info('pots') WHERE name = ?1")
        .map_err(|err| err.to_string())?;
    let mut rows = statement
        .query(params![column])
        .map_err(|err| err.to_string())?;
    Ok(rows.next().map_err(|err| err.to_string())?.is_some())
}

/// Fills pots created before `mint_url` existed. A pot that already has a URL,
/// including an empty one written at creation, is left alone.
pub fn remember_missing_mint(connection: &Connection, mint_url: &str) -> Result<u64, String> {
    if mint_url.is_empty() {
        return Ok(0);
    }
    let changed = connection
        .execute(
            "UPDATE pots SET mint_url = ?1 WHERE mint_url IS NULL",
            params![mint_url],
        )
        .map_err(|err| err.to_string())?;
    u64::try_from(changed).map_err(|err| err.to_string())
}

fn has_column(connection: &Connection, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare("SELECT 1 FROM pragma_table_info('contributions') WHERE name = ?1")
        .map_err(|err| err.to_string())?;
    let mut rows = statement
        .query(params![column])
        .map_err(|err| err.to_string())?;
    Ok(rows.next().map_err(|err| err.to_string())?.is_some())
}

pub struct NewContribution {
    pub amount_sats: u64,
    pub secret: String,
    pub proof: String,
}

pub fn secret_already_counted(connection: &Connection, secret: &str) -> Result<bool, ApiError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM contributions WHERE secret = ?1",
            params![secret],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    Ok(count > 0)
}

/// Inserts every row or none. A duplicate secret is rejected. The locked sum
/// is checked again inside the transaction.
pub fn insert_contributions(
    connection: &mut Connection,
    pot_id: &str,
    ceiling_sats: u64,
    rows: &[NewContribution],
    created_at_unix: i64,
) -> Result<(), ApiError> {
    if rows.is_empty() {
        return Err(ApiError::BadRequest("There is no proof.".to_string()));
    }
    let tx = connection.transaction().map_err(db_err)?;
    let locked = sum_locked(&tx, pot_id)?;
    let added = rows
        .iter()
        .try_fold(0u64, |acc, row| acc.checked_add(row.amount_sats))
        .ok_or_else(|| {
            ApiError::BadRequest("That amount passes the target plus the fee reserve.".to_string())
        })?;
    if locked
        .checked_add(added)
        .map(|total| total > ceiling_sats)
        .unwrap_or(true)
    {
        return Err(ApiError::BadRequest(
            "That amount passes the target plus the fee reserve.".to_string(),
        ));
    }
    for row in rows {
        if row.amount_sats == 0 {
            return Err(ApiError::BadRequest(
                "The proof amount is empty.".to_string(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let amount = i64::try_from(row.amount_sats).map_err(|_| ApiError::Database)?;
        tx.execute(
            "INSERT INTO contributions (
                id, pot_id, created_at_unix, amount_sats, secret, proof
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, pot_id, created_at_unix, amount, row.secret, row.proof],
        )
        .map_err(|err| {
            if unique_constraint(&err) {
                tracing::info!(pot_id, "contribution already counted");
                ApiError::BadRequest("This proof is already counted.".to_string())
            } else {
                db_err(err)
            }
        })?;
    }
    tx.commit().map_err(db_err)?;
    Ok(())
}

fn sum_locked(connection: &Connection, pot_id: &str) -> Result<u64, ApiError> {
    let locked: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(amount_sats), 0) FROM contributions WHERE pot_id = ?1",
            params![pot_id],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    u64::try_from(locked).map_err(|_| ApiError::Database)
}

fn unique_constraint(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(sqlite, _) => sqlite.extended_code == 2067,
        _ => false,
    }
}

pub fn insert_pot(
    connection: &Connection,
    id: &str,
    pot: &NewPot,
    mint_url: &str,
    created_at_unix: i64,
) -> Result<StoredPot, ApiError> {
    connection
        .execute(
            "INSERT INTO pots (
                id, title, kind, target_sats, deadline_unix, destination,
                organizer_pubkey, status, created_at_unix, mint_url
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9)",
            params![
                id,
                pot.title,
                pot.kind,
                i64::try_from(pot.target_sats).map_err(|_| ApiError::Database)?,
                pot.deadline_unix,
                pot.destination,
                pot.organizer_pubkey,
                created_at_unix,
                mint_url,
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

pub fn proofs_for_pot(connection: &Connection, pot_id: &str) -> Result<Vec<StoredProof>, ApiError> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT id, proof FROM contributions WHERE pot_id = ?1"
        ))
        .map_err(db_err)?;
    let rows = statement
        .query_map(params![pot_id], map_proof)
        .map_err(db_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_err)?;
    Ok(rows)
}

pub fn update_proof_for_pot(
    connection: &Connection,
    proof: &str,
    id: &str,
    pot_id: &str,
) -> Result<(), ApiError> {
    let mut statement = connection
        .prepare(&format!(
            "UPDATE contributions SET proof = ?1 WHERE id = ?2 AND pot_id = ?3"
        ))
        .map_err(db_err)?;
    let changed = statement.execute([proof, id, pot_id]).map_err(db_err)?;

    if changed == 1 {
        Ok(())
    } else {
        Err(ApiError::Database)
    }
}

/// Replaces every stored proof, or none of them. `rows` is `(contribution id, proof json)`.
pub fn update_proofs_for_pot(
    connection: &mut Connection,
    pot_id: &str,
    rows: &[(String, String)],
) -> Result<(), ApiError> {
    let tx = connection.transaction().map_err(db_err)?;
    for (id, proof) in rows {
        update_proof_for_pot(&tx, proof, id, pot_id)?;
    }
    tx.commit().map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
fn contributions_store_a_proof(connection: &Connection) -> Result<bool, ApiError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('contributions')
             WHERE name IN ('amount_sats', 'secret', 'proof')",
            [],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    Ok(count == 3)
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
    (SELECT COUNT(*) FROM contributions c WHERE c.pot_id = p.id) AS contribution_count,
    (SELECT COALESCE(SUM(amount_sats), 0) FROM contributions c WHERE c.pot_id = p.id) AS locked_sats,
    p.mint_url
FROM pots p
";

fn map_proof(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredProof> {
    Ok(StoredProof {
        id: row.get(0)?,
        proof: row.get(1)?,
    })
}

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
        locked_sats: {
            let locked: i64 = row.get(10)?;
            u64::try_from(locked).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Integer,
                    Box::new(err),
                )
            })?
        },
        mint_url: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
    })
}

fn db_err(err: rusqlite::Error) -> ApiError {
    tracing::error!(error = %err, "pot store query failed");
    ApiError::Database
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ApiError;
    use crate::pot::NewPot;

    #[test]
    fn round_trip_stays_open_with_nothing_locked() {
        let path = std::env::temp_dir().join(format!("vaca-db-{}.sqlite", uuid::Uuid::new_v4()));
        let connection = open(&path).expect("open");
        assert!(contributions_store_a_proof(&connection).unwrap());
        let pot = NewPot {
            title: "Cause".to_string(),
            kind: "cause".to_string(),
            target_sats: 100_000,
            deadline_unix: 1_800_000_000,
            destination: "lno1qq".to_string(),
            organizer_pubkey: None,
        };
        let stored = insert_pot(&connection, "pot-1", &pot, "", 1_700_000_000).expect("insert");
        assert_eq!(stored.mint_url, "");
        assert_eq!(stored.status, "open");
        assert_eq!(stored.contribution_count, 0);
        assert_eq!(stored.locked_sats, 0);
        assert_eq!(list_pots(&connection).unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrate_adds_proof_columns_to_an_old_table() {
        let path =
            std::env::temp_dir().join(format!("vaca-db-old-{}.sqlite", uuid::Uuid::new_v4()));
        let connection = Connection::open(&path).expect("open");
        connection
            .execute_batch(
                "CREATE TABLE contributions (
                    id TEXT PRIMARY KEY,
                    pot_id TEXT NOT NULL,
                    created_at_unix INTEGER NOT NULL
                );",
            )
            .expect("old table");
        drop(connection);
        let mut connection = open(&path).expect("migrate");
        assert!(contributions_store_a_proof(&connection).unwrap());
        let pot = NewPot {
            title: "Cause".to_string(),
            kind: "cause".to_string(),
            target_sats: 100,
            deadline_unix: 1_800_000_000,
            destination: "lno1qq".to_string(),
            organizer_pubkey: None,
        };
        insert_pot(&connection, "pot-1", &pot, "", 1_700_000_000).expect("pot");
        let row = NewContribution {
            amount_sats: 4,
            secret: "one".to_string(),
            proof: "{}".to_string(),
        };
        insert_contributions(&mut connection, "pot-1", 100, &[row], 1_700_000_001).expect("row");
        let again = NewContribution {
            amount_sats: 4,
            secret: "one".to_string(),
            proof: "{}".to_string(),
        };
        let err = insert_contributions(&mut connection, "pot-1", 100, &[again], 1_700_000_002)
            .expect_err("duplicate");
        match err {
            ApiError::BadRequest(message) => {
                assert_eq!(message, "This proof is already counted.");
            }
            other => panic!("unexpected {other:?}"),
        }
        let stored = get_pot(&connection, "pot-1").unwrap().unwrap();
        assert_eq!(stored.locked_sats, 4);
        assert_eq!(stored.contribution_count, 1);
        assert_eq!(stored.status, "open");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remember_missing_mint_fills_old_rows_once() {
        let path =
            std::env::temp_dir().join(format!("vaca-db-mint-{}.sqlite", uuid::Uuid::new_v4()));
        let connection = Connection::open(&path).expect("open");
        connection
            .execute_batch(
                "CREATE TABLE pots (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    target_sats INTEGER NOT NULL,
                    deadline_unix INTEGER NOT NULL,
                    destination TEXT NOT NULL,
                    organizer_pubkey TEXT,
                    status TEXT NOT NULL,
                    created_at_unix INTEGER NOT NULL
                );
                INSERT INTO pots (
                    id, title, kind, target_sats, deadline_unix, destination,
                    organizer_pubkey, status, created_at_unix
                ) VALUES (
                    'old', 'Cause', 'cause', 1000, 1800000000, 'asd',
                    NULL, 'open', 1700000000
                );",
            )
            .expect("old pots");
        drop(connection);
        let connection = open(&path).expect("migrate");
        let filled = remember_missing_mint(&connection, "http://127.0.0.1:8085").expect("fill");
        assert_eq!(filled, 1);
        let stored = get_pot(&connection, "old").unwrap().unwrap();
        assert_eq!(stored.mint_url, "http://127.0.0.1:8085");
        let again = remember_missing_mint(&connection, "http://mint.example").expect("again");
        assert_eq!(again, 0);
        assert_eq!(
            get_pot(&connection, "old").unwrap().unwrap().mint_url,
            "http://127.0.0.1:8085"
        );
        let pot = NewPot {
            title: "Cause".to_string(),
            kind: "cause".to_string(),
            target_sats: 100,
            deadline_unix: 1_800_000_000,
            destination: "lno1qq".to_string(),
            organizer_pubkey: None,
        };
        insert_pot(&connection, "empty", &pot, "", 1_700_000_000).expect("empty");
        insert_pot(
            &connection,
            "kept",
            &pot,
            "http://kept.example",
            1_700_000_000,
        )
        .expect("kept");
        let skipped = remember_missing_mint(&connection, "http://mint.example").expect("skip");
        assert_eq!(skipped, 0);
        assert_eq!(get_pot(&connection, "empty").unwrap().unwrap().mint_url, "");
        assert_eq!(
            get_pot(&connection, "kept").unwrap().unwrap().mint_url,
            "http://kept.example"
        );
        let untouched = remember_missing_mint(&connection, "").expect("blank");
        assert_eq!(untouched, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_signature_write_keeps_every_proof_unsigned() {
        let path = std::env::temp_dir().join(format!("vaca-db-{}.sqlite", uuid::Uuid::new_v4()));
        let mut connection = open(&path).expect("open");
        let pot = NewPot {
            title: "Cause".to_string(),
            kind: "cause".to_string(),
            target_sats: 1_000,
            deadline_unix: 1_800_000_000,
            destination: "lno1qq".to_string(),
            organizer_pubkey: None,
        };
        insert_pot(&connection, "pot-1", &pot, "http://127.0.0.1:9", 1_700_000_000).expect("pot");
        insert_contributions(
            &mut connection,
            "pot-1",
            1_001,
            &[
                NewContribution {
                    amount_sats: 500,
                    secret: "one".to_string(),
                    proof: "unsigned-one".to_string(),
                },
                NewContribution {
                    amount_sats: 501,
                    secret: "two".to_string(),
                    proof: "unsigned-two".to_string(),
                },
            ],
            1_700_000_001,
        )
        .expect("rows");
        let stored = proofs_for_pot(&connection, "pot-1").expect("proofs");
        let err = update_proofs_for_pot(
            &mut connection,
            "pot-1",
            &[
                (stored[0].id.clone(), "signed-one".to_string()),
                ("missing-row".to_string(), "signed-two".to_string()),
            ],
        )
        .expect_err("missing row");
        assert!(matches!(err, ApiError::Database));
        let mut texts: Vec<String> = proofs_for_pot(&connection, "pot-1")
            .expect("proofs")
            .into_iter()
            .map(|proof| proof.proof)
            .collect();
        texts.sort();
        assert_eq!(
            texts,
            vec!["unsigned-one".to_string(), "unsigned-two".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }
}
