//! Writing the candidate's profile, ported from modules/profile/Profile_model.php: the whole
//! profile replaced with what was read from a CV, all or nothing. The desktop app has no account
//! that makes the candidates row, so the first fill makes it, from the CV's name and email.

use sqlx::SqlitePool;

use super::profile_fields::ProfileFromCv;

/// Longest CV file name kept, in characters (the VARCHAR(255) column).
const CV_NAME_LENGTH: usize = 255;

/// What the profile was last filled from, for the setup wizard.
#[derive(Debug, Clone, PartialEq)]
pub struct FilledFrom {
    pub name: String,
    /// The CV's file name, or "" when it was pasted.
    pub cv_name: String,
    /// When it was filled (unix seconds), or None when it was written by hand or imported.
    pub filled_at: Option<i64>,
}

/// The user's profile header, when they have one.
pub async fn filled_from(db: &SqlitePool, user_id: i64) -> sqlx::Result<Option<FilledFrom>> {
    let row = sqlx::query!(
        r#"
        SELECT candidates.name, candidate_profiles.filled_from_cv_name, candidate_profiles.filled_from_cv_at
        FROM candidates
        JOIN candidate_profiles ON candidate_profiles.candidate_id = candidates.id
        WHERE candidates.trongate_user_id = ?1
        "#,
        user_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| FilledFrom {
        name: row.name,
        cv_name: row.filled_from_cv_name,
        filled_at: row.filled_from_cv_at,
    }))
}

