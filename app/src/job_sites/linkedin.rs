//! LinkedIn (linkedin.com), read in JobFinder's signed-in browser (signed_in): its job search
//! page, newest first, and its posts, whose pages show them with JavaScript for a signed-in
//! member. A post is always linked as https://www.linkedin.com/jobs/view/{id}/, however it was
//! found (a search page's currentJobId, a job alert's /comm/ link, a slug before the id).
//!
//! What is read from a page is found by LinkedIn's own class names, which it changes now and
//! then: a page whose posts aren't found any more says so rather than reading nothing.

use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};

use super::ListedPost;
use super::signed_in::{Seen, SignedInBrowser};
use crate::matches::page_reader::{self, PageText};

/// The domain the sites box keeps for LinkedIn.
pub const DOMAIN: &str = "linkedin.com";

/// Where to sign in to LinkedIn.
pub const SIGN_IN_PAGE: &str = "https://www.linkedin.com/login";

/// Where LinkedIn is.
const SITE: &str = "https://www.linkedin.com";

/// The place a search without one is read near: LinkedIn's search wants a place.
const ANYWHERE: &str = "Denmark";

/// LinkedIn's "posted in the past week" filter (f_TPR), in seconds.
const PAST_WEEK: &str = "r604800";

/// LinkedIn's "most recent first" order (sortBy).
const NEWEST_FIRST: &str = "DD";

/// Least characters of a post's text for its page to count as showing it.
const MIN_POST_TEXT: usize = 200;

/// A LinkedIn post's id in its link: /jobs/view/{id}, after an optional slug, or a search
/// page's currentJobId.
static POST_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://(?:[a-z]{2,3}\.|www\.)?linkedin\.com/(?:comm/)?jobs/(?:view/(?:[^/?#]*-)?(\d+)|.*[?&](?:currentJobId|jobId)=(\d+))")
        .expect("LinkedIn post id")
});

/// Pages LinkedIn shows instead of the one asked for when the browser has to sign in.
static SIGN_IN_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://[^/]*linkedin\.com/(?:login|authwall|uas/login|signup)").expect("sign-in path")
});

/// Pages LinkedIn shows when it wants the browser to show it is a person.
static CHECK_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^https?://[^/]*linkedin\.com/checkpoint").expect("check path"));

/// The cards of a search page's list, each carrying its post's id.
static CARD: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("[data-job-id], [data-occludable-job-id]").expect("card"));
/// A link to a post, in a card or anywhere on the page.
static POST_LINK: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(r#"a[href*="/jobs/view/"]"#).expect("post link"));
/// What a search page shows when nothing fits.
static NO_RESULTS: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(".jobs-search-no-results-banner, .jobs-search-no-results").expect("no results"));

/// Where a card has its company and its place.
const CARD_COMPANY: [&str; 2] = [
    ".artdeco-entity-lockup__subtitle",
    ".job-card-container__primary-description",
];
const CARD_LOCATION: [&str; 2] = [".artdeco-entity-lockup__caption", ".job-card-container__metadata-item"];

/// Where a post's page (or a search page's post beside its list) has its title, company, place
/// and text, tried in order.
const POST_TITLE: [&str; 3] = [
    ".job-details-jobs-unified-top-card__job-title",
    ".top-card-layout__title",
    "h1",
];
const POST_COMPANY: [&str; 2] = [
    ".job-details-jobs-unified-top-card__company-name",
    ".topcard__org-name-link",
];
const POST_LOCATION: [&str; 2] = [
    ".job-details-jobs-unified-top-card__primary-description-container",
    ".topcard__flavor--bullet",
];
const POST_TEXT: [&str; 6] = [
    ABOUT_THE_JOB,
    "#job-details",
    ".jobs-description__content",
    ".jobs-description-content__text",
    ".jobs-box__html-content",
    ".description__text",
];

/// The post's description in LinkedIn's newer layout (from late 2026), whose class names are
/// build hashes: its "About the job" box, with that heading (an h2) first.
const ABOUT_THE_JOB: &str = r#"[data-view-name="job-details-about-the-job"]"#;

/// A heading inside the post's description box, left out of its text.
static HEADING: LazyLock<Selector> = LazyLock::new(|| Selector::parse("h2").expect("heading"));

/// The page's own title: "{title} | {company} | LinkedIn" on a post's page, after "(3) " when
/// the member has notifications. The newer layout has no h1 and no stable names for the post's
/// title and company, so they are read from here.
static PAGE_TITLE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("head > title").expect("page title"));

