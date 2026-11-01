//! What the model was asked and answered while a piece of work ran, for the job scan's timeline
//! (job_scan::run_steps): every structured() and web_search() call inside recording() is kept,
//! with how long it took.
//!
//! The calls are kept in a tokio task-local, so the language model calls deep inside the match
//! (match_prompts, ai_match, document_writer) need no extra parameter. It is set only for the
//! future recording() runs, and only that task sees it: two scans, or a scan and a match on the
//! match page, never see each other's calls, and outside recording() nothing is kept. Work that
//! goes on in a task of its own (match_service::write) carries it over with carried().

use std::cell::RefCell;
use std::future::Future;
use std::time::Instant;

/// One call to the model: what it was asked and what it answered (or why it failed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exchange {
    /// The instructions and the question, as sent.
    pub asked: String,
    /// The answer as JSON text, or the error.
    pub answered: String,
    pub took_ms: i64,
}

tokio::task_local! {
    /// The calls of the recording() running in this task.
    static CALLS: RefCell<Vec<Exchange>>;
}

/// Runs work, and keeps every call to the model it makes.
pub async fn recording<F: Future>(work: F) -> (F::Output, Vec<Exchange>) {
    CALLS
        .scope(RefCell::new(Vec::new()), async move {
            let output = work.await;
            let calls = CALLS.with(|calls| calls.take());
            (output, calls)
        })
        .await
}

/// Whether this task is recording.
pub fn is_recording() -> bool {
    CALLS.try_with(|_| ()).is_ok()
}

/// work for a task of its own: recorded when this task is recording, its calls handed back
/// with keep() once it is done.
pub async fn carried<F: Future>(recording_now: bool, work: F) -> (F::Output, Vec<Exchange>) {
    if recording_now {
        recording(work).await
    } else {
        (work.await, Vec::new())
    }
}

/// Adds calls made elsewhere (a task of their own) to this task's recording, if it records.
pub fn keep(calls: Vec<Exchange>) {
    let _ = CALLS.try_with(|kept| kept.borrow_mut().extend(calls));
}

/// A call starting now, which call() finishes.
pub struct Call {
    asked: Option<String>,
    started: Instant,
}

/// Starts keeping a call, when this task records: the instructions, then the question.
pub fn call(system: &str, user: &str) -> Call {
    Call {
        asked: is_recording().then(|| format!("{system}\n\n{user}")),
        started: Instant::now(),
    }
}

impl Call {
    /// Keeps the call with its answer (JSON or the error).
    pub fn answered(self, answered: impl FnOnce() -> String) {
        let Some(asked) = self.asked else {
            return;
        };
        let took_ms = i64::try_from(self.started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let exchange = Exchange {
            asked,
            answered: answered(),
            took_ms,
        };
        keep(vec![exchange]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(question: &str) {
        call("system", question).answered(|| format!("answer to {question}"));
    }

    #[tokio::test]
    async fn only_the_work_recorded_keeps_its_calls() {
        ask("before");
        let ((), calls) = recording(async {
            ask("one");
            let ((), inner) = carried(is_recording(), async { ask("two") }).await;
            keep(inner);
        })
        .await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].asked, "system\n\none");
        assert_eq!(calls[1].answered, "answer to two");
        assert!(!is_recording());
        let ((), none) = carried(false, async { ask("unrecorded") }).await;
        assert!(none.is_empty());
    }
}
