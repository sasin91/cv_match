//! The match page and the endpoints its JS calls, for the one local user (always signed in). The AI
//! match and writing run on the user's own key (llm_key); errors answer JSON {error}, which the
//! page shows.

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use serde_json::json;

use super::application_delivery::{ApplicationDelivery, rawurlencode};
use super::batch_row::BatchRow;
use super::document_box::{APPLICATION, DocumentBox, RESUME};
use super::joblog_panel::{JoblogPanel, Logged};
use super::llm_key;
use super::match_model::{LISTS, SHORTLIST};
use super::match_result::MatchResult;
use super::match_service::{self, MatchRequest, RejectOutcome};
use super::page_error::PageError;
use super::resume_comparison::ResumeComparison;
use super::saved_match_row::SavedMatchRow;
use super::{Job, Profile};
use crate::context::Context;
use crate::dashboard::{self, Dashboard, Findings};
use crate::profile;
use crate::server::AppState;

/// Most job posts the page compares against one CV in one go.
const MAX_JOBS: usize = 10;

type Answer = Result<Response, PageError>;

fn html(page: impl Template) -> Answer {
    Ok(Html(page.render()?).into_response())
}

/// A list tab: where it links, its label, whether it is the list shown.
pub struct ListTab {
    pub href: String,
    pub label: &'static str,
    pub current: bool,
}

#[derive(Template)]
#[template(path = "match/match.html")]
struct MatchPage {
    flash: Option<String>,
    dashboard: Dashboard,
    findings: Findings,
    ai_ready: bool,
    ai_uses_json: String,
    max_jobs: usize,
    open_id_json: String,
    profile_text: Option<String>,
    list_tabs: Vec<ListTab>,
    matches: Vec<SavedMatchRow>,
    next: Option<String>,
    list_query: String,
    empty_text: &'static str,
}

#[derive(Template)]
#[template(path = "match/history.html")]
struct History {
    matches: Vec<SavedMatchRow>,
    next: Option<String>,
    list_query: String,
}

#[derive(Template)]
#[template(path = "match/history_latest.html")]
struct HistoryLatest {
    findings: Findings,
    matches: Vec<SavedMatchRow>,
    next: Option<String>,
    list_query: String,
}

#[derive(Template)]
#[template(path = "match/result.html")]
struct ResultView {
    result: MatchResult,
}

#[derive(Template)]
#[template(path = "match/batch_row.html")]
struct BatchRowView {
    result: MatchResult,
    row: BatchRow,
}

#[derive(Template)]
#[template(path = "match/joblog_panel.html")]
struct JoblogPanelView {
    panel: JoblogPanel,
}

#[derive(Template)]
#[template(path = "match/joblog_logged.html")]
struct JoblogLoggedView {
    logged: Logged,
}

/// ?list= and ?after= of the saved matches' list.
#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    list: Option<String>,
    after: Option<String>,
}

/// ?view=row asks for a result as a row of the ranked jobs.
#[derive(Debug, Default, Deserialize)]
pub struct ViewQuery {
    view: Option<String>,
}

/// A batch of the saved matches as the page lists them, best first, with the query strings of
/// its list and of the next batch.
struct SavedMatches {
    matches: Vec<SavedMatchRow>,
    list: &'static str,
    list_query: String,
    next: Option<String>,
}

async fn saved_matches(context: &Context, query: &ListQuery, first: bool) -> Result<SavedMatches, PageError> {
    let after = if first {
        ""
    } else {
        query.after.as_deref().unwrap_or("")
    };
    let found = match_service::saved_matches(context, query.list.as_deref(), after).await?;
    let list_param = if found.list == SHORTLIST {
        String::new()
    } else {
        format!("list={}", found.list)
    };
    Ok(SavedMatches {
        matches: found.matches,
        list: found.list,
        list_query: if list_param.is_empty() {
            String::new()
        } else {
            format!("?{list_param}")
        },
        next: found.next_after.map(|next| {
            let after = format!("after={}", rawurlencode(&next));
            if list_param.is_empty() {
                after
            } else {
                format!("{list_param}&{after}")
            }
        }),
    })
}

/// The lists as tabs, the shortlist first.
fn list_tabs(current: &str) -> Vec<ListTab> {
    LISTS
        .iter()
        .map(|&(list, label, _)| ListTab {
            href: if list == SHORTLIST {
                "match".to_string()
            } else {
                format!("match?list={list}")
            },
            label,
            current: list == current,
        })
        .collect()
}

