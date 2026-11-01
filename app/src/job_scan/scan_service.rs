//! The job scan's work, for whichever front end asks: `jobfinder scan` (which the schedule runs),
//! the dashboard's and a search's Scan now, and a desktop window later all call scan(), so a scan
//! is the same whoever starts it. Ported from recruiter's Job_scan (_scan and _auto_match),
//! without its queue: a scan finds each search's new posts and matches them one by one before it
//! ends.
//!
//! A scan reads the job sites the app knows (job_sites: Jobnet and Jobindex) itself for each
//! active search, and asks the model, searching the web (OpenAI's Responses API), for the rest
//! when the key picked for evaluating job posts is OpenAI's. So a scan needs no OpenAI key: on
//! any other model (the one on this computer too) it reads the sites it knows only. Each new
//! post is fetched, read and matched with the profile like a post on the match page
//! (match_service), saved as a match, and from the search's tailor_from_score up its tailored
//! résumé and application are written right away, so both are ready in the match's Apply
//! dialog.

use std::sync::Arc;

use serde_json::{Value, json};

use super::job_scan_model::{
    self as model, DISMISSED, FAILED, FoundAbout, MATCHED, MATCHING, NOT_A_JOB, NewPost, RecentPost, RunEnd,
};
use super::job_search_rules;
use super::run_steps::{self, NewStep};
use super::scan_lock;
use super::search_service;
use crate::account::account_model::{APPLICATION, JOB_EVALUATION, RESUME};
use crate::context::Context;
use crate::job_sites::{JobSite, ListedPost};
use crate::llm::transcript::{self, Exchange};
use crate::llm::{Effort, OPENAI};
use crate::matches::match_service::{self, MatchRequest};
use crate::matches::page_reader::ReadWith;
use crate::matches::{Profile, llm_key, match_model, page_reader};
use crate::profile;
use crate::service_error::{ErrorKind, ServiceError};

/// Search terms of a search read on each signed-in site (LinkedIn, Indeed) in one scan, at
/// most: each is a page in JobFinder's signed-in browser, and those sites get few.
const SIGNED_IN_TERMS: usize = 2;

/// New posts one scan of a search adds (and matches), at most; the rest wait for the next.
pub const POSTS_PER_SCAN: usize = 15;

/// The found posts of a search the scan is told about, to skip them.
const RECENT_POSTS_SKIPPED: i64 = 40;

/// A search is scanned at most once in this many seconds, whoever asks (an hour), so a click
/// after the schedule's scan, or two clicks, can't run up the bill.
pub const MIN_SECONDS_BETWEEN_SCANS: i64 = 3600;

/// A post claimed for matching this many seconds ago and not matched was left by a match that
/// stopped without saying so (the program was closed): another may take it (half an hour).
pub const RUN_GIVE_UP_SECONDS: i64 = 1800;

/// What a scan cut off before it finished says (the program was closed while it ran).
const CUT_OFF: &str = "The scan stopped before it finished: the app was closed while it ran.";

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

/// Why nothing can be scanned now, or None: every post found is matched with the profile, on
/// the model picked for evaluating job posts (any: the one on this computer too).
pub async fn scan_problem(context: &Context) -> Result<Option<String>, ServiceError> {
    if llm_key::llm_settings(context, JOB_EVALUATION).await?.is_none() {
        return Ok(Some(
            "The job scan matches every post it finds on the model picked for evaluating job posts, and none is. Pick one on the API key page."
                .to_string(),
        ));
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
    let running = is_running(context)?;
    let problem = match scan_problem(context).await? {
        Some(problem) => Some(problem),
        None => model::first_pause(db).await?,
    };
    let last_scan = model::last_run(db, None).await?.map(|run| {
        let failed = super::job_search_summary::run_state(&run, running) == super::job_search_summary::RunState::Error;
        LatestScan {
            started_at: run.started_at,
            text: super::job_search_summary::run_text(&run, running),
            failed,
        }
    });
    Ok(ScanStatus {
        active_searches: model::active_count(db).await?,
        problem,
        running,
        last_scan,
        new_last_day: model::new_posts_since(db, now - DAY_SECONDS).await?,
    })
}

/// Whether a scan is running now (here, or in the scheduled `jobfinder scan`): one holds
/// scan_lock.
pub fn is_running(context: &Context) -> Result<bool, ServiceError> {
    scan_lock::is_held(context).map_err(|e| lock_failed(&e))
}

/// The scan running now, as Scan now says it: its search and when it started it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningScan {
    pub search_id: i64,
    pub search_name: String,
    pub started_at: i64,
}

