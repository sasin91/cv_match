//! The job scan's work, for whichever front end asks: `jobfinder scan` (which the schedule runs),
//! the dashboard's and a search's Scan now, and a desktop window later all call scan(), so a scan
//! is the same whoever starts it. Ported from recruiter's Job_scan (_scan and _auto_match),
//! without its queue: a scan finds each search's new posts and matches them one by one before it
//! ends.
//!
//! A scan asks the model, searching the web (OpenAI's Responses API), for new job posts that fit
//! each active search. Each new post is fetched, read and matched with the profile like a post on
//! the match page (match_service), saved as a match, and from the search's tailor_from_score up
//! its tailored résumé and application are written right away, so both are ready in the match's
//! Apply dialog.

use std::sync::Arc;

use serde_json::{Value, json};

use super::job_scan_model::{
    self as model, DISMISSED, FAILED, FoundAbout, MATCHED, MATCHING, NOT_A_JOB, NewPost, RecentPost, RunEnd,
};
use super::job_search_rules;
use crate::account::account_model::{APPLICATION, JOB_EVALUATION, RESUME};
use crate::context::Context;
use crate::llm::{Effort, OPENAI};
use crate::matches::match_service::{self, MatchRequest};
use crate::matches::{Profile, llm_key, match_model, page_reader};
use crate::profile;
use crate::service_error::{ErrorKind, ServiceError};

/// Posts one scan of a search asks the model for, at most.
pub const POSTS_PER_SCAN: usize = 15;

/// The found posts of a search the scan is told about, to skip them.
const RECENT_POSTS_SKIPPED: i64 = 40;

/// A search is scanned at most once in this many seconds, whoever asks (an hour), so a click
/// after the schedule's scan, or two clicks, can't run up the bill.
pub const MIN_SECONDS_BETWEEN_SCANS: i64 = 3600;

/// A scan not finished after this many seconds stopped without saying so (the program was
/// closed): it no longer counts as running (half an hour).
pub const RUN_GIVE_UP_SECONDS: i64 = 1800;

/// The country the web search is made from.
const SEARCH_COUNTRY: &str = "DK";

/// The name a match with the profile is saved under, as recruiter saves it.
pub const PROFILE_CV_NAME: &str = "Your profile";

/// What a scan did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanReport {
    /// Searches scanned.
    pub scanned: usize,
    /// Searches skipped: scanned less than MIN_SECONDS_BETWEEN_SCANS ago, or by another scan.
    pub skipped: usize,
    pub posts_found: i64,
    pub posts_new: i64,
    /// New posts matched and saved.
    pub matched: usize,
    /// Why a search's scan or a post's match failed, in words to show.
    pub problems: Vec<String>,
}

impl ScanReport {
    /// "Scanned 2 searches: 14 posts found, 5 new, 4 matched." and what went wrong.
    pub fn summary(&self) -> String {
        if self.scanned == 0 {
            return if self.skipped > 0 {
                "Every search was scanned less than an hour ago. Scan again a little later.".to_string()
            } else {
                "There is no active job search to scan.".to_string()
            };
        }
        let searches = if self.scanned == 1 { "search" } else { "searches" };
        let mut said = format!(
            "Scanned {} {searches}: {} posts found, {} new, {} matched.",
            self.scanned, self.posts_found, self.posts_new, self.matched
        );
        for problem in &self.problems {
            said.push(' ');
            said.push_str(problem);
        }
        said
    }
}

/// Why nothing can be scanned now, or None: the scan searches the web on the OpenAI key picked
/// for evaluating job posts, and matches with the profile.
pub async fn scan_problem(context: &Context) -> Result<Option<String>, ServiceError> {
    let problem = match llm_key::llm_settings(context, JOB_EVALUATION).await? {
        None => Some(
            "The job scan searches the web on your OpenAI key. Pick it for evaluating job posts on the API key page.",
        ),
        Some(key) if key.provider != OPENAI => Some(
            "The job scan searches the web, which only OpenAI's API does here. Pick your OpenAI key for evaluating job posts on the API key page.",
        ),
        Some(_) => None,
    };
    if problem.is_some() {
        return Ok(problem.map(str::to_string));
    }
    if profile::text_for_user(context.db(), context.user_id).await?.is_none() {
        return Ok(Some(
            "Every post is matched with your profile, and there is none yet. Read your CV in setup first.".to_string(),
        ));
    }
    Ok(None)
}

