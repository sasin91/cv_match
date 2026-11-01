//! The match page's work, for whichever front end asks: the web pages (pages.rs) today, a desktop
//! window later. Each function takes the app's shared state (Context) and plain inputs, and
//! answers a view model or data, or a ServiceError to show. Nothing here knows about HTTP: the
//! pages read the request, call one of these, and print what comes back.

use std::sync::Arc;

use super::ai_match::{self, AiMatch, Matching};
use super::application_delivery::ApplicationDelivery;
use super::cv_links::CvLink;
use super::document_box::{DocumentBox, RESUME};
use super::document_writer::{self, WriteError};
use super::free_reader::{FreeReader, YearMonth};
use super::joblog::{self, Entry};
use super::joblog_panel::{JoblogPanel, Logged};
use super::llm_key;
use super::match_documents::{self, MatchDocuments};
use super::match_model::{self, JOBNET_JOBLOG, LISTS, ListedMatch, NewMatch, SHORTLIST, SavedMatch};
use super::match_result::{Fresh, MatchResult};
use super::matcher::{self, Matcher, Verdict, Verdicts};
use super::page_reader::{self, Page};
use super::resume_comparison::ResumeComparison;
use super::saved_match_row::SavedMatchRow;
use super::{Job, Profile, apply_instructions, match_prompts, pdf_writer, reject_reason};
use crate::account::account_model::JOB_EVALUATION;
use crate::context::Context;
use crate::laya::laya_client;
use crate::profile;
use crate::service_error::{ErrorKind, ServiceError};

/// Longest CV or job post the free match reads, and longest text made into a PDF, in characters.
pub const MAX_TEXT: usize = 40_000;

/// Saved matches a list gives at a time; the next ones come after the last one's cursor.
const PER_PAGE: usize = 25;

/// Most pages fetch_job reads in FETCH_WINDOW_SECONDS: a batch of links a few times over, not a
/// crawler.
pub const MAX_FETCHES: usize = 30;
/// The window fetches are counted in, in seconds.
pub const FETCH_WINDOW_SECONDS: i64 = 600;

/// What the free match says of a requirement the taxonomy didn't find in the CV.
const NOT_FOUND_REASON: &str = "Not named in the CV. The AI match also looks for related experience.";

/// The extension's history: the latest matches, and the ones to log on Jobnet first.
const HISTORY_SIZE: usize = 50;

/// A batch of a list's saved matches, best first.
pub struct SavedMatchList {
    pub matches: Vec<SavedMatchRow>,
    /// The list shown (match_model::LISTS), the shortlist when the one asked for isn't known.
    pub list: &'static str,
    /// The cursor of the next batch, when there is one.
    pub next_after: Option<String>,
}

/// A batch of the list named list (the shortlist when None or unknown), after the cursor after
/// ("" for the first batch).
pub async fn saved_matches(context: &Context, list: Option<&str>, after: &str) -> Result<SavedMatchList, ServiceError> {
    let list = LISTS
        .iter()
        .map(|(name, ..)| *name)
        .find(|name| list == Some(*name))
        .unwrap_or(SHORTLIST);
    let found = match_model::best_first(context.db(), context.user_id, PER_PAGE, after, list).await?;
    Ok(SavedMatchList {
        matches: found.rows.iter().map(SavedMatchRow::from).collect(),
        list,
        next_after: found.next,
    })
}

/// One of the user's saved matches by its id; ids below 1 are no match.
pub async fn saved_match(context: &Context, id: i64) -> Result<SavedMatch, ServiceError> {
    if id <= 0 {
        return Err(ServiceError::not_found());
    }
    match_model::find(context.db(), id, context.user_id)
        .await?
        .ok_or_else(ServiceError::not_found)
}

/// A saved match as its result shows it, with its application and résumé panel.
pub async fn saved_result(context: &Context, id: i64) -> Result<MatchResult, ServiceError> {
    let found = saved_match(context, id).await?;
    let uses = llm_key::ai_uses(context).await?;
    let documents = MatchDocuments::from(
        Some(found.id),
        "",
        &match_documents::written_of(&found),
        uses,
        found.apply_instructions.as_ref(),
    );
    Ok(MatchResult::from_saved(&found, true, documents))
}