/// The scan running now, if one is: the latest scan's search (a scan scans its searches one
/// after the other).
pub async fn running_scan(context: &Context) -> Result<Option<RunningScan>, ServiceError> {
    if !is_running(context)? {
        return Ok(None);
    }
    let db = context.db();
    let Some(run) = model::last_run(db, None).await? else {
        return Ok(None);
    };
    let search_name = model::find(db, run.job_search_id)
        .await?
        .map(|search| search.name)
        .unwrap_or_default();
    Ok(Some(RunningScan {
        search_id: run.job_search_id,
        search_name,
        started_at: run.started_at,
    }))
}

/// "A scan is running already: Backend, since 19:02. …", for Scan now.
fn running_text(running: Option<&RunningScan>) -> String {
    match running {
        Some(scan) if !scan.search_name.is_empty() => format!(
            "A scan is running already: {}, since {}. Its steps show on that search's screen as it goes.",
            scan.search_name,
            crate::matches::dates::clock(scan.started_at)
        ),
        _ => "A scan is running already. Its steps show on each search's screen as it goes.".to_string(),
    }
}

/// The lock couldn't be looked at: its file can't be opened.
fn lock_failed(e: &std::io::Error) -> ServiceError {
    crate::log_line!("job_scan: the scan's lock file can't be used: {e}");
    ServiceError::new(
        ErrorKind::Internal,
        format!("The job scan's lock file beside the database can't be used: {e}"),
    )
}

/// Scans every active search that isn't paused and wasn't scanned in the last
/// MIN_SECONDS_BETWEEN_SCANS, and matches the new posts it finds. The one scan behind
/// `jobfinder scan`, Scan now and a desktop window.
pub async fn scan(context: Arc<Context>) -> Result<ScanReport, ServiceError> {
    let db = context.db();
    let Some(_held) = scan_lock::hold(&context).await.map_err(|e| lock_failed(&e))? else {
        let running = running_scan(&context).await?;
        return Err(ServiceError::new(ErrorKind::TooMany, running_text(running.as_ref())));
    };
    // Holding the lock, no other scan runs: one not finished was cut off.
    let stopped = model::stop_unfinished_runs(db, CUT_OFF, chrono::Utc::now().timestamp()).await?;
    if stopped > 0 {
        crate::log_line!("job_scan: {stopped} earlier scan(s) were cut off before they finished.");
    }
    model::resume_after_new_key(db).await?;
    if let Some(problem) = scan_problem(&context).await? {
        return Err(ServiceError::need_key(problem));
    }
    // The model only searches when it searches the web; every post is matched on it too
    // (match_service), which asks for it there.
    let web = if searches_the_web(&context).await? {
        Some(llm_key::llm(&context, JOB_EVALUATION).await?.llm)
    } else {
        None
    };
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
        let found = match search_posts(&context, web.as_ref(), &search, run_id).await {
            Ok(found) => found,
            Err(why) => {
                report.problems.push(format!("{}: {why}", search.name));
                continue;
            }
        };
        report.posts_found += found.posts_found;
        report.posts_new += found.new_posts.len() as i64;
        if let Some(problem) = found.problem {
            report.problems.push(format!("{}: {problem}", search.name));
        }
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
    match_post_with(context, post_id, cv, None).await
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
    /// What went wrong, though something was found.
    problem: Option<String>,
}