/// How the job scan stands, for the dashboard.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanStatus {
    /// Searches switched on.
    pub active_searches: i64,
    /// Why nothing can be scanned now (scan_problem), or the first search's pause.
    pub problem: Option<String>,
    pub running: bool,
    /// The latest scan of any search.
    pub last_scan: Option<LatestScan>,
    /// New posts the scans found in the last day.
    pub new_last_day: i64,
}

/// The latest scan, as the dashboard says it.
#[derive(Debug, Clone, PartialEq)]
pub struct LatestScan {
    pub started_at: i64,
    /// "found 12, 3 new.", or what went wrong.
    pub text: String,
    pub failed: bool,
}

/// Seconds in a day, which the dashboard counts new posts over.
const DAY_SECONDS: i64 = 86_400;

pub async fn status(context: &Context) -> Result<ScanStatus, ServiceError> {
    let db = context.db();
    let now = chrono::Utc::now().timestamp();
    let running_since = now - RUN_GIVE_UP_SECONDS;
    let problem = match scan_problem(context).await? {
        Some(problem) => Some(problem),
        None => model::first_pause(db).await?,
    };
    let last_scan = model::last_run(db, None).await?.map(|run| {
        let failed =
            super::job_search_summary::run_state(&run, running_since) == super::job_search_summary::RunState::Error;
        LatestScan {
            started_at: run.started_at,
            text: super::job_search_summary::run_text(&run, running_since),
            failed,
        }
    });
    Ok(ScanStatus {
        active_searches: model::active_count(db).await?,
        problem,
        running: model::is_running(db, running_since).await?,
        last_scan,
        new_last_day: model::new_posts_since(db, now - DAY_SECONDS).await?,
    })
}

/// Whether a scan is running now (here, or in the scheduled `jobfinder scan`).
pub async fn is_running(context: &Context) -> Result<bool, ServiceError> {
    let now = chrono::Utc::now().timestamp();
    Ok(model::is_running(context.db(), now - RUN_GIVE_UP_SECONDS).await?)
}

/// Scans every active search that isn't paused and wasn't scanned in the last
/// MIN_SECONDS_BETWEEN_SCANS, and matches the new posts it finds. The one scan behind
/// `jobfinder scan`, Scan now and a desktop window.
pub async fn scan(context: Arc<Context>) -> Result<ScanReport, ServiceError> {
    let db = context.db();
    model::resume_after_new_key(db).await?;
    if let Some(problem) = scan_problem(&context).await? {
        return Err(ServiceError::need_key(problem));
    }
    let keyed = llm_key::llm(&context, JOB_EVALUATION).await?;
    // Read when the first post needs it: a scan that finds nothing to match costs no call for it.
    let mut cv: Option<ProfileCv> = None;
    let mut report = ScanReport::default();
    // What an earlier scan found but didn't get to match, before it stopped.
    let stopped_before = chrono::Utc::now().timestamp() - RUN_GIVE_UP_SECONDS;
    for post_id in model::unfinished_posts(db, stopped_before).await? {
        match match_post_once_read(&context, post_id, &mut cv).await {
            Ok(true) => report.matched += 1,
            Ok(false) => {}
            Err(e) => report.problems.push(e.message),
        }
    }
    for search_id in model::scannable(db).await? {
        let now = chrono::Utc::now().timestamp();
        let Some(run_id) = model::start_run(db, search_id, now - MIN_SECONDS_BETWEEN_SCANS, now).await? else {
            report.skipped += 1;
            continue;
        };
        report.scanned += 1;
        let Some(search) = model::find(db, search_id).await? else {
            continue;
        };
        let found = match search_posts(&context, &keyed.llm, &search, run_id).await {
            Ok(found) => found,
            Err(why) => {
                report.problems.push(format!("{}: {why}", search.name));
                continue;
            }
        };
        report.posts_found += found.posts_found;
        report.posts_new += found.new_posts.len() as i64;
        for post_id in found.new_posts {
            match match_post_once_read(&context, post_id, &mut cv).await {
                Ok(true) => report.matched += 1,
                Ok(false) => {}
                Err(e) => report.problems.push(e.message),
            }
        }
    }
    Ok(report)
}