/// The free match: both texts read with the free reader (no model), matched with the taxonomy
/// and scored like the full match; what the taxonomy can't find in the CV counts as missing.
/// Nothing is saved. With Laya's model, the result has its answer on these verdicts.
pub async fn quick_match(context: &Context, job_text: &str, cv_text: &str) -> Result<MatchResult, ServiceError> {
    let job_text = job_text.trim();
    let cv_text = cv_text.trim();
    if job_text.is_empty() || cv_text.is_empty() {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Paste or upload both the CV and the job post.",
        ));
    }
    if job_text.chars().count() > MAX_TEXT || cv_text.chars().count() > MAX_TEXT {
        return Err(ServiceError::new(
            ErrorKind::TooLarge,
            "That text is too long. Paste only the CV and the job post.",
        ));
    }
    let taxonomy = &context.taxonomy;
    let mut reader = FreeReader::new(taxonomy, this_month());
    let job = reader.job(job_text);
    let profile = reader.cv(cv_text);
    let matcher = Matcher::new(taxonomy, profile.clone(), reader.known.clone());
    let groups = matcher::criteria(&job);
    let mut verdicts = Verdicts::new();
    for unit in matcher::units(&groups) {
        let verdict = matcher.decide(unit).unwrap_or_else(not_found_verdict);
        verdicts.insert(unit.id.clone(), verdict);
    }
    let verdicts = matcher::combine(&groups, &verdicts);
    if groups.values().all(Vec::is_empty) {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "The free match found no skills it knows in this post. The AI match reads requirements written as sentences too.",
        ));
    }
    let score = matcher::score(&groups, &verdicts);
    let apply = apply_instructions::read(job_text);
    let laya = laya_client::reading(context.laya.as_ref(), &matcher::laya_summary(&job, &groups, &verdicts)).await;
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
    let writer = llm_key::ai_uses(context).await?.any();
    Ok(MatchResult::from(&fresh, writer, None, "").with_cv_found(&profile))
}

