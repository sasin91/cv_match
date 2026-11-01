//! Whether the user is busy with something that background work shouldn't slow down: a
//! full-screen game, a presentation, or another full-screen app, as Windows' own "do not
//! disturb" state says it (SHQueryUserNotificationState, which Windows also uses to hold back
//! notifications). The job scan waits while it lasts (wait_until_free).
//!
//! Elsewhere the user never counts as busy.

use std::time::{Duration, Instant};

/// How often a waiting scan looks again (every half minute).
const LOOK_EVERY: Duration = Duration::from_secs(30);

/// Whether the user is busy now; what Windows says when it can't tell is logged and counts as
/// not busy.
pub fn busy() -> bool {
    match windows_state() {
        Ok(busy) => busy,
        Err(why) => {
            crate::log_line!("Couldn't tell whether a full-screen app runs: {why}");
            false
        }
    }
}

/// Waits while the user is busy, looking every LOOK_EVERY, for at most give_up_after. Err says
/// so when they were still busy then.
pub async fn wait_until_free(give_up_after: Duration) -> Result<(), String> {
    wait_while(busy, give_up_after, LOOK_EVERY).await
}

/// Waits while is_busy says so, looking every look_every, for at most give_up_after.
async fn wait_while(is_busy: impl Fn() -> bool, give_up_after: Duration, look_every: Duration) -> Result<(), String> {
    if !is_busy() {
        return Ok(());
    }
    crate::say!("A full-screen app or game runs, so the job scan waits until it ends.");
    let started = Instant::now();
    loop {
        if started.elapsed() >= give_up_after {
            return Err(format!(
                "A full-screen app or game ran for {} minutes, so the scan stopped; the posts left are matched by the next scan.",
                give_up_after.as_secs() / 60
            ));
        }
        tokio::time::sleep(look_every).await;
        if !is_busy() {
            crate::say!(
                "The full-screen app ended, so the job scan goes on ({:.0} minutes later).",
                started.elapsed().as_secs_f64() / 60.0
            );
            return Ok(());
        }
    }
}

#[cfg(windows)]
fn windows_state() -> Result<bool, String> {
    use windows::Win32::UI::Shell::{
        QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    };
    // SAFETY: takes nothing and returns the state by value.
    let state = unsafe { SHQueryUserNotificationState() }.map_err(|e| e.to_string())?;
    Ok([QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE].contains(&state))
}

#[cfg(not(windows))]
fn windows_state() -> Result<bool, String> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn a_free_user_isnt_waited_for() {
        assert_eq!(wait_while(|| false, Duration::ZERO, Duration::ZERO).await, Ok(()));
    }

    #[tokio::test]
    async fn the_scan_goes_on_once_the_game_ends() {
        let looks = AtomicUsize::new(0);
        // Busy for the first two looks.
        let busy = || looks.fetch_add(1, Ordering::Relaxed) < 2;
        let waited = wait_while(busy, Duration::from_secs(60), Duration::from_millis(1)).await;
        assert_eq!(waited, Ok(()));
        assert_eq!(looks.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn a_game_that_goes_on_stops_the_scan() {
        let why = wait_while(|| true, Duration::from_millis(5), Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(why.starts_with("A full-screen app or game ran for"), "{why}");
    }
}