/// The profile, as the CV posts are matched with: its text, and read as the match reads a CV.
pub struct ProfileCv {
    pub text: String,
    pub profile: Profile,
}

/// Matches a post with the profile, read on the first post of a scan and kept for the rest.
async fn match_post_once_read(
    context: &Arc<Context>,
    post_id: i64,
    cv: &mut Option<ProfileCv>,
) -> Result<bool, ServiceError> {
    let cv = match cv {
        Some(cv) => cv,
        None => cv.insert(profile_cv(context).await?),
    };
    match_post_with(context, post_id, cv).await
}

/// The profile read once for a whole scan, rather than once per post.
async fn profile_cv(context: &Context) -> Result<ProfileCv, ServiceError> {
    let Some(text) = profile::text_for_user(context.db(), context.user_id).await? else {
        return Err(ServiceError::need_key(
            "Every post is matched with your profile, and there is none yet. Read your CV in setup first.",
        ));
    };
    let profile = match_service::extract_cv(context, &text).await?;
    Ok(ProfileCv { text, profile })
}

/// The posts one scan of a search found.
struct SearchFound {
    posts_found: i64,
    /// The ones the search didn't have, added to it.
    new_posts: Vec<i64>,
}

/// Asks the model, searching the web, for new posts that fit search, and adds the ones the
/// search doesn't have. The run keeps what it found and cost, or why it failed; a failure the
/// model says won't pass by itself (a rejected key) pauses the search.
async fn search_posts(
    context: &Context,
    llm: &crate::llm::Llm,
    search: &model::JobSearch,
    run_id: i64,
) -> Result<SearchFound, String> {
    let db = context.db();
    let recent = model::recent_posts(db, search.id, RECENT_POSTS_SKIPPED)
        .await
        .map_err(|e| e.to_string())?;
    let (system, user) = scan_prompt(search, &recent);
    let sites: Vec<&str> = search.sites.iter().map(String::as_str).collect();
    let answer = llm
        .web_search(&system, &user, &posts_schema(), &sites, SEARCH_COUNTRY, Effort::Low)
        .await;
    let now = chrono::Utc::now().timestamp();
    let answer = match answer {
        Ok(answer) => answer,
        Err(e) => {
            crate::log_line!("job_scan: scanning search {} failed: {}", search.id, e.details());
            let error = format!("The scan failed: {}", e.message);
            let end = RunEnd {
                error: error.clone(),
                ..RunEnd::default()
            };
            model::finish_run(db, run_id, &end, now)
                .await
                .map_err(|e| e.to_string())?;
            if !e.retryable {
                let reason = format!("{error} Fix it on the API key page, or save the search again.");
                model::pause(db, search.id, &reason, now)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            return Err(error);
        }
    };
    let posts = found_posts(&answer.answer);
    let mut new_posts = Vec::new();
    for post in &posts {
        let added = model::add_post(db, search.id, Some(run_id), post, now)
            .await
            .map_err(|e| e.to_string())?;
        new_posts.extend(added);
    }
    let end = RunEnd {
        posts_found: posts.len() as i64,
        posts_new: new_posts.len() as i64,
        input_tokens: answer.input_tokens,
        output_tokens: answer.output_tokens,
        error: String::new(),
    };
    model::finish_run(db, run_id, &end, now)
        .await
        .map_err(|e| e.to_string())?;
    Ok(SearchFound {
        posts_found: end.posts_found,
        new_posts,
    })
}

/// The posts of the model's answer with an http(s) link, at most POSTS_PER_SCAN.
fn found_posts(answer: &Value) -> Vec<NewPost<'_>> {
    answer["posts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .take(POSTS_PER_SCAN)
        .map(|post| NewPost {
            url: text_of(post, "url").trim(),
            title: text_of(post, "title"),
            company: text_of(post, "company"),
            location: text_of(post, "location"),
        })
        .filter(|post| job_search_rules::is_http_link(post.url))
        .collect()
}

/// A post's field as text, "" when it isn't.
fn text_of<'a>(post: &'a Value, key: &str) -> &'a str {
    post[key].as_str().unwrap_or("")
}