/// The free match's verdict on a requirement the taxonomy didn't find in the CV.
fn not_found_verdict() -> Verdict {
    Verdict {
        verdict: "missing".to_string(),
        by: "taxonomy".to_string(),
        method: "not_found".to_string(),
        evidence: String::new(),
        reason: NOT_FOUND_REASON.to_string(),
    }
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

/// What a reject or its undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectOutcome {
    /// Done, with what to tell the user; None when the match was already where it was asked to be.
    Done(Option<&'static str>),
    /// Not done: what is missing from the reasons, to show back on the match.
    Unfinished(String),
}

/// Rejects a saved match (not interested) for reasons, with notes per reason, so it leaves every
/// list but Rejected; undo brings it back instead.
pub async fn reject(
    context: &Context,
    id: i64,
    reasons: &[String],
    notes: &[(String, String)],
    undo: bool,
) -> Result<RejectOutcome, ServiceError> {
    if !undo {
        let problem = reject_reason::problem(reasons, notes);
        if !problem.is_empty() {
            return Ok(RejectOutcome::Unfinished(problem));
        }
    }
    let done = if undo {
        match_model::bring_back(context.db(), id, context.user_id).await?
    } else {
        let now = chrono::Utc::now().timestamp();
        match_model::reject(
            context.db(),
            id,
            context.user_id,
            &reject_reason::rows(reasons, notes),
            now,
        )
        .await?
    };
    if done {
        // Its found post, if the job scan found it, says the same.
        crate::job_scan::job_scan_model::follow_match(context.db(), id, undo).await?;
    }
    let message = match (done, undo) {
        (false, _) => None,
        (true, true) => Some("The match is back in your lists."),
        (true, false) => Some("Rejected. It is in the Rejected list if you change your mind."),
    };
    Ok(RejectOutcome::Done(message))
}

/// The saved matches for the Jobnet extension.
pub struct ExtensionHistory {
    /// The latest ones, newest first.
    pub matches: Vec<ListedMatch>,
    /// The ones applied for but not in Jobnet's joblog yet ("Applied, not on Jobnet yet"), best
    /// first, which the extension offers first.
    pub not_joblogged: Vec<ListedMatch>,
}

pub async fn extension_history(context: &Context) -> Result<ExtensionHistory, ServiceError> {
    let db = context.db();
    Ok(ExtensionHistory {
        matches: match_model::recent(db, context.user_id, HISTORY_SIZE).await?,
        not_joblogged: match_model::best_first(db, context.user_id, HISTORY_SIZE, "", "not_joblogged")
            .await?
            .rows,
    })
}

/// A saved match as an entry for Jobnet's joblog (joblog::read).
pub async fn joblog_entry(context: &Context, id: i64) -> Result<Entry, ServiceError> {
    Ok(joblog::read(&saved_match(context, id).await?))
}

/// The same entry as the match prints it under the result.
pub async fn joblog_panel(context: &Context, id: i64) -> Result<JoblogPanel, ServiceError> {
    let found = saved_match(context, id).await?;
    Ok(JoblogPanel::from(found.id, &joblog::read(&found)))
}

/// Saves how far the candidate is with the job in Jobnet's words (joblog::SEARCH_STATUSES), and
/// whether they entered it in their Jobnet joblog; answers the "In your Jobnet joblog" section.
pub async fn save_joblog_status(
    context: &Context,
    id: i64,
    status: &str,
    logged: bool,
) -> Result<Logged, ServiceError> {
    let found = saved_match(context, id).await?;
    if joblog::search_status(status).is_none() {
        let statuses: Vec<&str> = joblog::SEARCH_STATUSES.iter().map(|(_, text)| *text).collect();
        return Err(ServiceError::new(
            ErrorKind::Malformed,
            format!("Pick one of Jobnet's statuses: {}.", statuses.join(", ")),
        ));
    }
    let now = chrono::Utc::now().timestamp();
    let saved = match_model::save_integration(context.db(), found.id, JOBNET_JOBLOG, status, logged, now).await?;
    Ok(Logged::from(
        saved.as_ref().and_then(|saved| saved.logged_at),
        joblog::applied_on(saved.as_ref()),
    ))
}

/// A PDF of a saved match's document, named for the job.
#[derive(Debug, Clone)]
pub struct PdfFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A saved match's job application (kind application), tailored résumé (resume), the CV it was
/// matched with (cv) or its job post (job) as a PDF (pdf_writer). A résumé with fields
/// (tailored_resume) is laid out from them. Otherwise edited is the text as the user sees it,
/// edits included; without it the saved text is used; the CV and the job post are always the
/// saved text. document_id, an earlier version, is its text (and fields) as written.
pub async fn pdf(
    context: &Context,
    id: i64,
    kind: &str,
    edited: &str,
    document_id: Option<i64>,
) -> Result<PdfFile, ServiceError> {
    if !pdf_writer::KINDS.contains(&kind) {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Choose the application, the résumé, the CV or the job post.",
        ));
    }
    let found = saved_match(context, id).await?;
    let version = match document_id {
        Some(document_id) => Some(
            match_model::version(context.db(), found.id, document_id)
                .await?
                .filter(|version| version.kind == kind)
                .ok_or_else(|| ServiceError::new(ErrorKind::NotFound, "That version was not found."))?,
        ),
        None => None,
    };
    let text = match &version {
        Some(version) => version.text.trim().to_string(),
        None => pdf_text(&found, kind, edited.trim()),
    };
    if text.is_empty() {
        return Err(ServiceError::new(ErrorKind::Invalid, "There is no text to export yet."));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(ServiceError::new(
            ErrorKind::TooLarge,
            "That text is too long for a PDF.",
        ));
    }
    let title = pdf_title(&found, kind);
    // A résumé written with fields is laid out from them; the text is only for one written
    // before they existed.
    let fields = match (&version, kind) {
        (Some(version), RESUME) => match_model::resume_fields_of(context.db(), version.id).await?,
        (None, RESUME) => match_model::resume_fields(context.db(), found.id).await?,
        _ => None,
    };
    let made = match &fields {
        Some(fields) => pdf_writer::resume_pdf(fields, &title),
        None => pdf_writer::pdf(kind, &text, &title),
    };
    let bytes = made.map_err(|e| ServiceError::internal("match/pdf", format!("The PDF couldn't be made: {e}")))?;
    Ok(PdfFile {
        name: pdf_writer::file_name(kind, &found.job_title, &found.company),
        bytes,
    })
}

