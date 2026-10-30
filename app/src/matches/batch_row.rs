//! A match as a row of "Jobs ranked by match", for several job posts matched with one CV,
//! ported from modules/cv_match/Batch_row.php: the score, the job, where the post is from, when
//! it was matched before and what the post asks of applicants, over the result's own body.

use super::match_result::MatchResult;

pub const UNTITLED: &str = "Job post";
pub const NOT_AGAIN: &str = "not matched again";

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyChip {
    pub label: &'static str,
    pub trap: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchRow {
    pub percent: i64,
    pub tag: String,
    pub heading: String,
    /// "jobindex.dk", or '' when the post wasn't read from a link.
    pub host: String,
    pub matched_at: String,
    pub apply_chip: Option<ApplyChip>,
    pub line: String,
}

impl BatchRow {
    /// job_url: where the post was read from.
    pub fn from(result: &MatchResult, job_url: &str) -> Self {
        BatchRow {
            percent: result.score.percent,
            tag: result.score.tag.clone(),
            heading: if result.heading.is_empty() {
                UNTITLED.to_string()
            } else {
                result.heading.clone()
            },
            host: host(job_url),
            matched_at: result.saved_at.clone(),
            apply_chip: result.apply.as_ref().map(|apply| ApplyChip {
                label: if apply.trap {
                    "Special instructions"
                } else {
                    "Apply by email"
                },
                trap: apply.trap,
            }),
            line: if result.saved_at.is_empty() {
                result.parts.clone()
            } else {
                format!("{} · {NOT_AGAIN}", result.parts)
            },
        }
    }
}

/// The host of a URL, as PHP's parse_url($url, PHP_URL_HOST) gives it; '' without one.
fn host(url: &str) -> String {
    let Some((_, rest)) = url.split_once("://") else {
        return String::new();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map(|(_, host)| host).unwrap_or(authority);
    let host = if authority.starts_with('[') {
        authority.split_inclusive(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host.to_string()
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn the_host_of_a_link() {
        assert_eq!(host("https://www.jobindex.dk/job/123?x=1"), "www.jobindex.dk");
        assert_eq!(host("http://user@example.com:8080/a"), "example.com");
        assert_eq!(host("not a url"), "");
    }
}