/// The answer the scan asks for: the posts, each with its link, title, company and location.
fn posts_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["posts"],
        "properties": {
            "posts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["url", "title", "company", "location"],
                    "properties": {
                        "url": { "type": "string" },
                        "title": { "type": "string" },
                        "company": { "type": "string" },
                        "location": { "type": "string" },
                    },
                },
            },
        },
    })
}

/// The scan's instructions and question for search: what to look for, where, and the posts it
/// found lately (to skip).
fn scan_prompt(search: &model::JobSearch, recent: &[RecentPost]) -> (String, String) {
    let system = format!(
        "You find job posts on the web for a job seeker in Denmark. Search the job sites (Jobnet, Jobindex, LinkedIn Jobs, The Hub, Graduateland, Indeed and the like) and company career pages for open positions that fit what they are looking for, and list them.

- Only open positions that were published or updated in the last 14 days and still take applications. No expired posts, no articles, no search result pages, no company pages without a specific job.
- url is the address of the job post itself, exactly as you found it. Never make one up or guess one: leave out a post whose address you don't have.
- title, company and location as the post gives them; location is the town or \"Remote\".
- At most {POSTS_PER_SCAN} posts, the best fits first. Skip the posts listed as found already, also when you find them under another address.
- Answer with an empty list when you find nothing new that fits."
    );
    let location = search.location.trim();
    let place = if location.is_empty() {
        "anywhere in Denmark"
    } else {
        location
    };
    let remote = if search.remote_ok {
        "\nRemote positions (from Denmark) fit too."
    } else {
        ""
    };
    let sites = if search.sites.is_empty() {
        String::new()
    } else {
        format!("\nSearch only these sites: {}.", search.sites.join(", "))
    };
    let skip = if recent.is_empty() {
        "- (none yet)".to_string()
    } else {
        recent.iter().map(skip_line).collect::<Vec<_>>().join("\n")
    };
    let user = format!(
        "Looking for: {}\nWhere: {place}{remote}{sites}\n\nFound already:\n{skip}",
        search.keywords
    );
    (system, user)
}

/// "- PHP developer Acme (https://…)": a post the scan found already.
fn skip_line(post: &RecentPost) -> String {
    let named = format!("{} {}", post.title, post.company);
    format!("- {} ({})", named.trim(), post.url)
}

/// Matches one post on its own (one added by its link, or tried again), with the profile as it
/// is now. Ok(true) when it is matched and saved.
pub async fn match_post(context: Arc<Context>, post_id: i64) -> Result<bool, ServiceError> {
    let cv = profile_cv(&context).await?;
    match_post_with(&context, post_id, &cv).await
}

