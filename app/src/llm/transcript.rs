//! What the model was asked and answered while a piece of work ran, for the job scan's timeline
//! (job_scan::run_steps): every structured() and web_search() call inside recording() is kept,
//! with how long it took.
//!
//! The recording is kept in the task's crate::task_scope, so the language model calls deep inside
//! the match (match_prompts, ai_match, document_writer) need no extra parameter. It is set only
//! for the future recording() runs, and only that task sees it: two scans, or a scan and a match
//! on the match page, never see each other's calls, and outside recording() nothing is kept. Work
//! that goes on in a task of its own (match_service::write) keeps its calls in the same recording
//! when it is started with task_scope::carried().

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::task_scope;

/// One call to the model: what it was asked and what it answered (or why it failed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Exchange {
    /// The instructions and the question, as sent.
    pub asked: String,
    /// The answer as JSON text, or the error.
    pub answered: String,
    pub took_ms: i64,
}

/// Where a recording() keeps its calls: shared with the tasks its work carries it into.
pub type Recording = Arc<Mutex<Vec<Exchange>>>;

/// Runs work, and keeps every call to the model it makes.
pub fn recording<F: Future>(work: F) -> impl Future<Output = (F::Output, Vec<Exchange>)> {
    let recording = Recording::default();
    let recorded = task_scope::with_recording(Arc::clone(&recording), work);
    async move {
        let output = recorded.await;
        let calls = std::mem::take(&mut *lock(&recording));
        (output, calls)
    }
}

/// Whether this task is recording.
pub fn is_recording() -> bool {
    task_scope::read(|scope| scope.recording.is_some()).unwrap_or(false)
}

/// Adds calls to this task's recording, if it records.
fn keep(calls: Vec<Exchange>) {
    task_scope::read(|scope| {
        if let Some(recording) = &scope.recording {
            lock(recording).extend(calls);
        }
    });
}

/// The recording's calls, also when a task panicked holding them.
fn lock(recording: &Recording) -> std::sync::MutexGuard<'_, Vec<Exchange>> {
    recording.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
            // A task of its own, carried: its calls are kept too.
            tokio::spawn(task_scope::carried(async { ask("two") })).await.unwrap();
            // One not carried isn't recording.
            tokio::spawn(async { ask("unrecorded") }).await.unwrap();
        })
        .await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].asked, "system\n\none");
        assert_eq!(calls[1].answered, "answer to two");
        assert!(!is_recording());
    }
}
