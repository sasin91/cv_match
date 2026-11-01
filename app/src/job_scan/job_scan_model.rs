//! The job searches (job_searches + job_search_sites), their scans (job_search_runs) and the posts
//! they found (scanned_job_posts), as recruiter's Job_scan_model reads and writes them. There is
//! one local user, so nothing here takes a user id.

use sqlx::SqlitePool;

use super::job_search_rules::{self, MAX_ERROR, MAX_FIELD, MAX_URL};
use crate::matches::score_cursor::{self, NO_SCORE};

/// Post statuses (scanned_job_posts.status).
pub const NEW: &str = "new";
pub const MATCHING: &str = "matching";
pub const MATCHED: &str = "matched";
pub const NOT_A_JOB: &str = "not_a_job";
pub const FAILED: &str = "failed";
pub const DISMISSED: &str = "dismissed";

/// A job search as the list of searches shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedSearch {
    pub id: i64,
    pub name: String,
    pub keywords: String,
    pub location: String,
    pub active: bool,
    pub paused_reason: String,
    pub last_scanned_at: Option<i64>,
    /// Posts it found, not counting dismissed ones.
    pub posts: i64,
    pub matched: i64,
}

/// A job search with its sites.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JobSearch {
    pub id: i64,
    pub name: String,
    pub keywords: String,
    pub location: String,
    pub remote_ok: bool,
    pub tailor_from_score: i64,
    pub active: bool,
    pub paused_reason: String,
    pub last_scanned_at: Option<i64>,
    /// The domains the scan leaves alone; none to read every site.
    pub skipped_sites: Vec<String>,
}

/// What the search form saves.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchFields {
    pub name: String,
    pub keywords: String,
    pub location: String,
    pub remote_ok: bool,
    pub tailor_from_score: i64,
    pub active: bool,
    /// The domains the scan leaves alone.
    pub skipped_sites: Vec<String>,
}

/// A found post as its search's list shows it, with its saved match's score.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundPost {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub status: String,
    pub error: String,
    pub found_at: i64,
    pub match_id: Option<i64>,
    pub score: Option<f64>,
    pub tag: Option<String>,
    pub has_application: bool,
    pub has_resume: bool,
    /// The location requirement the match missed, the likely disqualifier.
    pub location_missed: Option<String>,
}

/// A batch of found posts, and the cursor of the next batch when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct PostBatch {
    pub rows: Vec<FoundPost>,
    pub next: Option<String>,
}

/// A found post as its match needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct PostForMatching {
    pub id: i64,
    pub job_search_id: i64,
    /// The scan that found it; None for a post added by its link.
    pub job_search_run_id: Option<i64>,
    pub url: String,
    pub status: String,
    pub tailor_from_score: i64,
}

/// One scan of a search.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: i64,
    pub job_search_id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub posts_found: i64,
    pub posts_new: i64,
    pub error: String,
}

/// What a search's scans cost over a while.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenUse {
    pub scans: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// A post found lately, which the scan is told to skip.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentPost {
    pub url: String,
    pub title: String,
    pub company: String,
}

/// What the scan found, from what the page or the model said about the post: the title and
/// company to keep when the scan didn't say them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FoundAbout {
    pub title: String,
    pub company: String,
}

/// The searches, newest first, with their post counts.
pub async fn searches(db: &SqlitePool) -> sqlx::Result<Vec<ListedSearch>> {
    let rows = sqlx::query_file!("queries/job_scan_model/searches.sql")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| ListedSearch {
            id: row.id,
            name: row.name,
            keywords: row.keywords,
            location: row.location,
            active: row.active != 0,
            paused_reason: row.paused_reason,
            last_scanned_at: row.last_scanned_at,
            posts: row.posts,
            matched: row.matched,
        })
        .collect())
}

/// A search with its sites, or None.
pub async fn find(db: &SqlitePool, id: i64) -> sqlx::Result<Option<JobSearch>> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, name, keywords, location, remote_ok, tailor_from_score, active, paused_reason,
               last_scanned_at
        FROM job_searches
        WHERE id = ?1
        "#,
        id
    )
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(JobSearch {
        id: row.id,
        name: row.name,
        keywords: row.keywords,
        location: row.location,
        remote_ok: row.remote_ok != 0,
        tailor_from_score: row.tailor_from_score,
        active: row.active != 0,
        paused_reason: row.paused_reason,
        last_scanned_at: row.last_scanned_at,
        skipped_sites: skipped_sites(db, id).await?,
    }))
}

