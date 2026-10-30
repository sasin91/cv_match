//! The match page and the endpoints its JS calls, ported from modules/cv_match/Cv_match.php for
//! the one local user (always signed in). Endpoints that need what isn't ported yet (the AI
//! match and writing) answer JSON {error} saying so, as the PHP endpoints answer their errors,
//! so the page shows it.

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use serde_json::json;

use super::application_delivery::rawurlencode;
use super::batch_row::BatchRow;
use super::free_reader::{FreeReader, YearMonth};
use super::joblog;
use super::joblog_panel::{JoblogPanel, Logged};
use super::match_documents::{self, MatchDocuments, Uses};
use super::match_model::{self, JOBNET_JOBLOG, LISTS, SHORTLIST, SavedMatch};
use super::match_result::{Fresh, MatchResult};
use super::matcher::{self, Matcher, Verdict, Verdicts};
use super::saved_match_row::SavedMatchRow;
use super::{apply_instructions, pdf_writer, reject_reason};
use crate::profile;
use crate::server::AppState;

/// Longest CV or job post quick_match reads, in characters.
const MAX_TEXT: usize = 40_000;

/// Most job posts the page compares against one CV in one go.
const MAX_JOBS: usize = 10;

/// Saved matches the page lists at a time; scrolling to the end loads the next ones.
const PER_PAGE: usize = 25;

/// What the free match says of a requirement the taxonomy didn't find in the CV.
const NOT_FOUND_REASON: &str = "Not named in the CV. The AI match also looks for related experience.";

/// An error the page shows: JSON {error} with its status.
pub struct PageError(StatusCode, String);

impl PageError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        PageError(status, message.into())
    }

    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "That saved match was not found.")
    }
}

impl IntoResponse for PageError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<sqlx::Error> for PageError {
    fn from(e: sqlx::Error) -> Self {
        eprintln!("match: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The database couldn't answer: {e}"),
        )
    }
}

impl From<askama::Error> for PageError {
    fn from(e: askama::Error) -> Self {
        eprintln!("match: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The page couldn't be printed: {e}"),
        )
    }
}

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

/// The extension's history: the latest matches, and the ones to log on Jobnet first.
const HISTORY_SIZE: usize = 50;

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

/// Which keys the AI may use; none until the AI endpoints (extract_job, match, ...) are ported,
/// so a key saved on the API key page doesn't send the page to endpoints that aren't there.
fn ai_uses() -> Uses {
    Uses::default()
}

/// A batch of the saved matches as the page lists them, best first.
struct SavedMatches {
    matches: Vec<SavedMatchRow>,
    list: &'static str,
    list_query: String,
    next: Option<String>,
}

