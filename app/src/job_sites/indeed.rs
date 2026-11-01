//! Indeed Denmark (dk.indeed.com), read in JobFinder's signed-in browser (signed_in): its search
//! page, newest first, and its posts. Indeed needs no sign-in to search, but it shows its pages
//! only to a browser that passes its bot check, which a headless one may not: a check is said,
//! and the scan leaves Indeed alone until the user opens the sign-in window again. A post is
//! always linked as https://{host}/viewjob?jk={id}, however it was found (a search page's vjk).
//!
//! What is read from a page is found by Indeed's own names, which it changes now and then.

use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};

use super::ListedPost;
use super::signed_in::{Seen, SignedInBrowser};
use crate::matches::page_reader::{self, PageText};

/// The domain the sites box keeps for Indeed Denmark.
pub const DOMAIN: &str = "dk.indeed.com";

/// The other domain the sites box may keep for Indeed, read as Indeed Denmark.
pub const SHORT_DOMAIN: &str = "indeed.com";

/// Where Indeed Denmark is, and where to sign in to it.
pub const SITE: &str = "https://dk.indeed.com";

/// The oldest post a search lists, in days since it was published (fromage).
const MAX_AGE_DAYS: u32 = 7;

/// Least characters of a post's text for its page to count as showing it.
const MIN_POST_TEXT: usize = 200;

/// An Indeed post's id in its link: jk (its own page) or vjk (a search page's post beside the
/// list), on any of Indeed's hosts.
static POST_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://((?:[a-z]{2}\.)?indeed\.com)/[^#]*[?&]v?jk=([0-9a-f]{8,32})").expect("Indeed post id")
});

/// What Indeed's bot check shows instead of the page (Cloudflare's, or its own).
static CHECK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)challenges\.cloudflare\.com|cf-challenge|<title>[^<]*(?:just a moment|bare et øjeblik|security check)",
    )
    .expect("check")
});

/// Indeed's sign-in pages.
static SIGN_IN_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^https?://secure\.indeed\.com/(?:auth|account/login)").expect("sign-in path"));

/// The cards of a search page, each carrying its post's id.
static CARD: LazyLock<Selector> = LazyLock::new(|| Selector::parse("[data-jk]").expect("card"));
/// The class of the box a search page's card is in.
const CARD_CLASS: &str = "job_seen_beacon";
/// What a search page shows when nothing fits.
static NO_RESULTS: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(".jobsearch-NoResult, .jobsearch-NoResult-messageContainer").expect("no results"));