/// The domains search id skips (job_search_sites).
async fn skipped_sites(db: &SqlitePool, id: i64) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!(
        r#"SELECT domain FROM job_search_sites WHERE job_search_id = ?1 ORDER BY domain"#,
        id
    )
    .fetch_all(db)
    .await
}

/// Saves a search (a new one when id is 0) and replaces its sites; an active search's pause is
/// lifted. Returns the search's id.
pub async fn save(db: &SqlitePool, id: i64, fields: &SearchFields, now: i64) -> sqlx::Result<i64> {
    let mut tx = crate::db::write_transaction(db).await?;
    let (remote_ok, active) = (i64::from(fields.remote_ok), i64::from(fields.active));
    let id = if id == 0 {
        sqlx::query!(
            r#"
            INSERT INTO job_searches
              (name, keywords, location, remote_ok, tailor_from_score, active, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            "#,
            fields.name,
            fields.keywords,
            fields.location,
            remote_ok,
            fields.tailor_from_score,
            active,
            now
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid()
    } else {
        sqlx::query!(
            r#"
            UPDATE job_searches
            SET name = ?2, keywords = ?3, location = ?4, remote_ok = ?5, tailor_from_score = ?6,
                active = ?7,
                paused_reason = CASE WHEN ?7 = 1 THEN '' ELSE paused_reason END,
                updated_at = ?8
            WHERE id = ?1
            "#,
            id,
            fields.name,
            fields.keywords,
            fields.location,
            remote_ok,
            fields.tailor_from_score,
            active,
            now
        )
        .execute(&mut *tx)
        .await?;
        id
    };
    sqlx::query!(r#"DELETE FROM job_search_sites WHERE job_search_id = ?1"#, id)
        .execute(&mut *tx)
        .await?;
    for domain in &fields.skipped_sites {
        sqlx::query!(
            r#"INSERT INTO job_search_sites (job_search_id, domain) VALUES (?1, ?2)"#,
            id,
            domain
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// Deletes a search, with its sites, scans and found posts; false when there's none. The saved
/// matches stay.
pub async fn delete_search(db: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let done = sqlx::query!(r#"DELETE FROM job_searches WHERE id = ?1"#, id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() > 0)
}

/// A batch of a search's found posts, best first, without the dismissed ones unless dismissed;
/// after is the cursor of the last post shown ("" for the first batch).
pub async fn posts(
    db: &SqlitePool,
    search_id: i64,
    dismissed: bool,
    limit: usize,
    after: &str,
) -> sqlx::Result<PostBatch> {
    let place = score_cursor::place(after);
    let after_score = place.as_ref().map(|place| place.score);
    let after_at = place.as_ref().map(|place| place.at);
    let after_id = place.as_ref().map(|place| place.id);
    let dismissed = i64::from(dismissed);
    // One more than the batch, to tell whether another batch follows.
    let fetch = limit as i64 + 1;
    let mut rows: Vec<FoundPost> = sqlx::query_file!(
        "queries/job_scan_model/posts.sql",
        search_id,
        dismissed,
        NO_SCORE,
        after_score,
        after_at,
        after_id,
        fetch
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|row| FoundPost {
        id: row.id,
        url: row.url,
        title: row.title,
        company: row.company,
        location: row.location,
        status: row.status,
        error: row.error,
        found_at: row.found_at,
        match_id: row.match_id,
        score: row.score,
        tag: row.tag,
        has_application: row.has_application,
        has_resume: row.has_resume,
        location_missed: row.location_missed,
    })
    .collect();
    let mut next = None;
    if rows.len() > limit {
        rows.truncate(limit);
        let last = rows.last().expect("limit rows");
        next = Some(score_cursor::after(last.score, last.found_at, last.id));
    }
    Ok(PostBatch { rows, next })
}

/// Dismisses (or, with undo, brings back) a post: (its search, its saved match), or None when
/// there's no such post. Brought back, a post shows as matched when it has a match, and
/// otherwise waits to be matched again.
pub async fn dismiss(db: &SqlitePool, post_id: i64, undo: bool) -> sqlx::Result<Option<(i64, Option<i64>)>> {
    let undo = i64::from(undo);
    let row = sqlx::query!(
        r#"
        UPDATE scanned_job_posts
        SET status = CASE
          WHEN ?2 = 0 THEN 'dismissed'
          WHEN match_id IS NOT NULL THEN 'matched'
          ELSE 'new'
        END
        WHERE id = ?1
        RETURNING job_search_id, match_id
        "#,
        post_id,
        undo
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.job_search_id, row.match_id)))
}

/// Dismisses (or, with undo, brings back) the posts of a saved match, when the match is rejected
/// or brought back on the match page, so the two say the same.
pub async fn follow_match(db: &SqlitePool, match_id: i64, undo: bool) -> sqlx::Result<()> {
    let (status, from) = if undo {
        (MATCHED, DISMISSED)
    } else {
        (DISMISSED, MATCHED)
    };
    sqlx::query!(
        r#"UPDATE scanned_job_posts SET status = ?2 WHERE match_id = ?1 AND status = ?3"#,
        match_id,
        status,
        from
    )
    .execute(db)
    .await?;
    Ok(())
}

/// A post to add to a search.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewPost<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub company: &'a str,
    pub location: &'a str,
}

/// Adds a post to a search, unless the search lists that post already (by url_hash). Returns
/// the new post's id, or None for a post it has.
pub async fn add_post(
    db: &SqlitePool,
    search_id: i64,
    run_id: Option<i64>,
    post: &NewPost<'_>,
    now: i64,
) -> sqlx::Result<Option<i64>> {
    let hash = job_search_rules::url_hash(post.url);
    let url = job_search_rules::cut(post.url, MAX_URL);
    let title = job_search_rules::cut(post.title, MAX_FIELD);
    let company = job_search_rules::cut(post.company, MAX_FIELD);
    let location = job_search_rules::cut(post.location, MAX_FIELD);
    let added = sqlx::query!(
        r#"
        INSERT INTO scanned_job_posts
          (job_search_id, job_search_run_id, url, url_hash, title, company, location, status, found_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'new', ?8)
        ON CONFLICT (job_search_id, url_hash) DO NOTHING
        "#,
        search_id,
        run_id,
        url,
        hash,
        title,
        company,
        location,
        now
    )
    .execute(db)
    .await?;
    Ok((added.rows_affected() > 0).then(|| added.last_insert_rowid()))
}

/// A post with its search's tailoring score, for its match; None when it's gone.
pub async fn for_matching(db: &SqlitePool, post_id: i64) -> sqlx::Result<Option<PostForMatching>> {
    let row = sqlx::query!(
        r#"
        SELECT p.id, p.job_search_id, p.job_search_run_id, p.url, p.status, s.tailor_from_score
        FROM scanned_job_posts p
        JOIN job_searches s ON s.id = p.job_search_id
        WHERE p.id = ?1
        "#,
        post_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| PostForMatching {
        id: row.id,
        job_search_id: row.job_search_id,
        job_search_run_id: row.job_search_run_id,
        url: row.url,
        status: row.status,
        tailor_from_score: row.tailor_from_score,
    }))
}

/// Claims a post for a match starting now: it is new, failed or not a job post, or its last
/// match started before stale_before and stopped without finishing. One statement, so two
/// matches (a scan's and Try again, or two scans) can't both claim it. False when it isn't to be
/// matched now: matching already, matched, dismissed or gone.
pub async fn claim_for_matching(db: &SqlitePool, post_id: i64, stale_before: i64, now: i64) -> sqlx::Result<bool> {
    let claimed = sqlx::query!(
        r#"
        UPDATE scanned_job_posts
        SET status = 'matching', matching_since = ?3, error = ''
        WHERE id = ?1
          AND (
            status IN ('new', 'failed', 'not_a_job')
            OR (status = 'matching' AND (matching_since IS NULL OR matching_since < ?2))
          )
        "#,
        post_id,
        stale_before,
        now
    )
    .execute(db)
    .await?;
    Ok(claimed.rows_affected() > 0)
}

/// Posts a scan or a match left unmatched when it stopped (the program was closed): new ones
/// found before stale_before, and ones whose match started before it, oldest first.
pub async fn unfinished_posts(db: &SqlitePool, stale_before: i64) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        r#"
        SELECT p.id AS "id!"
        FROM scanned_job_posts p
        JOIN job_searches s ON s.id = p.job_search_id
        WHERE s.active = 1
          AND (
            (p.status = 'new' AND p.found_at < ?1)
            OR (p.status = 'matching' AND (p.matching_since IS NULL OR p.matching_since < ?1))
          )
        ORDER BY p.found_at, p.id
        "#,
        stale_before
    )
    .fetch_all(db)
    .await
}

/// A post's search and status, or None.
pub async fn post_status(db: &SqlitePool, post_id: i64) -> sqlx::Result<Option<(i64, String)>> {
    let row = sqlx::query!(
        r#"SELECT job_search_id, status FROM scanned_job_posts WHERE id = ?1"#,
        post_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| (row.job_search_id, row.status)))
}