/// Matches a found post with the profile and saves the match; from the search's
/// tailor_from_score up, writes the tailored résumé and the application too (on the keys picked
/// for them). The post's status says how it went (matched, not_a_job, failed with why). Ok(true)
/// when it is matched.
async fn match_post_with(context: &Arc<Context>, post_id: i64, cv: &ProfileCv) -> Result<bool, ServiceError> {
    let db = context.db();
    let Some(post) = model::for_matching(db, post_id).await? else {
        return Ok(false);
    };
    if !claim(context, post_id).await? {
        return Ok(false);
    }
    let set = |status: &'static str, error: String, match_id: Option<i64>, found: FoundAbout| async move {
        let now = chrono::Utc::now().timestamp();
        model::set_status(db, post_id, status, &error, match_id, &found, now).await
    };
    let page = match page_reader::read(&post.url).await {
        Ok(page) => page,
        Err(why) => {
            crate::log_line!("job_scan: post {post_id}: {}: {why}", post.url);
            // A link a scan found that doesn't open is most likely gone, or one the model got
            // wrong: not a job post to try again.
            let (status, said) = match post.job_search_run_id {
                Some(_) => (NOT_A_JOB, "The link the scan found doesn't open: "),
                None => (FAILED, "The post couldn't be fetched: "),
            };
            set(status, format!("{said}{why}"), None, FoundAbout::default()).await?;
            return Ok(false);
        }
    };
    let job_text = page_text(&page);
    let matched = match matched_post(context, &page, &job_text, cv).await {
        Ok(matched) => matched,
        Err(e) => {
            set(
                FAILED,
                format!("The match failed: {}", e.message),
                None,
                FoundAbout::default(),
            )
            .await?;
            return Ok(false);
        }
    };
    let (match_id, found) = match matched {
        Matched::NotAJob(why) => {
            set(NOT_A_JOB, why.to_string(), None, FoundAbout::default()).await?;
            return Ok(false);
        }
        Matched::Saved(match_id, found) => (match_id, found),
    };
    let Some(saved) = match_model::find(db, match_id, context.user_id).await? else {
        set(FAILED, "The match wasn't saved.".to_string(), None, found).await?;
        return Ok(false);
    };
    // The same job found again for a match the user rejected stays out of sight.
    if saved.rejected_at.is_some() {
        set(DISMISSED, String::new(), Some(match_id), found).await?;
        return Ok(false);
    }
    if dismissed_meanwhile(context, post_id, match_id).await? {
        set(DISMISSED, String::new(), Some(match_id), found).await?;
        return Ok(false);
    }
    let mut errors = Vec::new();
    if tailors(saved.score, post.tailor_from_score) {
        errors.extend(write_document(context, &saved, RESUME).await);
        errors.extend(write_document(context, &saved, APPLICATION).await);
    }
    set(MATCHED, errors.join(" "), Some(match_id), found).await?;
    Ok(!dismissed_meanwhile(context, post_id, match_id).await?)
}

/// Claims a post for a match starting now (job_scan_model::claim_for_matching); false when it
/// isn't to be matched now (one is running on it, or it is matched or dismissed).
async fn claim(context: &Context, post_id: i64) -> Result<bool, ServiceError> {
    let now = chrono::Utc::now().timestamp();
    Ok(model::claim_for_matching(context.db(), post_id, now - RUN_GIVE_UP_SECONDS, now).await?)
}

/// Whether the user dismissed the post while it was being matched; its new match is rejected
/// then, as a dismissal rejects a post's match (search_service::dismiss).
async fn dismissed_meanwhile(context: &Context, post_id: i64, match_id: i64) -> Result<bool, ServiceError> {
    let db = context.db();
    let dismissed = model::post_status(db, post_id)
        .await?
        .is_some_and(|(_, status)| status == DISMISSED);
    if dismissed {
        let now = chrono::Utc::now().timestamp();
        match_model::reject(db, match_id, context.user_id, &[], now).await?;
    }
    Ok(dismissed)
}

