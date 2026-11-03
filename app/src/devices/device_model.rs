//! The devices table: the devices paired with the app. Their tokens are in the key vault
//! (account::key_vault), not here.

use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection, SqlitePool};

/// A paired device, as the Devices screen lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: i64,
    pub name: String,
    pub paired_at: i64,
    /// Its last request; None until it sends one after pairing.
    pub last_seen_at: Option<i64>,
}

/// Keeps a device named name, paired at now; its id.
pub async fn add(db: &SqlitePool, name: &str, now: i64) -> sqlx::Result<i64> {
    let added = sqlx::query!(
        r#"
        INSERT INTO devices (name, paired_at)
        VALUES (?1, ?2)
        "#,
        name,
        now
    )
    .execute(db)
    .await?;
    Ok(added.last_insert_rowid())
}

/// The device with this id, or None.
pub async fn find(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Device>> {
    sqlx::query_as!(
        Device,
        r#"
        SELECT id AS "id!", name, paired_at, last_seen_at
        FROM devices
        WHERE id = ?1
        "#,
        id
    )
    .fetch_optional(db)
    .await
}

/// Every paired device, the one paired first first.
pub async fn all(db: &SqlitePool) -> sqlx::Result<Vec<Device>> {
    sqlx::query_as!(
        Device,
        r#"
        SELECT id AS "id!", name, paired_at, last_seen_at
        FROM devices
        ORDER BY paired_at, id
        "#
    )
    .fetch_all(db)
    .await
}

/// Notes that the device with this id sent a request at now.
pub async fn seen(db: &SqlitePool, id: i64, now: i64) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        UPDATE devices
        SET last_seen_at = ?2
        WHERE id = ?1
        "#,
        id,
        now
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Forgets the device with this id; whether there was one.
pub async fn remove(db: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let removed = sqlx::query!(
        r#"
        DELETE FROM devices
        WHERE id = ?1
        "#,
        id
    )
    .execute(db)
    .await?;
    Ok(removed.rows_affected() > 0)
}

/// The devices paired with the database file at path, read without changing it, for the cleanup
/// (whose app isn't running): none when there is no file, or no devices table in it yet.
pub async fn in_file(path: &Path) -> sqlx::Result<Vec<Device>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let has_table = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!: i64"
        FROM sqlite_master
        WHERE type = 'table' AND name = 'devices'
        "#
    )
    .fetch_one(&mut connection)
    .await?
        > 0;
    if !has_table {
        return Ok(Vec::new());
    }
    sqlx::query_as!(
        Device,
        r#"
        SELECT id AS "id!", name, paired_at, last_seen_at
        FROM devices
        ORDER BY paired_at, id
        "#
    )
    .fetch_all(&mut connection)
    .await
}
