//! The match page and the endpoints its JS calls, ported from modules/cv_match/Cv_match.php for
//! the one local user (always signed in). Endpoints that need what isn't ported yet (the AI
//! match, writing, PDFs) answer JSON {error} saying so, as the PHP endpoints
//! answer their errors, so the page shows it.

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use serde_json::json;

use super::batch_row::BatchRow;
use super::cv_match_model::{self, JOBNET_JOBLOG, LISTS, SHORTLIST, SavedMatch};
use super::cv_matcher::{self, CvMatcher, Verdict, Verdicts};
use super::free_reader::{FreeReader, YearMonth};
use super::joblog;
use super::joblog_panel::{JoblogPanel, Logged};
use super::match_documents::{self, MatchDocuments, Uses};
use super::match_result::{Fresh, MatchResult};
use super::saved_match_row::SavedMatchRow;
use super::{apply_instructions, reject_reason};
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

impl From<rusqlite::Error> for PageError {
    fn from(e: rusqlite::Error) -> Self {
        eprintln!("cv_match: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The database couldn't answer: {e}"),
        )
    }
}

impl From<askama::Error> for PageError {
    fn from(e: askama::Error) -> Self {
        eprintln!("cv_match: {e}");
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
#[template(path = "cv_match/cv_match.html")]
struct CvMatchPage {
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
#[template(path = "cv_match/history.html")]
struct History {
    matches: Vec<SavedMatchRow>,
    next: Option<String>,
    list_query: String,
}

#[derive(Template)]
#[template(path = "cv_match/history_latest.html")]
struct HistoryLatest {
    matches: Vec<SavedMatchRow>,
    next: Option<String>,
    list_query: String,
}

#[derive(Template)]
#[template(path = "cv_match/result.html")]
struct ResultView {
    result: MatchResult,
}

#[derive(Template)]
#[template(path = "cv_match/batch_row.html")]
struct BatchRowView {
    result: MatchResult,
    row: BatchRow,
}

#[derive(Template)]
#[template(path = "cv_match/joblog_panel.html")]
struct JoblogPanelView {
    panel: JoblogPanel,
}

#[derive(Template)]
#[template(path = "cv_match/joblog_logged.html")]
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

/// Which keys the AI may use; none until the LLM adapters are ported.
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

fn saved_matches(state: &AppState, query: &ListQuery, first: bool) -> Result<SavedMatches, PageError> {
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
    let found = cv_match_model::best_first(&state.db(), state.user_id, PER_PAGE, after, list)?;
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
            let after = format!("after={}", super::application_delivery::rawurlencode(&next));
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
                "cv_match".to_string()
            } else {
                format!("cv_match?list={list}")
            },
            label,
            current: list == current,
        })
        .collect()
}

/// GET /: the match page is the app's front page.
pub async fn home() -> Redirect {
    Redirect::to("/cv_match")
}

/// GET cv_match: the match page.
pub async fn index(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    match_page(&state, &query, None)
}

/// GET cv_match/{id}: the match page with that saved match open, so it can be linked to. Any
/// other cv_match/{name} is an endpoint not in the desktop app yet.
pub async fn show(
    State(state): State<Arc<AppState>>,
    Path(segment): Path<String>,
    Query(query): Query<ListQuery>,
) -> Answer {
    match segment.parse::<i64>() {
        Ok(id) => match_page(&state, &query, (id > 0).then_some(id)),
        Err(_) => Err(not_yet_for(&segment)),
    }
}

fn match_page(state: &AppState, query: &ListQuery, open_id: Option<i64>) -> Answer {
    let saved = saved_matches(state, query, false)?;
    let uses = ai_uses();
    let empty_text = LISTS
        .iter()
        .find(|(name, ..)| *name == saved.list)
        .map(|(.., empty)| *empty)
        .unwrap_or("");
    html(CvMatchPage {
        flash: state.take_flash(),
        ai_ready: uses.any(),
        ai_uses_json:
            json!({ "resume": uses.resume, "application": uses.application, "job_evaluation": uses.job_evaluation })
                .to_string(),
        max_jobs: MAX_JOBS,
        open_id_json: json!(open_id).to_string(),
        profile_text: profile::text_for_user(&state.db(), state.user_id)?,
        list_tabs: list_tabs(saved.list),
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
        empty_text,
    })
}

