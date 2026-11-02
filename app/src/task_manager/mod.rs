//! The task manager: what JobFinder runs now and for how long, for the window's Task manager
//! screen, and stopping one of them. It lists the work in running_work (scans, matches, writing,
//! model calls, browsers, PDF readers, downloads) with what the app keeps loaded: llama-server
//! and Laya's model. A scan run by another JobFinder (the schedule's `jobfinder scan`) shows too,
//! read from the scan's lock and the database, and is stopped through scan_lock's stop file.
//!
//! Work that goes on in steps (a scan, a post's match, writing, a download) can be paused too, at
//! the top of the list: it holds at its next step until resumed (running_work says how). A row's
//! link (the post or page it works on) opens in the browser.
//!
//! Later, a priority (how nice the work is to the rest of the computer) fits beside Stop: a
//! Task names what to set it on, a process's priority class goes on its process_id, and the
//! in-app work of a WorkKind would wait on that kind's priority before it starts.

pub mod running_work;

use std::time::Duration;

use self::running_work::{WorkId, WorkKind};
use crate::context::Context;
use crate::job_scan::{scan_lock, scan_service};
use crate::service_error::{ErrorKind, ServiceError};

/// What a row of the task manager is, to stop it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// A piece of running_work.
    Work(WorkId),
    /// The app's own llama-server.
    LocalModelServer,
    /// Laya's model in memory.
    Laya,
    /// A scan another JobFinder runs on this database.
    ScanElsewhere,
}

/// A row as the task manager shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub task: Task,
    /// How many rows it is under (0 at the top).
    pub depth: usize,
    /// "Job scan", "Model call", "Local model".
    pub kind: &'static str,
    /// What it works on, in words.
    pub what: String,
    pub process_id: Option<u32>,
    /// When it started, in unix seconds; None when that isn't known.
    pub started_at: Option<i64>,
    /// What stopping it is called in its menu ("Stop", "Unload"), or None when it can't be.
    pub stop: Option<&'static str>,
    /// Asked to stop, and finishing.
    pub stopping: bool,
    /// Whether it can be paused: work at the top of the list that goes on in steps.
    pub pausable: bool,
    /// Paused, until resumed.
    pub paused: bool,
}

impl TaskRow {
    /// How long it has run at now (unix seconds).
    pub fn running_for(&self, now: i64) -> Option<Duration> {
        self.started_at
            .map(|started_at| Duration::from_secs(u64::try_from(now - started_at).unwrap_or(0)))
    }

    /// The web address in what it works on (a post, a page being read), to open.
    pub fn link(&self) -> Option<&str> {
        link_in(&self.what)
    }
}

/// The first http(s) address in text.
fn link_in(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|word| word.starts_with("https://") || word.starts_with("http://"))
        .map(|word| word.trim_end_matches([',', '.', ')', ';']))
}

/// What the Kind column calls the app's own model server.
const LOCAL_MODEL: &str = "Local model";
/// What the Kind column calls Laya's model.
const LAYA: &str = "Laya";
/// The menu's word for stopping work.
const STOP: &str = "Stop";
/// The menu's word for letting a model go.
const UNLOAD: &str = "Unload";

/// Everything running now, the work first (each piece followed by the pieces under it), then
/// what is kept loaded.
pub async fn rows(context: &Context) -> Vec<TaskRow> {
    let mut rows = Vec::new();
    if let Some(row) = scan_elsewhere(context).await {
        rows.push(row);
    }
    rows.extend(context.running.rows().into_iter().map(|row| TaskRow {
        task: Task::Work(row.id),
        depth: row.depth,
        kind: row.kind.label(),
        what: row.what,
        process_id: row.process_id,
        started_at: Some(row.started_at),
        stop: Some(STOP),
        stopping: row.stopping,
        pausable: row.depth == 0 && row.kind.pausable(),
        paused: row.paused,
    }));
    if let Some(server) = context.llama.as_ref().and_then(|llama| llama.running_process()) {
        rows.push(TaskRow {
            task: Task::LocalModelServer,
            depth: 0,
            kind: LOCAL_MODEL,
            what: format!("llama-server with {}", server.model),
            process_id: server.process_id,
            started_at: Some(server.started_at),
            stop: Some(STOP),
            stopping: false,
            pausable: false,
            paused: false,
        });
    }
    if let Some(loaded_at) = context.laya.as_ref().and_then(|laya| laya.loaded_at()) {
        rows.push(TaskRow {
            task: Task::Laya,
            depth: 0,
            kind: LAYA,
            what: "Laya's model, in memory".to_string(),
            process_id: None,
            started_at: Some(loaded_at),
            stop: Some(UNLOAD),
            stopping: false,
            pausable: false,
            paused: false,
        });
    }
    rows
}

/// The scan another JobFinder runs on this database, when one does: the lock is held, and not by
/// a scan of this app's.
async fn scan_elsewhere(context: &Context) -> Option<TaskRow> {
    if context.running.runs(WorkKind::Scan) {
        return None;
    }
    let running = match scan_service::running_scan(context).await {
        Ok(running) => running?,
        Err(e) => {
            crate::log_line!("task_manager: the running scan couldn't be read: {}", e.message);
            return None;
        }
    };
    let what = if running.search_name.is_empty() {
        "In another JobFinder (the schedule's)".to_string()
    } else {
        format!("{}, in another JobFinder (the schedule's)", running.search_name)
    };
    Some(TaskRow {
        task: Task::ScanElsewhere,
        depth: 0,
        kind: WorkKind::Scan.label(),
        what,
        process_id: None,
        started_at: Some(running.started_at),
        stop: Some(STOP),
        stopping: false,
        pausable: true,
        paused: scan_lock::pause_asked(context),
    })
}