/// Replaces the user's whole profile with what was read from a CV (profile_fields::from_cv),
/// making the candidate when there is none. The name, email and phone are only changed where
/// the CV has them.
pub async fn replace_from_cv(
    db: &SqlitePool,
    user_id: i64,
    profile: &ProfileFromCv,
    cv_name: &str,
) -> sqlx::Result<()> {
    let now = chrono::Utc::now().timestamp();
    let cv_name: String = cv_name.trim().chars().take(CV_NAME_LENGTH).collect();
    let mut tx = db.begin().await?;
    let candidate_id = sqlx::query_scalar!(
        r#"
        INSERT INTO candidates (trongate_user_id, name, email, phone, created_at, updated_at)
        VALUES (?1, ?2, ?3, NULLIF(?4, ''), ?5, ?5)
        ON CONFLICT (trongate_user_id) DO UPDATE SET
          name = CASE WHEN excluded.name <> '' THEN excluded.name ELSE candidates.name END,
          email = CASE WHEN excluded.email <> '' THEN excluded.email ELSE candidates.email END,
          phone = COALESCE(excluded.phone, candidates.phone),
          updated_at = excluded.updated_at
        RETURNING id AS "id!: i64"
        "#,
        user_id,
        profile.name,
        profile.email,
        profile.phone,
        now
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM candidate_profile_entries WHERE candidate_id = ?1"#,
        candidate_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"DELETE FROM candidate_profile_lines WHERE candidate_id = ?1"#,
        candidate_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        INSERT INTO candidate_profiles (candidate_id, title, location, about, filled_from_cv_name, filled_from_cv_at, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6)
        ON CONFLICT (candidate_id) DO UPDATE SET
          title = excluded.title, location = excluded.location, about = excluded.about,
          filled_from_cv_name = excluded.filled_from_cv_name, filled_from_cv_at = excluded.filled_from_cv_at,
          updated_at = excluded.updated_at
        "#,
        candidate_id,
        profile.title,
        profile.location,
        profile.about,
        cv_name,
        now
    )
    .execute(&mut *tx)
    .await?;
    for (sort_order, entry) in (0_i64..).zip(&profile.entries) {
        let entry_id = sqlx::query_scalar!(
            r#"
            INSERT INTO candidate_profile_entries
              (candidate_id, section, title, organisation, location, url, started_on, ended_on, summary, note, sort_order, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
            RETURNING id AS "id!: i64"
            "#,
            candidate_id,
            entry.section,
            entry.title,
            entry.organisation,
            entry.location,
            entry.url,
            entry.started_on,
            entry.ended_on,
            entry.summary,
            entry.note,
            sort_order,
            now
        )
        .fetch_one(&mut *tx)
        .await?;
        insert_lines(&mut tx, candidate_id, Some(entry_id), "bullet", &entry.bullets).await?;
    }
    for (kind, items) in &profile.lines {
        insert_lines(&mut tx, candidate_id, None, kind, items).await?;
    }
    tx.commit().await
}

async fn insert_lines(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    candidate_id: i64,
    entry_id: Option<i64>,
    kind: &str,
    items: &[String],
) -> sqlx::Result<()> {
    for (sort_order, text) in (0_i64..).zip(items) {
        sqlx::query!(
            r#"
            INSERT INTO candidate_profile_lines (candidate_id, entry_id, kind, text, sort_order)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            candidate_id,
            entry_id,
            kind,
            text,
            sort_order
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::profile::profile_fields::ProfileEntry;
    use crate::profile::text_for_user;
    use sqlx::Executor;

    fn read() -> ProfileFromCv {
        ProfileFromCv {
            name: "Mette Sørensen".to_string(),
            email: "mette@example.com".to_string(),
            phone: String::new(),
            title: "Udvikler".to_string(),
            location: "Slagelse".to_string(),
            about: "Jeg bygger ting.".to_string(),
            entries: vec![ProfileEntry {
                section: "experience".to_string(),
                title: "Udvikler".to_string(),
                organisation: "Firma".to_string(),
                location: String::new(),
                url: String::new(),
                started_on: Some("2017-01-01".to_string()),
                ended_on: None,
                summary: String::new(),
                note: String::new(),
                bullets: vec!["Byggede en API".to_string()],
            }],
            lines: vec![
                ("skill", vec!["PHP".to_string()]),
                ("language", vec![]),
                ("certificate", vec![]),
            ],
        }
    }

    #[tokio::test]
    async fn a_cv_makes_the_candidate_and_their_profile() {
        let db = db::open_in_memory().await.unwrap();
        db.execute(sqlx::raw_sql("INSERT INTO trongate_users (id) VALUES (1)"))
            .await
            .unwrap();
        assert_eq!(filled_from(&db, 1).await.unwrap(), None);
        replace_from_cv(&db, 1, &read(), "cv.pdf").await.unwrap();
        assert_eq!(
            text_for_user(&db, 1).await.unwrap().unwrap(),
            "Mette Sørensen\nUdvikler\nSlagelse · mette@example.com\n\nJeg bygger ting.\n\n\
             Experience\n\nUdvikler · Firma\nJanuary 2017 – now\n- Byggede en API\n\nSkills\n- PHP"
        );
        let filled = filled_from(&db, 1).await.unwrap().unwrap();
        assert_eq!(
            (filled.name.as_str(), filled.cv_name.as_str()),
            ("Mette Sørensen", "cv.pdf")
        );
        assert!(filled.filled_at.is_some());
    }

    #[tokio::test]
    async fn filling_again_replaces_the_profile_and_keeps_what_the_cv_leaves_out() {
        let db = db::open_in_memory().await.unwrap();
        db.execute(sqlx::raw_sql(
            "INSERT INTO trongate_users (id) VALUES (1);
             INSERT INTO candidates (id, trongate_user_id, name, email, phone, created_at, updated_at)
             VALUES (4, 1, 'Mette', 'old@example.com', '12345678', 0, 0);",
        ))
        .await
        .unwrap();
        replace_from_cv(&db, 1, &read(), "").await.unwrap();
        let mut again = read();
        again.email = String::new();
        again.entries[0].title = "Lead".to_string();
        replace_from_cv(&db, 1, &again, "new.pdf").await.unwrap();
        let text = text_for_user(&db, 1).await.unwrap().unwrap();
        assert!(text.contains("12345678 · mette@example.com"), "{text}");
        assert!(
            text.contains("Lead · Firma") && !text.contains("Udvikler · Firma"),
            "{text}"
        );
    }
}