/// Keeps a run's steps (run_steps), for the search's page. A step that can't be kept is logged,
/// and the scan goes on.
#[derive(Clone, Copy)]
struct Timeline<'a> {
    db: &'a sqlx::SqlitePool,
    /// None for a post no scan found (added by its link): its steps aren't kept.
    run_id: Option<i64>,
    post_id: Option<i64>,
}

/// A step starting now.
struct StepStart {
    kind: &'static str,
    started_at_ms: i64,
}

impl Timeline<'_> {
    fn start(&self, kind: &'static str) -> StepStart {
        StepStart {
            kind,
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Keeps a step that started at started, as text, with the model's calls in it.
    async fn keep(&self, started: StepStart, text: &str, failed: bool, calls: &[Exchange]) {
        let Some(run_id) = self.run_id else {
            return;
        };
        let step = NewStep {
            run_id,
            post_id: self.post_id,
            kind: started.kind,
            text,
            failed,
            started_at_ms: started.started_at_ms,
            took_ms: chrono::Utc::now().timestamp_millis() - started.started_at_ms,
            calls,
        };
        if let Err(e) = run_steps::add(self.db, &step).await {
            crate::log_line!("job_scan: keeping a step of run {run_id} failed: {e}");
        }
    }
}

/// Where a scan looks for a search's posts.
#[derive(Debug, Clone, PartialEq)]
struct Sources {
    /// The sites the app reads itself.
    read: Vec<JobSite>,
    /// Whether the model searches the rest of the web too.
    web: bool,
}

/// Where search is looked for: every site the app reads, but the ones the search skips, and the
/// rest of the web when web_search can (the key picked is OpenAI's).
fn sources(search: &model::JobSearch, web_search: bool) -> Sources {
    let read = JobSite::ALL
        .into_iter()
        .chain(JobSite::SIGNED_IN)
        .filter(|site| {
            !search
                .skipped_sites
                .iter()
                .any(|domain| JobSite::of(domain) == Some(*site))
        })
        .collect();
    Sources { read, web: web_search }
}

/// Whether url is on one of the sites skipped (a domain, or a part of one).
fn on_skipped_site(url: &str, skipped: &[String]) -> bool {
    let Some(host) = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_lowercase))
    else {
        return false;
    };
    let host = host.strip_prefix("www.").unwrap_or(&host);
    skipped
        .iter()
        .any(|domain| host == domain || host.ends_with(&format!(".{domain}")))
}

/// Whether the key picked for evaluating job posts searches the web (OpenAI's does).
pub async fn searches_the_web(context: &Context) -> Result<bool, ServiceError> {
    Ok(llm_key::llm_settings(context, JOB_EVALUATION)
        .await?
        .is_some_and(|key| key.provider == OPENAI))
}

/// The posts sources list for search, and what went wrong on the way.
#[derive(Debug, Default)]
struct Listed {
    posts: Vec<ListedPost>,
    problems: Vec<String>,
    input_tokens: i64,
    output_tokens: i64,
}