/// The unread-notifications count LinkedIn puts before a page's title, "(3) ".
static NOTIFICATIONS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\(\d+\)\s*").expect("notifications"));

/// What LinkedIn puts last in a page's title.
const TITLE_SITE: &str = "LinkedIn";

/// What separates the parts of a page's title.
const TITLE_SEPARATOR: &str = " | ";

/// The post's link as LinkedIn's posts are always written, when url is a LinkedIn post's.
pub fn post_link(url: &str) -> Option<String> {
    let found = POST_ID.captures(url.trim())?;
    let id = found.get(1).or_else(|| found.get(2))?.as_str();
    Some(format!("{SITE}/jobs/view/{id}/"))
}

/// LinkedIn's newest posts for term near place ("" for anywhere), read in browser.
pub async fn search(browser: &SignedInBrowser, term: &str, place: &str) -> Result<Vec<ListedPost>, String> {
    browser.read(DOMAIN, &search_url(term, place), listed).await
}

/// The post at url (post_link's), read in browser.
pub async fn read_post(browser: &SignedInBrowser, url: &str) -> Result<PageText, String> {
    browser.read(DOMAIN, url, post_page).await
}

/// The search page for term near place, newest first, of the past week.
fn search_url(term: &str, place: &str) -> String {
    let place = if place.trim().is_empty() {
        ANYWHERE
    } else {
        place.trim()
    };
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("keywords", term.trim())
        .append_pair("location", place)
        .append_pair("f_TPR", PAST_WEEK)
        .append_pair("sortBy", NEWEST_FIRST);
    format!("{SITE}/jobs/search/?{}", query.finish())
}

/// Whether the page at location is LinkedIn asking to sign in or for a check.
fn asks(location: &str) -> Option<Seen<()>> {
    if SIGN_IN_PATH.is_match(location) {
        Some(Seen::SignIn)
    } else if CHECK_PATH.is_match(location) {
        Some(Seen::Check)
    } else {
        None
    }
}

/// The search page's posts, once its list shows.
pub fn listed(location: &str, html: &str) -> Seen<Vec<ListedPost>> {
    match asks(location) {
        Some(Seen::SignIn) => return Seen::SignIn,
        Some(_) => return Seen::Check,
        None => {}
    }
    let page = Html::parse_document(html);
    let mut posts: Vec<ListedPost> = Vec::new();
    let mut add = |post: ListedPost| {
        if !posts.iter().any(|known| known.url == post.url) {
            posts.push(post);
        }
    };
    for card in page.select(&CARD) {
        let id = card
            .attr("data-job-id")
            .or_else(|| card.attr("data-occludable-job-id"))
            .unwrap_or("");
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let part = |selectors: &[&str]| {
            selectors.iter().find_map(|selector| {
                let selector = Selector::parse(selector).ok()?;
                card.select(&selector)
                    .map(page_reader::part_text)
                    .find(|text| !text.is_empty())
            })
        };
        add(ListedPost {
            url: format!("{SITE}/jobs/view/{id}/"),
            title: card_title(card),
            company: part(&CARD_COMPANY).unwrap_or_default(),
            location: part(&CARD_LOCATION).unwrap_or_default(),
        });
    }
    for link in page.select(&POST_LINK) {
        let Some(url) = link.attr("href").and_then(|href| post_link(&absolute(href))) else {
            continue;
        };
        add(ListedPost {
            url,
            title: first_line(&page_reader::part_text(link)),
            ..ListedPost::default()
        });
    }
    if posts.is_empty() && page.select(&NO_RESULTS).next().is_none() {
        return Seen::NotYet;
    }
    Seen::Shown(posts)
}

