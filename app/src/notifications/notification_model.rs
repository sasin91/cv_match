//! The notifications table: what the app told the user, newest first, for the window's bell.

use sqlx::SqlitePool;

use super::{Kind, Notice};

/// How many notifications the bell lists.
pub const LISTED: i64 = 50;
/// How long a notification is kept, in days, before start-up deletes it.
pub const KEEP_DAYS: i64 = 30;
/// Seconds in a day.
const DAY_SECONDS: i64 = 86_400;

/// A kept notification, as the bell lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: i64,
    /// None for a kind this version doesn't know (kept by a newer one).
    pub kind: Option<Kind>,
    pub title: String,
    pub text: String,
    pub link: String,
    pub created_at: i64,
    pub read: bool,
}

/// Keeps notice, told at now; its id.
pub async fn add(db: &SqlitePool, notice: &Notice, now: i64) -> sqlx::Result<i64> {
    let kind = notice.kind.as_str();
    let added = sqlx::query!(
        r#"
        INSERT INTO notifications (kind, title, text, link, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        kind,
        notice.title,
        notice.text,
        notice.link,
        now
    )
    .execute(db)
    .await?;
    Ok(added.last_insert_rowid())
}

/// The newest LISTED notifications, newest first.
pub async fn newest(db: &SqlitePool) -> sqlx::Result<Vec<Notification>> {
    let rows = sqlx::query!(
        r#"
        SELECT id, kind, title, text, link, created_at, read_at
        FROM notifications
        ORDER BY created_at DESC, id DESC
        LIMIT ?1
        "#,
        LISTED
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Notification {
            id: row.id,
            kind: Kind::kept_as(&row.kind),
            title: row.title,
            text: row.text,
            link: row.link,
            created_at: row.created_at,
            read: row.read_at.is_some(),
        })
        .collect())
}

/// How many notifications haven't been read: the bell's count.
pub async fn unread(db: &SqlitePool) -> sqlx::Result<i64> {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM notifications WHERE read_at IS NULL"#)
        .fetch_one(db)
        .await
}

/// Marks every notification told up to and including up_to_id as read at now: the ones the
/// bell showed, not one told meanwhile.
pub async fn mark_read(db: &SqlitePool, up_to_id: i64, now: i64) -> sqlx::Result<()> {
    sqlx::query!(
        r#"UPDATE notifications SET read_at = ?2 WHERE read_at IS NULL AND id <= ?1"#,
        up_to_id,
        now
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes notifications older than KEEP_DAYS; how many.
pub async fn forget_old(db: &SqlitePool, now: i64) -> sqlx::Result<u64> {
    let before = now - KEEP_DAYS * DAY_SECONDS;
    let deleted = sqlx::query!(r#"DELETE FROM notifications WHERE created_at < ?1"#, before)
        .execute(db)
        .await?;
    Ok(deleted.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(title: &str) -> Notice {
        Notice {
            kind: Kind::GoodMatch,
            title: title.to_string(),
            text: "82%, Acme".to_string(),
            link: "match/1".to_string(),
            pops_up: true,
        }
    }

    #[tokio::test]
    async fn the_bell_reads_newest_first_and_counts_the_unread() {
        let db = crate::db::open_in_memory().await.unwrap();
        let first = add(&db, &notice("First"), 100).await.unwrap();
        add(&db, &notice("Second"), 200).await.unwrap();
        let listed = newest(&db).await.unwrap();
        assert_eq!(listed[0].title, "Second");
        assert_eq!(listed[1].kind, Some(Kind::GoodMatch));
        assert_eq!(unread(&db).await.unwrap(), 2);
        mark_read(&db, first, 300).await.unwrap();
        assert_eq!(unread(&db).await.unwrap(), 1);
        assert!(newest(&db).await.unwrap()[1].read);
    }

    #[tokio::test]
    async fn old_notifications_are_forgotten() {
        let db = crate::db::open_in_memory().await.unwrap();
        let now = 100 * DAY_SECONDS;
        add(&db, &notice("Old"), now - (KEEP_DAYS + 1) * DAY_SECONDS)
            .await
            .unwrap();
        add(&db, &notice("New"), now).await.unwrap();
        assert_eq!(forget_old(&db, now).await.unwrap(), 1);
        assert_eq!(newest(&db).await.unwrap()[0].title, "New");
    }
}
