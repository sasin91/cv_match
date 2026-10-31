//! A post a job search found as a card of the search's list (templates/job_scan/posts.html):
//! what it is, how it matched, and its buttons, as recruiter's Found_post_card shapes it.

use super::job_scan_model::{DISMISSED, FAILED, FoundPost, MATCHED, NOT_A_JOB};
use crate::matches::dates;
use crate::matches::match_score::MatchScore;

/// The statuses as the card names them.
const STATUS_NAMES: [(&str, &str); 6] = [
    ("new", "Waiting to be matched"),
    ("matching", "Matching"),
    (MATCHED, "Matched"),
    (NOT_A_JOB, "Not a job post"),
    (FAILED, "Failed"),
    (DISMISSED, "Dismissed"),
];

/// A link to the post's saved match.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchLink {
    pub url: String,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoundPostCard {
    pub id: i64,
    pub url: String,
    /// The post's title, or its link when the scan found no title.
    pub heading: String,
    /// "Nordisk Fragt · Odense · found 21 Oct 08:12".
    pub meta: String,
    /// The score once the post is matched.
    pub score: Option<MatchScore>,
    pub status: String,
    pub status_name: String,
    /// Why matching failed; a dismissed post keeps quiet about it.
    pub error: String,
    /// "Likely disqualifier: Located close to Copenhagen" for a missed location, else "".
    pub disqualifier: String,
    pub match_link: Option<MatchLink>,
    pub can_retry: bool,
    /// Dismiss, or Bring back for a dismissed post.
    pub dismiss_label: &'static str,
    /// The dismiss form's undo field: "1" to bring the post back.
    pub dismiss_undo: &'static str,
}

impl FoundPostCard {
    pub fn from(post: &FoundPost) -> Self {
        let dismissed = post.status == DISMISSED;
        FoundPostCard {
            id: post.id,
            url: post.url.clone(),
            heading: if post.title.is_empty() {
                post.url.clone()
            } else {
                post.title.clone()
            },
            meta: meta_line(post),
            score: shown_score(post),
            status: post.status.clone(),
            status_name: status_name(&post.status).to_string(),
            error: if dismissed { String::new() } else { post.error.clone() },
            disqualifier: post
                .location_missed
                .as_ref()
                .map(|missed| format!("Likely disqualifier: {missed}"))
                .unwrap_or_default(),
            match_link: post.match_id.map(|id| MatchLink {
                url: format!("match/{id}"),
                label: match_label(post),
            }),
            can_retry: post.status == FAILED || post.status == NOT_A_JOB,
            dismiss_label: if dismissed { "Bring back" } else { "Dismiss" },
            dismiss_undo: if dismissed { "1" } else { "" },
        }
    }
}

fn status_name(status: &str) -> &str {
    STATUS_NAMES
        .iter()
        .find(|(name, _)| *name == status)
        .map_or(status, |(_, shown)| *shown)
}

fn meta_line(post: &FoundPost) -> String {
    [
        post.company.clone(),
        post.location.clone(),
        format!("found {}", dates::short_moment(post.found_at)),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

fn shown_score(post: &FoundPost) -> Option<MatchScore> {
    if post.status != MATCHED {
        return None;
    }
    MatchScore::from(post.score, post.tag.as_deref().unwrap_or(""))
}

/// What is ready to open: the match, and the tailored résumé and application when written.
fn match_label(post: &FoundPost) -> &'static str {
    match (post.has_resume, post.has_application) {
        (true, true) => "Open the match, tailored résumé and application",
        (true, false) => "Open the match and tailored résumé",
        (false, true) => "Open the match and application",
        (false, false) => "Open the match",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(status: &str) -> FoundPost {
        FoundPost {
            id: 7,
            url: "https://jobnet.dk/7".to_string(),
            title: String::new(),
            company: "Nordisk Fragt".to_string(),
            location: String::new(),
            status: status.to_string(),
            error: "It broke.".to_string(),
            found_at: 0,
            match_id: Some(4),
            score: Some(0.814),
            tag: Some("good".to_string()),
            has_application: true,
            has_resume: true,
            location_missed: Some("Located close to Copenhagen".to_string()),
        }
    }

    #[test]
    fn a_matched_post_shows_its_score_and_whats_written() {
        let card = FoundPostCard::from(&post(MATCHED));
        assert_eq!(card.heading, "https://jobnet.dk/7");
        assert_eq!(card.score.map(|score| score.percent), Some(81));
        assert_eq!(
            card.match_link.map(|link| (link.url, link.label)),
            Some(("match/4".to_string(), "Open the match, tailored résumé and application"))
        );
        assert_eq!(card.disqualifier, "Likely disqualifier: Located close to Copenhagen");
        assert!(!card.can_retry);
        assert_eq!(card.dismiss_label, "Dismiss");
    }

    #[test]
    fn a_dismissed_post_can_come_back_and_keeps_quiet() {
        let card = FoundPostCard::from(&post(DISMISSED));
        assert_eq!(card.error, "");
        assert_eq!(card.score, None);
        assert_eq!((card.dismiss_label, card.dismiss_undo), ("Bring back", "1"));
        assert_eq!(card.status_name, "Dismissed");
        assert!(FoundPostCard::from(&post(FAILED)).can_retry);
    }
}
