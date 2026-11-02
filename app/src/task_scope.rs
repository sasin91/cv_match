//! What the work a tokio task runs belongs to: its place in the task manager's list
//! (task_manager::running_work) and the recording that keeps its model calls for the job scan's
//! timeline (llm::transcript). One task-local holds both, so the calls deep inside the work need
//! no extra parameter, and work that goes on in a task of its own (tokio::spawn) takes both along
//! with one carried().
//!
//! A task-local, not static state: only the task running the work sees it, so two scans, or a
//! scan and a match on the match page, never see each other's. Outside any (tests, the read-pdf
//! child) both are None.

use std::future::Future;

use crate::llm::transcript::Recording;
use crate::task_manager::running_work::WorkPlace;

tokio::task_local! {
    /// What this task's work belongs to, inside with_work() or with_recording().
    static SCOPE: TaskScope;
}

#[derive(Clone, Default)]
pub struct TaskScope {
    /// The list, and the piece of work this task runs.
    pub work: Option<WorkPlace>,
    /// Where this task's model calls are kept, while it records.
    pub recording: Option<Recording>,
}

/// What f answers about this task's scope; None outside one.
pub fn read<R>(f: impl FnOnce(&TaskScope) -> R) -> Option<R> {
    SCOPE.try_with(f).ok()
}

/// This task's scope (empty outside one).
fn current() -> TaskScope {
    read(TaskScope::clone).unwrap_or_default()
}

/// Runs work in place in the task manager's list, recording as this task records. Read when it
/// is first polled: in the task that runs it.
pub fn with_work<F: Future>(place: WorkPlace, work: F) -> impl Future<Output = F::Output> {
    // On the heap, as running_work's module doc says why.
    let work = Box::pin(work);
    async move {
        let scope = TaskScope {
            work: Some(place),
            ..current()
        };
        SCOPE.scope(scope, work).await
    }
}

/// Runs work keeping its model calls in recording, in this task's place in the list.
pub fn with_recording<F: Future>(recording: Recording, work: F) -> impl Future<Output = F::Output> {
    let work = Box::pin(work);
    async move {
        let scope = TaskScope {
            recording: Some(recording),
            ..current()
        };
        SCOPE.scope(scope, work).await
    }
}

/// work, for a task of its own (tokio::spawn): in the list, under the piece, and in the
/// recording this task's work is in now, as it would be here.
pub fn carried<F: Future>(work: F) -> impl Future<Output = F::Output> {
    let scope = current();
    SCOPE.scope(scope, Box::pin(work))
}