/// GET match: the dashboard, with the match form and the saved matches.
pub async fn index(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    match_page(&state, &query, None).await
}

/// GET match/{id}: the match page with that saved match open, so it can be linked to. Any
/// other match/{name} is an endpoint not in the desktop app yet.
pub async fn show(
    State(state): State<Arc<AppState>>,
    Path(segment): Path<String>,
    Query(query): Query<ListQuery>,
) -> Answer {
    match segment.parse::<i64>() {
        Ok(id) => match_page(&state, &query, (id > 0).then_some(id)).await,
        Err(_) => Err(not_yet_for(&segment)),
    }
}

async fn match_page(state: &AppState, query: &ListQuery, open_id: Option<i64>) -> Answer {
    let context = &state.context;
    let saved = saved_matches(context, query, false).await?;
    let uses = llm_key::ai_uses(context).await?;
    let empty_text = LISTS
        .iter()
        .find(|(name, ..)| *name == saved.list)
        .map(|(.., empty)| *empty)
        .unwrap_or("");
    let dashboard = dashboard::dashboard(context).await.map_err(dashboard_error)?;
    html(MatchPage {
        flash: state.take_flash(),
        dashboard,
        findings: findings(context).await?,
        ai_ready: uses.any(),
        ai_uses_json:
            json!({ "resume": uses.resume, "application": uses.application, "job_evaluation": uses.job_evaluation })
                .to_string(),
        max_jobs: MAX_JOBS,
        open_id_json: json!(open_id).to_string(),
        profile_text: profile::text_for_user(context.db(), context.user_id).await?,
        list_tabs: list_tabs(saved.list),
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
        empty_text,
    })
}

/// The dashboard's glance at the saved matches.
async fn findings(context: &Context) -> Result<Findings, PageError> {
    dashboard::findings(context).await.map_err(dashboard_error)
}

/// Why the dashboard couldn't be read, logged.
fn dashboard_error(why: String) -> PageError {
    crate::log_line!("match: the dashboard couldn't be read: {why}");
    PageError::new(StatusCode::INTERNAL_SERVER_ERROR, why)
}

