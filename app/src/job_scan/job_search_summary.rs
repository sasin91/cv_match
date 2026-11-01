//! The top of a job search's page (templates/job_scan/search.html): what it looks for, and how
//! its last scan went, as recruiter's Job_search_summary shapes it.

use super::job_scan_model::{JobSearch, Run, TokenUse};
use crate::matches::dates;

/// How a scan stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunState {
    Running,
    Error,
    Done,
}

/// The last scan as the page prints it.
#[derive(Debug, Clone, PartialEq)]
pub struct LastScan {
    pub when: String,
    pub state: RunState,
    /// "running…", the run's error, or "found 12, 3 new."
    pub text: String,
    /// "Scans in the last 30 days: 4, 52,301 tokens."
    pub usage: String,
}

impl LastScan {
    pub fn is_error(&self) -> bool {
        self.state == RunState::Error
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobSearchSummary {
    /// "php developer · Odense · remote too · on jobnet.dk · tailors from 60 · last scanned 21 Oct 2026 08:12".
    pub line: String,
    pub last_scan: Option<LastScan>,
}

impl JobSearchSummary {
    /// scan_running: a scan runs now (scan_service::is_running), so an unfinished run is running;
    /// otherwise it stopped without saying so.
    pub fn from(search: &JobSearch, last_run: Option<&Run>, tokens: TokenUse, scan_running: bool) -> Self {
        JobSearchSummary {
            line: line(search),
            last_scan: last_run.map(|run| last_scan(run, tokens, scan_running)),
        }
    }
}

fn line(search: &JobSearch) -> String {
    let sites = if search.sites.is_empty() {
        String::new()
    } else {
        format!("on {}", search.sites.join(", "))
    };
    let scanned = match search.last_scanned_at {
        Some(at) => format!("last scanned {}", dates::moment(at)),
        None => "not scanned yet".to_string(),
    };
    [
        search.keywords.clone(),
        search.location.clone(),
        if search.remote_ok {
            "remote too".to_string()
        } else {
            String::new()
        },
        sites,
        format!("tailors from {}", search.tailor_from_score),
        scanned,
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// A run's state: running while unfinished and a scan runs, an error when it failed or stopped.
pub fn run_state(run: &Run, scan_running: bool) -> RunState {
    match run.finished_at {
        None if scan_running => RunState::Running,
        None => RunState::Error,
        Some(_) if !run.error.is_empty() => RunState::Error,
        Some(_) => RunState::Done,
    }
}

/// "running…", the run's error, or "found 12, 3 new."
pub fn run_text(run: &Run, scan_running: bool) -> String {
    match run_state(run, scan_running) {
        RunState::Running => "running…".to_string(),
        RunState::Error if run.finished_at.is_none() => "it stopped before it finished.".to_string(),
        RunState::Error => run.error.clone(),
        RunState::Done => format!("found {}, {} new.", run.posts_found, run.posts_new),
    }
}

fn last_scan(run: &Run, tokens: TokenUse, scan_running: bool) -> LastScan {
    LastScan {
        when: dates::short_moment(run.started_at),
        state: run_state(run, scan_running),
        text: run_text(run, scan_running),
        usage: format!(
            "Scans in the last 30 days: {}, {} tokens.",
            tokens.scans,
            thousands(tokens.input_tokens + tokens.output_tokens)
        ),
    }
}

/// 52301 as "52,301".
fn thousands(number: i64) -> String {
    let digits = number.abs().to_string();
    let mut grouped = String::new();
    for (n, digit) in digits.chars().enumerate() {
        if n > 0 && (digits.len() - n).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if number < 0 { format!("-{grouped}") } else { grouped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(finished_at: Option<i64>, error: &str) -> Run {
        Run {
            id: 1,
            job_search_id: 1,
            started_at: 1000,
            finished_at,
            posts_found: 12,
            posts_new: 3,
            error: error.to_string(),
        }
    }

    #[test]
    fn a_run_says_how_it_went() {
        assert_eq!(run_text(&run(Some(1100), ""), false), "found 12, 3 new.");
        assert_eq!(
            run_text(&run(Some(1100), "The scan failed: no."), false),
            "The scan failed: no."
        );
        assert_eq!(run_text(&run(None, ""), true), "running…");
        assert_eq!(run_state(&run(None, ""), false), RunState::Error);
        assert_eq!(run_text(&run(None, ""), false), "it stopped before it finished.");
    }

    #[test]
    fn token_counts_are_grouped() {
        assert_eq!(thousands(52_301), "52,301");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn the_line_says_what_the_search_looks_for() {
        let search = JobSearch {
            keywords: "php developer".to_string(),
            location: "Odense".to_string(),
            remote_ok: true,
            tailor_from_score: 60,
            sites: vec!["jobnet.dk".to_string()],
            ..JobSearch::default()
        };
        assert_eq!(
            line(&search),
            "php developer · Odense · remote too · on jobnet.dk · tailors from 60 · not scanned yet"
        );
    }
}
