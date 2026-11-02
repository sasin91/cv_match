//! Whether the user is busy with something that background work shouldn't slow down: a game, a
//! presentation, or another full-screen app. Each signal the computer gives is asked in turn,
//! and the first that says so is why the user is busy (Busy):
//!
//! - Windows' own "do not disturb" state (SHQueryUserNotificationState, which Windows also uses
//!   to hold back notifications): exclusive full-screen games and presentations.
//! - The app in front is one the Xbox Game Bar knows as a game (its list in
//!   HKCU\System\GameConfigStore\Children, the one Windows' Game Mode goes by). This catches
//!   the borderless windowed games the first misses.
//! - Steam runs a game: its RunningAppID (Windows' registry, or ~/.steam/registry.vdf).
//! - Feral's gamemode runs a game (its ClientCount on the session D-Bus), which games started
//!   with `gamemoderun` (Steam's launch option `gamemoderun %command%`) ask for on Linux.
//!
//! Windows' Game Mode itself can't be asked from outside: HasExpandedResources answers for the
//! calling process only.
//!
//! The job scan waits while the user is busy (wait_until_free), unless gaming mode sends its
//! work to the cloud meanwhile (gaming_mode); the models let go of the graphics card as soon as
//! no call needs them (idle).

#[cfg(target_os = "linux")]
mod linux;
mod steam;
#[cfg(windows)]
mod windows;

use std::fmt;
use std::time::{Duration, Instant};

/// How often a waiting scan looks again (every half minute).
const LOOK_EVERY: Duration = Duration::from_secs(30);

/// Why the user counts as busy: the first signal that said so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Busy {
    /// Windows' "do not disturb" state: a full-screen game, a presentation or another
    /// full-screen app.
    FullScreen,
    /// The app in front is in the Xbox Game Bar's list of games: its program file's name.
    KnownGame { program: String },
    /// Steam runs the game with this app ID.
    Steam { app_id: u32 },
    /// Feral's gamemode runs for this many games.
    GameMode { games: i32 },
}

impl fmt::Display for Busy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Busy::FullScreen => f.write_str("a full-screen app or game runs"),
            Busy::KnownGame { program } => write!(f, "the game {program} is in front"),
            Busy::Steam { app_id } => write!(f, "Steam runs a game (app {app_id})"),
            Busy::GameMode { games: 1 } => f.write_str("gamemode runs a game"),
            Busy::GameMode { games } => write!(f, "gamemode runs {games} games"),
        }
    }
}

/// What one signal says: Some when it says the user is busy, Err when it couldn't be asked.
type Signal = fn() -> Result<Option<Busy>, String>;

/// Why the user is busy now, or None. A signal that couldn't be asked is logged and counts as
/// saying nothing.
pub fn why_busy() -> Option<Busy> {
    first_busy(&SIGNALS)
}

/// Whether the user is busy now (why_busy).
pub fn busy() -> bool {
    why_busy().is_some()
}

/// The signals this computer gives, in the order they're asked.
#[cfg(windows)]
const SIGNALS: [Signal; 3] = [windows::full_screen, windows::known_game_in_front, windows::steam_game];

#[cfg(target_os = "linux")]
const SIGNALS: [Signal; 2] = [linux::steam_game, linux::game_mode];

#[cfg(not(any(windows, target_os = "linux")))]
const SIGNALS: [Signal; 0] = [];

/// The first signal's Busy, asking them in turn; failures are logged and skipped.
fn first_busy(signals: &[Signal]) -> Option<Busy> {
    signals.iter().find_map(|signal| match signal() {
        Ok(busy) => busy,
        Err(why) => {
            crate::log_line!("Couldn't tell whether a game runs: {why}");
            None
        }
    })
}

/// Waits while the user is busy, looking every LOOK_EVERY, for at most give_up_after. Err says
/// so when they were still busy then.
pub async fn wait_until_free(give_up_after: Duration) -> Result<(), String> {
    wait_while(why_busy, give_up_after, LOOK_EVERY).await
}

/// Waits while why_busy says so, looking every look_every, for at most give_up_after.
async fn wait_while(
    why_busy: impl Fn() -> Option<Busy>,
    give_up_after: Duration,
    look_every: Duration,
) -> Result<(), String> {
    let Some(busy) = why_busy() else {
        return Ok(());
    };
    crate::say!("{}, so the job scan waits until it ends.", capitalized(&busy));
    let started = Instant::now();
    loop {
        if started.elapsed() >= give_up_after {
            return Err(format!(
                "A game or full-screen app ran for {} minutes, so the scan stopped; the posts left are matched by the next scan.",
                give_up_after.as_secs() / 60
            ));
        }
        tokio::time::sleep(look_every).await;
        if why_busy().is_none() {
            crate::say!(
                "The game ended, so the job scan goes on ({:.0} minutes later).",
                started.elapsed().as_secs_f64() / 60.0
            );
            return Ok(());
        }
    }
}

/// busy's sentence with its first letter capitalized ("Steam runs a game (app 570)").
pub fn capitalized(busy: &Busy) -> String {
    let sentence = busy.to_string();
    let mut chars = sentence.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => sentence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn a_free_user_isnt_waited_for() {
        assert_eq!(wait_while(|| None, Duration::ZERO, Duration::ZERO).await, Ok(()));
    }

    #[tokio::test]
    async fn the_scan_goes_on_once_the_game_ends() {
        let looks = AtomicUsize::new(0);
        // Busy for the first two looks.
        let busy = || (looks.fetch_add(1, Ordering::Relaxed) < 2).then_some(Busy::FullScreen);
        let waited = wait_while(busy, Duration::from_secs(60), Duration::from_millis(1)).await;
        assert_eq!(waited, Ok(()));
        assert_eq!(looks.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn a_game_that_goes_on_stops_the_scan() {
        let why = wait_while(
            || Some(Busy::Steam { app_id: 570 }),
            Duration::from_millis(5),
            Duration::from_millis(1),
        )
        .await
        .unwrap_err();
        assert!(why.starts_with("A game or full-screen app ran for"), "{why}");
    }

    fn free() -> Result<Option<Busy>, String> {
        Ok(None)
    }

    fn broken() -> Result<Option<Busy>, String> {
        Err("no answer".to_string())
    }

    fn steam() -> Result<Option<Busy>, String> {
        Ok(Some(Busy::Steam { app_id: 570 }))
    }

    fn game_mode() -> Result<Option<Busy>, String> {
        Ok(Some(Busy::GameMode { games: 2 }))
    }

    #[test]
    fn the_first_signal_that_says_busy_is_why() {
        assert_eq!(first_busy(&[free, steam, game_mode]), Some(Busy::Steam { app_id: 570 }));
    }

    #[test]
    fn a_signal_that_cant_be_asked_says_nothing() {
        assert_eq!(first_busy(&[broken, game_mode]), Some(Busy::GameMode { games: 2 }));
        assert_eq!(first_busy(&[broken, free]), None);
    }

    #[test]
    fn each_reason_reads_as_a_sentence() {
        let program = "Cyberpunk2077.exe".to_string();
        assert_eq!(
            capitalized(&Busy::KnownGame { program }),
            "The game Cyberpunk2077.exe is in front"
        );
        assert_eq!(Busy::GameMode { games: 1 }.to_string(), "gamemode runs a game");
        assert_eq!(Busy::GameMode { games: 3 }.to_string(), "gamemode runs 3 games");
    }
}