/// What matching a fetched post came to.
enum Matched {
    /// The page isn't a job post the match could read, and why.
    NotAJob(&'static str),
    /// Saved as this match (or matched before as it), with the title and company it read.
    Saved(i64, FoundAbout),
}

/// The page read, matched with the profile and saved, unless the same post text was matched
/// with the same profile before.
async fn matched_post(
    context: &Context,
    page: &page_reader::Page,
    job_text: &str,
    cv: &ProfileCv,
) -> Result<Matched, ServiceError> {
    if let Some(existing) = match_service::already_matched(context, &cv.text, job_text).await? {
        return Ok(Matched::Saved(existing, FoundAbout::default()));
    }
    let job = match_service::extract_job(context, job_text).await?;
    if job.job_title.trim().is_empty() && job.requirements.is_empty() {
        return Ok(Matched::NotAJob(
            "This page has no job post the match could read. It may need JavaScript, or the post is gone.",
        ));
    }
    let found = FoundAbout {
        title: if job.job_title.is_empty() {
            page.title.clone()
        } else {
            job.job_title.clone()
        },
        company: job.company.clone(),
    };
    let request = MatchRequest {
        job,
        profile: cv.profile.clone(),
        job_text: job_text.to_string(),
        job_url: page.url.clone(),
        cv_name: PROFILE_CV_NAME.to_string(),
        cv_text: cv.text.clone(),
    };
    let scored = match_service::ai_match(context, &request).await?;
    match scored.saved_id {
        Some(id) => Ok(Matched::Saved(id, found)),
        None => Err(ServiceError::failed(
            "job_scan/match",
            scored
                .save_error
                .unwrap_or_else(|| "The match wasn't saved.".to_string()),
        )),
    }
}

/// The post's text as the match reads it: its title, then its text.
fn page_text(page: &page_reader::Page) -> String {
    if page.title.is_empty() {
        page.text.trim().to_string()
    } else {
        format!("{}\n\n{}", page.title, page.text).trim().to_string()
    }
}

/// Whether a match's 0..1 score reaches the search's tailor_from_score (a percent).
fn tailors(score: f64, tailor_from_score: i64) -> bool {
    crate::matches::match_score::percent(score) >= tailor_from_score
}

/// Writes kind (RESUME or APPLICATION) of a saved match, unless it is written already or no key
/// is picked for it (it can be written from the match then). Some(why) when it failed.
async fn write_document(context: &Arc<Context>, saved: &match_model::SavedMatch, kind: &'static str) -> Option<String> {
    let written = if kind == RESUME {
        saved.resume_written_at
    } else {
        saved.application_written_at
    };
    if written.is_some() {
        return None;
    }
    match llm_key::llm_settings(context, kind).await {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(e) => return Some(e.message),
    }
    let e = match_service::write(Arc::clone(context), saved.id, "", kind)
        .await
        .err()?;
    Some(if kind == RESUME {
        format!("The tailored résumé failed: {} Tailor it from the match.", e.message)
    } else {
        format!("The application failed: {} Write it from the match.", e.message)
    })
}

/// Starts a scan in the background (scan()), for Scan now; Ok is what to say. A scan that can't
/// run (no OpenAI key, no profile) or one running already is said now rather than later.
pub async fn scan_now(context: Arc<Context>) -> Result<&'static str, ServiceError> {
    if let Some(problem) = scan_problem(&context).await? {
        return Err(ServiceError::need_key(problem));
    }
    if is_running(&context).await? {
        return Err(ServiceError::new(
            ErrorKind::TooMany,
            "A scan is running already. Its posts show up here as it finds them.",
        ));
    }
    tokio::spawn(async move {
        match scan(context).await {
            Ok(report) => crate::say!("job_scan: {}", report.summary()),
            Err(e) => crate::log_line!("job_scan: the scan failed: {}", e.message),
        }
    });
    Ok("The scan has started. It takes a few minutes; reload to see what it finds.")
}

/// Adds a job post to a search by its link, and matches it in the background like a post the
/// scan found. Ok is what to say.
pub async fn add_post(context: Arc<Context>, search_id: i64, url: &str) -> Result<&'static str, ServiceError> {
    let url = url.trim();
    if !job_search_rules::is_http_link(url) {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Paste the link of a job post, starting with https://.",
        ));
    }
    let db = context.db();
    if model::find(db, search_id).await?.is_none() {
        return Err(ServiceError::new(ErrorKind::NotFound, "That job search was not found."));
    }
    let now = chrono::Utc::now().timestamp();
    let post = NewPost {
        url,
        ..NewPost::default()
    };
    let Some(post_id) = model::add_post(db, search_id, None, &post, now).await? else {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "This search has that post already.",
        ));
    };
    match_in_background(context, post_id);
    Ok("The post is added. It is matched in a minute; reload to see it.")
}

