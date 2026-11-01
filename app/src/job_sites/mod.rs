//! The job sites the app reads itself, without a language model searching the web: their own
//! search (Jobnet's JSON API, Jobindex's search page) for the job scan, and Jobnet's API for one
//! of its posts, whose page shows the post with JavaScript. Pages that need JavaScript
//! elsewhere are read in the computer's own browser (browser). LinkedIn and Indeed, which show
//! their posts to a signed-in browser, are read in JobFinder's own signed-in browser (signed_in),
//! and only for a search that names them in its sites.
//!
//! Every request the app makes itself goes through page_reader's fetching rules (public
//! addresses only, every redirect checked).

pub mod browser;
pub mod cdp;
pub mod indeed;
pub mod jobindex;
pub mod jobnet;
pub mod linkedin;
pub mod signed_in;

use crate::matches::page_reader::{self, Page, ReadWith, Readers};
use signed_in::{Seen, SignedInBrowser};

/// A site the app searches itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSite {
    Jobnet,
    Jobindex,
    Linkedin,
    Indeed,
}

impl JobSite {
    /// The sites a search without sites of its own is read on: the ones read without signing in.
    pub const ALL: [JobSite; 2] = [JobSite::Jobnet, JobSite::Jobindex];

    /// The sites read in JobFinder's signed-in browser, for a search that names them.
    pub const SIGNED_IN: [JobSite; 2] = [JobSite::Linkedin, JobSite::Indeed];

    /// The site's domain as the sites box keeps it (job_search_rules::sites).
    pub fn domain(self) -> &'static str {
        match self {
            JobSite::Jobnet => jobnet::DOMAIN,
            JobSite::Jobindex => jobindex::DOMAIN,
            JobSite::Linkedin => linkedin::DOMAIN,
            JobSite::Indeed => indeed::DOMAIN,
        }
    }

    /// Whether the site is read in JobFinder's signed-in browser.
    pub fn is_signed_in(self) -> bool {
        JobSite::SIGNED_IN.contains(&self)
    }

    /// The site the app reads for a domain of the sites box, if it reads it.
    pub fn of(domain: &str) -> Option<JobSite> {
        if domain == indeed::SHORT_DOMAIN {
            return Some(JobSite::Indeed);
        }
        JobSite::ALL
            .into_iter()
            .chain(JobSite::SIGNED_IN)
            .find(|site| site.domain() == domain)
    }

    /// The site's newest posts for one search term, near place ("" for anywhere). A signed-in
    /// site is read in signed_in, and can't be without one.
    pub async fn search(
        self,
        term: &str,
        place: &str,
        signed_in: Option<&SignedInBrowser>,
    ) -> Result<Vec<ListedPost>, String> {
        match (self, signed_in) {
            (JobSite::Jobnet, _) => jobnet::search(term, place).await,
            (JobSite::Jobindex, _) => jobindex::search(term, place).await,
            (JobSite::Linkedin, Some(browser)) => linkedin::search(browser, term, place).await,
            (JobSite::Indeed, Some(browser)) => indeed::search(browser, term, place).await,
            (_, None) => Err("JobFinder has no browser of its own here to read it in.".to_string()),
        }
    }
}

/// The pages JobFinder's signed-in browser opens on to sign in to the signed-in sites.
pub const SIGN_IN_PAGES: [&str; 2] = [linkedin::SIGN_IN_PAGE, indeed::SITE];

/// A post as a site's search lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListedPost {
    pub url: String,
    pub title: String,
    pub company: String,
    pub location: String,
}

/// The post at url read by its site's own reader, when it is a site whose posts are read that
/// way: Jobnet from its API, LinkedIn and Indeed in the signed-in browser (when readers has
/// one), else None.
pub async fn read_post(url: &str, readers: Readers<'_>) -> Option<Result<Page, String>> {
    if let Some(id) = jobnet::post_id(url) {
        return Some(jobnet::read_post(&id).await);
    }
    let browser = readers.signed_in?;
    let (link, read) = if let Some(link) = linkedin::post_link(url) {
        let read = linkedin::read_post(browser, &link).await;
        (link, read)
    } else {
        let link = indeed::post_link(url)?;
        let read = indeed::read_post(browser, &link).await;
        (link, read)
    };
    Some(read.map(|text| Page {
        read_with: ReadWith::SignedIn,
        ..Page::of(link, text)
    }))
}

/// A post's link as its site always writes it (LinkedIn's and Indeed's, however they were
/// found), so the same post found twice is one post; other links as they are.
pub fn post_link(url: &str) -> String {
    linkedin::post_link(url)
        .or_else(|| indeed::post_link(url))
        .unwrap_or_else(|| url.trim().to_string())
}

/// The post a page the user's browser showed has (its HTML as shown, sent by the extension),
/// read as its site's reader reads it (LinkedIn, Indeed), else as any page is read. url is
/// post_link's.
pub fn sent_page(url: &str, html: &str) -> Result<Page, String> {
    let by_site = if linkedin::post_link(url).is_some() {
        linkedin::post_page(url, html)
    } else if indeed::post_link(url).is_some() {
        indeed::post_page(url, html)
    } else {
        Seen::NotYet
    };
    let text = match by_site {
        Seen::Shown(text) => text,
        _ => page_reader::to_text(html.as_bytes(), "text/html", url)?,
    };
    Ok(Page {
        read_with: ReadWith::Sent,
        ..Page::of(url.to_string(), text)
    })
}

/// A JSON value's text, "" when it isn't text.
fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("").trim()
}

/// The first of texts that isn't empty, or "".
fn first_of<'a>(texts: &[&'a str]) -> &'a str {
    texts.iter().copied().find(|text| !text.is_empty()).unwrap_or("")
}
