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

use super::ai_match::{self, AiMatch, Matching};
use super::application_delivery::{ApplicationDelivery, rawurlencode};
use super::batch_row::BatchRow;
use super::cv_links::CvLink;
use super::document_box::{APPLICATION, DocumentBox, RESUME};
use super::document_writer::{self, WriteError};
use super::free_reader::{FreeReader, YearMonth};
use super::joblog;
use super::joblog_panel::{JoblogPanel, Logged};
use super::llm_key;
use super::match_documents::{self, MatchDocuments};
use super::match_model::{self, JOBNET_JOBLOG, LISTS, NewMatch, SHORTLIST, SavedMatch};
use super::match_result::{Fresh, MatchResult};
use super::matcher::{self, Matcher, Verdict, Verdicts};
use super::page_error::PageError;
use super::page_reader;
use super::resume_comparison::ResumeComparison;
use super::saved_match_row::SavedMatchRow;
use super::{Job, Profile, apply_instructions, match_prompts, pdf_writer, reject_reason};
use crate::account::account_model::JOB_EVALUATION;
use crate::laya::laya_client;
use crate::profile;
use crate::server::AppState;

/// Longest CV or job post quick_match reads, in characters.
const MAX_TEXT: usize = 40_000;

/// Most job posts the page compares against one CV in one go.
const MAX_JOBS: usize = 10;

/// Saved matches the page lists at a time; scrolling to the end loads the next ones.
const PER_PAGE: usize = 25;

/// Most pages fetch_job reads in FETCH_WINDOW_SECONDS: a batch of MAX_JOBS links a few times
/// over, not a crawler.
pub const MAX_FETCHES: usize = 30;
pub const FETCH_WINDOW_SECONDS: i64 = 600;

/// What the free match says of a requirement the taxonomy didn't find in the CV.
const NOT_FOUND_REASON: &str = "Not named in the CV. The AI match also looks for related experience.";

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
    let uses = llm_key::ai_uses(state).await?;
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
    let uses = llm_key::ai_uses(&state).await?;
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
/// what the taxonomy can't find in the CV counts as missing. Nothing is saved. With Laya's model,
/// the result has its answer on these verdicts.
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
    let laya = laya_client::reading(state.laya.as_ref(), &matcher::laya_summary(&job, &groups, &verdicts)).await;
    let fresh = Fresh {
        job: &job,
        groups: &groups,
        verdicts: &verdicts,
        score: &score,
        soft_skills: matcher::soft_skills(&job),
        apply_instructions: apply.as_ref(),
        saved_id: None,
        laya: &laya,
    };
    let writer = llm_key::ai_uses(&state).await?.any();
    let result = MatchResult::from(&fresh, writer, None, "").with_cv_found(&profile);
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

