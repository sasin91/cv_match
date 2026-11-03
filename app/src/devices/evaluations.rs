//! The job posts paired devices send to be matched with the user's profile (device_service::
//! evaluate), and what became of each, kept while the app runs: a device asks for them again
//! (EVALUATIONS_PATH) until each is done. What a match finds is saved as any other match, so only
//! the last few per device are kept here, and none past a restart.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use device_protocol::{EVALUATION_DONE, EVALUATION_FAILED, EVALUATION_WORKING, Evaluation};

use super::lock;
use crate::matches::match_service::ReadCv;

/// How many of a device's evaluations are kept, the latest; older ones are forgotten.
const KEPT_PER_DEVICE: usize = 20;

/// How many evaluations one device may have working at once: each is a match with the model,
/// so a device can't queue up the computer's afternoon.
pub const WORKING_PER_DEVICE: usize = 3;

/// Most characters of what was sent that an evaluation shows as Evaluation::sent.
const SENT_SHOWN_CHARS: usize = 120;

#[derive(Default)]
pub struct Evaluations {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The number the next evaluation gets: only ever counts up.
    next_id: u64,
    /// Each device's evaluations, the oldest first.
    by_device: HashMap<i64, VecDeque<Evaluation>>,
    /// The profile as read for the last evaluation, used again while its text is the same, so
    /// each post costs no model call to read the CV.
    read_cv: Option<ReadCv>,
}

/// What a finished evaluation came to.
pub struct Outcome {
    pub match_id: Option<i64>,
    pub percent: i64,
    pub tag: String,
    pub tag_name: String,
    pub heading: String,
    /// What to know of the result.
    pub said: String,
}

impl Evaluations {
    /// Starts an evaluation of text for device_id, sent at now; None when the device has
    /// WORKING_PER_DEVICE working already.
    pub fn start(&self, device_id: i64, text: &str, now: i64) -> Option<Evaluation> {
        let mut inner = lock(&self.inner);
        inner.next_id += 1;
        let id = inner.next_id;
        let kept = inner.by_device.entry(device_id).or_default();
        let working = kept.iter().filter(|e| e.state == EVALUATION_WORKING).count();
        if working >= WORKING_PER_DEVICE {
            return None;
        }
        let evaluation = Evaluation {
            id,
            sent: shown_sent(text),
            sent_at: now,
            state: EVALUATION_WORKING.to_string(),
            said: "Waiting to start…".to_string(),
            match_id: None,
            percent: None,
            tag: String::new(),
            tag_name: String::new(),
            heading: String::new(),
        };
        kept.push_back(evaluation.clone());
        // Forget the oldest finished ones past KEPT_PER_DEVICE; working ones stay till done.
        while kept.len() > KEPT_PER_DEVICE {
            let Some(oldest_done) = kept.iter().position(|e| e.state != EVALUATION_WORKING) else {
                break;
            };
            kept.remove(oldest_done);
        }
        Some(evaluation)
    }

    /// Notes the step evaluation id of device_id is at.
    pub fn say(&self, device_id: i64, id: u64, line: &str) {
        self.update(device_id, id, |evaluation| evaluation.said = line.to_string());
    }

    /// Ends evaluation id of device_id: its outcome, or why it failed.
    pub fn finish(&self, device_id: i64, id: u64, outcome: Result<Outcome, String>) {
        self.update(device_id, id, |evaluation| match outcome {
            Ok(outcome) => {
                evaluation.state = EVALUATION_DONE.to_string();
                evaluation.said = outcome.said;
                evaluation.match_id = outcome.match_id;
                evaluation.percent = Some(outcome.percent);
                evaluation.tag = outcome.tag;
                evaluation.tag_name = outcome.tag_name;
                evaluation.heading = outcome.heading;
            }
            Err(why) => {
                evaluation.state = EVALUATION_FAILED.to_string();
                evaluation.said = why;
            }
        });
    }