/// Sets a post's status, and what goes with it (a post the user dismissed meanwhile stays
/// dismissed, with the rest saved): the error to show, or the saved match (with
/// when), and the title and company the page gave when the scan didn't say them.
pub async fn set_status(
    db: &SqlitePool,
    post_id: i64,
    status: &str,
    error: &str,
    match_id: Option<i64>,
    found: &FoundAbout,
    now: i64,
) -> sqlx::Result<()> {
    let error = job_search_rules::cut(error, MAX_ERROR);
    let title = job_search_rules::cut(&found.title, MAX_FIELD);
    let company = job_search_rules::cut(&found.company, MAX_FIELD);
    sqlx::query!(
        r#"
        UPDATE scanned_job_posts
        SET status = CASE WHEN status = 'dismissed' THEN status ELSE ?2 END,
            matching_since = NULL,
            error = ?3,
            match_id = COALESCE(?4, match_id),
            matched_at = CASE WHEN ?4 IS NULL THEN matched_at ELSE ?5 END,
            title = CASE WHEN ?6 = '' THEN title ELSE ?6 END,
            company = CASE WHEN ?7 = '' THEN company ELSE ?7 END
        WHERE id = ?1
        "#,
        post_id,
        status,
        error,
        match_id,
        now,
        title,
        company
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Pauses a search, with why (shown on its page and the dashboard). updated_at marks when, so
/// the pause is lifted by a key saved after it (resume_after_new_key).
pub async fn pause(db: &SqlitePool, search_id: i64, reason: &str, now: i64) -> sqlx::Result<()> {
    let reason = job_search_rules::cut(reason, MAX_ERROR);
    sqlx::query!(
        r#"UPDATE job_searches SET paused_reason = ?2, updated_at = ?3 WHERE id = ?1"#,
        search_id,
        reason,
        now
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Lifts the pause of the active searches paused before an API key was last saved or picked for
/// a use: the key may fix what paused them.
pub async fn resume_after_new_key(db: &SqlitePool) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        UPDATE job_searches
        SET paused_reason = ''
        WHERE active = 1
          AND paused_reason <> ''
          AND updated_at <= (SELECT COALESCE(MAX(updated_at), 0) FROM user_llm_keys)
        "#
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The active searches that aren't paused, the longest unscanned first.
pub async fn scannable(db: &SqlitePool) -> sqlx::Result<Vec<i64>> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM job_searches
        WHERE active = 1 AND paused_reason = ''
        ORDER BY last_scanned_at IS NOT NULL, last_scanned_at, id
        "#
    )
    .fetch_all(db)
    .await
}

/// Claims a search for a scan starting now: marks it scanned now, unless it was scanned since
/// scanned_before. One statement, so two scans at once (the schedule's and Scan now's) can't
/// both claim it. Returns the run's id when this scan has it.
pub async fn start_run(db: &SqlitePool, search_id: i64, scanned_before: i64, now: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = crate::db::write_transaction(db).await?;
    let claimed = sqlx::query!(
        r#"
        UPDATE job_searches
        SET last_scanned_at = ?3
        WHERE id = ?1 AND (last_scanned_at IS NULL OR last_scanned_at <= ?2)
        "#,
        search_id,
        scanned_before,
        now
    )
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        return Ok(None);
    }
    let run = sqlx::query!(
        r#"INSERT INTO job_search_runs (job_search_id, started_at) VALUES (?1, ?2)"#,
        search_id,
        now
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    tx.commit().await?;
    Ok(Some(run))
}

/// What a scan ended with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunEnd {
    pub posts_found: i64,
    pub posts_new: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub error: String,
}

/// Ends a scan with what it found and cost, or why it failed.
pub async fn finish_run(db: &SqlitePool, run_id: i64, end: &RunEnd, now: i64) -> sqlx::Result<()> {
    let error = job_search_rules::cut(&end.error, MAX_ERROR);
    sqlx::query!(
        r#"
        UPDATE job_search_runs
        SET finished_at = ?2, posts_found = ?3, posts_new = ?4, input_tokens = ?5,
            output_tokens = ?6, error = ?7
        WHERE id = ?1
        "#,
        run_id,
        now,
        end.posts_found,
        end.posts_new,
        end.input_tokens,
        end.output_tokens,
        error
    )
    .execute(db)
    .await?;
    Ok(())
}

/// A search's latest scan (search Some), or the latest of any search (None).
pub async fn last_run(db: &SqlitePool, search_id: Option<i64>) -> sqlx::Result<Option<Run>> {
    let row = sqlx::query!(
        r#"
        SELECT id, job_search_id, started_at, finished_at, posts_found, posts_new, error
        FROM job_search_runs
        WHERE ?1 IS NULL OR job_search_id = ?1
        ORDER BY started_at DESC, id DESC
        LIMIT 1
        "#,
        search_id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| Run {
        id: row.id,
        job_search_id: row.job_search_id,
        started_at: row.started_at,
        finished_at: row.finished_at,
        posts_found: row.posts_found,
        posts_new: row.posts_new,
        error: row.error,
    }))
}

