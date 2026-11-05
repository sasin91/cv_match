//! The work JobFinder runs now, for the task manager: each job scan, each post being matched,
//! each document being written, each call to a language model, each headless browser and each
//! PDF read in a child process, and the model downloads. Every piece runs through one code path,
//! run() and part(), which lists it while it runs and lets it be stopped.
//!
//! The list lives in the Context (no static state): a piece started inside another one (a post's
//! model call inside the scan that found the post) finds the list, and the piece it is part of,
//! in the task's crate::task_scope, the one task-local it shares with llm::transcript's
//! recording; so the calls deep inside the match need no extra parameter. Work that goes on in a
//! task of its own takes both along with task_scope::carried(). Outside a piece of work (tests, the read-pdf child) part()
//! runs its work as it is, unlisted.
//!
//! Pausing a piece holds it and every piece under it at its next step: what it is in the middle
//! of (a model's answer, a page being read) finishes, and nothing new starts under it until it is
//! resumed. A piece started under a paused one waits before it starts (track), work that goes on
//! step by step waits between its steps (until_resumed: a scan between two posts, a download
//! between two chunks), and the time limits work keeps (Clock, timeout_unpaused) don't count the
//! time it was paused, so a paused post isn't failed for taking too long.
//!
//! Stopping a piece asks it and every piece under it to stop. Work that only waits on something
//! (a model's answer, a child process) is dropped at once, which ends its HTTP request or its
//! process (kill_on_drop). Work that keeps the books (a scan, a post's match) is given its
//! grace (WorkKind::grace) to finish them: the pieces under it fail, so the post is marked failed and the scan
//! says it was stopped, rather than leaving a post claimed for half an hour.
//!
//! run(), part(), within() and spawn() (and task_scope's wrappers) put the work they are given on the heap
//! (Box::pin) before anything else: the future they answer holds a pointer to it, not the work
//! itself. Otherwise each future would hold the whole work it wraps, the scan's ten or so levels
//! (scan, search, post, match, model call) would add up to one future of tens of KB, and a debug
//! build's poll of it took more than a tokio worker's 2 MiB stack (Start scan crashed).

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use crate::task_scope;

/// How long the innermost work that keeps the books (a document being written) may take to finish
/// once it was asked to stop; it is dropped after. Work around it gets one STOP_GRACE more for
/// each level it holds (WorkKind::grace), so its own books get done after the inner's.
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
    /// llama.cpp, whisper.cpp or one of their models being downloaded.
    Download,
    /// A recording being turned into text by whisper-cli.
    Transcribing,
}

impl WorkKind {
    /// Whether the task manager offers to pause it: work that goes on in steps. A model call, a
    /// browser or a PDF read is one step, which finishes anyway; pause the work it is part of.
    pub fn pausable(self) -> bool {
        matches!(
            self,
            WorkKind::Scan | WorkKind::Matching | WorkKind::Writing | WorkKind::Download
        )
    }

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
            WorkKind::Transcribing => "Transcribing",
        }
    }

    /// How long the work may take to finish its books when asked to stop, or None when it is
    /// dropped at once. A scan holds a post's match, which holds the writing of its documents:
    /// each outer one waits out the inner one's grace, and then does its own books.
    fn grace(self) -> Option<Duration> {
        let levels = match self {
            WorkKind::Writing => 1,
            WorkKind::Matching => 2,
            WorkKind::Scan => 3,
            _ => return None,
        };
        Some(STOP_GRACE * levels)
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
    /// Paused: holds at its next step until resumed.
    pub paused: bool,
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
    /// true while it is paused.
    pause: watch::Sender<bool>,
    /// Since when it is paused, while it is.
    paused_since: Option<Instant>,
    /// How long it was paused before, all told.
    paused_before: Duration,
}

impl Entry {
    /// Pauses it, or resumes it.
    fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_since) {
            (true, None) => self.paused_since = Some(Instant::now()),
            (false, Some(since)) => {
                self.paused_before += since.elapsed();
                self.paused_since = None;
            }
            _ => {}
        }
        self.pause.send_replace(paused);
    }

    /// How long it was paused, all told, now included.
    fn paused_for(&self) -> Duration {
        self.paused_before + self.paused_since.map_or(Duration::ZERO, |since| since.elapsed())
    }
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

/// The list, and the piece of work a task runs, inside run(), part() or within(); kept in the
/// task's crate::task_scope.
#[derive(Clone)]
pub struct WorkPlace {
    list: Arc<RunningWork>,
    /// None inside within(): work started there goes at the top of the list.
    piece: Option<Piece>,
}

