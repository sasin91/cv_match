//! job_scan: saved job searches, scanned for new job posts on the job scan's schedule (`recruiter
//! scan`, which Task Scheduler or the systemd timer runs) or with Scan now, as recruiter's
//! job_scan module does without its ChatGPT, API and MCP routes. Each post found is matched with
//! the profile and saved with the matches; a good match gets its tailored résumé and application
//! right away.
//!
//! The work is in scan_service (the scan) and search_service (the searches and their posts); the
//! handlers here read the form, call one of them and print the page.

pub mod found_post_card;
pub mod job_scan_model;
pub mod job_search_card;
pub mod job_search_rules;
pub mod job_search_summary;
pub mod scan_service;
pub mod search_service;

use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use self::found_post_card::FoundPostCard;
use self::job_scan_model::JobSearch;
use self::job_search_card::JobSearchCard;
use self::job_search_rules::{MAX_FIELD, MAX_KEYWORDS, MAX_NAME, MAX_TAILOR_FROM};
use self::job_search_summary::JobSearchSummary;
use self::search_service::{SaveError, SearchForm};
use crate::server::AppState;
use crate::service_error::{ErrorKind, ServiceError};

/// The list of searches, where the forms send the browser back to.
const PAGE: &str = "/job_scan";

#[derive(Template)]
#[template(path = "job_scan/index.html")]
struct IndexPage {
    title: &'static str,
    flash: Option<String>,
    problem: Option<String>,
    searches: Vec<JobSearchCard>,
}

#[derive(Template)]
#[template(path = "job_scan/form.html")]
struct FormPage {
    title: &'static str,
    id: i64,
    search: FormValues,
    sites_text: String,
    errors: Vec<String>,
    has_profile: bool,
    max_name: usize,
    max_keywords: usize,
    max_field: usize,
    max_tailor_from: i64,
}

/// The form's fields as the page prints them.
struct FormValues {
    name: String,
    keywords: String,
    location: String,
    remote_ok: bool,
    tailor_from_score: String,
    active: bool,
}

#[derive(Template)]
#[template(path = "job_scan/search.html")]
struct SearchPageView {
    title: String,
    flash: Option<String>,
    search: JobSearch,
    summary: JobSearchSummary,
    problem: Option<String>,
    posts: Vec<FoundPostCard>,
    next: Option<String>,
    dismissed: bool,
    first_batch: bool,
    search_id: i64,
}

#[derive(Template)]
#[template(path = "job_scan/not_found.html")]
struct NotFoundPage {
    title: &'static str,
}

/// A search page's query: ?dismissed=1 lists dismissed posts too; after is the cursor of the
/// batch to show.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SearchQuery {
    dismissed: String,
    after: String,
}

/// Scan now's form: the page to come back to.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ScanForm {
    back: String,
}

/// The dismiss form: undo "1" brings the post back.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct DismissForm {
    undo: String,
}

/// The add form: the post's link.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AddForm {
    url: String,
}

/// GET job_scan: the searches.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    let list = match search_service::searches(&state.context).await {
        Ok(list) => list,
        Err(e) => return failure(&e),
    };
    page(IndexPage {
        title: "Job scan",
        flash: state.take_flash(),
        problem: list.problem,
        searches: list.searches,
    })
}

/// GET job_scan/create: the form for a new search.
pub async fn create(State(state): State<Arc<AppState>>) -> Response {
    form(&state, 0, &SearchForm::new_search(), Vec::new()).await
}

/// GET job_scan/edit/{id}: the form for a saved search.
pub async fn edit(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match search_service::search(&state.context, id.parse().unwrap_or(0)).await {
        Ok(search) => form(&state, search.id, &SearchForm::of(&search), Vec::new()).await,
        Err(e) => failure(&e),
    }
}

/// POST job_scan/submit/{id, 0 for a new search}: saves the form and opens the search.
pub async fn submit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(posted): Form<SearchForm>,
) -> Response {
    let id = id.parse().unwrap_or(-1);
    match search_service::save(&state.context, id, &posted).await {
        Ok((id, said)) => {
            state.set_flash(said.to_string());
            Redirect::to(&format!("{PAGE}/search/{id}")).into_response()
        }
        Err(SaveError::Invalid(errors)) => form(&state, id, &posted, errors).await,
        Err(SaveError::Failed(e)) => failure(&e),
    }
}

/// POST job_scan/submit_delete/{id}: deletes the search and the posts it found.
pub async fn submit_delete(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let said = search_service::delete(&state.context, id.parse().unwrap_or(0)).await;
    state.set_flash(said.map_or_else(|e| e.message, str::to_string));
    Redirect::to(PAGE).into_response()
}