/// Matches a post that failed (or wasn't a job post) again, in the background. Ok is the post's
/// search, and what to say.
pub async fn retry(context: Arc<Context>, post_id: i64) -> Result<(i64, &'static str), ServiceError> {
    let Some((search_id, status)) = model::post_status(context.db(), post_id).await? else {
        return Err(ServiceError::new(ErrorKind::NotFound, "That post was not found."));
    };
    if status == MATCHING {
        let now = chrono::Utc::now().timestamp();
        let stale = model::unfinished_posts(context.db(), now - RUN_GIVE_UP_SECONDS).await?;
        if !stale.contains(&post_id) {
            return Ok((
                search_id,
                "That post is being matched now; reload in a minute to see it.",
            ));
        }
    } else if ![FAILED, NOT_A_JOB, model::NEW].contains(&status.as_str()) {
        return Ok((search_id, "That post is matched already."));
    }
    match_in_background(context, post_id);
    Ok((search_id, "The post is matched again in a minute; reload to see it."))
}

/// Matches a post as its own task, so it finishes when the page that asked is gone; how it went
/// is the post's status, and the log.
fn match_in_background(context: Arc<Context>, post_id: i64) {
    tokio::spawn(async move {
        if let Err(e) = match_post(context, post_id).await {
            crate::log_line!("job_scan: matching post {post_id} failed: {}", e.message);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search() -> model::JobSearch {
        model::JobSearch {
            id: 1,
            name: "Backend".to_string(),
            keywords: "PHP udvikler".to_string(),
            location: "Odense".to_string(),
            remote_ok: true,
            tailor_from_score: 60,
            active: true,
            sites: vec!["jobnet.dk".to_string()],
            ..model::JobSearch::default()
        }
    }

    #[test]
    fn the_prompt_says_what_where_and_what_to_skip() {
        let recent = [RecentPost {
            url: "https://jobnet.dk/1".to_string(),
            title: "PHP developer".to_string(),
            company: "Acme".to_string(),
        }];
        let (system, user) = scan_prompt(&search(), &recent);
        assert!(system.contains("At most 15 posts"), "{system}");
        assert_eq!(
            user,
            "Looking for: PHP udvikler\nWhere: Odense\nRemote positions (from Denmark) fit too.\nSearch only these sites: jobnet.dk.\n\nFound already:\n- PHP developer Acme (https://jobnet.dk/1)"
        );
        let anywhere = model::JobSearch {
            location: String::new(),
            remote_ok: false,
            sites: Vec::new(),
            ..search()
        };
        let (_, user) = scan_prompt(&anywhere, &[]);
        assert_eq!(
            user,
            "Looking for: PHP udvikler\nWhere: anywhere in Denmark\n\nFound already:\n- (none yet)"
        );
    }

    #[test]
    fn only_linked_posts_are_kept_and_at_most_a_scans_worth() {
        let mut posts: Vec<Value> = (0..20)
            .map(|n| json!({ "url": format!("https://jobnet.dk/{n}"), "title": "T", "company": "C", "location": "L" }))
            .collect();
        posts.insert(
            0,
            json!({ "url": "not a link", "title": "T", "company": "C", "location": "L" }),
        );
        let answer = json!({ "posts": posts });
        let found = found_posts(&answer);
        assert_eq!(found.len(), POSTS_PER_SCAN - 1);
        assert_eq!(found[0].url, "https://jobnet.dk/0");
        assert!(found_posts(&json!({})).is_empty());
    }

    #[test]
    fn a_match_tailors_from_the_searchs_score() {
        assert!(tailors(0.6, 60));
        assert!(tailors(0.596, 60));
        assert!(!tailors(0.59, 60));
        assert!(tailors(0.0, 0));
    }

    #[test]
    fn the_report_says_what_the_scan_did() {
        let report = ScanReport {
            scanned: 2,
            posts_found: 14,
            posts_new: 5,
            matched: 4,
            problems: vec!["Backend: The scan failed: no.".to_string()],
            ..ScanReport::default()
        };
        assert_eq!(
            report.summary(),
            "Scanned 2 searches: 14 posts found, 5 new, 4 matched. Backend: The scan failed: no."
        );
        let skipped = ScanReport {
            skipped: 1,
            ..ScanReport::default()
        };
        assert!(
            skipped
                .summary()
                .starts_with("Every search was scanned less than an hour ago.")
        );
        assert_eq!(
            ScanReport::default().summary(),
            "There is no active job search to scan."
        );
    }
}
