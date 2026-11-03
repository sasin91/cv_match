//! What a job scan tells the user (notifications): each good match as it is saved, and how the
//! scan went when it ends.

use super::{Kind, Notice};
use crate::job_scan::scan_service::{STOPPED_SCAN, ScanReport};
use crate::matches::match_model::SavedMatch;
use crate::matches::match_score;
use crate::service_error::{ErrorKind, ServiceError};

/// The window's link to the job searches, where a scan's runs show.
const SEARCHES_LINK: &str = "job_scan";
/// The window's link to the Match screen, where saved jobs that closed show.
const MATCHES_LINK: &str = "match";

/// A post a scan found that matched at or above its search's "Tailor from" score: "Good match:
/// Backend developer", "82%, Acme".
pub fn good_match(saved: &SavedMatch) -> Notice {
    let percent = match_score::percent(saved.score);
    let text = match saved.company.trim() {
        "" => format!("{percent}%"),
        company => format!("{percent}%, {company}"),
    };
    Notice {
        kind: Kind::GoodMatch,
        title: format!("Good match: {}", saved.job_title),
        text,
        link: format!("match/{}", saved.id),
        pops_up: true,
    }
}

/// How a scan went, as told when it ends: nothing for one that didn't run (another was running,
/// or every search was scanned a short while ago) or that was stopped in the task manager (the
/// user knows); a failure pops up, and a finished scan pops up when it found new posts.
pub fn scan_ended(scanned: &Result<ScanReport, ServiceError>) -> Vec<Notice> {
    match scanned {
        Err(e) if e.kind == ErrorKind::TooMany || e.message == STOPPED_SCAN => Vec::new(),
        Err(e) => vec![Notice {
            kind: Kind::ScanFailed,
            title: "The job scan failed".to_string(),
            text: e.message.clone(),
            link: SEARCHES_LINK.to_string(),
            pops_up: true,
        }],
        Ok(report) => {
            let mut notices: Vec<Notice> = finished(report).into_iter().collect();
            notices.extend(jobs_closed(report));
            notices
        }
    }
}

/// "Job scan: 5 new posts", with the scan's summary; None when it scanned and matched nothing
/// and nothing went wrong. A scan that found and matched nothing because each search or post
/// failed is a failed one (all_failed).
fn finished(report: &ScanReport) -> Option<Notice> {
    let did_nothing = report.scanned == 0 && report.matched == 0 && report.problems.is_empty();
    if did_nothing || report.stopped.as_deref() == Some(STOPPED_SCAN) {
        return None;
    }
    if all_failed(report) {
        return Some(Notice {
            kind: Kind::ScanFailed,
            title: "The job scan failed".to_string(),
            text: report.summary(),
            link: SEARCHES_LINK.to_string(),
            pops_up: true,
        });
    }
    let title = match report.posts_new {
        0 => "Job scan: nothing new".to_string(),
        1 => "Job scan: 1 new post".to_string(),
        new => format!("Job scan: {new} new posts"),
    };
    Some(Notice {
        kind: Kind::ScanFinished,
        title,
        text: report.summary(),
        link: SEARCHES_LINK.to_string(),
        pops_up: report.posts_new > 0,
    })
}

/// Whether something went wrong and nothing came of the scan: no post found, none matched.
fn all_failed(report: &ScanReport) -> bool {
    !report.problems.is_empty() && report.posts_found == 0 && report.matched == 0
}

/// "2 saved jobs have closed", when the still-open check after the scan found any.
fn jobs_closed(report: &ScanReport) -> Option<Notice> {
    let checked = report.still_open.as_ref().filter(|checked| checked.closed > 0)?;
    let title = match checked.closed {
        1 => "1 saved job has closed".to_string(),
        closed => format!("{closed} saved jobs have closed"),
    };
    Some(Notice {
        kind: Kind::JobsClosed,
        title,
        text: checked.summary(),
        link: MATCHES_LINK.to_string(),
        pops_up: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matches::still_open::CheckReport;

    fn report(scanned: usize, posts_new: i64) -> ScanReport {
        ScanReport {
            scanned,
            posts_found: posts_new,
            posts_new,
            ..ScanReport::default()
        }
    }

    #[test]
    fn a_scan_with_new_posts_pops_up() {
        let notices = scan_ended(&Ok(report(2, 5)));
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].kind, Kind::ScanFinished);
        assert_eq!(notices[0].title, "Job scan: 5 new posts");
        assert!(notices[0].pops_up);
    }

    #[test]
    fn a_scan_with_nothing_new_is_only_kept_for_the_bell() {
        let notices = scan_ended(&Ok(report(2, 0)));
        assert_eq!(notices[0].title, "Job scan: nothing new");
        assert!(!notices[0].pops_up);
    }

    #[test]
    fn a_scan_that_scanned_nothing_tells_nothing() {
        let skipped = ScanReport {
            skipped: 2,
            ..ScanReport::default()
        };
        assert!(scan_ended(&Ok(skipped)).is_empty());
    }

    #[test]
    fn a_scan_stopped_in_the_task_manager_tells_nothing() {
        let stopped = ScanReport {
            stopped: Some(STOPPED_SCAN.to_string()),
            ..report(1, 3)
        };
        assert!(scan_ended(&Ok(stopped)).is_empty());
        let cut_off = ServiceError::new(ErrorKind::Failed, STOPPED_SCAN);
        assert!(scan_ended(&Err(cut_off)).is_empty());
    }

    #[test]
    fn a_failed_scan_pops_up_but_one_already_running_tells_nothing() {
        let failed = scan_ended(&Err(ServiceError::need_key("No key.")));
        assert_eq!(failed[0].kind, Kind::ScanFailed);
        assert_eq!(failed[0].text, "No key.");
        assert!(failed[0].pops_up);
        let running = ServiceError::new(ErrorKind::TooMany, "A scan is running already.");
        assert!(scan_ended(&Err(running)).is_empty());
    }

    #[test]
    fn a_scan_whose_every_search_failed_pops_up_as_failed() {
        let failed = ScanReport {
            problems: vec!["Backend: no site could be read.".to_string()],
            ..report(1, 0)
        };
        let notices = scan_ended(&Ok(failed));
        assert_eq!(notices[0].kind, Kind::ScanFailed);
        assert!(notices[0].pops_up);
        let partly = ScanReport {
            problems: vec!["Backend: LinkedIn couldn't be read.".to_string()],
            ..report(1, 3)
        };
        assert_eq!(scan_ended(&Ok(partly))[0].kind, Kind::ScanFinished);
    }

    #[test]
    fn saved_jobs_that_closed_are_told_beside_the_scan() {
        let checked = ScanReport {
            still_open: Some(CheckReport {
                read: 4,
                closed: 2,
                ..CheckReport::default()
            }),
            ..report(1, 0)
        };
        let notices = scan_ended(&Ok(checked));
        assert_eq!(notices[1].kind, Kind::JobsClosed);
        assert_eq!(notices[1].title, "2 saved jobs have closed");
        assert_eq!(notices[1].link, "match");
        assert!(!notices[1].pops_up);
    }
}