/// GET match/history_list: the first batch of the page's list, polled by Trongate MX so
/// matches saved elsewhere show up on their own.
pub async fn history_list(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state.context, &query, true).await?;
    html(HistoryLatest {
        findings: findings(&state.context).await?,
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET match/history_more?after=...: the next saved matches of the page's list.
pub async fn history_more(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state.context, &query, false).await?;
    html(History {
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET match/result/{id}: a saved match as the page shows it, with its application and
/// résumé panel; ?view=row as a row of the ranked jobs.
pub async fn result(State(state): State<Arc<AppState>>, Path(id): Path<i64>, Query(view): Query<ViewQuery>) -> Answer {
    let result = match_service::saved_result(&state.context, id).await?;
    respond_result(result, view, "")
}

/// The result as the open match, or with ?view=row as a row of the ranked jobs, whose post was
/// read from job_url (a saved match knows its own).
fn respond_result(result: MatchResult, view: ViewQuery, job_url: &str) -> Answer {
    if view.view.as_deref() != Some("row") {
        return html(ResultView { result });
    }
    let job_url = if job_url.is_empty() {
        result.job_url.clone()
    } else {
        job_url.to_string()
    };
    let row = BatchRow::from(&result, &job_url);
    html(BatchRowView { result, row })
}

/// quick_match's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct QuickMatchInput {
    job_url: String,
    job_text: String,
    cv_text: String,
}

/// POST match/quick_match {job_text, cv_text, job_url}: the free match (match_service::quick_match),
/// as the open result or, with ?view=row, as a row of the ranked jobs.
pub async fn quick_match(
    State(state): State<Arc<AppState>>,
    Query(view): Query<ViewQuery>,
    Json(input): Json<QuickMatchInput>,
) -> Answer {
    let result = match_service::quick_match(&state.context, &input.job_text, &input.cv_text).await?;
    respond_result(result, view, &input.job_url)
}

/// POST match/submit_reject/{id} with reasons[] and notes[reason]: rejects a saved match (not
/// interested), so it leaves every list but Rejected; undo=1 brings it back instead. Back to
/// the shortlist after a reject, to the match after undo or when the reason is missing.
pub async fn submit_reject(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Form(fields): Form<Vec<(String, String)>>,
) -> Answer {
    let undo = fields.iter().any(|(name, value)| name == "undo" && value == "1");
    let reasons: Vec<String> = fields
        .iter()
        .filter(|(name, _)| name == "reasons[]")
        .map(|(_, value)| value.clone())
        .collect();
    let notes: Vec<(String, String)> = fields
        .iter()
        .filter_map(|(name, value)| {
            let reason = name.strip_prefix("notes[")?.strip_suffix(']')?;
            Some((reason.to_string(), value.clone()))
        })
        .collect();
    match match_service::reject(&state.context, id, &reasons, &notes, undo).await? {
        RejectOutcome::Unfinished(problem) => {
            state.set_flash(problem);
            Ok(Redirect::to(&format!("/match/{id}")).into_response())
        }
        RejectOutcome::Done(message) => {
            if let Some(message) = message {
                state.set_flash(message.to_string());
            }
            let back = if undo {
                format!("/match/{id}")
            } else {
                "/match".to_string()
            };
            Ok(Redirect::to(&back).into_response())
        }
    }
}

/// An id from the address; anything but a number is no match.
fn id_of(segment: &str) -> i64 {
    segment.parse().unwrap_or(0)
}

/// An id in a JSON body: the page sends it as text, the extension as a number.
fn id_in_body(id: &serde_json::Value) -> i64 {
    match id {
        serde_json::Value::Number(id) => id.as_i64().or_else(|| id.as_f64().map(|id| id as i64)).unwrap_or(0),
        serde_json::Value::String(id) => id_of(id.trim()),
        _ => 0,
    }
}

/// GET match/history: the saved matches for the Jobnet extension: matches, the latest ones,
/// newest first; not_joblogged, the ones applied for but not in Jobnet's joblog yet (the list
/// "Applied, not on Jobnet yet"), best first, which the extension offers first.
pub async fn history(State(state): State<Arc<AppState>>) -> Answer {
    let history = match_service::extension_history(&state.context).await?;
    Ok(Json(json!({ "matches": history.matches, "not_joblogged": history.not_joblogged })).into_response())
}

/// GET match/joblog/{id}: a saved match as an entry for Jobnet's joblog (joblog::read): its
/// fields in Jobnet's order, filled in from the match, and which of the three PDFs Jobnet keeps
/// can be downloaded.
pub async fn joblog(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let entry = match_service::joblog_entry(&state.context, id_of(&id)).await?;
    Ok(Json(entry).into_response())
}

/// GET match/joblog_panel/{id}: the same entry as joblog/{id} as the page prints it under the
/// match.
pub async fn joblog_panel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    html(JoblogPanelView {
        panel: match_service::joblog_panel(&state.context, id_of(&id)).await?,
    })
}

/// joblog_status's body: the page sends the id as text, the extension as a number.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct JoblogStatusInput {
    id: serde_json::Value,
    status: serde_json::Value,
    logged: serde_json::Value,
}

/// POST match/joblog_status {id, status, logged}: how far the candidate is with the job in
/// Jobnet's words (joblog::SEARCH_STATUSES), and whether they have entered it in their Jobnet
/// joblog. Answers the "In your Jobnet joblog" section as the page prints it.
pub async fn joblog_status(State(state): State<Arc<AppState>>, Json(input): Json<JoblogStatusInput>) -> Answer {
    // Anything but a known status's text is answered with the list of them.
    let status = input.status.as_str().unwrap_or("");
    let logged = input.logged == serde_json::Value::Bool(true);
    let logged = match_service::save_joblog_status(&state.context, id_in_body(&input.id), status, logged).await?;
    html(JoblogLoggedView { logged })
}

#[derive(Template)]
#[template(path = "joblog_extension.html")]
struct JoblogExtensionPage {
    folder: Result<String, String>,
    version: String,
}

/// GET joblog_extension: how to load the Jobnet extension the app ships, from the folder it
/// wrote it to.
pub async fn joblog_extension(State(state): State<Arc<AppState>>) -> Answer {
    html(JoblogExtensionPage {
        folder: state
            .context
            .extension_folder
            .as_ref()
            .map(|folder| folder.display().to_string())
            .map_err(Clone::clone),
        version: crate::extension::version(),
    })
}

/// POST joblog_extension/open_edge: opens Edge's extensions page (edge://extensions), which the
/// setup page can't link to, so the user can load the extension from the folder the page copied.
pub async fn open_edge(State(state): State<Arc<AppState>>) -> Answer {
    if let Err(why) = &state.context.extension_folder {
        return Err(PageError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("There is no extension folder to load. {why}"),
        ));
    }
    crate::extension::open_extensions_page().map_err(|e| {
        crate::log_line!("joblog_extension: open_edge: {e}");
        PageError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Edge couldn't be opened: {e} Open {} yourself.",
                crate::extension::EDGE_EXTENSIONS_PAGE
            ),
        )
    })?;
    Ok(Json(json!({ "opened": true })).into_response())
}