/// The text a PDF is made from: what the user edited, or the saved text when there's none; the
/// CV and the job post are always the saved text, the post under its link.
fn pdf_text(found: &SavedMatch, kind: &str, edited: &str) -> String {
    let saved = |text: Option<&str>| text.unwrap_or("").trim().to_string();
    let text = if edited.is_empty() || kind == "cv" || kind == "job" {
        match kind {
            "application" => saved(found.application_text.as_deref()),
            "resume" => saved(found.resume_text.as_deref()),
            "cv" => saved(Some(&found.cv_text)),
            _ => saved(Some(&found.job_text)),
        }
    } else {
        edited.to_string()
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

/// The newest saved match of this CV with the same job post text, if any, so a post matched
/// before can be skipped.
pub async fn already_matched(context: &Context, cv_text: &str, job_text: &str) -> Result<Option<i64>, ServiceError> {
    Ok(match_model::existing(context.db(), context.user_id, cv_text.trim(), job_text.trim()).await?)
}

/// The job post at a pasted link (page_reader). Fetches are counted (MAX_FETCHES in
/// FETCH_WINDOW_SECONDS), so a batch of links can't become a crawler.
pub async fn fetch_job(context: &Context, url: &str) -> Result<Page, ServiceError> {
    if let Err(minutes) = context.count_fetch(chrono::Utc::now().timestamp()) {
        let plural = if minutes == 1 { "" } else { "s" };
        return Err(ServiceError::new(
            ErrorKind::TooMany,
            format!(
                "That's a lot of links in a short time. Try again in {minutes} minute{plural}, or paste the post text."
            ),
        ));
    }
    page_reader::read(url).await.map_err(|why| {
        eprintln!("match (fetch_job): {url}: {why}");
        ServiceError::new(ErrorKind::Invalid, why)
    })
}

/// The job post as structured requirements, read by the model, or by the free reader when the
/// key is off for evaluating job posts.
pub async fn extract_job(context: &Context, text: &str) -> Result<Job, ServiceError> {
    if llm_key::llm_settings(context, JOB_EVALUATION).await?.is_none() {
        return Ok(FreeReader::new(&context.taxonomy, this_month()).job(text));
    }
    let keyed = llm_key::llm(context, JOB_EVALUATION).await?;
    match_prompts::read_job(&keyed.llm, text)
        .await
        .map_err(|e| ServiceError::llm("match/extract_job", &e))
}

/// The CV as a structured profile, read by the model, or by the free reader when the key is off
/// for evaluating job posts.
pub async fn extract_cv(context: &Context, text: &str) -> Result<Profile, ServiceError> {
    if llm_key::llm_settings(context, JOB_EVALUATION).await?.is_none() {
        return Ok(FreeReader::new(&context.taxonomy, this_month()).cv(text));
    }
    let keyed = llm_key::llm(context, JOB_EVALUATION).await?;
    match_prompts::read_cv(&keyed.llm, text)
        .await
        .map_err(|e| ServiceError::llm("match/extract_cv", &e))
}

/// A read job post and a read CV, with their texts, to match.
#[derive(Debug, Default, Clone)]
pub struct MatchRequest {
    pub job: Job,
    pub profile: Profile,
    pub job_text: String,
    pub job_url: String,
    pub cv_name: String,
    pub cv_text: String,
}

/// The match of a read job post with a read CV (ai_match), saved when both texts are given.
pub async fn ai_match(context: &Context, request: &MatchRequest) -> Result<AiMatch, ServiceError> {
    let keyed = match llm_key::llm_settings(context, JOB_EVALUATION).await? {
        Some(_) => Some(llm_key::llm(context, JOB_EVALUATION).await?),
        None => None,
    };
    let matching = Matching {
        db: context.db(),
        taxonomy: &context.taxonomy,
        user_id: context.user_id,
        now: chrono::Utc::now().timestamp(),
        laya: context.laya.as_ref(),
    };
    let new = NewMatch {
        job: &request.job,
        job_url: &request.job_url,
        job_text: &request.job_text,
        cv_name: &request.cv_name,
        cv_text: &request.cv_text,
    };
    let llm = keyed.as_ref().map(|keyed| &keyed.llm);
    ai_match::ai_match(&matching, llm, new, &request.profile, Default::default())
        .await
        .map_err(|e| ServiceError::llm("match/match", &e))
}

/// The AI match as its result shows it, with the application and résumé panel of its saved match.
pub async fn ai_match_result(
    context: &Context,
    request: &MatchRequest,
    found: &AiMatch,
) -> Result<MatchResult, ServiceError> {
    let uses = llm_key::ai_uses(context).await?;
    let documents = MatchDocuments::from(
        found.saved_id,
        found.save_error.as_deref().unwrap_or(""),
        &Default::default(),
        uses,
        found.apply_instructions.as_ref(),
    );
    let fresh = Fresh {
        job: &request.job,
        groups: &found.groups,
        verdicts: &found.verdicts,
        score: &found.result,
        soft_skills: found.soft_skills.clone(),
        apply_instructions: found.apply_instructions.as_ref(),
        saved_id: found.saved_id,
        laya: &found.laya_reading,
    };
    let laya_input = ai_match::laya_input(&request.job, &request.profile, &request.cv_text, &found.laya_summary);
    Ok(MatchResult::from(&fresh, true, Some(documents), &laya_input))
}

/// Writes kind (document_box::APPLICATION or RESUME) for a saved match from its CV and post, with
/// the candidate's notes, keeps it as the match's newest version, and answers its box. The
/// writing runs as its own task, so it finishes and is saved even when the one who asked stops
/// waiting (a page reloaded, a window closed); the saved match has it afterwards.
pub async fn write(
    context: Arc<Context>,
    id: i64,
    notes: &str,
    kind: &'static str,
) -> Result<DocumentBox, ServiceError> {
    let found = saved_match(&context, id).await?;
    let keyed = llm_key::llm(&context, kind).await?;
    let notes = document_writer::notes(notes);
    let task_context = context.clone();
    let task_found = found.clone();
    let writing = tokio::spawn(async move {
        let db = task_context.db();
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
    let work = if kind == RESUME {
        "match/tailor_resume"
    } else {
        "match/write_application"
    };
    let (keywords, left_out) = writing
        .await
        .map_err(|e| ServiceError::failed(work, format!("The writing stopped: {e}")))?
        .map_err(|e| match e {
            WriteError::Llm(e) => ServiceError::llm(work, &e),
            WriteError::Database(e) => ServiceError::from(e),
            WriteError::Empty(why) => ServiceError::failed(work, why),
        })?;
    let found = saved_match(&context, found.id).await?;
    let uses = llm_key::ai_uses(&context).await?;
    let (application, resume) = match_documents::written_of(&found);
    let written = if kind == RESUME { resume } else { application };
    Ok(DocumentBox::from(
        kind,
        found.id,
        &written,
        uses.of(kind),
        found.apply_instructions.as_ref(),
    )
    .with_keywords(keywords, &left_out))
}

/// The tailored résumé's saved match (resume_id) against the CV's (id).
pub async fn resume_comparison(context: &Context, id: i64, resume_id: i64) -> Result<ResumeComparison, ServiceError> {
    let cv_match = saved_match(context, id).await?;
    let resume_match = saved_match(context, resume_id).await?;
    Ok(ResumeComparison::from(&cv_match, &resume_match))
}

/// The Apply dialog's content for a saved match: how the application is delivered, by email or on
/// the employer's form, with the PDFs and the profile links to copy.
pub async fn application_delivery(context: &Context, id: i64) -> Result<ApplicationDelivery, ServiceError> {
    let found = saved_match(context, id).await?;
    let links: Vec<CvLink> = profile::links_for_user(context.db(), context.user_id)
        .await?
        .into_iter()
        .map(|(label, url)| CvLink { label, url })
        .collect();
    Ok(ApplicationDelivery::from(&found, &links))
}