/// Where a card has its title, company and place.
const CARD_TITLE: [&str; 3] = ["h2.jobTitle span[title]", "h2.jobTitle", ".jcs-JobTitle"];
const CARD_COMPANY: [&str; 2] = [r#"[data-testid="company-name"]"#, ".companyName"];
const CARD_LOCATION: [&str; 2] = [r#"[data-testid="text-location"]"#, ".companyLocation"];

/// Where a post's page (or a search page's post beside its list) has its title, company, place
/// and text, tried in order.
const POST_TITLE: [&str; 3] = [
    r#"[data-testid="jobsearch-JobInfoHeader-title"]"#,
    ".jobsearch-JobInfoHeader-title",
    "h1",
];
const POST_COMPANY: [&str; 2] = [r#"[data-testid="inlineHeader-companyName"]"#, "[data-company-name]"];
const POST_LOCATION: [&str; 2] = [
    r#"[data-testid="inlineHeader-companyLocation"]"#,
    r#"[data-testid="job-location"]"#,
];
const POST_TEXT: [&str; 1] = ["#jobDescriptionText"];

/// The post's link as Indeed's posts are always written, when url is an Indeed post's.
pub fn post_link(url: &str) -> Option<String> {
    let found = POST_ID.captures(url.trim())?;
    let host = found[1].to_lowercase();
    let host = if host == SHORT_DOMAIN { DOMAIN.to_string() } else { host };
    Some(format!("https://{host}/viewjob?jk={}", found[2].to_lowercase()))
}

/// Indeed's newest posts for term near place ("" for anywhere), read in browser.
pub async fn search(browser: &SignedInBrowser, term: &str, place: &str) -> Result<Vec<ListedPost>, String> {
    browser.read(DOMAIN, &search_url(term, place), listed).await
}

/// The post at url (post_link's), read in browser.
pub async fn read_post(browser: &SignedInBrowser, url: &str) -> Result<PageText, String> {
    browser.read(DOMAIN, url, post_page).await
}

/// The search page for term near place, newest first, of the last MAX_AGE_DAYS.
fn search_url(term: &str, place: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("q", term.trim())
        .append_pair("l", place.trim())
        .append_pair("sort", "date")
        .append_pair("fromage", &MAX_AGE_DAYS.to_string());
    format!("{SITE}/jobs?{}", query.finish())
}

/// Whether the page at location is Indeed asking to sign in or for a check.
fn asks(location: &str, html: &str) -> Option<bool> {
    if SIGN_IN_PATH.is_match(location) {
        Some(true)
    } else if CHECK.is_match(html) {
        Some(false)
    } else {
        None
    }
}

/// The search page's posts, once its list shows.
pub fn listed(location: &str, html: &str) -> Seen<Vec<ListedPost>> {
    match asks(location, html) {
        Some(true) => return Seen::SignIn,
        Some(false) => return Seen::Check,
        None => {}
    }
    let page = Html::parse_document(html);
    let mut posts: Vec<ListedPost> = Vec::new();
    for card in page.select(&CARD) {
        let id = card.attr("data-jk").unwrap_or("").to_lowercase();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let url = format!("{SITE}/viewjob?jk={id}");
        if posts.iter().any(|known| known.url == url) {
            continue;
        }
        let card = whole_card(card);
        let part = |selectors: &[&str]| {
            selectors.iter().find_map(|selector| {
                let selector = Selector::parse(selector).ok()?;
                card.select(&selector)
                    .map(|element| {
                        let text = page_reader::part_text(element);
                        if text.is_empty() {
                            element.attr("title").unwrap_or("").trim().to_string()
                        } else {
                            text
                        }
                    })
                    .find(|text| !text.is_empty())
            })
        };
        posts.push(ListedPost {
            url,
            title: part(&CARD_TITLE).unwrap_or_default(),
            company: part(&CARD_COMPANY).unwrap_or_default(),
            location: part(&CARD_LOCATION).unwrap_or_default(),
        });
    }
    if posts.is_empty() && page.select(&NO_RESULTS).next().is_none() {
        return Seen::NotYet;
    }
    Seen::Shown(posts)
}

/// The card a post's id is on: its title link carries the id, inside the card with its company
/// and place (a .job_seen_beacon, in a list item).
fn whole_card(element: scraper::ElementRef<'_>) -> scraper::ElementRef<'_> {
    element
        .ancestors()
        .filter_map(scraper::ElementRef::wrap)
        .find(|ancestor| {
            let ancestor = ancestor.value();
            ancestor.has_class(CARD_CLASS, scraper::CaseSensitivity::CaseSensitive) || ancestor.name() == "li"
        })
        .unwrap_or(element)
}

/// The post a page shows (its own page, or a search page's post beside its list), once its
/// text shows.
pub fn post_page(location: &str, html: &str) -> Seen<PageText> {
    match asks(location, html) {
        Some(true) => return Seen::SignIn,
        Some(false) => return Seen::Check,
        None => {}
    }
    let page = Html::parse_document(html);
    let Some(text) = page_reader::element_text(&page, &POST_TEXT).filter(|text| text.chars().count() >= MIN_POST_TEXT)
    else {
        return Seen::NotYet;
    };
    let title = page_reader::element_text(&page, &POST_TITLE).unwrap_or_default();
    let about: Vec<String> = [&POST_COMPANY[..], &POST_LOCATION[..]]
        .iter()
        .filter_map(|selectors| page_reader::element_text(&page, selectors))
        .collect();
    let text = about.into_iter().chain([text]).collect::<Vec<_>>().join("\n\n");
    Seen::Shown(PageText { title, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_is_always_linked_the_same_way() {
        let link = Some("https://dk.indeed.com/viewjob?jk=0123456789abcdef".to_string());
        assert_eq!(
            post_link("https://dk.indeed.com/viewjob?jk=0123456789abcdef&from=serp"),
            link
        );
        assert_eq!(
            post_link("https://dk.indeed.com/jobs?q=php&l=&vjk=0123456789ABCDEF"),
            link
        );
        assert_eq!(post_link("https://indeed.com/viewjob?jk=0123456789abcdef"), link);
        assert_eq!(
            post_link("https://de.indeed.com/viewjob?jk=0123456789abcdef").as_deref(),
            Some("https://de.indeed.com/viewjob?jk=0123456789abcdef")
        );
        assert_eq!(post_link("https://dk.indeed.com/cmp/acme"), None);
    }

    #[test]
    fn the_search_is_newest_first() {
        let url = search_url("PHP udvikler", "Odense");
        assert_eq!(
            url,
            "https://dk.indeed.com/jobs?q=PHP+udvikler&l=Odense&sort=date&fromage=7"
        );
    }

    #[test]
    fn a_search_page_lists_its_cards() {
        let html = r#"<html><body><ul>
            <li><div class="job_seen_beacon"><a data-jk="0123456789abcdef" class="jcs-JobTitle">
              <h2 class="jobTitle"><span title="PHP-udvikler">PHP-udvikler</span></h2></a>
              <span data-testid="company-name">Acme A/S</span>
              <div data-testid="text-location">5000 Odense C</div></div></li>
          </ul></body></html>"#;
        let Seen::Shown(posts) = listed("https://dk.indeed.com/jobs?q=php", html) else {
            panic!("the list shows");
        };
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].url, "https://dk.indeed.com/viewjob?jk=0123456789abcdef");
        assert_eq!(posts[0].title, "PHP-udvikler");
        assert_eq!(posts[0].company, "Acme A/S");
        assert_eq!(posts[0].location, "5000 Odense C");
    }

    #[test]
    fn the_bot_check_is_said() {
        let check = "<html><head><title>Just a moment...</title></head><body></body></html>";
        assert_eq!(listed("https://dk.indeed.com/jobs?q=php", check), Seen::Check);
        assert_eq!(
            listed("https://dk.indeed.com/jobs?q=php", "<html><body></body></html>"),
            Seen::NotYet
        );
    }

    #[test]
    fn a_post_page_is_read_once_its_text_shows() {
        let description = "Vi søger en erfaren PHP-udvikler til vores team i Odense. ".repeat(5);
        let html = format!(
            r#"<html><body><h1 data-testid="jobsearch-JobInfoHeader-title">PHP-udvikler</h1>
              <div data-testid="inlineHeader-companyName">Acme A/S</div>
              <div id="jobDescriptionText"><p>{description}</p></div></body></html>"#
        );
        let Seen::Shown(post) = post_page("https://dk.indeed.com/viewjob?jk=0123456789abcdef", &html) else {
            panic!("the post shows");
        };
        assert_eq!(post.title, "PHP-udvikler");
        assert!(post.text.starts_with("Acme A/S\n\nVi søger"));
    }
}