/// Ends the scans that never finished, with error, at now: a scan cut off while it listed its
/// posts (the program was closed). Only the scan holding scan_lock calls it, so none of them
/// still runs.
pub async fn stop_unfinished_runs(db: &SqlitePool, error: &str, now: i64) -> sqlx::Result<u64> {
    let stopped = sqlx::query!(
        r#"UPDATE job_search_runs SET finished_at = ?2, error = ?1 WHERE finished_at IS NULL"#,
        error,
        now
    )
    .execute(db)
    .await?;
    Ok(stopped.rows_affected())
}

/// New posts the scans found since since, of every search.
pub async fn new_posts_since(db: &SqlitePool, since: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!: i64"
        FROM scanned_job_posts
        WHERE found_at >= ?1 AND job_search_run_id IS NOT NULL
        "#,
        since
    )
    .fetch_one(db)
    .await
}

/// What a search's scans since since cost.
pub async fn token_use(db: &SqlitePool, search_id: i64, since: i64) -> sqlx::Result<TokenUse> {
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) AS "scans!: i64",
               COALESCE(SUM(input_tokens), 0) AS "input_tokens!: i64",
               COALESCE(SUM(output_tokens), 0) AS "output_tokens!: i64"
        FROM job_search_runs
        WHERE job_search_id = ?1 AND started_at >= ?2
        "#,
        search_id,
        since
    )
    .fetch_one(db)
    .await?;
    Ok(TokenUse {
        scans: row.scans,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
    })
}