/// A card's title: its post link's aria-label (its text has the title twice, once for screen
/// readers), else the first line of its text.
fn card_title(card: scraper::ElementRef<'_>) -> String {
    card.select(&POST_LINK)
        .next()
        .map(|link| match link.attr("aria-label").map(str::trim) {
            Some(label) if !label.is_empty() => label.to_string(),
            _ => first_line(&page_reader::part_text(link)),
        })
        .unwrap_or_default()
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

/// A link of the page as a whole link.
fn absolute(href: &str) -> String {
    if href.starts_with('/') {
        format!("{SITE}{href}")
    } else {
        href.to_string()
    }
}

/// The post a page shows (its own page, or a search page's post beside its list), once its
/// text shows.
pub fn post_page(location: &str, html: &str) -> Seen<PageText> {
    match asks(location) {
        Some(Seen::SignIn) => return Seen::SignIn,
        Some(_) => return Seen::Check,
        None => {}
    }
    let page = Html::parse_document(html);
    let Some(text) = description(&page).filter(|text| text.chars().count() >= MIN_POST_TEXT) else {
        return Seen::NotYet;
    };
    let (page_title, page_company) = title_parts(&page);
    let title = page_reader::element_text(&page, &POST_TITLE)
        .map(|title| first_line(&title))
        .or(page_title)
        .unwrap_or_default();
    let company = page_reader::element_text(&page, &POST_COMPANY).or(page_company);
    let about: Vec<String> = [company, page_reader::element_text(&page, &POST_LOCATION)]
        .into_iter()
        .flatten()
        .collect();
    let text = about.into_iter().chain([text]).collect::<Vec<_>>().join("\n\n");
    Seen::Shown(PageText { title, text })
}

/// The post's description: the first of POST_TEXT's boxes with text, without its heading.
fn description(page: &Html) -> Option<String> {
    POST_TEXT.iter().find_map(|selector| {
        let selector = Selector::parse(selector).ok()?;
        page.select(&selector)
            .map(|element| without_heading(element, page_reader::part_text(element)))
            .find(|text| !text.is_empty())
    })
}

/// text (box's) without the heading box starts with ("About the job", "Om jobbet").
fn without_heading(element: scraper::ElementRef<'_>, text: String) -> String {
    let Some(heading) = element.select(&HEADING).next().map(page_reader::part_text) else {
        return text;
    };
    match text.strip_prefix(heading.as_str()) {
        Some(rest) if !heading.is_empty() => rest.trim_start().to_string(),
        _ => text,
    }
}

/// The post's title and company as the page's own title has them, when it is a post's.
fn title_parts(page: &Html) -> (Option<String>, Option<String>) {
    let Some(whole) = page.select(&PAGE_TITLE).next().map(page_reader::part_text) else {
        return (None, None);
    };
    let whole = NOTIFICATIONS.replace(&whole, "");
    let mut parts: Vec<&str> = whole.split(TITLE_SEPARATOR).map(str::trim).collect();
    if parts.last() == Some(&TITLE_SITE) {
        parts.pop();
    }
    let part = |index: usize| {
        parts
            .get(index)
            .filter(|part| !part.is_empty())
            .map(|part| part.to_string())
    };
    match parts.len() {
        0 | 1 => (None, None),
        _ => (part(0), part(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_post_is_always_linked_the_same_way() {
        let link = Some("https://www.linkedin.com/jobs/view/4012345678/".to_string());
        assert_eq!(post_link("https://www.linkedin.com/jobs/view/4012345678"), link);
        assert_eq!(
            post_link("https://dk.linkedin.com/jobs/view/php-udvikler-at-acme-4012345678?trk=public"),
            link
        );
        assert_eq!(
            post_link("https://www.linkedin.com/comm/jobs/view/4012345678/?trackingId=x"),
            link
        );
        assert_eq!(
            post_link("https://www.linkedin.com/jobs/search/?keywords=php&currentJobId=4012345678"),
            link
        );
        assert_eq!(
            post_link("https://www.linkedin.com/jobs/collections/recommended/?currentJobId=4012345678"),
            link
        );
        assert_eq!(post_link("https://www.linkedin.com/in/someone/"), None);
        assert_eq!(post_link("https://www.jobindex.dk/jobs/view/1"), None);
    }

    #[test]
    fn the_search_is_newest_first_near_a_place() {
        let url = search_url("PHP udvikler", "");
        assert!(url.starts_with("https://www.linkedin.com/jobs/search/?keywords=PHP+udvikler&location=Denmark"));
        assert!(url.contains("sortBy=DD"));
        assert!(search_url("PHP", "Odense").contains("location=Odense"));
    }

    #[test]
    fn a_search_page_lists_its_cards() {
        let html = r#"<html><body><ul>
            <li data-occludable-job-id="4012345678"><div class="job-card-container">
              <a class="job-card-list__title" aria-label="PHP-udvikler" href="/jobs/view/4012345678/?trk=x">
                <span aria-hidden="true"><strong>PHP-udvikler</strong></span>
                <span class="visually-hidden">PHP-udvikler</span>
              </a>
              <div class="artdeco-entity-lockup__subtitle"><span>Acme A/S</span></div>
              <div class="artdeco-entity-lockup__caption"><span>Odense, Denmark (Hybrid)</span></div>
            </div></li>
            <li data-occludable-job-id="4012345679"></li>
          </ul></body></html>"#;
        let Seen::Shown(posts) = listed("https://www.linkedin.com/jobs/search/?keywords=php", html) else {
            panic!("the list shows");
        };
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].url, "https://www.linkedin.com/jobs/view/4012345678/");
        assert_eq!(posts[0].title, "PHP-udvikler");
        assert_eq!(posts[0].company, "Acme A/S");
        assert_eq!(posts[0].location, "Odense, Denmark (Hybrid)");
        assert_eq!(posts[1].url, "https://www.linkedin.com/jobs/view/4012345679/");
    }

    #[test]
    fn a_search_page_still_loading_is_not_yet() {
        let html = "<html><body><div class=\"jobs-search-results-list\"></div></body></html>";
        assert_eq!(listed("https://www.linkedin.com/jobs/search/", html), Seen::NotYet);
        let none =
            "<html><body><div class=\"jobs-search-no-results-banner\">No matching jobs found.</div></body></html>";
        assert_eq!(
            listed("https://www.linkedin.com/jobs/search/", none),
            Seen::Shown(Vec::new())
        );
    }

    #[test]
    fn being_sent_to_sign_in_or_a_check_is_said() {
        assert_eq!(
            listed("https://www.linkedin.com/login?session_redirect=x", ""),
            Seen::SignIn
        );
        assert_eq!(post_page("https://www.linkedin.com/authwall?trk=x", ""), Seen::SignIn);
        assert_eq!(
            post_page("https://www.linkedin.com/checkpoint/challenge/x", ""),
            Seen::Check
        );
    }

    #[test]
    fn a_post_page_is_read_once_its_text_shows() {
        let description = "Vi søger en erfaren PHP-udvikler til vores team i Odense. ".repeat(5);
        let html = format!(
            r#"<html><body>
              <div class="job-details-jobs-unified-top-card__job-title"><h1>PHP-udvikler</h1></div>
              <div class="job-details-jobs-unified-top-card__company-name"><a>Acme A/S</a></div>
              <div id="job-details"><p>{description}</p></div>
            </body></html>"#
        );
        let Seen::Shown(post) = post_page("https://www.linkedin.com/jobs/view/4012345678/", &html) else {
            panic!("the post shows");
        };
        assert_eq!(post.title, "PHP-udvikler");
        assert!(post.text.starts_with("Acme A/S\n\nVi søger"));
        let loading = r#"<html><body><h1>PHP-udvikler</h1><div id="job-details"></div></body></html>"#;
        assert_eq!(
            post_page("https://www.linkedin.com/jobs/view/4012345678/", loading),
            Seen::NotYet
        );
    }

    #[test]
    fn a_post_page_in_the_newer_layout_is_read_by_its_view_name_and_title() {
        let description = "Vi søger en erfaren PHP-udvikler til vores team i Odense. ".repeat(5);
        let html = format!(
            r#"<html><head><title>(3) PHP-udvikler (m/w/x) | Acme A/S | LinkedIn</title></head><body>
              <p class="b6bk8d">PHP-udvikler (m/w/x)</p>
              <div class="b6bax7" data-view-name="job-details-about-the-job"><div componentkey="x">
                <div class="b6bayf"><h2 class="b6bk8d">Om jobbet</h2></div>
                <div class="b6bk8d"><p>{description}</p></div>
              </div></div>
            </body></html>"#
        );
        let Seen::Shown(post) = post_page("https://www.linkedin.com/jobs/view/4012345678/", &html) else {
            panic!("the post shows");
        };
        assert_eq!(post.title, "PHP-udvikler (m/w/x)");
        assert!(post.text.starts_with("Acme A/S\n\nVi søger"), "{}", post.text);
        let loading = r#"<html><head><title>LinkedIn</title></head><body>
            <div data-view-name="job-details-about-the-job"><h2>Om jobbet</h2></div></body></html>"#;
        assert_eq!(
            post_page("https://www.linkedin.com/jobs/view/4012345678/", loading),
            Seen::NotYet
        );
    }
}