/// Stops task, and says what was done.
pub async fn stop(context: &Context, task: Task) -> Result<String, ServiceError> {
    let said = match task {
        Task::Work(id) => {
            if !context.running.stop(id) {
                return Err(ServiceError::new(ErrorKind::NotFound, "That finished already."));
            }
            "Stopping it. What it was in the middle of is marked as stopped.".to_string()
        }
        Task::LocalModelServer => match context.llama.as_ref() {
            Some(llama) => match llama.stop_now().await {
                Some(model) => format!("llama-server with {model} is stopped. It starts again for the next call."),
                None => "llama-server isn't running.".to_string(),
            },
            None => return Err(ServiceError::not_found()),
        },
        Task::Laya => match context.laya.as_ref() {
            Some(laya) if laya.unload_if_idle(Duration::ZERO) => {
                "Laya's model is let go. It loads again for the next match.".to_string()
            }
            Some(_) => {
                return Err(ServiceError::new(
                    ErrorKind::TooMany,
                    "Laya is answering a match now. Stop that match first, or unload Laya once it is done.",
                ));
            }
            None => return Err(ServiceError::not_found()),
        },
        Task::ScanElsewhere => match scan_lock::ask_to_stop(context) {
            Ok(true) => "The other JobFinder's scan is asked to stop; it stops within a few seconds.".to_string(),
            Ok(false) => return Err(ServiceError::not_found()),
            Err(e) => {
                crate::log_line!("task_manager: the scan's stop file couldn't be written: {e}");
                return Err(ServiceError::internal(
                    "task_manager",
                    format!("The scan couldn't be asked to stop: {e}"),
                ));
            }
        },
    };
    crate::log_line!("task_manager: {said}");
    Ok(said)
}

/// Pauses task (paused) or lets it go on, and says what was done.
pub async fn pause(context: &Context, task: Task, paused: bool) -> Result<String, ServiceError> {
    let said = match task {
        Task::Work(id) => {
            if !context.running.pause(id, paused) {
                return Err(ServiceError::new(ErrorKind::NotFound, "That finished already."));
            }
            if paused {
                "Paused. What it is in the middle of finishes; nothing new starts until you resume it."
            } else {
                "Resumed."
            }
            .to_string()
        }
        Task::ScanElsewhere => match scan_lock::ask_to_pause(context, paused) {
            Ok(true) if paused => {
                "The other JobFinder's scan is asked to pause; it pauses within a few seconds.".to_string()
            }
            Ok(true) => "The other JobFinder's scan is asked to go on.".to_string(),
            Ok(false) => return Err(ServiceError::not_found()),
            Err(e) => {
                crate::log_line!("task_manager: the scan's pause file couldn't be changed: {e}");
                return Err(ServiceError::internal(
                    "task_manager",
                    format!("The scan couldn't be asked to pause or go on: {e}"),
                ));
            }
        },
        Task::LocalModelServer | Task::Laya => {
            return Err(ServiceError::new(
                ErrorKind::Invalid,
                "That can't be paused; stop or unload it.",
            ));
        }
    };
    crate::log_line!("task_manager: {said}");
    Ok(said)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::account::key_vault::KeyVault;

    async fn context() -> Arc<Context> {
        let db = crate::db::open_in_memory().await.unwrap();
        let user_id = crate::server::local_user(&db).await.unwrap();
        let taxonomy = Arc::new(crate::taxonomy::Taxonomy::load());
        Arc::new(Context::new(db, taxonomy, user_id, KeyVault::in_memory()))
    }

    #[tokio::test]
    async fn running_work_shows_and_stops_from_the_task_manager() {
        let context = context().await;
        assert!(rows(&context).await.is_empty());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let list = Arc::clone(&context.running);
        let download = tokio::spawn(async move {
            list.run(WorkKind::Download, "model.gguf", async move {
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .await
        });
        started_rx.await.unwrap();
        let listed = rows(&context).await;
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].kind, listed[0].what.as_str()), ("Download", "model.gguf"));
        assert_eq!(listed[0].stop, Some(STOP));
        stop(&context, listed[0].task).await.unwrap();
        assert_eq!(download.await.unwrap(), Err(running_work::Stopped));
        assert!(rows(&context).await.is_empty());
        let again = stop(&context, listed[0].task).await.unwrap_err();
        assert_eq!(again.kind, ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn work_at_the_top_pauses_and_resumes_from_the_task_manager() {
        let context = context().await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let list = Arc::clone(&context.running);
        let scan = tokio::spawn(async move {
            list.run(WorkKind::Scan, "Backend", async move {
                running_work::part(WorkKind::ModelCall, "model", async move {
                    let _ = started_tx.send(());
                    std::future::pending::<()>().await;
                })
                .await
            })
            .await
        });
        started_rx.await.unwrap();
        let listed = rows(&context).await;
        assert_eq!(listed.iter().map(|row| row.pausable).collect::<Vec<_>>(), [true, false]);
        pause(&context, listed[0].task, true).await.unwrap();
        assert!(rows(&context).await.iter().all(|row| row.paused));
        pause(&context, listed[0].task, false).await.unwrap();
        assert!(rows(&context).await.iter().all(|row| !row.paused));
        let model = pause(&context, Task::Laya, true).await.unwrap_err();
        assert_eq!(model.kind, ErrorKind::Invalid);
        scan.abort();
    }

    #[test]
    fn a_rows_link_is_the_address_in_what_it_works_on() {
        assert_eq!(
            link_in("Reading https://jobnet.dk/job/1, page 2"),
            Some("https://jobnet.dk/job/1")
        );
        assert_eq!(link_in("Every active search"), None);
    }
}