async fn saved_matches(state: &AppState, query: &ListQuery, first: bool) -> Result<SavedMatches, PageError> {
    let list = LISTS
        .iter()
        .map(|(name, ..)| *name)
        .find(|name| query.list.as_deref() == Some(*name))
        .unwrap_or(SHORTLIST);
    let after = if first {
        ""
    } else {
        query.after.as_deref().unwrap_or("")
    };
    let found = match_model::best_first(state.db(), state.user_id, PER_PAGE, after, list).await?;
    let list_param = if list == SHORTLIST {
        String::new()
    } else {
        format!("list={list}")
    };
    Ok(SavedMatches {
        matches: found.rows.iter().map(SavedMatchRow::from).collect(),
        list,
        list_query: if list_param.is_empty() {
            String::new()
        } else {
            format!("?{list_param}")
        },
        next: found.next.map(|next| {
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

/// GET /: the match page is the app's front page.
pub async fn home() -> Redirect {
    Redirect::to("/match")
}

/// GET match: the match page.
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
    let saved = saved_matches(state, query, false).await?;
    let uses = ai_uses();
    let empty_text = LISTS
        .iter()
        .find(|(name, ..)| *name == saved.list)
        .map(|(.., empty)| *empty)
        .unwrap_or("");
    html(MatchPage {
        flash: state.take_flash(),
        ai_ready: uses.any(),
        ai_uses_json:
            json!({ "resume": uses.resume, "application": uses.application, "job_evaluation": uses.job_evaluation })
                .to_string(),
        max_jobs: MAX_JOBS,
        open_id_json: json!(open_id).to_string(),
        profile_text: profile::text_for_user(state.db(), state.user_id).await?,
        list_tabs: list_tabs(saved.list),
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
        empty_text,
    })
}

/// GET match/history_list: the first batch of the page's list, polled by Trongate MX so
/// matches saved elsewhere show up on their own.
pub async fn history_list(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state, &query, true).await?;
    html(HistoryLatest {
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET match/history_more?after=...: the next saved matches of the page's list.
pub async fn history_more(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state, &query, false).await?;
    html(History {
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET match/result/{id}: a saved match as the page shows it, with its application and
/// résumé panel; ?view=row as a row of the ranked jobs.
pub async fn result(State(state): State<Arc<AppState>>, Path(id): Path<i64>, Query(view): Query<ViewQuery>) -> Answer {
    let found = match_model::find(state.db(), id, state.user_id)
        .await?
        .ok_or_else(PageError::not_found)?;
    let uses = ai_uses();
    let documents = MatchDocuments::from(
        Some(found.id),
        "",
        &match_documents::written_of(&found),
        uses,
        found.apply_instructions.as_ref(),
    );
    let result = MatchResult::from_saved(&found, true, documents);
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

/// POST match/quick_match {job_text, cv_text, job_url}: the free match. Both texts are read
/// with the free reader (no model), matched with the taxonomy and scored like the full match;
/// what the taxonomy can't find in the CV counts as missing. Nothing is saved.
pub async fn quick_match(
    State(state): State<Arc<AppState>>,
    Query(view): Query<ViewQuery>,
    Json(input): Json<QuickMatchInput>,
) -> Answer {
    let job_text = input.job_text.trim();
    let cv_text = input.cv_text.trim();
    if job_text.is_empty() || cv_text.is_empty() {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Paste or upload both the CV and the job post.",
        ));
    }
    if job_text.chars().count() > MAX_TEXT || cv_text.chars().count() > MAX_TEXT {
        return Err(PageError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "That text is too long. Paste only the CV and the job post.",
        ));
    }
    let taxonomy = &state.taxonomy;
    let mut reader = FreeReader::new(taxonomy, this_month());
    let job = reader.job(job_text);
    let profile = reader.cv(cv_text);
    let matcher = Matcher::new(taxonomy, profile.clone(), reader.known.clone());
    let groups = matcher::criteria(&job);
    let mut verdicts = Verdicts::new();
    for unit in matcher::units(&groups) {
        let verdict = matcher.decide(unit).unwrap_or_else(|| Verdict {
            verdict: "missing".to_string(),
            by: "taxonomy".to_string(),
            method: "not_found".to_string(),
            evidence: String::new(),
            reason: NOT_FOUND_REASON.to_string(),
        });
        verdicts.insert(unit.id.clone(), verdict);
    }
    let verdicts = matcher::combine(&groups, &verdicts);
    if groups.values().all(Vec::is_empty) {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The free match found no skills it knows in this post. The AI match reads requirements written as sentences too.",
        ));
    }
    let score = matcher::score(&groups, &verdicts);
    let apply = apply_instructions::read(job_text);
    let fresh = Fresh {
        job: &job,
        groups: &groups,
        verdicts: &verdicts,
        score: &score,
        soft_skills: matcher::soft_skills(&job),
        apply_instructions: apply.as_ref(),
        saved_id: None,
    };
    let result = MatchResult::from(&fresh, ai_uses().any(), None, "").with_cv_found(&profile);
    respond_result(result, view, &input.job_url)
}

/// This month, which the free reader counts "until now" to.
fn this_month() -> YearMonth {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    YearMonth {
        year: i64::from(today.year()),
        month: i64::from(today.month()),
    }
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
    let problem = if undo {
        String::new()
    } else {
        reject_reason::problem(&reasons, &notes)
    };
    if !problem.is_empty() {
        state.set_flash(problem);
        return Ok(Redirect::to(&format!("/match/{id}")).into_response());
    }
    let done = if undo {
        match_model::bring_back(state.db(), id, state.user_id).await?
    } else {
        let now = chrono::Utc::now().timestamp();
        match_model::reject(
            state.db(),
            id,
            state.user_id,
            &reject_reason::rows(&reasons, &notes),
            now,
        )
        .await?
    };
    if done {
        state.set_flash(
            if undo {
                "The match is back in your lists."
            } else {
                "Rejected. It is in the Rejected list if you change your mind."
            }
            .to_string(),
        );
    }
    let back = if undo {
        format!("/match/{id}")
    } else {
        "/match".to_string()
    };
    Ok(Redirect::to(&back).into_response())
}

/// One of the user's saved matches by its id in the address, or the error the page shows.
async fn saved_match(state: &AppState, id: i64) -> Result<SavedMatch, PageError> {
    if id <= 0 {
        return Err(PageError::not_found());
    }
    match_model::find(state.db(), id, state.user_id)
        .await?
        .ok_or_else(PageError::not_found)
}

/// An id from the address; anything but a number is no match.
fn id_of(segment: &str) -> i64 {
    segment.parse().unwrap_or(0)
}

/// An id in a JSON body: the page sends it as text, the extension as a number (PHP's (int)).
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
    let db = state.db();
    let matches = match_model::recent(db, state.user_id, HISTORY_SIZE).await?;
    let not_joblogged = match_model::best_first(db, state.user_id, HISTORY_SIZE, "", "not_joblogged")
        .await?
        .rows;
    Ok(Json(json!({ "matches": matches, "not_joblogged": not_joblogged })).into_response())
}

/// GET match/joblog/{id}: a saved match as an entry for Jobnet's joblog (joblog::read): its
/// fields in Jobnet's order, filled in from the match, and which of the three PDFs Jobnet keeps
/// can be downloaded.
pub async fn joblog(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let found = saved_match(&state, id_of(&id)).await?;
    Ok(Json(joblog::read(&found)).into_response())
}

/// GET match/joblog_panel/{id}: the same entry as joblog/{id} as the page prints it under the
/// match.
pub async fn joblog_panel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let found = saved_match(&state, id_of(&id)).await?;
    html(JoblogPanelView {
        panel: JoblogPanel::from(found.id, &joblog::read(&found)),
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
    let found = saved_match(&state, id_in_body(&input.id)).await?;
    let Some(status) = input
        .status
        .as_str()
        .filter(|status| joblog::search_status(status).is_some())
    else {
        let statuses: Vec<&str> = joblog::SEARCH_STATUSES.iter().map(|(_, text)| *text).collect();
        return Err(PageError::new(
            StatusCode::BAD_REQUEST,
            format!("Pick one of Jobnet's statuses: {}.", statuses.join(", ")),
        ));
    };
    let logged = input.logged == serde_json::Value::Bool(true);
    let now = chrono::Utc::now().timestamp();
    let saved = match_model::save_integration(state.db(), found.id, JOBNET_JOBLOG, status, logged, now).await?;
    html(JoblogLoggedView {
        logged: Logged::from(
            saved.as_ref().and_then(|saved| saved.logged_at),
            joblog::applied_on(saved.as_ref()),
        ),
    })
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
            .extension_folder
            .as_ref()
            .map(|folder| folder.display().to_string())
            .map_err(Clone::clone),
        version: crate::extension::version(),
    })
}

/// pdf's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PdfInput {
    id: serde_json::Value,
    kind: String,
    text: String,
}

/// POST match/pdf {id, kind, text}: a saved match's job application (kind application),
/// tailored résumé (kind resume), the CV it was matched with (kind cv) or its job post (kind job)
/// as a PDF download (pdf_writer). A résumé with fields (tailored_resume) is laid out from them,
/// so edits to its text aren't in the PDF (the page says so first). Otherwise text is what the
/// page shows, edits included; without it the saved text is used; the CV and the job post are
/// always the saved text. Errors come back as JSON like the other endpoints.
pub async fn pdf(State(state): State<Arc<AppState>>, Json(input): Json<PdfInput>) -> Answer {
    let kind = input.kind.as_str();
    if !pdf_writer::KINDS.contains(&kind) {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Choose the application, the résumé, the CV or the job post.",
        ));
    }
    let found = saved_match(&state, id_in_body(&input.id)).await?;
    let text = pdf_text(&found, kind, crate::php::trim(&input.text));
    if text.is_empty() {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "There is no text to export yet.",
        ));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(PageError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "That text is too long for a PDF.",
        ));
    }
    let title = pdf_title(&found, kind);
    // A résumé written with fields is laid out from them; the text is only for one written
    // before they existed.
    let fields = if kind == "resume" {
        match_model::resume_fields(state.db(), found.id).await?
    } else {
        None
    };
    let made = match &fields {
        Some(fields) => pdf_writer::resume_pdf(fields, &title),
        None => pdf_writer::pdf(kind, &text, &title),
    };
    let bytes = made.map_err(|e| {
        eprintln!("match: pdf: {e}");
        PageError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The PDF couldn't be made: {e}"),
        )
    })?;
    let name = pdf_writer::file_name(kind, &found.job_title, &found.company);
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition(&name)),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// The text a PDF is made from: what the page sent, or the saved text when it sent none; the
/// CV and the job post are always the saved text, the post under its link.
fn pdf_text(found: &SavedMatch, kind: &str, sent: &str) -> String {
    let saved = |text: Option<&str>| crate::php::trim(text.unwrap_or("")).to_string();
    let text = if sent.is_empty() || kind == "cv" || kind == "job" {
        match kind {
            "application" => saved(found.application_text.as_deref()),
            "resume" => saved(found.resume_text.as_deref()),
            "cv" => saved(Some(&found.cv_text)),
            _ => saved(Some(&found.job_text)),
        }
    } else {
        sent.to_string()
    };
    let job_url = found.job_url.as_deref().unwrap_or("");
    if kind == "job" && !text.is_empty() && !job_url.is_empty() {
        format!("{job_url}\n\n{text}")
    } else {
        text
    }
}

