//! The work JobFinder runs now, for the task manager: each job scan, each post being matched,
//! each document being written, each call to a language model, each headless browser and each
//! PDF read in a child process, and the model downloads. Every piece runs through one code path,
//! run() and part(), which lists it while it runs and lets it be stopped.
//!
//! The list lives in the Context (no static state): a piece started inside another one (a post's
//! model call inside the scan that found the post) finds the list, and the piece it is part of,
//! in a tokio task-local, as llm::transcript finds its recording; so the calls deep inside the
//! match need no extra parameter. Outside a piece of work (tests, the read-pdf child) part()
//! runs its work as it is, unlisted.
//!
//! Stopping a piece asks it and every piece under it to stop. Work that only waits on something
//! (a model's answer, a child process) is dropped at once, which ends its HTTP request or its
//! process (kill_on_drop). Work that keeps the books (a scan, a post's match) is given
//! STOP_GRACE to finish them: the pieces under it fail, so the post is marked failed and the scan
//! says it was stopped, rather than leaving a post claimed for half an hour.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

/// How long work that keeps the books (WorkKind::winds_down) may take to finish once it was asked
/// to stop; it is dropped after.
pub const STOP_GRACE: Duration = Duration::from_secs(10);

/// What a piece of work is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    /// A job scan: all its searches, and the posts it finds.
    Scan,
    /// One job post being read, matched and saved.
    Matching,
    /// A tailored résumé or an application being written.
    Writing,
    /// One call to a language model.
    ModelCall,
    /// A headless browser reading a page.
    Browser,
    /// A PDF read in a child process (`jobfinder read-pdf`).
    PdfReader,
    /// llama.cpp or one of its models being downloaded.
    Download,
}

impl WorkKind {
    /// The kind as the task manager's Kind column says it.
    pub fn label(self) -> &'static str {
        match self {
            WorkKind::Scan => "Job scan",
            WorkKind::Matching => "Matching",
            WorkKind::Writing => "Writing",
            WorkKind::ModelCall => "Model call",
            WorkKind::Browser => "Browser",
            WorkKind::PdfReader => "PDF reader",
            WorkKind::Download => "Download",
        }
    }

    /// Whether the work finishes its books when asked to stop (STOP_GRACE), rather than being
    /// dropped at once.
    fn winds_down(self) -> bool {
        matches!(self, WorkKind::Scan | WorkKind::Matching | WorkKind::Writing)
    }
}

/// A piece of work's number in the list, unique for as long as the app runs.
pub type WorkId = u64;

/// A piece of work as the list shows it now.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkRow {
    pub id: WorkId,
    /// The piece it is part of, while that one runs.
    pub parent: Option<WorkId>,
    /// How many pieces it is under (0 at the top).
    pub depth: usize,
    pub kind: WorkKind,
    /// What it works on, in words ("Backend", "https://jobnet.dk/…").
    pub what: String,
    /// When it started, in unix seconds.
    pub started_at: i64,
    pub running_for: Duration,
    /// The child process it runs, when it runs one.
    pub process_id: Option<u32>,
    /// Asked to stop, and finishing.
    pub stopping: bool,
}

/// The work was stopped from the task manager before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

impl Stopped {
    /// What stopped work says, wherever its failure is shown.
    pub const SAID: &'static str = "Stopped in the task manager.";
}

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(Self::SAID)
    }
}

/// One piece in the list.
struct Entry {
    parent: Option<WorkId>,
    kind: WorkKind,
    what: String,
    started_at: i64,
    started: Instant,
    process_id: Option<u32>,
    /// true once it is asked to stop.
    stop: watch::Sender<bool>,
}

#[derive(Default)]
struct Entries {
    /// The number the next piece gets: only ever counts up.
    next_id: WorkId,
    rows: BTreeMap<WorkId, Entry>,
}

/// The work running now. One per Context.
#[derive(Default)]
pub struct RunningWork {
    entries: Mutex<Entries>,
}

tokio::task_local! {
    /// The list, and the piece of work this task runs, inside run(), part() or within().
    static CURRENT: Current;
}

#[derive(Clone)]
struct Current {
    list: Arc<RunningWork>,
    /// None inside within(): work started there goes at the top of the list.
    piece: Option<Piece>,
}

#[derive(Clone)]
struct Piece {
    id: WorkId,
    stop: watch::Receiver<bool>,
}

/// Takes a piece out of the list when its work ends, however it ends (dropped too).
struct Listed<'a> {
    list: &'a RunningWork,
    id: WorkId,
}

impl Drop for Listed<'_> {
    fn drop(&mut self) {
        self.list.lock().rows.remove(&self.id);
    }
}