/// pdf's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PdfInput {
    id: serde_json::Value,
    kind: String,
    text: String,
    /// An earlier version of the application or résumé (documents), whose PDF is made as it was
    /// written instead.
    document_id: Option<i64>,
}

/// POST match/pdf {id, kind, text, document_id}: a saved match's application, résumé, CV or job
/// post as a PDF download (match_service::pdf); text is what the page shows, edits included. A
/// résumé with fields is laid out from them, so edits to its text aren't in the PDF (the page
/// says so first). Errors come back as JSON like the other endpoints.
pub async fn pdf(State(state): State<Arc<AppState>>, Json(input): Json<PdfInput>) -> Answer {
    let made = match_service::pdf(
        &state.context,
        id_in_body(&input.id),
        &input.kind,
        &input.text,
        input.document_id,
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(&made.name)),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        made.bytes,
    )
        .into_response())
}

/// The download's Content-Disposition: the name in ASCII (each other byte and " as _) for old
/// clients, and in full as filename*=UTF-8''.
fn content_disposition(name: &str) -> String {
    let ascii: String = name
        .bytes()
        .map(|byte| {
            if (0x20..=0x7e).contains(&byte) && byte != b'"' {
                byte as char
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        rawurlencode(name)
    )
}

/// already_matched's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AlreadyMatchedInput {
    cv_text: String,
    job_text: String,
}

/// POST match/already_matched {cv_text, job_text}: the newest saved match of this CV with the
/// same job post text, as {match: {id}}, or {match: null}. The page uses it to skip posts it
/// has matched before.
pub async fn already_matched(State(state): State<Arc<AppState>>, Json(input): Json<AlreadyMatchedInput>) -> Answer {
    let id = match_service::already_matched(&state.context, &input.cv_text, &input.job_text).await?;
    Ok(Json(json!({ "match": id.map(|id| json!({ "id": id })) })).into_response())
}

/// Every other address: a match endpoint not in the desktop app, said the way the page shows
/// errors, or a page that isn't there.
pub async fn not_yet(uri: Uri) -> PageError {
    match uri.path().strip_prefix("/match/") {
        Some(rest) => not_yet_for(rest.split('/').next().unwrap_or("")),
        None => PageError::new(StatusCode::NOT_FOUND, "There is no such page in the desktop app."),
    }
}

fn not_yet_for(endpoint: &str) -> PageError {
    PageError::new(
        StatusCode::NOT_IMPLEMENTED,
        format!("match/{endpoint} isn't in the desktop app."),
    )
}

/// fetch_job's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FetchJobInput {
    url: serde_json::Value,
}

/// POST match/fetch_job {url}: the job post at a pasted link as {url, title, text}
/// (match_service::fetch_job, which counts fetches so a batch of links can't become a crawler).
pub async fn fetch_job(State(state): State<Arc<AppState>>, Json(input): Json<FetchJobInput>) -> Answer {
    let page = match_service::fetch_job(&state.context, input.url.as_str().unwrap_or("")).await?;
    Ok(Json(page).into_response())
}

/// extract_job's and extract_cv's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ExtractInput {
    text: String,
}

/// POST match/extract_job {text}: the job post as structured requirements, read by the model,
/// or by the free reader when the key is off for evaluating job posts.
pub async fn extract_job(State(state): State<Arc<AppState>>, Json(input): Json<ExtractInput>) -> Answer {
    let job: Job = match_service::extract_job(&state.context, &input.text).await?;
    Ok(Json(job).into_response())
}

/// POST match/extract_cv {text}: the CV as a structured profile, read by the model, or by the
/// free reader when the key is off for evaluating job posts.
pub async fn extract_cv(State(state): State<Arc<AppState>>, Json(input): Json<ExtractInput>) -> Answer {
    let profile: Profile = match_service::extract_cv(&state.context, &input.text).await?;
    Ok(Json(profile).into_response())
}

