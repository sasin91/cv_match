//! The job sites the app reads itself, without a language model searching the web: their own
//! search (Jobnet's JSON API, Jobindex's search page) for the job scan, and Jobnet's API for one
//! of its posts, whose page shows the post with JavaScript. Pages that need JavaScript
//! elsewhere are read in the computer's own browser (browser).
//!
//! Every request goes through page_reader's fetching rules (public addresses only, every
//! redirect checked).

pub mod browser;
pub mod jobindex;
pub mod jobnet;

/// A site the app searches itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobSite {
    Jobnet,
    Jobindex,
}

impl JobSite {
    /// The sites a search without sites of its own is read on.
    pub const ALL: [JobSite; 2] = [JobSite::Jobnet, JobSite::Jobindex];

    /// The site's domain as the sites box keeps it (job_search_rules::sites).
    pub fn domain(self) -> &'static str {
        match self {
            JobSite::Jobnet => jobnet::DOMAIN,
            JobSite::Jobindex => jobindex::DOMAIN,
        }
    }

    /// The site the app reads for a domain of the sites box, if it reads it.
    pub fn of(domain: &str) -> Option<JobSite> {
        JobSite::ALL.into_iter().find(|site| site.domain() == domain)
    }

    /// The site's newest posts for one search term, near place ("" for anywhere).
    pub async fn search(self, term: &str, place: &str) -> Result<Vec<ListedPost>, String> {
        match self {
            JobSite::Jobnet => jobnet::search(term, place).await,
            JobSite::Jobindex => jobindex::search(term, place).await,
        }
    }
}

/// A post as a site's search lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListedPost {
    pub url: String,
    pub title: String,
    pub company: String,
    pub location: String,
}

/// The post at url read from its site's own API, when it is a site whose posts are read that way
/// (Jobnet), else None.
pub async fn read_post(url: &str) -> Option<Result<crate::matches::page_reader::Page, String>> {
    let id = jobnet::post_id(url)?;
    Some(jobnet::read_post(&id).await)
}

/// A JSON value's text, "" when it isn't text.
fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("").trim()
}

/// The first of texts that isn't empty, or "".
fn first_of<'a>(texts: &[&'a str]) -> &'a str {
    texts.iter().copied().find(|text| !text.is_empty()).unwrap_or("")
}
