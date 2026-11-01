//! A job search as a card of the list of searches (templates/job_scan/index.html), as
//! recruiter's Job_search_card shapes it.

use super::job_scan_model::ListedSearch;
use crate::matches::dates;

/// A chip beside a search's name.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub label: &'static str,
    pub class: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobSearchCard {
    pub id: i64,
    pub name: String,
    pub chip: Option<Chip>,
    /// "php developer · Odense · 12 found, 4 matched · last scanned 21 Oct 08:12".
    pub meta: String,
}

impl JobSearchCard {
    pub fn from(search: &ListedSearch) -> Self {
        JobSearchCard {
            id: search.id,
            name: search.name.clone(),
            chip: chip(search),
            meta: meta_line(search),
        }
    }
}

/// Off, Paused (scans stopped, e.g. a rejected key), or none while it runs.
fn chip(search: &ListedSearch) -> Option<Chip> {
    if !search.active {
        return Some(Chip {
            label: "Off",
            class: "",
        });
    }
    (!search.paused_reason.is_empty()).then_some(Chip {
        label: "Paused",
        class: "warn",
    })
}

fn meta_line(search: &ListedSearch) -> String {
    let scanned = match search.last_scanned_at {
        Some(at) => format!("last scanned {}", dates::short_moment(at)),
        None => "not scanned yet".to_string(),
    };
    let counts = format!("{} found, {} matched", search.posts, search.matched);
    [search.keywords.clone(), search.location.clone(), counts, scanned]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_says_what_the_search_looks_for_and_found() {
        let search = ListedSearch {
            id: 3,
            name: "Backend".to_string(),
            keywords: "php developer".to_string(),
            location: String::new(),
            active: true,
            paused_reason: String::new(),
            last_scanned_at: None,
            posts: 12,
            matched: 4,
        };
        let card = JobSearchCard::from(&search);
        assert_eq!(card.meta, "php developer · 12 found, 4 matched · not scanned yet");
        assert_eq!(card.chip, None);
        let paused = ListedSearch {
            paused_reason: "No key.".to_string(),
            ..search.clone()
        };
        assert_eq!(JobSearchCard::from(&paused).chip.map(|chip| chip.label), Some("Paused"));
        let off = ListedSearch {
            active: false,
            ..search
        };
        assert_eq!(JobSearchCard::from(&off).chip.map(|chip| chip.label), Some("Off"));
    }
}