/// A search's latest found posts, for the scan to skip.
pub async fn recent_posts(db: &SqlitePool, search_id: i64, limit: i64) -> sqlx::Result<Vec<RecentPost>> {
    let rows = sqlx::query!(
        r#"
        SELECT url, title, company
        FROM scanned_job_posts
        WHERE job_search_id = ?1
        ORDER BY found_at DESC, id DESC
        LIMIT ?2
        "#,
        search_id,
        limit
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| RecentPost {
            url: row.url,
            title: row.title,
            company: row.company,
        })
        .collect())
}

/// The first reason an active search is paused for, if one is.
pub async fn first_pause(db: &SqlitePool) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar!(
        r#"
        SELECT paused_reason FROM job_searches
        WHERE active = 1 AND paused_reason <> ''
        ORDER BY updated_at DESC
        LIMIT 1
        "#
    )
    .fetch_optional(db)
    .await
}

/// How many searches are switched on.
pub async fn active_count(db: &SqlitePool) -> sqlx::Result<i64> {
    sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!: i64" FROM job_searches WHERE active = 1"#)
        .fetch_one(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fields(name: &str) -> SearchFields {
        SearchFields {
            name: name.to_string(),
            keywords: "PHP".to_string(),
            location: String::new(),
            remote_ok: true,
            tailor_from_score: 60,
            active: true,
            skipped_sites: vec!["jobnet.dk".to_string(), "jobindex.dk".to_string()],
        }
    }

    #[tokio::test]
    async fn a_search_is_saved_with_its_sites_and_found_again() {
        let db = db::open_in_memory().await.unwrap();
        let id = save(&db, 0, &fields("Backend"), 100).await.unwrap();
        let found = find(&db, id).await.unwrap().unwrap();
        assert_eq!(found.name, "Backend");
        assert_eq!(found.skipped_sites, vec!["jobindex.dk", "jobnet.dk"]);
        pause(&db, id, "No key.", 200).await.unwrap();
        let edited = SearchFields {
            skipped_sites: Vec::new(),
            ..fields("Frontend")
        };
        assert_eq!(save(&db, id, &edited, 300).await.unwrap(), id);
        let found = find(&db, id).await.unwrap().unwrap();
        assert_eq!((found.name.as_str(), found.skipped_sites.len()), ("Frontend", 0));
        assert_eq!(found.paused_reason, "", "saving an active search lifts its pause");
        assert_eq!(searches(&db).await.unwrap().len(), 1);
        assert!(delete_search(&db, id).await.unwrap());
        assert!(find(&db, id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_search_lists_the_same_post_once() {
        let db = db::open_in_memory().await.unwrap();
        let id = save(&db, 0, &fields("Backend"), 100).await.unwrap();
        let post = |url| NewPost {
            url,
            title: "PHP developer",
            ..NewPost::default()
        };
        let first = add_post(&db, id, None, &post("https://www.jobnet.dk/job/1/"), 100)
            .await
            .unwrap();
        assert!(first.is_some());
        let again = add_post(&db, id, None, &post("https://jobnet.dk/job/1?utm_source=x"), 100)
            .await
            .unwrap();
        assert_eq!(again, None);
        assert!(
            add_post(&db, id, None, &post("https://jobnet.dk/job/2"), 100)
                .await
                .unwrap()
                .is_some()
        );
        let batch = posts(&db, id, false, 1, "").await.unwrap();
        assert_eq!(batch.rows.len(), 1);
        let rest = posts(&db, id, false, 10, batch.next.as_deref().unwrap()).await.unwrap();
        assert_eq!(rest.rows.len(), 1);
        assert_ne!(rest.rows[0].id, batch.rows[0].id);
        assert_eq!(dismiss(&db, first.unwrap(), false).await.unwrap(), Some((id, None)));
        assert_eq!(posts(&db, id, false, 10, "").await.unwrap().rows.len(), 1);
        assert_eq!(posts(&db, id, true, 10, "").await.unwrap().rows.len(), 2);
        dismiss(&db, first.unwrap(), true).await.unwrap();
        assert_eq!(post_status(&db, first.unwrap()).await.unwrap().unwrap().1, NEW);
        assert_eq!(unfinished_posts(&db, 101).await.unwrap().len(), 2);
        assert!(unfinished_posts(&db, 100).await.unwrap().is_empty());

        // A match claims a post; a second can't, until the first is stale.
        let second = rest.rows[0].id.max(batch.rows[0].id);
        assert!(claim_for_matching(&db, second, 100, 200).await.unwrap());
        assert!(!claim_for_matching(&db, second, 150, 201).await.unwrap());
        assert_eq!(unfinished_posts(&db, 150).await.unwrap(), vec![first.unwrap()]);
        assert!(claim_for_matching(&db, second, 250, 300).await.unwrap());

        // Dismissed while its match runs, a post stays dismissed when the match ends.
        dismiss(&db, first.unwrap(), false).await.unwrap();
        set_status(&db, first.unwrap(), MATCHED, "", None, &FoundAbout::default(), 200)
            .await
            .unwrap();
        assert_eq!(post_status(&db, first.unwrap()).await.unwrap().unwrap().1, DISMISSED);
    }

    #[tokio::test]
    async fn two_scans_at_once_cant_both_claim_a_search() {
        let db = db::open_in_memory().await.unwrap();
        let id = save(&db, 0, &fields("Backend"), 100).await.unwrap();
        let run = start_run(&db, id, 1000 - 3600, 1000).await.unwrap();
        assert!(run.is_some());
        assert_eq!(start_run(&db, id, 1001 - 3600, 1001).await.unwrap(), None);
        let end = RunEnd {
            posts_found: 3,
            posts_new: 2,
            ..RunEnd::default()
        };
        finish_run(&db, run.unwrap(), &end, 1100).await.unwrap();
        let last = last_run(&db, None).await.unwrap().unwrap();
        assert_eq!((last.posts_found, last.posts_new), (3, 2));
        let cut_off = start_run(&db, id, 4600 - 3600, 4600).await.unwrap();
        assert!(cut_off.is_some());
        assert_eq!(stop_unfinished_runs(&db, "Stopped.", 4700).await.unwrap(), 1);
        let last = last_run(&db, None).await.unwrap().unwrap();
        assert_eq!((last.finished_at, last.error.as_str()), (Some(4700), "Stopped."));
        assert_eq!(stop_unfinished_runs(&db, "Stopped.", 4800).await.unwrap(), 0);
    }
}