impl RunningWork {
    fn lock(&self) -> std::sync::MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs work as a piece of the list called what: under the piece this task runs, when it runs
    /// one of this list's, else at the top. Err when it was stopped before it finished.
    pub async fn run<F: Future>(
        self: &Arc<Self>,
        kind: WorkKind,
        what: impl Into<String>,
        work: F,
    ) -> Result<F::Output, Stopped> {
        let parent = CURRENT
            .try_with(|current| {
                Arc::ptr_eq(&current.list, self)
                    .then(|| current.piece.as_ref().map(|piece| piece.id))
                    .flatten()
            })
            .ok()
            .flatten();
        track(Arc::clone(self), parent, kind, what.into(), work).await
    }

    /// Runs work with this list as the one the pieces started in it go in, at the top: for work a
    /// window or a page asks for, whose model calls are listed then.
    pub async fn within<F: Future>(self: &Arc<Self>, work: F) -> F::Output {
        let current = Current {
            list: Arc::clone(self),
            piece: None,
        };
        CURRENT.scope(current, work).await
    }

    /// The pieces running now, each followed by the pieces under it, oldest first.
    pub fn rows(&self) -> Vec<WorkRow> {
        let entries = self.lock();
        let mut rows = Vec::with_capacity(entries.rows.len());
        // A piece whose parent ended already (a task of its own, carried()) shows at the top.
        let tops = entries
            .rows
            .iter()
            .filter(|(_, entry)| entry.parent.is_none_or(|parent| !entries.rows.contains_key(&parent)));
        for (id, _) in tops {
            push_with_children(&entries, *id, 0, &mut rows);
        }
        rows
    }

    /// Asks the piece id, and every piece under it, to stop. False when it isn't running (it
    /// ended meanwhile).
    pub fn stop(&self, id: WorkId) -> bool {
        let entries = self.lock();
        if !entries.rows.contains_key(&id) {
            return false;
        }
        let mut asked = vec![id];
        while let Some(next) = asked.pop() {
            if let Some(entry) = entries.rows.get(&next) {
                entry.stop.send_replace(true);
            }
            asked.extend(children(&entries, next));
        }
        true
    }

    /// Whether a piece of kind runs now.
    pub fn runs(&self, kind: WorkKind) -> bool {
        self.lock().rows.values().any(|entry| entry.kind == kind)
    }

    /// Lists a new piece, already asked to stop when the piece it is under is.
    fn add(&self, parent: Option<WorkId>, kind: WorkKind, what: String) -> (WorkId, watch::Receiver<bool>) {
        let mut entries = self.lock();
        let stopping = parent
            .and_then(|parent| entries.rows.get(&parent))
            .is_some_and(|entry| *entry.stop.borrow());
        let (stop, asked) = watch::channel(stopping);
        let id = entries.next_id;
        entries.next_id += 1;
        entries.rows.insert(
            id,
            Entry {
                parent,
                kind,
                what,
                started_at: chrono::Utc::now().timestamp(),
                started: Instant::now(),
                process_id: None,
                stop,
            },
        );
        (id, asked)
    }

    fn set_process(&self, id: WorkId, process_id: Option<u32>) {
        if let Some(entry) = self.lock().rows.get_mut(&id) {
            entry.process_id = process_id;
        }
    }
}

/// The pieces directly under id.
fn children(entries: &Entries, id: WorkId) -> impl Iterator<Item = WorkId> + '_ {
    entries
        .rows
        .iter()
        .filter(move |(_, entry)| entry.parent == Some(id))
        .map(|(child, _)| *child)
}

/// Adds id as a row at depth, then the pieces under it.
fn push_with_children(entries: &Entries, id: WorkId, depth: usize, rows: &mut Vec<WorkRow>) {
    let Some(entry) = entries.rows.get(&id) else {
        return;
    };
    rows.push(WorkRow {
        id,
        parent: entry.parent,
        depth,
        kind: entry.kind,
        what: entry.what.clone(),
        started_at: entry.started_at,
        running_for: entry.started.elapsed(),
        process_id: entry.process_id,
        stopping: *entry.stop.borrow(),
    });
    for child in children(entries, id) {
        push_with_children(entries, child, depth + 1, rows);
    }
}

/// Lists work under parent while it runs, and stops it when asked (the module's doc says how).
async fn track<F: Future>(
    list: Arc<RunningWork>,
    parent: Option<WorkId>,
    kind: WorkKind,
    what: String,
    work: F,
) -> Result<F::Output, Stopped> {
    let (id, mut asked) = list.add(parent, kind, what);
    let _listed = Listed { list: &list, id };
    let current = Current {
        list: Arc::clone(&list),
        piece: Some(Piece {
            id,
            stop: asked.clone(),
        }),
    };
    // On the heap: the futures of work listed inside work listed (a model call in a post's match
    // in a scan) would otherwise nest too deep for the compiler to lay them out.
    let mut work = Box::pin(CURRENT.scope(current, work));
    tokio::select! {
        biased;
        done = &mut work => return Ok(done),
        () = asked_to_stop(&mut asked) => {}
    }
    if !kind.winds_down() {
        return Err(Stopped);
    }
    tokio::time::timeout(STOP_GRACE, work).await.map_err(|_| Stopped)
}