/// match's body: the post and the CV as extract_job and extract_cv read them, with their texts.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MatchInput {
    job: Job,
    profile: Profile,
    job_text: String,
    job_url: String,
    cv_name: String,
    cv_text: String,
}

/// POST match/match {job, profile, job_text, job_url, cv_name, cv_text}: the match of a read job
/// post with a read CV (ai_match), saved when both texts are given. Asked for text/html, the
/// answer is the result as the page shows it with the application and résumé panel, carrying
/// the saved id (?view=row: as a row of the ranked jobs); otherwise JSON, which the page reads
/// when it matches a tailored résumé only to compare its score (resume_comparison).
pub async fn r#match(
    State(state): State<Arc<AppState>>,
    Query(view): Query<ViewQuery>,
    headers: axum::http::HeaderMap,
    Json(input): Json<MatchInput>,
) -> Answer {
    let request = MatchRequest {
        job: input.job,
        profile: input.profile,
        job_text: input.job_text,
        job_url: input.job_url,
        cv_name: input.cv_name,
        cv_text: input.cv_text,
    };
    let found = match_service::ai_match(&state.context, &request).await?;
    if !wants_html(&headers) {
        return Ok(Json(found).into_response());
    }
    let result: MatchResult = match_service::ai_match_result(&state.context, &request, &found).await?;
    respond_result(result, view, &request.job_url)
}

/// Whether the page asked for HTML (the result as it shows it) rather than JSON.
fn wants_html(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"))
}

/// GET match/saved/{id}: one saved match with its items and documents, as JSON. The page reads
/// it to pick up an application or résumé written while it was away.
pub async fn saved(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let found = match_service::saved_match(&state.context, id_of(&id)).await?;
    Ok(Json(found).into_response())
}

/// write_application's and tailor_resume's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct WriteInput {
    id: serde_json::Value,
    notes: String,
}

#[derive(Template)]
#[template(path = "match/document_box.html")]
struct DocumentBoxView {
    doc: DocumentBox,
}

/// POST match/write_application {id, notes}: a job application for a saved match, written from
/// the CV and the post in the post's language, and kept with the match as its newest version.
/// notes is optional context from the candidate. Answers the application's box as the page
/// prints it.
pub async fn write_application(State(state): State<Arc<AppState>>, Json(input): Json<WriteInput>) -> Answer {
    write(state, input, APPLICATION).await
}

/// POST match/tailor_resume {id, notes}: the CV rewritten for a saved match's post (from the CV's
/// own facts and the candidate's notes only), kept with the match as its newest version. Answers
/// the résumé's box with the post's keywords it took over and the ones it left out; the page
/// then matches the résumé against the post as a match of its own, to compare
/// (resume_comparison).
pub async fn tailor_resume(State(state): State<Arc<AppState>>, Json(input): Json<WriteInput>) -> Answer {
    write(state, input, RESUME).await
}

/// Writes kind for the saved match (match_service::write) and answers its box. The writing
/// finishes and is saved even when the page drops the connection meanwhile (a laptop asleep, a tab
/// reloaded); the page picks it up from the saved match when it's back.
async fn write(state: Arc<AppState>, input: WriteInput, kind: &'static str) -> Answer {
    let doc = match_service::write(state.context.clone(), id_in_body(&input.id), &input.notes, kind).await?;
    html(DocumentBoxView { doc })
}

#[derive(Template)]
#[template(path = "match/resume_comparison.html")]
struct ResumeComparisonView {
    comparison: ResumeComparison,
}

/// GET match/resume_comparison/{id}/{resume_id}: the tailored résumé's saved match (resume_id)
/// against the CV's (id), as the résumé box prints it.
pub async fn resume_comparison(
    State(state): State<Arc<AppState>>,
    Path((id, resume_id)): Path<(String, String)>,
) -> Answer {
    html(ResumeComparisonView {
        comparison: match_service::resume_comparison(&state.context, id_of(&id), id_of(&resume_id)).await?,
    })
}

#[derive(Template)]
#[template(path = "match/application_delivery.html")]
struct ApplicationDeliveryView {
    delivery: ApplicationDelivery,
}

/// GET match/application_delivery/{id}: the Apply dialog's content for a saved match: how the
/// application is delivered, by email (recipient, subject, body and mailto link) or on the
/// employer's form (the job post's link), with the PDFs and the profile links to copy.
pub async fn application_delivery(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    html(ApplicationDeliveryView {
        delivery: match_service::application_delivery(&state.context, id_of(&id)).await?,
    })
}
