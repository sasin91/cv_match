//! The job scan's timeline in the database (job_search_run_steps): each step a scan takes, kept
//! as it happens, so a search's page shows it while the scan runs and afterwards
//! (scan_timeline shapes it for the page).

use sqlx::SqlitePool;

use crate::llm::transcript::Exchange;

/// The scans of a search that keep their steps: its latest ones. An older scan's steps (with
/// the model's whole answers, a few hundred kB a scan) are deleted when a new scan starts.
pub const RUNS_WITH_STEPS: i64 = 10;

/// A site read for a search term.
pub const SITE: &str = "site";
/// The web search.
pub const WEB: &str = "web";
/// The posts listed, and how many are new.
pub const POSTS: &str = "posts";
/// A post's page read.
pub const READ: &str = "read";
/// A post read by the model into its requirements.
pub const EXTRACT: &str = "extract";
/// A post matched with the profile.
pub const MATCH: &str = "match";
/// A tailored résumé or application written.
pub const WRITE: &str = "write";

/// A step to keep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewStep<'a> {
    pub run_id: i64,
    pub post_id: Option<i64>,
    pub kind: &'a str,
    pub text: &'a str,
    pub failed: bool,
    pub started_at_ms: i64,
    pub took_ms: i64,
    /// The model's calls in the step, if any.
    pub calls: &'a [Exchange],
}

/// A kept step, without what the model was asked and answered (exchange() reads that).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub id: i64,
    pub post_id: Option<i64>,
    pub kind: String,
    pub text: String,
    pub failed: bool,
    pub started_at_ms: i64,
    pub took_ms: i64,
    /// The model was asked something in the step.
    pub asked_model: bool,
    /// The post's title and link, for a post's step ("" for others).
    pub post_title: String,
    pub post_url: String,
}

/// Keeps a step.
pub async fn add(db: &SqlitePool, step: &NewStep<'_>) -> sqlx::Result<()> {
    let (asked, answered) = joined(step.calls);
    sqlx::query!(
        r#"
        INSERT INTO job_search_run_steps
          (job_search_run_id, scanned_job_post_id, kind, text, failed, started_at_ms, took_ms, asked, answered)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        step.run_id,
        step.post_id,
        step.kind,
        step.text,
        step.failed,
        step.started_at_ms,
        step.took_ms,
        asked,
        answered
    )
    .execute(db)
    .await?;
    Ok(())
}

/// What calls asked and answered, each call one after the other, numbered when there are more.
fn joined(calls: &[Exchange]) -> (String, String) {
    let numbered = |n: usize, text: &str| {
        if calls.len() > 1 {
            format!("Call {} of {}:\n{text}", n + 1, calls.len())
        } else {
            text.to_string()
        }
    };
    let join = |part: fn(&Exchange) -> &str| {
        calls
            .iter()
            .enumerate()
            .map(|(n, call)| numbered(n, part(call)))
            .collect::<Vec<_>>()
            .join(CALL_SEPARATOR)
    };
    (join(|call| &call.asked), join(|call| &call.answered))
}

/// Between two calls in asked and answered.
const CALL_SEPARATOR: &str = "\n\n――――――――\n\n";

/// A scan's steps in the order they were kept.
pub async fn of_run(db: &SqlitePool, run_id: i64) -> sqlx::Result<Vec<Step>> {
    let rows = sqlx::query!(
        r#"
        SELECT step.id, step.scanned_job_post_id, step.kind, step.text, step.failed, step.started_at_ms,
          step.took_ms, step.asked != '' AS "asked_model!: bool",
          COALESCE(post.title, '') AS "post_title!: String", COALESCE(post.url, '') AS "post_url!: String"
        FROM job_search_run_steps step
        LEFT JOIN scanned_job_posts post ON post.id = step.scanned_job_post_id
        WHERE step.job_search_run_id = ?1
        ORDER BY step.id
        "#,
        run_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Step {
            id: row.id,
            post_id: row.scanned_job_post_id,
            kind: row.kind,
            text: row.text,
            failed: row.failed != 0,
            started_at_ms: row.started_at_ms,
            took_ms: row.took_ms,
            asked_model: row.asked_model,
            post_title: row.post_title,
            post_url: row.post_url,
        })
        .collect())
}

/// What the model was asked and answered in a step, if there is such a step.
pub async fn exchange(db: &SqlitePool, step_id: i64) -> sqlx::Result<Option<(String, String)>> {
    let row = sqlx::query!(
        r#"SELECT asked, answered FROM job_search_run_steps WHERE id = ?1"#,
        step_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.asked, row.answered)))
}

/// Deletes the steps of a search's scans older than its RUNS_WITH_STEPS latest.
pub async fn forget_old(db: &SqlitePool, search_id: i64) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        DELETE FROM job_search_run_steps
        WHERE job_search_run_id IN (
          SELECT id FROM job_search_runs
          WHERE job_search_id = ?1
          ORDER BY started_at DESC, id DESC
          LIMIT -1 OFFSET ?2
        )
        "#,
        search_id,
        RUNS_WITH_STEPS
    )
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_calls_are_numbered_one_after_the_other() {
        let call = |n: &str| Exchange {
            asked: format!("q{n}"),
            answered: format!("a{n}"),
            took_ms: 1,
        };
        assert_eq!(joined(&[]), (String::new(), String::new()));
        assert_eq!(joined(&[call("1")]), ("q1".to_string(), "a1".to_string()));
        let (asked, _) = joined(&[call("1"), call("2")]);
        assert_eq!(asked, format!("Call 1 of 2:\nq1{CALL_SEPARATOR}Call 2 of 2:\nq2"));
    }

    #[tokio::test]
    async fn a_scans_steps_are_kept_in_order_and_old_scans_forgotten() {
        let db = crate::db::open_in_memory().await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO job_searches (id, name, keywords, created_at, updated_at) VALUES (1, 'B', 'PHP', 1, 1);",
        )
        .execute(&db)
        .await
        .unwrap();
        for run in 1..=RUNS_WITH_STEPS + 1 {
            sqlx::query("INSERT INTO job_search_runs (id, job_search_id, started_at) VALUES (?1, 1, ?1)")
                .bind(run)
                .execute(&db)
                .await
                .unwrap();
            let calls = [Exchange {
                asked: "q".to_string(),
                answered: "a".to_string(),
                took_ms: 5,
            }];
            let step = NewStep {
                run_id: run,
                kind: EXTRACT,
                text: "Read the post",
                calls: &calls,
                ..NewStep::default()
            };
            add(&db, &step).await.unwrap();
            add(
                &db,
                &NewStep {
                    run_id: run,
                    kind: POSTS,
                    text: "3 posts",
                    ..NewStep::default()
                },
            )
            .await
            .unwrap();
        }
        let steps = of_run(&db, 2).await.unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[0].asked_model && !steps[1].asked_model);
        assert_eq!(steps[1].text, "3 posts");
        assert_eq!(
            exchange(&db, steps[0].id).await.unwrap(),
            Some(("q".to_string(), "a".to_string()))
        );
        forget_old(&db, 1).await.unwrap();
        assert!(of_run(&db, 1).await.unwrap().is_empty());
        assert_eq!(of_run(&db, 2).await.unwrap().len(), 2);
    }
}