/// Looks for new posts that fit search on its sources, and adds the ones the search doesn't
/// have, at most POSTS_PER_SCAN. The run keeps what it found and cost, and what went wrong; it
/// failed when nothing could be searched.
async fn search_posts(
    context: &Context,
    web: Option<&crate::llm::Llm>,
    search: &model::JobSearch,
    run_id: i64,
) -> Result<SearchFound, String> {
    let db = context.db();
    let timeline = Timeline {
        db,
        run_id: Some(run_id),
        post_id: None,
    };
    if let Err(e) = run_steps::forget_old(db, search.id).await {
        crate::log_line!("job_scan: forgetting search {}'s old steps failed: {e}", search.id);
    }
    let sources = sources(search, web.is_some());
    let mut listed = read_sites(context, &sources.read, search, timeline).await;
    if let (Some(llm), true) = (web, sources.web) {
        match web_posts(context, llm, search, &[], timeline).await {
            Ok(found) => {
                listed.posts.extend(found.posts);
                listed.input_tokens = found.input_tokens;
                listed.output_tokens = found.output_tokens;
            }
            Err(why) => listed.problems.push(why),
        }
    }
    let now = chrono::Utc::now().timestamp();
    let mut new_posts = Vec::new();
    listed
        .posts
        .retain(|post| !on_skipped_site(&post.url, &search.skipped_sites));
    for post in &listed.posts {
        if new_posts.len() >= POSTS_PER_SCAN {
            break;
        }
        let post = NewPost {
            url: &post.url,
            title: &post.title,
            company: &post.company,
            location: &post.location,
        };
        let added = model::add_post(db, search.id, Some(run_id), &post, now)
            .await
            .map_err(|e| e.to_string())?;
        new_posts.extend(added);
    }
    let posts_found = listed.posts.len() as i64;
    let started = timeline.start(run_steps::POSTS);
    timeline
        .keep(started, &posts_text(posts_found, new_posts.len()), false, &[])
        .await;
    let error = run_error(&listed.problems, posts_found, new_posts.len());
    let end = RunEnd {
        posts_found,
        posts_new: new_posts.len() as i64,
        input_tokens: listed.input_tokens,
        output_tokens: listed.output_tokens,
        error: error.clone(),
    };
    model::finish_run(db, run_id, &end, now)
        .await
        .map_err(|e| e.to_string())?;
    if posts_found == 0 && !listed.problems.is_empty() {
        return Err(error);
    }
    if !error.is_empty() {
        crate::log_line!("job_scan: search {}: {error}", search.id);
    }
    Ok(SearchFound {
        posts_found,
        new_posts,
        problem: (!error.is_empty()).then_some(error),
    })
}

/// "26 posts listed, 6 new.", and that the rest wait when a scan's worth is new.
fn posts_text(posts_found: i64, posts_new: usize) -> String {
    let said = format!("{posts_found} posts listed, {posts_new} new.");
    if posts_new >= POSTS_PER_SCAN {
        format!("{said} That is a scan's worth: the rest are added next time.")
    } else {
        said
    }
}

/// The run's error: none when nothing went wrong; what went wrong, after what was found anyway.
fn run_error(problems: &[String], posts_found: i64, posts_new: usize) -> String {
    if problems.is_empty() {
        return String::new();
    }
    let problems = problems.join(" ");
    if posts_found == 0 {
        format!("The scan failed: {problems}")
    } else {
        format!("Found {posts_found}, {posts_new} new, but: {problems}")
    }
}

/// The posts sites list for each of search's terms near its place, newest first, taking turns
/// between the lists so each site and term gets its share of a scan's posts. A signed-in site
/// (LinkedIn, Indeed) is read for SIGNED_IN_TERMS terms at most, and not after a failure: that
/// may be the site asking to sign in again.
async fn read_sites(context: &Context, sites: &[JobSite], search: &model::JobSearch, timeline: Timeline<'_>) -> Listed {
    let place = search_place(search);
    let signed_in = context.signed_in.as_ref();
    let mut lists = Vec::new();
    let mut problems = Vec::new();
    for site in sites {
        let mut failed = None;
        let terms = job_search_rules::search_terms(&search.keywords);
        let most = if site.is_signed_in() {
            SIGNED_IN_TERMS
        } else {
            terms.len()
        };
        for term in terms.into_iter().take(most) {
            let started = timeline.start(run_steps::SITE);
            let read = site.search(&term, place, signed_in).await;
            let said = site_text(site.domain(), &term, place, &read);
            timeline.keep(started, &said, read.is_err(), &[]).await;
            match read {
                Ok(posts) => lists.push(posts),
                Err(why) => failed = Some(why),
            }
            if failed.is_some() && site.is_signed_in() {
                break;
            }
        }
        if let Some(why) = failed {
            crate::log_line!(
                "job_scan: search {}: reading {} failed: {why}",
                search.id,
                site.domain()
            );
            problems.push(format!("Reading {} failed: {why}", site.domain()));
        }
    }
    Listed {
        posts: taking_turns(lists),
        problems,
        ..Listed::default()
    }
}