/// The PDF's title: "Job application: Acme", "Résumé: Acme", "CV: Acme", or the job post's
/// "Title · Company".
fn pdf_title(found: &SavedMatch, kind: &str) -> String {
    if kind == "job" {
        let title: Vec<&str> = [found.job_title.as_str(), found.company.as_str()]
            .into_iter()
            .filter(|part| crate::php::filled(part))
            .collect();
        return if title.is_empty() {
            "Job post".to_string()
        } else {
            title.join(" · ")
        };
    }
    let label = match kind {
        "application" => "Job application",
        "resume" => "Résumé",
        _ => "CV",
    };
    if found.company.is_empty() {
        label.to_string()
    } else {
        format!("{label}: {}", found.company)
    }
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
    let id = match_model::existing(state.db(), state.user_id, input.cv_text.trim(), input.job_text.trim()).await?;
    Ok(Json(json!({ "match": id.map(|id| json!({ "id": id })) })).into_response())
}

/// What each endpoint not in the desktop app yet does, for the page to say so.
const NOT_YET: [(&str, &str); 8] = [
    ("fetch_job", "Fetching a job post from a link"),
    ("extract_job", "The AI match"),
    ("extract_cv", "Reading a CV with the AI"),
    ("match", "The AI match"),
    ("write_application", "Writing the application"),
    ("tailor_resume", "Tailoring the résumé"),
    ("resume_comparison", "Comparing the tailored résumé"),
    ("application_delivery", "The Apply dialog"),
];

/// Every other address: a match endpoint not in the desktop app yet, said the way the page
/// shows errors, or a page that isn't there.
pub async fn not_yet(uri: Uri) -> PageError {
    match uri.path().strip_prefix("/match/") {
        Some(rest) => not_yet_for(rest.split('/').next().unwrap_or("")),
        None => PageError::new(StatusCode::NOT_FOUND, "There is no such page in the desktop app."),
    }
}

fn not_yet_for(endpoint: &str) -> PageError {
    let what = NOT_YET
        .iter()
        .find(|(name, _)| *name == endpoint)
        .map(|(_, what)| *what)
        .unwrap_or("That");
    PageError::new(
        StatusCode::NOT_IMPLEMENTED,
        format!("{what} isn't in the desktop app yet."),
    )
}
