//! Jobnet (jobnet.dk, the Danish state's job bank), read through the JSON API its own pages use
//! (jobnet.dk/bff): FindJob/Search for a search's newest posts, FindJob/JobAdDetails for one
//! post. The pages themselves show the post with JavaScript, so a plain fetch of one has no
//! post in it. The API wants an "x-csrf: 1" header, and no login.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::{ListedPost, first_of, text};
use crate::matches::joblog;
use crate::matches::page_reader::{self, Page, ReadWith, Wanted};

/// The domain the sites box keeps for Jobnet.
pub const DOMAIN: &str = "jobnet.dk";

/// Where Jobnet's API is.
const API: &str = "https://jobnet.dk/bff";

/// Where a post's own page is, before its id.
const POST_PAGE: &str = "https://jobnet.dk/find-job/";

/// Posts one search asks for: Jobnet's first page of them, newest first.
const RESULTS_PER_SEARCH: usize = 20;

/// How far from the search's place a post may be, in km, when the place is a Danish town or
/// postcode.
const RADIUS_KM: u32 = 50;

/// The deadline Jobnet gives a post without one.
const NO_DEADLINE: &str = "1900-01-01";

/// What the API is asked for and answers.
const JSON: Wanted = Wanted {
    accept: "application/json",
    takes: is_json,
    headers: &[("x-csrf", "1")],
};

/// A post's id in its page's link: "https://jobnet.dk/find-job/3f2c…" (or /job/…).
static POST_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://(?:www\.)?jobnet\.dk/(?:find-job|job)/([0-9a-z-]{1,64})/?(?:[?#].*)?$")
        .expect("jobnet post link")
});

fn is_json(content_type: &str) -> bool {
    content_type.to_ascii_lowercase().contains("json")
}

/// Jobnet's newest posts for term, near place ("" for anywhere in Denmark; a place that isn't a
/// Danish town or postcode is anywhere too, and the match says how far it is).
pub async fn search(term: &str, place: &str) -> Result<Vec<ListedPost>, String> {
    let url = search_url(term, place);
    let fetched = page_reader::get(&url, &JSON).await?;
    let answer: Value =
        serde_json::from_slice(&fetched.body).map_err(|e| format!("Jobnet's search answered something else: {e}"))?;
    Ok(listed(&answer))
}

/// The API's search for term near place, newest first.
fn search_url(term: &str, place: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("searchString", term)
        .append_pair("resultsPerPage", &RESULTS_PER_SEARCH.to_string())
        .append_pair("pageNumber", "1")
        .append_pair("orderType", "PublicationDate");
    if let Some(postcode) = joblog::postcode_of(place) {
        query
            .append_pair("postalCode", postcode)
            .append_pair("kmRadius", &RADIUS_KM.to_string());
    }
    format!("{API}/FindJob/Search?{}", query.finish())
}

/// The posts of a search's answer ({"jobAds": [...]}) that have an id.
fn listed(answer: &Value) -> Vec<ListedPost> {
    answer["jobAds"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|ad| !text(ad, "jobAdId").is_empty())
        .map(|ad| ListedPost {
            url: format!("{POST_PAGE}{}", text(ad, "jobAdId")),
            title: text(ad, "title").to_string(),
            company: text(ad, "hiringOrgName").to_string(),
            location: first_of(&[text(ad, "postalDistrictName"), text(ad, "municipality")]).to_string(),
        })
        .collect()
}

/// The post id of a link to a Jobnet post's page, else None.
pub fn post_id(url: &str) -> Option<String> {
    POST_LINK.captures(url.trim()).map(|found| found[1].to_lowercase())
}

/// Jobnet's post id, read from the API as a page: its title, and its text as the match reads a
/// post.
pub async fn read_post(id: &str) -> Result<Page, String> {
    let url = format!("{API}/FindJob/JobAdDetails/{id}?incrementViews=false");
    let fetched = page_reader::get(&url, &JSON).await?;
    let details: Value =
        serde_json::from_slice(&fetched.body).map_err(|e| format!("Jobnet answered something else: {e}"))?;
    post_page(id, &details)
}