#[derive(Clone)]
struct Piece {
    id: WorkId,
    stop: watch::Receiver<bool>,
    pause: watch::Receiver<bool>,
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
    pub fn run<F: Future>(
        self: &Arc<Self>,
        kind: WorkKind,
        what: impl Into<String>,
        work: F,
    ) -> impl Future<Output = Result<F::Output, Stopped>> {
        let list = Arc::clone(self);
        let what = what.into();
        let work = Box::pin(work);
        async move {
            let parent = this_place(|place| {
                Arc::ptr_eq(&place.list, &list)
                    .then(|| place.piece.as_ref().map(|piece| piece.id))
                    .flatten()
            })
            .flatten();
            track(list, parent, kind, what, work).await
        }
    }

    /// Runs work with this list as the one the pieces started in it go in, at the top: for work a
    /// window or a page asks for, whose model calls are listed then.
    pub fn within<F: Future>(self: &Arc<Self>, work: F) -> impl Future<Output = F::Output> + use<F> {
        let place = WorkPlace {
            list: Arc::clone(self),
            piece: None,
        };
        task_scope::with_work(place, work)
    }

    /// Runs work as a task of its own (tokio::spawn) within this list: work a page or a window
    /// starts and leaves running, whose model calls are listed then.
    pub fn spawn<F>(self: &Arc<Self>, work: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(self.within(work))
    }

