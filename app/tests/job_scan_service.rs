//! The job searches as the Job searches screen calls them (search_service, scan_service): made,
//! listed, scanned only with a model and a profile, and a dismissed post's match rejected with it.

use std::sync::{Arc, LazyLock};

use jobfinder::account::account_model;
use jobfinder::account::key_vault::KeyVault;
use jobfinder::context::Context;
use jobfinder::db;
use jobfinder::job_scan::scan_service;
use jobfinder::job_scan::search_service::{self, SaveError, SearchForm};
use jobfinder::matches::match_service;
use jobfinder::server;
use jobfinder::service_error::ErrorKind;
use jobfinder::taxonomy::Taxonomy;
use sqlx::Executor;

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// The work on a new database: nothing in it but the local user.
async fn context() -> Arc<Context> {
    let db = db::open_in_memory().await.unwrap();
    let user_id = server::local_user(&db).await.unwrap();
    Arc::new(Context::new(db, TAXONOMY.clone(), user_id, KeyVault::in_memory()))
}

fn form(name: &str, keywords: &str) -> SearchForm {
    SearchForm {
        name: name.to_string(),
        keywords: keywords.to_string(),
        tailor_from_score: "60".to_string(),
        active: "1".to_string(),
        ..SearchForm::new_search()
    }
}

#[tokio::test]
async fn a_job_search_is_made_listed_and_scanned_only_with_a_model_and_a_profile() {
    let context = context().await;
    // The form says what's wrong.
    let Err(SaveError::Invalid(errors)) = search_service::save(&context, 0, &form("Backend", "")).await else {
        panic!("a search without keywords is refused");
    };
    assert!(errors.contains(&"Say what to look for.".to_string()), "{errors:?}");

    let saved = SearchForm {
        location: "Odense".to_string(),
        remote_ok: "1".to_string(),
        skipped_sites: "jobnet.dk".to_string(),
        ..form("Backend", "PHP udvikler")
    };
    let Ok((id, said)) = search_service::save(&context, 0, &saved).await else {
        panic!("the search is saved");
    };
    assert!(said.starts_with("Your job search is saved."), "{said}");
    let page = search_service::search_page(&context, id, false, "").await.unwrap();
    assert!(
        page.summary
            .line
            .starts_with("PHP udvikler · Odense · remote too · skips jobnet.dk · tailors from 60"),
        "{}",
        page.summary.line
    );
    assert!(page.posts.is_empty());
    let list = search_service::searches(&context).await.unwrap();
    assert_eq!(list.searches[0].name, "Backend");
    assert!(
        list.searches[0].meta.contains("0 found, 0 matched · not scanned yet"),
        "{}",
        list.searches[0].meta
    );

    // Without a model picked for evaluating job posts, nothing is scanned.
    let no_model = scan_service::scan_now(Arc::clone(&context)).await.unwrap_err();
    assert!(
        no_model.message.contains("Pick one on the API key page."),
        "{}",
        no_model.message
    );

    // A model scans too, once there is a profile to match with.
    account_model::save(
        context.db(),
        &context.key_vault,
        context.user_id,
        "local",
        "stand-in",
        "http://127.0.0.1:9/v1",
        "",
    )
    .await
    .unwrap();
    let no_profile = scan_service::scan_now(Arc::clone(&context)).await.unwrap_err();
    assert!(
        no_profile.message.contains("Read your CV in setup first."),
        "{}",
        no_profile.message
    );
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_search_runs")
        .fetch_one(context.db())
        .await
        .unwrap();
    assert_eq!(runs, 0);

    let unknown = search_service::search_page(&context, 99, false, "")
        .await
        .err()
        .unwrap();
    assert_eq!(unknown.kind, ErrorKind::NotFound);
}

#[tokio::test]
async fn dismissing_a_found_post_rejects_its_match_and_rejecting_the_match_dismisses_it() {
    let context = context().await;
    let Ok((id, _)) = search_service::save(&context, 0, &form("Backend", "PHP")).await else {
        panic!("the search is saved");
    };
    context
        .db()
        .execute(sqlx::raw_sql(
            r#"
            INSERT INTO matches (id, trongate_user_id, job_title, company, job_text, score, points, max_points, tag, created_at)
            VALUES (5, 1, 'PHP developer', 'Acme', 'post', 0.8, 8, 10, 'good', 100);
            INSERT INTO scanned_job_posts (id, job_search_id, url, url_hash, title, status, match_id, found_at)
            VALUES (1, 1, 'https://jobnet.dk/1', 'x', 'PHP developer', 'matched', 5, 100);
            "#,
        ))
        .await
        .unwrap();
    let page = search_service::search_page(&context, id, false, "").await.unwrap();
    assert!(page.posts[0].match_link.is_some(), "the post opens its match");

    assert_eq!(search_service::dismiss(&context, 1, false).await.unwrap(), id);
    let rejected: Option<i64> = sqlx::query_scalar("SELECT rejected_at FROM matches WHERE id = 5")
        .fetch_one(context.db())
        .await
        .unwrap();
    assert!(rejected.is_some());
    search_service::dismiss(&context, 1, true).await.unwrap();
    let status = || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM scanned_job_posts WHERE id = 1")
            .fetch_one(context.db())
            .await
            .unwrap()
    };
    assert_eq!(status().await, "matched");

    // Rejected on the Match screen, the post is dismissed with it.
    match_service::reject(&context, 5, &["salary".to_string()], &[], false)
        .await
        .unwrap();
    assert_eq!(status().await, "dismissed");
}