/// "jobnet.dk, “PHP udvikler” near Odense: 20 posts." or what went wrong.
fn site_text(domain: &str, term: &str, place: &str, read: &Result<Vec<ListedPost>, String>) -> String {
    let near = if place.is_empty() {
        String::new()
    } else {
        format!(" near {place}")
    };
    match read {
        Ok(posts) => format!("{domain}, “{term}”{near}: {} posts.", posts.len()),
        Err(why) => format!("{domain}, “{term}”{near}: {why}"),
    }
}

/// The place a search's sites are read near: its own, or anywhere for one that says only
/// remote.
fn search_place(search: &model::JobSearch) -> &str {
    let place = search.location.trim();
    if place.eq_ignore_ascii_case("remote") {
        ""
    } else {
        place
    }
}

/// The lists' posts, the first of each, then the second of each, and so on.
fn taking_turns(lists: Vec<Vec<ListedPost>>) -> Vec<ListedPost> {
    let mut iterators: Vec<_> = lists.into_iter().map(Vec::into_iter).collect();
    let mut posts = Vec::new();
    loop {
        let before = posts.len();
        posts.extend(iterators.iter_mut().filter_map(Iterator::next));
        if posts.len() == before {
            return posts;
        }
    }
}

/// Asks the model, searching the web (on domains, [] for any), for new posts that fit search.
async fn web_posts(
    context: &Context,
    llm: &crate::llm::Llm,
    search: &model::JobSearch,
    domains: &[String],
    timeline: Timeline<'_>,
) -> Result<Listed, String> {
    let recent = model::recent_posts(context.db(), search.id, RECENT_POSTS_SKIPPED)
        .await
        .map_err(|e| e.to_string())?;
    let (system, user) = scan_prompt(search, domains, &recent);
    let sites: Vec<&str> = domains.iter().map(String::as_str).collect();
    let started = timeline.start(run_steps::WEB);
    let (answer, calls) =
        transcript::recording(llm.web_search(&system, &user, &posts_schema(), &sites, SEARCH_COUNTRY, Effort::Low))
            .await;
    let said = web_text(domains, answer.as_ref().map(|answer| found_posts(&answer.answer).len()));
    timeline.keep(started, &said, answer.is_err(), &calls).await;
    let answer = answer.map_err(|e| {
        crate::log_line!(
            "job_scan: searching the web for search {} failed: {}",
            search.id,
            e.details()
        );
        format!("Searching the web failed: {}", e.message)
    })?;
    let posts = found_posts(&answer.answer)
        .iter()
        .map(|post| ListedPost {
            url: post.url.to_string(),
            title: post.title.to_string(),
            company: post.company.to_string(),
            location: post.location.to_string(),
        })
        .collect();
    Ok(Listed {
        posts,
        problems: Vec::new(),
        input_tokens: answer.input_tokens,
        output_tokens: answer.output_tokens,
    })
}