/// POST joblog_extension/open_edge: opens Edge's extensions page (edge://extensions), which the
/// setup page can't link to, so the user can load the extension from the folder the page copied.
pub async fn open_edge(State(state): State<Arc<AppState>>) -> Answer {
    if let Err(why) = &state.extension_folder {
        return Err(PageError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("There is no extension folder to load. {why}"),
        ));
    }
    crate::extension::open_extensions_page().map_err(|e| {
        eprintln!("joblog_extension: open_edge: {e}");
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

/// POST match/pdf {id, kind, text}: a saved match's job application (kind application),
/// tailored résumé (kind resume), the CV it was matched with (kind cv) or its job post (kind job)
/// as a PDF download (pdf_writer). A résumé with fields (tailored_resume) is laid out from them,
/// so edits to its text aren't in the PDF (the page says so first). Otherwise text is what the
/// page shows, edits included; without it the saved text is used; the CV and the job post are
/// always the saved text. document_id, an earlier version from the box's list, is its text (and
/// fields) as written. Errors come back as JSON like the other endpoints.
pub async fn pdf(State(state): State<Arc<AppState>>, Json(input): Json<PdfInput>) -> Answer {
    let kind = input.kind.as_str();
    if !pdf_writer::KINDS.contains(&kind) {
        return Err(PageError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Choose the application, the résumé, the CV or the job post.",
        ));
    }
    let found = saved_match(&state, id_in_body(&input.id)).await?;
    let version = match input.document_id {
        Some(document_id) => Some(
            match_model::version(state.db(), found.id, document_id)
                .await?
                .filter(|version| version.kind == kind)
                .ok_or_else(|| PageError::new(StatusCode::NOT_FOUND, "That version was not found."))?,
        ),
        None => None,
    };
    let text = match &version {
        Some(version) => version.text.trim().to_string(),
        None => pdf_text(&found, kind, input.text.trim()),
    };
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
    let fields = match (&version, kind) {
        (Some(version), RESUME) => match_model::resume_fields_of(state.db(), version.id).await?,
        (None, RESUME) => match_model::resume_fields(state.db(), found.id).await?,
        _ => None,
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
    let saved = |text: Option<&str>| text.unwrap_or("").trim().to_string();
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
            .filter(|part| !part.is_empty())
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
/// (page_reader). Fetches are counted (MAX_FETCHES in FETCH_WINDOW_SECONDS), so a batch of links
/// can't become a crawler.
pub async fn fetch_job(State(state): State<Arc<AppState>>, Json(input): Json<FetchJobInput>) -> Answer {
    if let Err(minutes) = state.count_fetch(chrono::Utc::now().timestamp()) {
        let plural = if minutes == 1 { "" } else { "s" };
        return Err(PageError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "That's a lot of links in a short time. Try again in {minutes} minute{plural}, or paste the post text."
            ),
        ));
    }
    let url = input.url.as_str().unwrap_or("");
    match page_reader::read(url).await {
        Ok(page) => Ok(Json(page).into_response()),
        Err(why) => {
            eprintln!("match (fetch_job): {url}: {why}");
            Err(PageError::new(StatusCode::UNPROCESSABLE_ENTITY, why))
        }
    }
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
    let job: Job = if llm_key::llm_settings(&state, JOB_EVALUATION).await?.is_none() {
        FreeReader::new(&state.taxonomy, this_month()).job(&input.text)
    } else {
        let keyed = llm_key::llm(&state, JOB_EVALUATION).await?;
        match_prompts::read_job(&keyed.llm, &input.text)
            .await
            .map_err(|e| PageError::llm("extract_job", &e))?
    };
    Ok(Json(job).into_response())
}

/// POST match/extract_cv {text}: the CV as a structured profile, read by the model, or by the
/// free reader when the key is off for evaluating job posts.
pub async fn extract_cv(State(state): State<Arc<AppState>>, Json(input): Json<ExtractInput>) -> Answer {
    let profile: Profile = if llm_key::llm_settings(&state, JOB_EVALUATION).await?.is_none() {
        FreeReader::new(&state.taxonomy, this_month()).cv(&input.text)
    } else {
        let keyed = llm_key::llm(&state, JOB_EVALUATION).await?;
        match_prompts::read_cv(&keyed.llm, &input.text)
            .await
            .map_err(|e| PageError::llm("extract_cv", &e))?
    };
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
    let keyed = match llm_key::llm_settings(&state, JOB_EVALUATION).await? {
        Some(_) => Some(llm_key::llm(&state, JOB_EVALUATION).await?),
        None => None,
    };
    let matching = Matching {
        db: state.db(),
        taxonomy: &state.taxonomy,
        user_id: state.user_id,
        now: chrono::Utc::now().timestamp(),
        laya: state.laya.as_ref(),
    };
    let new = NewMatch {
        job: &input.job,
        job_url: &input.job_url,
        job_text: &input.job_text,
        cv_name: &input.cv_name,
        cv_text: &input.cv_text,
    };
    let llm = keyed.as_ref().map(|keyed| &keyed.llm);
    let found = ai_match::ai_match(&matching, llm, new, &input.profile, Default::default())
        .await
        .map_err(|e| PageError::llm("match", &e))?;
    if !wants_html(&headers) {
        return Ok(Json(found).into_response());
    }
    let result = ai_match_result(&state, &input, &found).await?;
    respond_result(result, view, &input.job_url)
}

/// The AI match as the page shows it, with the application and résumé panel of its saved match.
async fn ai_match_result(state: &AppState, input: &MatchInput, found: &AiMatch) -> Result<MatchResult, PageError> {
    let uses = llm_key::ai_uses(state).await?;
    let documents = MatchDocuments::from(
        found.saved_id,
        found.save_error.as_deref().unwrap_or(""),
        &Default::default(),
        uses,
        found.apply_instructions.as_ref(),
    );
    let fresh = Fresh {
        job: &input.job,
        groups: &found.groups,
        verdicts: &found.verdicts,
        score: &found.result,
        soft_skills: found.soft_skills.clone(),
        apply_instructions: found.apply_instructions.as_ref(),
        saved_id: found.saved_id,
        laya: &found.laya_reading,
    };
    let laya_input = ai_match::laya_input(&input.job, &input.profile, &input.cv_text, &found.laya_summary);
    Ok(MatchResult::from(&fresh, true, Some(documents), &laya_input))
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
    let found = saved_match(&state, id_of(&id)).await?;
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

/// Writes kind for the saved match and answers its box. The writing runs as its own task, so it
/// finishes and is saved even when the page drops the connection meanwhile (a laptop asleep, a
/// tab reloaded); the page picks it up from the saved match when it's back.
async fn write(state: Arc<AppState>, input: WriteInput, kind: &'static str) -> Answer {
    let found = saved_match(&state, id_in_body(&input.id)).await?;
    let keyed = llm_key::llm(&state, kind).await?;
    let notes = document_writer::notes(&input.notes);
    let task_state = state.clone();
    let task_found = found.clone();
    let writing = tokio::spawn(async move {
        let db = task_state.db();
        let now = chrono::Utc::now().timestamp();
        if kind == RESUME {
            document_writer::tailor_resume(db, &task_found, &keyed, &notes, now)
                .await
                .map(|tailored| (tailored.keywords, tailored.left_out))
        } else {
            document_writer::write_application(db, &task_found, &keyed, &notes, now)
                .await
                .map(|_| (Vec::new(), Vec::new()))
        }
    });
    let endpoint = if kind == RESUME {
        "tailor_resume"
    } else {
        "write_application"
    };
    let (keywords, left_out) = writing
        .await
        .map_err(|e| PageError::failed(endpoint, format!("The writing stopped: {e}")))?
        .map_err(|e| match e {
            WriteError::Llm(e) => PageError::llm(endpoint, &e),
            WriteError::Database(e) => PageError::from(e),
            WriteError::Empty(why) => PageError::failed(endpoint, why),
        })?;
    let found = saved_match(&state, found.id).await?;
    let uses = llm_key::ai_uses(&state).await?;
    let (application, resume) = match_documents::written_of(&found);
    let written = if kind == RESUME { resume } else { application };
    let doc = DocumentBox::from(
        kind,
        found.id,
        &written,
        uses.of(kind),
        found.apply_instructions.as_ref(),
    )
    .with_keywords(keywords, &left_out);
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
    let cv_match = saved_match(&state, id_of(&id)).await?;
    let resume_match = saved_match(&state, id_of(&resume_id)).await?;
    html(ResumeComparisonView {
        comparison: ResumeComparison::from(&cv_match, &resume_match),
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
    let found = saved_match(&state, id_of(&id)).await?;
    let links: Vec<CvLink> = profile::links_for_user(state.db(), state.user_id)
        .await?
        .into_iter()
        .map(|(label, url)| CvLink { label, url })
        .collect();
    html(ApplicationDeliveryView {
        delivery: ApplicationDelivery::from(&found, &links),
    })
}