/// GET job_scan/search/{id}: the search and the posts it found.
pub async fn search(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Response {
    let id = id.parse().unwrap_or(0);
    let found = search_service::search_page(&state.context, id, query.dismissed == "1", &query.after).await;
    let found = match found {
        Ok(found) => found,
        Err(e) => return failure(&e),
    };
    page(SearchPageView {
        title: found.search.name.clone(),
        flash: state.take_flash(),
        search_id: found.search.id,
        search: found.search,
        summary: found.summary,
        problem: found.problem,
        posts: found.posts,
        next: found.next,
        dismissed: found.dismissed,
        first_batch: found.first_batch,
    })
}

/// POST job_scan/submit_scan {back}: Scan now, from the dashboard, the searches or a search. It
/// runs the one scan `jobfinder scan` runs (scan_service::scan), in the background, and goes back.
pub async fn submit_scan(State(state): State<Arc<AppState>>, Form(form): Form<ScanForm>) -> Response {
    let said = scan_service::scan_now(Arc::clone(&state.context)).await;
    state.set_flash(said.map_or_else(|e| e.message, str::to_string));
    Redirect::to(&back_to(&form.back)).into_response()
}

/// The page Scan now goes back to: one of the app's own pages, else the dashboard.
fn back_to(back: &str) -> String {
    let own_page = back.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/') && !back.starts_with('/');
    if own_page && !back.is_empty() {
        format!("/{back}")
    } else {
        "/match".to_string()
    }
}

/// POST job_scan/submit_dismiss/{post id} {undo}: dismisses the post (rejecting its match), or
/// brings it back.
pub async fn submit_dismiss(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<DismissForm>,
) -> Response {
    let undo = form.undo == "1";
    match search_service::dismiss(&state.context, id.parse().unwrap_or(0), undo).await {
        Ok(search_id) => {
            let query = if undo { "?dismissed=1" } else { "" };
            Redirect::to(&format!("{PAGE}/search/{search_id}{query}")).into_response()
        }
        Err(e) => {
            state.set_flash(e.message);
            Redirect::to(PAGE).into_response()
        }
    }
}

/// POST job_scan/submit_add/{search id} {url}: adds a job post by its link and matches it.
pub async fn submit_add(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Form(form): Form<AddForm>,
) -> Response {
    let id: i64 = id.parse().unwrap_or(0);
    let said = scan_service::add_post(Arc::clone(&state.context), id, &form.url).await;
    state.set_flash(said.map_or_else(|e| e.message, str::to_string));
    Redirect::to(&format!("{PAGE}/search/{id}")).into_response()
}

/// POST job_scan/submit_retry/{post id}: matches a failed post again.
pub async fn submit_retry(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match scan_service::retry(Arc::clone(&state.context), id.parse().unwrap_or(0)).await {
        Ok((search_id, said)) => {
            state.set_flash(said.to_string());
            Redirect::to(&format!("{PAGE}/search/{search_id}")).into_response()
        }
        Err(e) => {
            state.set_flash(e.message);
            Redirect::to(PAGE).into_response()
        }
    }
}

/// The search form, with what was posted and what is wrong with it.
async fn form(state: &AppState, id: i64, posted: &SearchForm, errors: Vec<String>) -> Response {
    let has_profile = match search_service::has_profile(&state.context).await {
        Ok(has) => has,
        Err(e) => return failure(&e),
    };
    page(FormPage {
        title: if id == 0 { "New job search" } else { "Edit job search" },
        id,
        search: FormValues {
            name: posted.name.clone(),
            keywords: posted.keywords.clone(),
            location: posted.location.clone(),
            remote_ok: posted.remote_ok == "1",
            tailor_from_score: posted.tailor_from_score.clone(),
            active: posted.active == "1",
        },
        sites_text: posted.sites.clone(),
        errors,
        has_profile,
        max_name: MAX_NAME,
        max_keywords: MAX_KEYWORDS,
        max_field: MAX_FIELD,
        max_tailor_from: MAX_TAILOR_FROM,
    })
}

fn page(template: impl Template) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            crate::log_line!("job_scan: the page couldn't be printed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("The page couldn't be printed: {e}"),
            )
                .into_response()
        }
    }
}

/// The page when the work failed: the not-found page, or what went wrong (logged where it was
/// made).
fn failure(e: &ServiceError) -> Response {
    if e.kind == ErrorKind::NotFound {
        let html = NotFoundPage { title: "Not found" }.render().unwrap_or_default();
        return (StatusCode::NOT_FOUND, Html(html)).into_response();
    }
    (StatusCode::INTERNAL_SERVER_ERROR, e.message.clone()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_now_goes_back_only_to_the_apps_own_pages() {
        assert_eq!(back_to("job_scan/search/3"), "/job_scan/search/3");
        assert_eq!(back_to("match"), "/match");
        assert_eq!(back_to(""), "/match");
        assert_eq!(back_to("//evil.example"), "/match");
        assert_eq!(back_to("https://evil.example"), "/match");
    }
}
