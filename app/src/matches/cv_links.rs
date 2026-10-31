//! The profile links a CV names (LinkedIn, GitHub, a website), read by rule from its text, for the
//! Apply dialog when the candidate has no profile links saved.
//!
//! Any http(s) address counts; without a scheme only linkedin.com/in/..., github.com/... and
//! gitlab.com/... do, as "Node.js/React" would otherwise read as one.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// host => the label a link there gets; any other host is WEBSITE.
pub const LABELS: [(&str, &str); 3] = [
    ("linkedin.com", "LinkedIn"),
    ("github.com", "GitHub"),
    ("gitlab.com", "GitLab"),
];
pub const WEBSITE: &str = "Website";

/// An address in the text: not part of an email address or a longer word, with a scheme or on
/// one of LABELS' sites, up to whitespace, quotes, angle brackets or a " · ". \w is PCRE's
/// ASCII one.
static ADDRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?<![@A-Za-z0-9_.])(?:https?://|(?:www\.)?(?:linkedin\.com/in|github\.com|gitlab\.com)/)[^\s<>"'·]+"#,
    )
    .expect("the address pattern")
});

static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^https?://").expect("the scheme pattern"));

/// Prose punctuation that ends a sentence around an address rather than the address.
const TRAILING_PUNCTUATION: &str = ".,;:!?'\"";

/// A link the CV names: its label and address.
#[derive(Debug, Clone, PartialEq)]
pub struct CvLink {
    pub label: String,
    pub url: String,
}

/// The CV's links in its order, each address once.
pub fn read(cv_text: &str) -> Vec<CvLink> {
    let mut links: Vec<CvLink> = Vec::new();
    for found in ADDRESS.find_iter(cv_text).flatten() {
        let url = with_scheme(without_trailing_punctuation(found.as_str()));
        let host = host_of(&url);
        if host.is_empty() || links.iter().any(|link| link.url == url) {
            continue;
        }
        let label = LABELS
            .iter()
            .find(|(site, _)| *site == host)
            .map(|(_, label)| *label)
            .unwrap_or(WEBSITE);
        links.push(CvLink {
            label: label.to_string(),
            url,
        });
    }
    links
}

/// The address's host in lower case without "www.", "" when it has none.
fn host_of(url: &str) -> String {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    host.strip_prefix("www.").map(str::to_string).unwrap_or(host)
}

/// The address without the prose punctuation after it ("see x.com/a, or (x.com/b)."), keeping
/// what belongs to it (";view=full", "(1)").
fn without_trailing_punctuation(address: &str) -> &str {
    let mut address = address;
    while let Some(last) = address.chars().last() {
        let unbalanced = last == ')' && address.matches('(').count() < address.matches(')').count();
        if !unbalanced && !TRAILING_PUNCTUATION.contains(last) {
            break;
        }
        address = &address[..address.len() - last.len_utf8()];
    }
    address
}

fn with_scheme(address: &str) -> String {
    if SCHEME.is_match(address).unwrap_or(false) {
        address.to_string()
    } else {
        format!("https://{address}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cvs_own_links_are_read_in_its_order() {
        let cv = "Jonas Hansen\nSlagelse · jonas.kerwin.hansen@gmail.com\nFull CV online: sasin91.xyz/cv\n\
            https://github.com/sasin91 · https://www.linkedin.com/in/jonas-hansen-2b6828110\n\
            Node.js/React, Vue.js. See www.linkedin.com/in/jonas-hansen-2b6828110 or https://jonas.dev/work.\n\
            Also gitlab.com/sasin91, email someone@github.com\n\
            Talks: https://example.com/profile;view=full, (https://example.com/wiki/PHP_(language)) and https://example.com/a?b=1,2.";
        let read: Vec<String> = read(cv)
            .iter()
            .map(|link| format!("{}: {}", link.label, link.url))
            .collect();
        assert_eq!(
            read,
            [
                "GitHub: https://github.com/sasin91",
                "LinkedIn: https://www.linkedin.com/in/jonas-hansen-2b6828110",
                "Website: https://jonas.dev/work",
                "GitLab: https://gitlab.com/sasin91",
                "Website: https://example.com/profile;view=full",
                "Website: https://example.com/wiki/PHP_(language)",
                "Website: https://example.com/a?b=1,2",
            ]
        );
    }
}