/// GET cv_match/history_list: the first batch of the page's list, polled by Trongate MX so
/// matches saved elsewhere show up on their own.
pub async fn history_list(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state, &query, true)?;
    html(HistoryLatest {
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET cv_match/history_more?after=...: the next saved matches of the page's list.
pub async fn history_more(State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>) -> Answer {
    let saved = saved_matches(&state, &query, false)?;
    html(History {
        matches: saved.matches,
        next: saved.next,
        list_query: saved.list_query,
    })
}

/// GET cv_match/result/{id}: a saved match as the page shows it, with its application and
/// résumé panel; ?view=row as a row of the ranked jobs.
pub async fn result(State(state): State<Arc<AppState>>, Path(id): Path<i64>, Query(view): Query<ViewQuery>) -> Answer {
    let found = cv_match_model::find(&state.db(), id, state.user_id)?.ok_or_else(PageError::not_found)?;
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

/// POST cv_match/quick_match {job_text, cv_text, job_url}: the free match. Both texts are read
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
    let matcher = CvMatcher::new(taxonomy, profile.clone(), reader.known.clone());
    let groups = cv_matcher::criteria(&job);
    let mut verdicts = Verdicts::new();
    for unit in cv_matcher::units(&groups) {
        let verdict = matcher.decide(unit).unwrap_or_else(|| Verdict {
            verdict: "missing".to_string(),
            by: "taxonomy".to_string(),
            method: "not_found".to_string(),
            evidence: String::new(),
            reason: NOT_FOUND_REASON.to_string(),
        });
        verdicts.insert(unit.id.clone(), verdict);
    }
    let verdicts = cv_matcher::combine(&groups, &verdicts);
    if groups.values().all(Vec::is_empty) {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "The free match found no skills it knows in this post. The AI match reads requirements written as sentences too.",
        ));
    }
    let score = cv_matcher::score(&groups, &verdicts);
    let apply = apply_instructions::read(job_text);
    let fresh = Fresh {
        job: &job,
        groups: &groups,
        verdicts: &verdicts,
        score: &score,
        soft_skills: cv_matcher::soft_skills(&job),
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

/// POST cv_match/submit_reject/{id} with reasons[] and notes[reason]: rejects a saved match (not
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
        return Ok(Redirect::to(&format!("/cv_match/{id}")).into_response());
    }
    let done = {
        let mut db = state.db();
        if undo {
            cv_match_model::bring_back(&mut db, id, state.user_id)?
        } else {
            let now = chrono::Utc::now().timestamp();
            cv_match_model::reject(&mut db, id, state.user_id, &reject_reason::rows(&reasons, &notes), now)?
        }
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
        format!("/cv_match/{id}")
    } else {
        "/cv_match".to_string()
    };
    Ok(Redirect::to(&back).into_response())
}

/// One of the user's saved matches by its id in the address, or the error the page shows.
fn saved_match(state: &AppState, id: i64) -> Result<SavedMatch, PageError> {
    if id <= 0 {
        return Err(PageError::not_found());
    }
    cv_match_model::find(&state.db(), id, state.user_id)?.ok_or_else(PageError::not_found)
}

/// An id from the address; anything but a number is no match.
fn id_of(segment: &str) -> i64 {
    segment.parse().unwrap_or(0)
}

/// GET cv_match/history: the saved matches for the Jobnet extension: matches, the latest ones,
/// newest first; not_joblogged, the ones applied for but not in Jobnet's joblog yet (the list
/// "Applied, not on Jobnet yet"), best first, which the extension offers first.
pub async fn history(State(state): State<Arc<AppState>>) -> Answer {
    let db = state.db();
    let matches = cv_match_model::recent(&db, state.user_id, HISTORY_SIZE)?;
    let not_joblogged = cv_match_model::best_first(&db, state.user_id, HISTORY_SIZE, "", "not_joblogged")?.rows;
    Ok(Json(json!({ "matches": matches, "not_joblogged": not_joblogged })).into_response())
}

/// GET cv_match/joblog/{id}: a saved match as an entry for Jobnet's joblog (joblog::read): its
/// fields in Jobnet's order, filled in from the match, and which of the three PDFs Jobnet keeps
/// can be downloaded.
pub async fn joblog(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let found = saved_match(&state, id_of(&id))?;
    Ok(Json(joblog::read(&found)).into_response())
}

/// GET cv_match/joblog_panel/{id}: the same entry as joblog/{id} as the page prints it under the
/// match.
pub async fn joblog_panel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Answer {
    let found = saved_match(&state, id_of(&id))?;
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

/// POST cv_match/joblog_status {id, status, logged}: how far the candidate is with the job in
/// Jobnet's words (joblog::SEARCH_STATUSES), and whether they have entered it in their Jobnet
/// joblog. Answers the "In your Jobnet joblog" section as the page prints it.
pub async fn joblog_status(State(state): State<Arc<AppState>>, Json(input): Json<JoblogStatusInput>) -> Answer {
    let id = match &input.id {
        serde_json::Value::Number(id) => id.as_i64().unwrap_or(0),
        serde_json::Value::String(id) => id_of(id.trim()),
        _ => 0,
    };
    let found = saved_match(&state, id)?;
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
    let saved = cv_match_model::save_integration(&state.db(), found.id, JOBNET_JOBLOG, status, logged, now)?;
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

/// already_matched's body.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AlreadyMatchedInput {
    cv_text: String,
    job_text: String,
}

/// POST cv_match/already_matched {cv_text, job_text}: the newest saved match of this CV with the
/// same job post text, as {match: {id}}, or {match: null}. The page uses it to skip posts it
/// has matched before.
pub async fn already_matched(State(state): State<Arc<AppState>>, Json(input): Json<AlreadyMatchedInput>) -> Answer {
    let id = cv_match_model::existing(&state.db(), state.user_id, input.cv_text.trim(), input.job_text.trim())?;
    Ok(Json(json!({ "match": id.map(|id| json!({ "id": id })) })).into_response())
}

/// What each endpoint not in the desktop app yet does, for the page to say so.
const NOT_YET: [(&str, &str); 9] = [
    ("fetch_job", "Fetching a job post from a link"),
    ("extract_job", "The AI match"),
    ("extract_cv", "Reading a CV with the AI"),
    ("match", "The AI match"),
    ("write_application", "Writing the application"),
    ("tailor_resume", "Tailoring the résumé"),
    ("resume_comparison", "Comparing the tailored résumé"),
    ("pdf", "Downloading a PDF"),
    ("application_delivery", "The Apply dialog"),
];

/// Every other address: a cv_match endpoint not in the desktop app yet, said the way the page
/// shows errors, or a page that isn't there.
pub async fn not_yet(uri: Uri) -> PageError {
    match uri.path().strip_prefix("/cv_match/") {
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