/// "Web search on thehub.io: 4 posts." (or on any other site), or that it failed.
fn web_text(domains: &[String], found: Result<usize, &crate::llm::LlmException>) -> String {
    let on = if domains.is_empty() {
        "any other site".to_string()
    } else {
        domains.join(", ")
    };
    match found {
        Ok(found) => format!("Web search on {on}: {found} posts."),
        Err(e) => format!("Web search on {on} failed: {}", e.message),
    }
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

/// The web search's instructions and question for search: what to look for, where (on domains,
/// [] for any site but the ones the app reads itself), and the posts it found lately (to skip).
fn scan_prompt(search: &model::JobSearch, domains: &[String], recent: &[RecentPost]) -> (String, String) {
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
    let sites = if domains.is_empty() {
        let read: Vec<&str> = sources(search, true).read.iter().map(|site| site.domain()).collect();
        let mut sites = format!("\nSkip {}: they are searched separately.", read.join(", "));
        if !search.skipped_sites.is_empty() {
            sites.push_str(&format!("\nLeave out posts on {}.", search.skipped_sites.join(", ")));
        }
        sites
    } else {
        format!("\nSearch only these sites: {}.", domains.join(", "))
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
    match_post_with(&context, post_id, &cv, None).await
}

/// Matches a found post with the profile and saves the match; from the search's
/// tailor_from_score up, writes the tailored résumé and the application too (on the keys picked
/// for them). The post's status says how it went (matched, not_a_job, failed with why). Ok(true)
/// when it is matched. sent is the post as the user's browser showed it, when the extension sent
/// it: it isn't fetched again then.
async fn match_post_with(
    context: &Arc<Context>,
    post_id: i64,
    cv: &ProfileCv,
    sent: Option<page_reader::Page>,
) -> Result<bool, ServiceError> {
    let db = context.db();
    let Some(post) = model::for_matching(db, post_id).await? else {
        return Ok(false);
    };
    if !claim(context, post_id).await? {
        return Ok(false);
    }
    // A post's steps are on the run that found it.
    let timeline = Timeline {
        db,
        run_id: post.job_search_run_id,
        post_id: Some(post_id),
    };
    let set = |status: &'static str, error: String, match_id: Option<i64>, found: FoundAbout| async move {
        let now = chrono::Utc::now().timestamp();
        model::set_status(db, post_id, status, &error, match_id, &found, now).await
    };
    let started = timeline.start(run_steps::READ);
    let read = match sent {
        Some(page) => Ok(page),
        None => page_reader::read(&post.url, context.readers()).await,
    };
    timeline
        .keep(started, &read_text(&post.url, &read), read.is_err(), &[])
        .await;
    let page = match read {
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
    let matched = match matched_post(context, &page, &job_text, cv, timeline).await {
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
        errors.extend(write_document(context, &saved, RESUME, timeline).await);
        errors.extend(write_document(context, &saved, APPLICATION, timeline).await);
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
    timeline: Timeline<'_>,
) -> Result<Matched, ServiceError> {
    if let Some(existing) = match_service::already_matched(context, &cv.text, job_text).await? {
        let started = timeline.start(run_steps::MATCH);
        timeline
            .keep(
                started,
                "Matched before: the same post with the same profile.",
                false,
                &[],
            )
            .await;
        return Ok(Matched::Saved(existing, FoundAbout::default()));
    }
    let started = timeline.start(run_steps::EXTRACT);
    let (job, calls) = transcript::recording(match_service::extract_job(context, job_text)).await;
    let read_as = job
        .as_ref()
        .map(extract_text)
        .map_err(|e| format!("Reading the post failed: {}", e.message));
    let said = read_as.clone().unwrap_or_else(|why| why);
    timeline.keep(started, &said, read_as.is_err(), &calls).await;
    let job = job?;
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
    let started = timeline.start(run_steps::MATCH);
    let (scored, calls) = transcript::recording(match_service::ai_match(context, &request)).await;
    let said = match &scored {
        Ok(scored) => match_text(scored),
        Err(e) => format!("The match failed: {}", e.message),
    };
    timeline.keep(started, &said, scored.is_err(), &calls).await;
    let scored = scored?;
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

/// "Read https://… from Jobnet's API." (or as a page, or in the browser), or why it couldn't be.
fn read_text(url: &str, read: &Result<page_reader::Page, String>) -> String {
    match read {
        Ok(page) => {
            let how = match page.read_with {
                ReadWith::Page => "as a web page",
                ReadWith::SiteApi => "from the site's own API",
                ReadWith::Browser => "in the browser, after its JavaScript ran",
                ReadWith::SignedIn => "in JobFinder's signed-in browser",
                ReadWith::Sent => "as your browser showed it (sent from the extension)",
            };
            format!("Read {url} {how}: {} characters.", page.text.chars().count())
        }
        Err(why) => format!("Couldn't read {url}: {why}"),
    }
}

/// "Read the post: PHP-udvikler at Acme A/S, 9 requirements."
fn extract_text(job: &crate::matches::Job) -> String {
    let title = if job.job_title.trim().is_empty() {
        "no job title"
    } else {
        job.job_title.trim()
    };
    let at = if job.company.trim().is_empty() {
        String::new()
    } else {
        format!(" at {}", job.company.trim())
    };
    format!("Read the post: {title}{at}, {} requirements.", job.requirements.len())
}

/// "Matched with your profile: 72 of 100, Good match."
fn match_text(scored: &crate::matches::ai_match::AiMatch) -> String {
    format!(
        "Matched with your profile: {} of 100, {}.",
        crate::matches::match_score::percent(scored.result.index),
        crate::matches::match_score::tag_name(&scored.result.tag)
    )
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
async fn write_document(
    context: &Arc<Context>,
    saved: &match_model::SavedMatch,
    kind: &'static str,
    timeline: Timeline<'_>,
) -> Option<String> {
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
    let started = timeline.start(run_steps::WRITE);
    let (written, calls) = transcript::recording(match_service::write(Arc::clone(context), saved.id, "", kind)).await;
    let what = if kind == RESUME {
        "The tailored résumé"
    } else {
        "The application"
    };
    let said = match &written {
        Ok(_) => format!("{what} is written."),
        Err(e) => format!("{what} failed: {}", e.message),
    };
    timeline.keep(started, &said, written.is_err(), &calls).await;
    let e = written.err()?;
    Some(if kind == RESUME {
        format!("The tailored résumé failed: {} Tailor it from the match.", e.message)
    } else {
        format!("The application failed: {} Write it from the match.", e.message)
    })
}

/// Starts a scan in the background (scan()), for Scan now; Ok is what to say. A scan that can't
/// run (no model picked, no profile) or one running already is said now rather than later.
pub async fn scan_now(context: Arc<Context>) -> Result<&'static str, ServiceError> {
    if let Some(problem) = scan_problem(&context).await? {
        return Err(ServiceError::need_key(problem));
    }
    if let Some(running) = running_scan(&context).await? {
        return Err(ServiceError::new(ErrorKind::TooMany, running_text(Some(&running))));
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
    let url = crate::job_sites::post_link(url);
    let now = chrono::Utc::now().timestamp();
    let post = NewPost {
        url: &url,
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

/// Most bytes of a page's HTML the extension may send (LinkedIn's pages run to a few MB).
pub const MAX_SENT_HTML: usize = 20_000_000;

/// Adds the job post the user's browser shows (url, and its HTML as shown, sent by the
/// extension) to a search, and matches it in the background like a post the scan found, from
/// the HTML sent rather than fetched again. search_id is the search to add it to; None is the
/// first active one. Ok is what to say.
pub async fn add_page(
    context: Arc<Context>,
    search_id: Option<i64>,
    url: &str,
    html: &str,
) -> Result<&'static str, ServiceError> {
    let url = url.trim();
    if !job_search_rules::is_http_link(url) || url.chars().count() > job_search_rules::MAX_URL {
        return Err(ServiceError::new(
            ErrorKind::Invalid,
            "Only a web page (https://…) can be sent to JobFinder.",
        ));
    }
    if html.len() > MAX_SENT_HTML {
        return Err(ServiceError::new(ErrorKind::TooLarge, "The page is too large to read."));
    }
    let db = context.db();
    let search_id = match search_id {
        Some(id) => model::find(db, id).await?.map(|search| search.id),
        None => search_service::first_active(&context).await?,
    }
    .ok_or_else(|| {
        ServiceError::new(
            ErrorKind::NotFound,
            "JobFinder has no job search to add the post to. Make one in JobFinder first (Settings, Job searches).",
        )
    })?;
    let url = crate::job_sites::post_link(url);
    let page = crate::job_sites::sent_page(&url, html).map_err(|why| ServiceError::new(ErrorKind::Invalid, why))?;
    let now = chrono::Utc::now().timestamp();
    let post = NewPost {
        url: &url,
        title: &page.title,
        ..NewPost::default()
    };
    let Some(post_id) = model::add_post(db, search_id, None, &post, now).await? else {
        return Ok("JobFinder has this post already: it's on the search's page.");
    };
    tokio::spawn(async move {
        let matched = match profile_cv(&context).await {
            Ok(cv) => match_post_with(&context, post_id, &cv, Some(page)).await,
            Err(e) => Err(e),
        };
        if let Err(e) = matched {
            crate::log_line!("job_scan: matching the sent post {post_id} failed: {}", e.message);
        }
    });
    Ok("Sent. JobFinder matches it in a minute or two; it shows on the search's page.")
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
            skipped_sites: vec!["jobindex.dk".to_string()],
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
        let (system, user) = scan_prompt(&search(), &["example.dk".to_string()], &recent);
        assert!(system.contains("At most 15 posts"), "{system}");
        assert_eq!(
            user,
            "Looking for: PHP udvikler\nWhere: Odense\nRemote positions (from Denmark) fit too.\nSearch only these sites: example.dk.\n\nFound already:\n- PHP developer Acme (https://jobnet.dk/1)"
        );
        let anywhere = model::JobSearch {
            location: String::new(),
            remote_ok: false,
            skipped_sites: Vec::new(),
            ..search()
        };
        let (_, user) = scan_prompt(&anywhere, &[], &[]);
        assert_eq!(
            user,
            "Looking for: PHP udvikler\nWhere: anywhere in Denmark\nSkip jobnet.dk, jobindex.dk, linkedin.com, dk.indeed.com: they are searched separately.\n\nFound already:\n- (none yet)"
        );
    }

    #[test]
    fn every_site_is_read_but_the_skipped_ones() {
        let all = model::JobSearch {
            skipped_sites: Vec::new(),
            ..search()
        };
        assert_eq!(
            sources(&all, false),
            Sources {
                read: vec![JobSite::Jobnet, JobSite::Jobindex, JobSite::Linkedin, JobSite::Indeed],
                web: false,
            }
        );
        assert!(sources(&all, true).web);
        let skipping = model::JobSearch {
            skipped_sites: vec!["indeed.com".to_string(), "jobindex.dk".to_string()],
            ..search()
        };
        assert_eq!(sources(&skipping, false).read, vec![JobSite::Jobnet, JobSite::Linkedin]);
    }

    #[test]
    fn posts_on_skipped_sites_are_left_out() {
        let skipped = ["stepstone.dk".to_string()];
        assert!(on_skipped_site("https://www.stepstone.dk/job/1", &skipped));
        assert!(on_skipped_site("https://jobs.stepstone.dk/job/1", &skipped));
        assert!(!on_skipped_site("https://notstepstone.dk/job/1", &skipped));
        assert!(!on_skipped_site("https://www.jobnet.dk/job/1", &skipped));
    }

    #[test]
    fn the_sites_lists_take_turns() {
        let post = |url: &str| ListedPost {
            url: url.to_string(),
            ..ListedPost::default()
        };
        let urls: Vec<String> = taking_turns(vec![vec![post("a1"), post("a2"), post("a3")], vec![], vec![post("b1")]])
            .into_iter()
            .map(|post| post.url)
            .collect();
        assert_eq!(urls, vec!["a1", "b1", "a2", "a3"]);
    }

    #[test]
    fn a_run_says_what_went_wrong_after_what_it_found() {
        assert_eq!(run_error(&[], 3, 1), "");
        let problems = ["Reading jobindex.dk failed: no.".to_string()];
        assert_eq!(
            run_error(&problems, 0, 0),
            "The scan failed: Reading jobindex.dk failed: no."
        );
        assert_eq!(
            run_error(&problems, 12, 3),
            "Found 12, 3 new, but: Reading jobindex.dk failed: no."
        );
    }

    #[test]
    fn a_remote_only_search_reads_the_sites_anywhere() {
        let remote = model::JobSearch {
            location: " Remote ".to_string(),
            ..search()
        };
        assert_eq!(search_place(&remote), "");
        assert_eq!(search_place(&search()), "Odense");
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
