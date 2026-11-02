//! Whose turn it is on llama-server, which runs one model at a time while each task may want its
//! own (account::task_model): the calls under way and the model they run on, so the server is
//! only switched to another model once they have finished.
//!
//! The rule: a call for the model the server runs joins it at once, unless a switch to another
//! model is waiting; then it queues behind the switch, so a stream of calls for the current
//! model can't keep the switch waiting forever. A switch waits until no call is under way, and
//! while it starts the new model, every other call waits. Switches to different models take
//! their turns as the calls of each finish, in whatever order they wake.
//!
//! Waiting holds no lock that a call under way needs: the state is a std mutex taken only to read
//! and change the counts, and the waiting is on a tokio Notify that every change wakes. A caller
//! that holds a ticket and asks for another model would wait on itself, so a switch waits at
//! most SWITCH_WAIT_SECONDS for the calls under way, then switches anyway (and logs it), as
//! the server did before it waited: the calls on the old model then fail.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Notify;

/// The longest a switch waits for the calls under way, in seconds: an hour, longer than any task's
/// calls on this app's own model take on a slow computer (llm::LOCAL_CALL_SECONDS is a quarter of
/// it), so only a caller waiting on itself reaches it.
const SWITCH_WAIT_SECONDS: u64 = 3_600;

/// The calls, the model they run on, and the switches waiting for them.
#[derive(Debug, Default)]
struct Turns {
    /// The model the server runs, or None when no start has succeeded since the last failed one.
    current: Option<PathBuf>,
    /// Calls holding a ticket.
    under_way: usize,
    /// When the last call ended.
    last_ended: Option<Instant>,
    /// Callers waiting to switch to another model than current.
    switches_waiting: usize,
    /// A switch is starting its model: everyone else waits.
    switching: bool,
}

/// The turns on one llama-server, shared by every caller.
#[derive(Debug, Default)]
pub struct ModelGate {
    turns: std::sync::Mutex<Turns>,
    /// Woken on every change to turns.
    changed: Notify,
}

/// What a caller may do now: join the running model with its ticket, or switch the server to its
/// model (start it), which it then reports with SwitchTurn::started.
pub enum Entry<'a> {
    Join(Ticket),
    Switch(SwitchTurn<'a>),
}

/// One call under way; dropped when it ends.
#[derive(Debug)]
pub struct Ticket {
    gate: Arc<ModelGate>,
}

/// The turn to switch the server to another model; dropped without started, the start failed.
pub struct SwitchTurn<'a> {
    gate: &'a Arc<ModelGate>,
    done: bool,
}

/// A switch counted as waiting, until it is dropped.
struct WaitingSwitch<'a> {
    gate: &'a ModelGate,
}

impl ModelGate {
    fn turns(&self) -> std::sync::MutexGuard<'_, Turns> {
        self.turns.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether no call is under way and none ended in the last idle_for.
    pub fn idle_for(&self, idle_for: Duration) -> bool {
        let turns = self.turns();
        turns.under_way == 0 && turns.last_ended.is_none_or(|ended| ended.elapsed() >= idle_for)
    }

    /// Waits for the turn of a call on wanted (a model file), as the module's rule says.
    pub async fn enter<'a>(self: &'a Arc<Self>, wanted: &Path) -> Entry<'a> {
        let waiting_since = Instant::now();
        let mut waiting: Option<WaitingSwitch<'_>> = None;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // Registered before the state is read, so a change in between still wakes it.
            notified.as_mut().enable();
            {
                let mut turns = self.turns();
                let others_waiting = turns.switches_waiting - usize::from(waiting.is_some());
                let runs_wanted = turns.current.as_deref() == Some(wanted);
                if !turns.switching && runs_wanted && others_waiting == 0 {
                    turns.under_way += 1;
                    drop(turns);
                    drop(waiting);
                    return Entry::Join(self.ticket());
                }
                let waited_too_long = waiting_since.elapsed() >= Duration::from_secs(SWITCH_WAIT_SECONDS);
                if !turns.switching && !runs_wanted && (turns.under_way == 0 || waited_too_long) {
                    if turns.under_way > 0 {
                        crate::log_line!(
                            "llama: switching to {} after waiting {SWITCH_WAIT_SECONDS} s; {} call(s) on {} may fail",
                            name_of(Some(wanted)),
                            turns.under_way,
                            name_of(turns.current.as_deref())
                        );
                    }
                    turns.switching = true;
                    drop(turns);
                    drop(waiting);
                    return Entry::Switch(SwitchTurn {
                        gate: self,
                        done: false,
                    });
                }
                if !runs_wanted && waiting.is_none() {
                    turns.switches_waiting += 1;
                    crate::log_line!(
                        "llama: waiting for {} call(s) on {} to finish before switching to {}",
                        turns.under_way,
                        name_of(turns.current.as_deref()),
                        name_of(Some(wanted))
                    );
                    waiting = Some(WaitingSwitch { gate: self });
                }
            }
            notified.await;
        }
    }

    fn ticket(self: &Arc<Self>) -> Ticket {
        Ticket { gate: Arc::clone(self) }
    }
}

impl SwitchTurn<'_> {
    /// The server now runs model: the switch is over, and the caller's call is under way.
    pub fn started(mut self, model: &Path) -> Ticket {
        self.done = true;
        let mut turns = self.gate.turns();
        turns.current = Some(model.to_path_buf());
        turns.switching = false;
        turns.under_way += 1;
        drop(turns);
        self.gate.changed.notify_waiters();
        self.gate.ticket()
    }
}