    /// device_id's evaluations, the latest first.
    pub fn of(&self, device_id: i64) -> Vec<Evaluation> {
        lock(&self.inner)
            .by_device
            .get(&device_id)
            .map(|kept| kept.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    /// Forgets device_id's evaluations: it was removed.
    pub fn forget(&self, device_id: i64) {
        lock(&self.inner).by_device.remove(&device_id);
    }

    /// The profile as read last.
    pub fn read_cv(&self) -> Option<ReadCv> {
        lock(&self.inner).read_cv.clone()
    }

    /// Keeps read as the profile read last.
    pub fn set_read_cv(&self, read: ReadCv) {
        lock(&self.inner).read_cv = Some(read);
    }

    fn update(&self, device_id: i64, id: u64, change: impl FnOnce(&mut Evaluation)) {
        let mut inner = lock(&self.inner);
        let found = inner
            .by_device
            .get_mut(&device_id)
            .and_then(|kept| kept.iter_mut().find(|e| e.id == id));
        if let Some(evaluation) = found {
            change(evaluation);
        }
    }
}

/// What was sent as an evaluation shows it: its first line, at most SENT_SHOWN_CHARS characters.
fn shown_sent(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("").trim();
    let mut shown: String = first.chars().take(SENT_SHOWN_CHARS).collect();
    if first.chars().count() > SENT_SHOWN_CHARS {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done() -> Outcome {
        Outcome {
            match_id: Some(7),
            percent: 81,
            tag: "good".to_string(),
            tag_name: "Good match".to_string(),
            heading: "PHP-udvikler · Firma".to_string(),
            said: "Saved to your matches.".to_string(),
        }
    }

    #[test]
    fn a_device_sees_its_own_evaluations_the_latest_first() {
        let evaluations = Evaluations::default();
        let first = evaluations.start(1, "https://jobnet.dk/1", 10).unwrap();
        let second = evaluations.start(1, "PHP-udvikler\nVi søger…", 11).unwrap();
        evaluations.start(2, "Another device's", 12).unwrap();
        evaluations.say(1, first.id, "Reading the job post…");
        evaluations.finish(1, second.id, Ok(done()));

        let seen = evaluations.of(1);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].sent, "PHP-udvikler");
        assert_eq!(seen[0].state, EVALUATION_DONE);
        assert_eq!(seen[0].match_id, Some(7));
        assert_eq!(seen[1].said, "Reading the job post…");
        assert_eq!(seen[1].state, EVALUATION_WORKING);

        evaluations.forget(1);
        assert!(evaluations.of(1).is_empty());
        assert_eq!(evaluations.of(2).len(), 1);
    }

    #[test]
    fn a_device_has_only_so_many_working_at_once() {
        let evaluations = Evaluations::default();
        let started: Vec<_> = (0..WORKING_PER_DEVICE)
            .map(|n| evaluations.start(1, "post", n as i64).unwrap())
            .collect();
        assert!(evaluations.start(1, "post", 9).is_none());
        evaluations.finish(1, started[0].id, Err("No.".to_string()));
        assert!(evaluations.start(1, "post", 10).is_some());
        assert_eq!(
            evaluations
                .of(1)
                .iter()
                .filter(|e| e.state == EVALUATION_FAILED)
                .count(),
            1
        );
    }

    #[test]
    fn only_the_latest_finished_ones_are_kept() {
        let evaluations = Evaluations::default();
        for n in 0..KEPT_PER_DEVICE + 5 {
            let evaluation = evaluations.start(1, &format!("post {n}"), n as i64).unwrap();
            evaluations.finish(1, evaluation.id, Ok(done()));
        }
        let kept = evaluations.of(1);
        assert_eq!(kept.len(), KEPT_PER_DEVICE);
        assert_eq!(kept[0].sent, format!("post {}", KEPT_PER_DEVICE + 4));
    }

    #[test]
    fn what_was_sent_shows_as_its_first_line_cut_short() {
        assert_eq!(shown_sent("  \n Backend\nmore"), "Backend");
        let long = "x".repeat(SENT_SHOWN_CHARS + 10);
        assert_eq!(shown_sent(&long).chars().count(), SENT_SHOWN_CHARS + 1);
    }
}