/// Waits until asked says stop; forever when it can't any more.
async fn asked_to_stop(asked: &mut watch::Receiver<bool>) {
    if asked.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Runs work as a piece of the list this task's work is in, under the piece it runs. Outside a
/// list (tests, the read-pdf child) it runs work as it is.
pub async fn part<F: Future>(kind: WorkKind, what: impl Into<String>, work: F) -> Result<F::Output, Stopped> {
    let Ok(current) = CURRENT.try_with(Current::clone) else {
        return Ok(work.await);
    };
    let parent = current.piece.map(|piece| piece.id);
    track(current.list, parent, kind, what.into(), work).await
}

/// work, for a task of its own (tokio::spawn): in the list, and under the piece, this task's work
/// is in, as part() inside it would see them.
pub fn carried<F: Future>(work: F) -> impl Future<Output = F::Output> {
    let current = CURRENT.try_with(Current::clone).ok();
    async move {
        match current {
            Some(current) => CURRENT.scope(current, work).await,
            None => work.await,
        }
    }
}

/// Whether the piece this task runs was asked to stop.
pub fn stop_asked() -> bool {
    CURRENT
        .try_with(|current| current.piece.as_ref().is_some_and(|piece| *piece.stop.borrow()))
        .unwrap_or(false)
}

/// Waits until the piece this task runs is asked to stop; forever outside one. For work that
/// waits on its own between steps (a scan's pause between two posts).
pub async fn until_stop_asked() {
    let asked = CURRENT
        .try_with(|current| current.piece.as_ref().map(|piece| piece.stop.clone()))
        .ok()
        .flatten();
    match asked {
        Some(mut asked) => asked_to_stop(&mut asked).await,
        None => std::future::pending().await,
    }
}

/// Asks the piece this task runs, and the pieces under it, to stop, as the task manager would.
pub fn stop_this() {
    let _ = CURRENT.try_with(|current| {
        if let Some(piece) = &current.piece {
            current.list.stop(piece.id);
        }
    });
}

/// Says the child process the piece this task runs started (its process id), for the list.
pub fn set_process(process_id: Option<u32>) {
    let _ = CURRENT.try_with(|current| {
        if let Some(piece) = &current.piece {
            current.list.set_process(piece.id, process_id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn work_is_listed_while_it_runs_with_its_parts_under_it() {
        let list = Arc::new(RunningWork::default());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let running = Arc::clone(&list);
        let scan = tokio::spawn(async move {
            running
                .run(WorkKind::Scan, "Backend", async move {
                    part(WorkKind::ModelCall, "gpt-5.4-mini", async move {
                        set_process(Some(42));
                        let _ = seen_tx.send(());
                        let _ = go_rx.await;
                    })
                    .await
                })
                .await
        });
        seen_rx.await.unwrap();
        let rows = list.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].kind, rows[0].depth, rows[0].what.as_str()),
            (WorkKind::Scan, 0, "Backend")
        );
        assert_eq!(
            (rows[1].kind, rows[1].depth, rows[1].parent),
            (WorkKind::ModelCall, 1, Some(rows[0].id))
        );
        assert_eq!(rows[1].process_id, Some(42));
        go_tx.send(()).unwrap();
        assert_eq!(scan.await.unwrap(), Ok(Ok(())));
        assert!(list.rows().is_empty());
    }

    #[tokio::test]
    async fn a_stopped_call_is_dropped_and_the_work_over_it_winds_down() {
        let list = Arc::new(RunningWork::default());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let running = Arc::clone(&list);
        let matching = tokio::spawn(async move {
            running
                .run(WorkKind::Matching, "a post", async move {
                    let _ = seen_tx.send(());
                    let called = part(WorkKind::ModelCall, "model", std::future::pending::<()>()).await;
                    // What a post's match does with a call that was stopped: says so.
                    (called, stop_asked())
                })
                .await
        });
        seen_rx.await.unwrap();
        tokio::task::yield_now().await;
        let top = list.rows()[0].id;
        assert!(list.stop(top));
        assert_eq!(matching.await.unwrap(), Ok((Err(Stopped), true)));
        assert!(list.rows().is_empty());
        assert!(!list.stop(top), "it ended");
    }

    #[tokio::test]
    async fn work_outside_a_list_runs_unlisted() {
        assert_eq!(part(WorkKind::PdfReader, "cv.pdf", async { 7 }).await, Ok(7));
        assert!(!stop_asked());
    }

    #[tokio::test]
    async fn work_started_within_goes_at_the_top() {
        let list = Arc::new(RunningWork::default());
        let rows = list
            .within(part(WorkKind::ModelCall, "model", async { RunningWork::rows(&list) }))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].depth, rows[0].parent), (0, None));
    }
}