impl Drop for SwitchTurn<'_> {
    /// A switch that didn't start: nothing is known to run.
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let mut turns = self.gate.turns();
        turns.current = None;
        turns.switching = false;
        drop(turns);
        self.gate.changed.notify_waiters();
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut turns = self.gate.turns();
        turns.under_way = turns.under_way.saturating_sub(1);
        turns.last_ended = Some(Instant::now());
        drop(turns);
        self.gate.changed.notify_waiters();
    }
}

impl Drop for WaitingSwitch<'_> {
    fn drop(&mut self) {
        let mut turns = self.gate.turns();
        turns.switches_waiting = turns.switches_waiting.saturating_sub(1);
        drop(turns);
        self.gate.changed.notify_waiters();
    }
}

/// A model file's name for the log, "no model" for none.
fn name_of(model: Option<&Path>) -> String {
    model
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "no model".to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// The models each test switches between.
    const GEMMA: &str = "gemma.gguf";
    const QWEN: &str = "qwen.gguf";

    /// How long a stand-in call runs, in milliseconds.
    const CALL_MILLISECONDS: u64 = 50;

    /// The models calls ran on, as each started and ended, and the model "the server" runs.
    #[derive(Default)]
    struct Server {
        running: Mutex<Option<PathBuf>>,
        log: Mutex<Vec<String>>,
    }

    /// One call on model: its turn, then a stand-in for the call that fails when the server isn't
    /// running its model when it starts or ends.
    async fn call(gate: &Arc<ModelGate>, server: &Server, model: &str) -> Result<(), String> {
        let wanted = PathBuf::from(model);
        let ticket = match gate.enter(&wanted).await {
            Entry::Join(ticket) => ticket,
            Entry::Switch(turn) => {
                *server.running.lock().unwrap() = Some(wanted.clone());
                turn.started(&wanted)
            }
        };
        let runs = |when: &str| {
            let running = server.running.lock().unwrap().clone();
            server.log.lock().unwrap().push(format!("{when} {model}"));
            match running {
                Some(running) if running == wanted => Ok(()),
                other => Err(format!("{model} {when} on {other:?}")),
            }
        };
        runs("start")?;
        tokio::time::sleep(Duration::from_millis(CALL_MILLISECONDS)).await;
        let ended = runs("end");
        drop(ticket);
        ended
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_calls_on_different_models_at_once_both_finish() {
        let gate = Arc::new(ModelGate::default());
        let server = Arc::new(Server::default());
        let on = |model: &'static str| {
            let gate = Arc::clone(&gate);
            let server = Arc::clone(&server);
            tokio::spawn(async move { call(&gate, &server, model).await })
        };
        let (gemma, qwen) = tokio::join!(on(GEMMA), on(QWEN));
        assert_eq!(gemma.unwrap(), Ok(()));
        assert_eq!(qwen.unwrap(), Ok(()));
        assert!(gate.idle_for(Duration::ZERO));
        let turns = gate.turns();
        assert_eq!(
            (turns.under_way, turns.switches_waiting, turns.switching),
            (0, 0, false)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn many_calls_on_two_models_never_overlap() {
        let gate = Arc::new(ModelGate::default());
        let server = Arc::new(Server::default());
        let calls: Vec<_> = (0..12)
            .map(|index| {
                let gate = Arc::clone(&gate);
                let server = Arc::clone(&server);
                let model = if index % 2 == 0 { GEMMA } else { QWEN };
                tokio::spawn(async move { call(&gate, &server, model).await })
            })
            .collect();
        for call in calls {
            assert_eq!(call.await.unwrap(), Ok(()));
        }
        assert_eq!(server.log.lock().unwrap().len(), 24);
    }

    #[tokio::test]
    async fn a_waiting_switch_goes_before_newer_calls_on_the_current_model() {
        let gate = Arc::new(ModelGate::default());
        let gemma = PathBuf::from(GEMMA);
        let Entry::Switch(turn) = gate.enter(&gemma).await else {
            panic!("the first call starts its model");
        };
        let first = turn.started(&gemma);

        let switching_gate = Arc::clone(&gate);
        let switch = tokio::spawn(async move {
            match switching_gate.enter(Path::new(QWEN)).await {
                Entry::Switch(turn) => Ok(turn.started(Path::new(QWEN))),
                Entry::Join(_) => Err("qwen isn't running yet"),
            }
        });
        while gate.turns().switches_waiting == 0 {
            tokio::task::yield_now().await;
        }
        let joining_gate = Arc::clone(&gate);
        let newer = tokio::spawn(async move { matches!(joining_gate.enter(Path::new(GEMMA)).await, Entry::Join(_)) });
        tokio::time::sleep(Duration::from_millis(CALL_MILLISECONDS)).await;
        assert!(!newer.is_finished(), "a newer gemma call queues behind the switch");
        assert!(!switch.is_finished(), "the switch waits for the gemma call under way");

        drop(first);
        let qwen = switch.await.unwrap().unwrap();
        assert_eq!(gate.turns().current.as_deref(), Some(Path::new(QWEN)));
        drop(qwen);
        assert!(!newer.await.unwrap(), "after the switch, gemma is a switch of its own");
    }

    #[tokio::test]
    async fn a_failed_start_lets_the_next_caller_start_again() {
        let gate = Arc::new(ModelGate::default());
        let Entry::Switch(turn) = gate.enter(Path::new(GEMMA)).await else {
            panic!("nothing runs yet");
        };
        drop(turn);
        assert!(matches!(gate.enter(Path::new(GEMMA)).await, Entry::Switch(_)));
    }
}