/// A post's details as a page: the title, then the employer, the place, the deadline and where
/// to apply, then the post's own text (HTML in "body").
fn post_page(id: &str, details: &Value) -> Result<Page, String> {
    let title = text(details, "title").to_string();
    let body_html = text(details, "body");
    let page_url = format!("{POST_PAGE}{id}");
    let body = if body_html.is_empty() {
        String::new()
    } else {
        page_reader::to_text(body_html.as_bytes(), "text/html", &page_url)
            .map(|page| page.text)
            .unwrap_or_default()
    };
    if title.is_empty() && body.is_empty() {
        return Err("Jobnet has no post with that id.".to_string());
    }
    let mut lines = Vec::new();
    let employer = text(&details["employer"], "name");
    if !employer.is_empty() {
        lines.push(employer.to_string());
    }
    let place = address(&details["job"]["address"]);
    if !place.is_empty() {
        lines.push(place);
    }
    let deadline = text(&details["application"], "deadlineDate");
    if !deadline.is_empty() && !deadline.starts_with(NO_DEADLINE) {
        lines.push(format!(
            "Ansøgningsfrist: {}",
            deadline.chars().take(10).collect::<String>()
        ));
    }
    let apply_at = text(&details["application"], "url");
    if !apply_at.is_empty() {
        lines.push(format!("Søg jobbet: {apply_at}"));
    }
    if !body.is_empty() {
        lines.push(String::new());
        lines.push(body);
    }
    Ok(Page {
        url: page_url,
        title,
        text: lines.join("\n").trim().to_string(),
        read_with: ReadWith::SiteApi,
    })
}

/// "5000 Odense C" (or the town alone), as the address gives it.
fn address(address: &Value) -> String {
    format!("{} {}", text(address, "postalCode"), text(address, "city"))
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_search_asks_for_the_newest_posts_near_the_place() {
        assert_eq!(
            search_url("PHP udvikler", "Odense"),
            "https://jobnet.dk/bff/FindJob/Search?searchString=PHP+udvikler&resultsPerPage=20&pageNumber=1&orderType=PublicationDate&postalCode=5000&kmRadius=50"
        );
        assert_eq!(
            search_url("PHP", "Remote"),
            "https://jobnet.dk/bff/FindJob/Search?searchString=PHP&resultsPerPage=20&pageNumber=1&orderType=PublicationDate"
        );
    }

    #[test]
    fn the_answer_lists_its_posts_by_their_pages() {
        let answer = json!({ "jobAds": [
            { "jobAdId": "3f2c-1", "title": "PHP-udvikler", "hiringOrgName": "Acme A/S", "postalDistrictName": "Odense C", "municipality": "Odense" },
            { "jobAdId": "", "title": "No id" },
            { "jobAdId": "9", "title": "Backend", "hiringOrgName": "B", "municipality": "Vejle" },
        ]});
        let posts = listed(&answer);
        assert_eq!(posts.len(), 2);
        assert_eq!(
            posts[0],
            ListedPost {
                url: "https://jobnet.dk/find-job/3f2c-1".to_string(),
                title: "PHP-udvikler".to_string(),
                company: "Acme A/S".to_string(),
                location: "Odense C".to_string(),
            }
        );
        assert_eq!(posts[1].location, "Vejle");
        assert!(listed(&json!({})).is_empty());
    }

    #[test]
    fn a_post_link_gives_its_id() {
        assert_eq!(
            post_id("https://jobnet.dk/find-job/3F2C-1?x=1"),
            Some("3f2c-1".to_string())
        );
        assert_eq!(post_id(" https://www.jobnet.dk/job/123/ "), Some("123".to_string()));
        assert_eq!(post_id("https://jobnet.dk/find-job"), None);
        assert_eq!(post_id("https://example.com/find-job/1"), None);
    }

    #[test]
    fn a_posts_details_read_as_its_page() {
        let details = json!({
            "title": "PHP-udvikler",
            "body": "<p>Vi søger en <b>PHP-udvikler</b>.</p><ul><li>Laravel</li></ul>",
            "employer": { "name": "Acme A/S" },
            "job": { "address": { "postalCode": "5000", "city": "Odense C" } },
            "application": { "deadlineDate": "2026-11-30T00:00:00", "url": "https://acme.dk/apply" },
        });
        let page = post_page("1", &details).unwrap();
        assert_eq!(page.url, "https://jobnet.dk/find-job/1");
        assert_eq!(page.title, "PHP-udvikler");
        assert_eq!(
            page.text,
            "Acme A/S\n5000 Odense C\nAnsøgningsfrist: 2026-11-30\nSøg jobbet: https://acme.dk/apply\n\nVi søger en PHP-udvikler.\n\n- Laravel"
        );
        let no_deadline = json!({ "title": "T", "application": { "deadlineDate": "1900-01-01T00:00:00" } });
        assert_eq!(post_page("1", &no_deadline).unwrap().text, "");
        assert!(post_page("1", &json!({})).is_err());
    }
}