    /// The pieces running now, each followed by the pieces under it, oldest first.
    pub fn rows(&self) -> Vec<WorkRow> {
        let entries = self.lock();
        let mut rows = Vec::with_capacity(entries.rows.len());
        // A piece whose parent ended already (a task of its own, task_scope::carried()) shows at the top.
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

    /// Pauses the piece id and every piece under it, or resumes them. False when it isn't
    /// running (it ended meanwhile).
    pub fn pause(&self, id: WorkId, paused: bool) -> bool {
        let mut entries = self.lock();
        if !entries.rows.contains_key(&id) {
            return false;
        }
        let mut asked = vec![id];
        while let Some(next) = asked.pop() {
            asked.extend(children(&entries, next).collect::<Vec<_>>());
            if let Some(entry) = entries.rows.get_mut(&next) {
                entry.set_paused(paused);
            }
        }
        true
    }

    /// How long the piece id was paused, all told; zero once it ended.
    fn paused_for(&self, id: WorkId) -> Duration {
        self.lock().rows.get(&id).map_or(Duration::ZERO, Entry::paused_for)
    }

    /// Whether a piece of kind runs now.
    pub fn runs(&self, kind: WorkKind) -> bool {
        self.lock().rows.values().any(|entry| entry.kind == kind)
    }

    /// Lists a new piece, already asked to stop when the piece it is under is, and paused when
    /// that one is.
    fn add(&self, parent: Option<WorkId>, kind: WorkKind, what: String) -> Piece {
        let mut entries = self.lock();
        let parent_entry = parent.and_then(|parent| entries.rows.get(&parent));
        let stopping = parent_entry.is_some_and(|entry| *entry.stop.borrow());
        let paused = parent_entry.is_some_and(|entry| *entry.pause.borrow());
        let (stop, asked) = watch::channel(stopping);
        let (pause, held) = watch::channel(false);
        let id = entries.next_id;
        entries.next_id += 1;
        let mut entry = Entry {
            parent,
            kind,
            what,
            started_at: chrono::Utc::now().timestamp(),
            started: Instant::now(),
            process_id: None,
            stop,
            pause,
            paused_since: None,
            paused_before: Duration::ZERO,
        };
        entry.set_paused(paused);
        entries.rows.insert(id, entry);
        Piece {
            id,
            stop: asked,
            pause: held,
        }
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
        paused: *entry.pause.borrow(),
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
    let piece = list.add(parent, kind, what);
    let _listed = Listed {
        list: &list,
        id: piece.id,
    };
    let mut asked = piece.stop.clone();
    // Started under a paused piece: it waits for that one to be resumed before it starts.
    if !wait_while_paused(&piece).await {
        return Err(Stopped);
    }
    let place = WorkPlace {
        list: Arc::clone(&list),
        piece: Some(piece),
    };
    // On the heap: the futures of work listed inside work listed (a model call in a post's match
    // in a scan) would otherwise nest too deep for the compiler to lay them out.
    let mut work = Box::pin(task_scope::with_work(place, work));
    tokio::select! {
        biased;
        done = &mut work => return Ok(done),
        () = asked_to_stop(&mut asked) => {}
    }
    let Some(grace) = kind.grace() else {
        return Err(Stopped);
    };
    tokio::time::timeout(grace, work).await.map_err(|_| Stopped)
}

/// Waits until asked says stop; forever when it can't any more.
async fn asked_to_stop(asked: &mut watch::Receiver<bool>) {
    if asked.wait_for(|stop| *stop).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Waits while piece is paused. False when it was asked to stop meanwhile (or before).
async fn wait_while_paused(piece: &Piece) -> bool {
    let mut held = piece.pause.clone();
    let mut asked = piece.stop.clone();
    tokio::select! {
        biased;
        () = asked_to_stop(&mut asked) => false,
        // An Err is the piece's entry gone, so nothing holds it any more.
        _ = held.wait_for(|paused| !*paused) => !*asked.borrow(),
    }
}

/// Runs work as a piece of the list this task's work is in, under the piece it runs. Outside a
/// list (tests, the read-pdf child) it runs work as it is. Under a paused piece work waits before
/// it is first polled, so a time limit for it goes inside it (an async block), not around it.
pub fn part<F: Future>(
    kind: WorkKind,
    what: impl Into<String>,
    work: F,
) -> impl Future<Output = Result<F::Output, Stopped>> {
    let what = what.into();
    let work = Box::pin(work);
    async move {
        let Some(place) = this_place(WorkPlace::clone) else {
            return Ok(work.await);
        };
        let parent = place.piece.map(|piece| piece.id);
        track(place.list, parent, kind, what, work).await
    }
}

/// What f answers about the list and the piece this task runs; None outside one.
fn this_place<R>(f: impl FnOnce(&WorkPlace) -> R) -> Option<R> {
    task_scope::read(|scope| scope.work.as_ref().map(f)).flatten()
}

/// Whether the piece this task runs was asked to stop.
pub fn stop_asked() -> bool {
    this_place(|place| place.piece.as_ref().is_some_and(|piece| *piece.stop.borrow())).unwrap_or(false)
}

/// Waits until the piece this task runs is asked to stop; forever outside one. For work that
/// waits on its own between steps (a scan's pause between two posts).
pub async fn until_stop_asked() {
    let asked = this_place(|place| place.piece.as_ref().map(|piece| piece.stop.clone())).flatten();
    match asked {
        Some(mut asked) => asked_to_stop(&mut asked).await,
        None => std::future::pending().await,
    }
}

/// Asks the piece this task runs, and the pieces under it, to stop, as the task manager would.
pub fn stop_this() {
    this_place(|place| {
        if let Some(piece) = &place.piece {
            place.list.stop(piece.id);
        }
    });
}

/// Whether the piece this task runs is paused.
pub fn paused() -> bool {
    this_place(|place| place.piece.as_ref().is_some_and(|piece| *piece.pause.borrow())).unwrap_or(false)
}

/// Waits while the piece this task runs is paused, until it is resumed or asked to stop; at once
/// when it isn't paused (and outside one). For work that goes on step by step, between its steps.
pub async fn until_resumed() {
    let piece = this_place(|place| place.piece.clone()).flatten();
    if let Some(piece) = piece {
        wait_while_paused(&piece).await;
    }
}

/// Pauses the piece this task runs, and the pieces under it, or resumes them, as the task
/// manager would.
pub fn pause_this(paused: bool) {
    this_place(|place| {
        if let Some(piece) = &place.piece {
            place.list.pause(piece.id, paused);
        }
    });
}

/// How long the piece this task runs was paused, all told; zero outside one.
fn paused_for() -> Duration {
    this_place(|place| place.piece.as_ref().map(|piece| place.list.paused_for(piece.id)))
        .flatten()
        .unwrap_or(Duration::ZERO)
}

/// A stopwatch for the piece this task runs that stands still while the piece is paused: for the
/// time limits work keeps, so a pause doesn't use them up.
pub struct Clock {
    began: Instant,
    paused_before: Duration,
}

impl Clock {
    /// Starts it now, in the piece this task runs.
    pub fn start() -> Self {
        Clock {
            began: Instant::now(),
            paused_before: paused_for(),
        }
    }

    /// How long the piece ran since start(), the time it was paused left out.
    pub fn ran(&self) -> Duration {
        let paused = paused_for().saturating_sub(self.paused_before);
        self.began.elapsed().saturating_sub(paused)
    }
}

/// The work ran past its time limit (timeout_unpaused).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedOut;

/// Runs work for at most limit, as tokio::time::timeout does, but the time the piece this task
/// runs is paused doesn't count: work is polled all along (what it is in the middle of finishes),
/// and the limit is looked at again whenever it would run out.
pub async fn timeout_unpaused<F: Future>(limit: Duration, work: F) -> Result<F::Output, TimedOut> {
    let clock = Clock::start();
    let mut work = Box::pin(work);
    loop {
        let left = limit.saturating_sub(clock.ran());
        if left.is_zero() {
            return Err(TimedOut);
        }
        tokio::select! {
            biased;
            done = &mut work => return Ok(done),
            () = tokio::time::sleep(left) => {}
        }
    }
}

/// Says the child process the piece this task runs started (its process id), for the list.
pub fn set_process(process_id: Option<u32>) {
    this_place(|place| {
        if let Some(piece) = &place.piece {
            place.list.set_process(piece.id, process_id);
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

    #[test]
    fn outer_work_waits_out_the_inner_works_grace_before_its_own_runs_out() {
        let grace = |kind: WorkKind| kind.grace().unwrap();
        assert!(grace(WorkKind::Matching) > grace(WorkKind::Writing));
        assert!(grace(WorkKind::Scan) > grace(WorkKind::Matching));
        assert_eq!(WorkKind::ModelCall.grace(), None);
    }

    #[tokio::test]
    async fn a_paused_piece_starts_nothing_new_under_it_until_resumed() {
        let list = Arc::new(RunningWork::default());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let (ran_tx, mut ran_rx) = tokio::sync::oneshot::channel();
        let running = Arc::clone(&list);
        let scan = tokio::spawn(async move {
            running
                .run(WorkKind::Scan, "Backend", async move {
                    let _ = seen_tx.send(());
                    let _ = go_rx.await;
                    part(WorkKind::ModelCall, "model", async move {
                        let _ = ran_tx.send(());
                    })
                    .await
                })
                .await
        });
        seen_rx.await.unwrap();
        let top = list.rows()[0].id;
        assert!(list.pause(top, true));
        go_tx.send(()).unwrap();
        // The call is listed, paused with its scan, and waits.
        while list.rows().len() < 2 {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ran_rx.try_recv().is_err(), "it waits while paused");
        assert!(list.rows().iter().all(|row| row.paused));
        assert!(list.pause(top, false));
        assert_eq!(scan.await.unwrap(), Ok(Ok(())));
        assert!(ran_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn a_paused_piece_can_still_be_stopped() {
        let list = Arc::new(RunningWork::default());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let running = Arc::clone(&list);
        let download = tokio::spawn(async move {
            running
                .run(WorkKind::Download, "model.gguf", async move {
                    pause_this(true);
                    let _ = seen_tx.send(());
                    until_resumed().await;
                    stop_asked()
                })
                .await
        });
        seen_rx.await.unwrap();
        assert!(list.rows()[0].paused);
        assert!(list.stop(list.rows()[0].id));
        // Its wait ends on the stop: it ends seeing it asked, or is dropped.
        let ended = download.await.unwrap();
        assert!(matches!(ended, Ok(true) | Err(Stopped)), "{ended:?}");
    }

    #[tokio::test]
    async fn time_paused_doesnt_count_toward_a_time_limit() {
        /// The limit the work keeps.
        const LIMIT: Duration = Duration::from_millis(200);
        /// How long it stays paused: longer than the limit.
        const PAUSED: Duration = Duration::from_millis(400);
        let list = Arc::new(RunningWork::default());
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        let running = Arc::clone(&list);
        let matching = tokio::spawn(async move {
            running
                .run(WorkKind::Matching, "a post", async move {
                    pause_this(true);
                    let _ = seen_tx.send(());
                    let clock = Clock::start();
                    let done = timeout_unpaused(LIMIT, until_resumed()).await;
                    (done, clock.ran())
                })
                .await
        });
        seen_rx.await.unwrap();
        tokio::time::sleep(PAUSED).await;
        assert!(list.pause(list.rows()[0].id, false));
        let (done, ran) = matching.await.unwrap().unwrap();
        assert_eq!(done, Ok(()));
        assert!(ran < LIMIT, "the pause isn't counted: {ran:?}");
        let ran_out = timeout_unpaused(Duration::from_millis(10), std::future::pending::<()>()).await;
        assert_eq!(ran_out, Err(TimedOut));
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
