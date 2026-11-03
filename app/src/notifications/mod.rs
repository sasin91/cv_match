//! What the app tells the user without being asked: a good match a scan found, a scan that
//! finished with new posts or failed, saved jobs that closed. Each is kept for the window's bell
//! (notification_model) and, when it is worth interrupting for (Notice::pops_up), raised as one
//! of the system's own notifications too (Windows' toasts, the desktop's on Linux).
//!
//! The scheduled scan runs in a process of its own, often with the window shut: it keeps and
//! raises its notifications itself, and the window reads them from the database when it next
//! reads.

pub mod notification_model;
pub mod scan_notices;
#[cfg(target_os = "linux")]
mod toast_linux;
#[cfg(windows)]
mod toast_windows;

use std::sync::{Arc, Mutex};

use crate::context::Context;

#[cfg(target_os = "linux")]
use toast_linux as toast;
#[cfg(windows)]
use toast_windows as toast;

/// What a click on a notification gives the link to: the window, while it is open.
pub type Clicked = Arc<dyn Fn(String) + Send + Sync>;

/// What a notification is about, as notifications.kind keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A post a scan found matched at or above its search's "Tailor from" score.
    GoodMatch,
    /// A scan finished.
    ScanFinished,
    /// A scan failed, or stopped before it was done.
    ScanFailed,
    /// The still-open check found saved jobs closed.
    JobsClosed,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::GoodMatch => "good_match",
            Kind::ScanFinished => "scan_finished",
            Kind::ScanFailed => "scan_failed",
            Kind::JobsClosed => "jobs_closed",
        }
    }

    /// The kind notifications.kind keeps as kind; None for one this version doesn't know.
    pub fn kept_as(kind: &str) -> Option<Kind> {
        [Kind::GoodMatch, Kind::ScanFinished, Kind::ScanFailed, Kind::JobsClosed]
            .into_iter()
            .find(|known| known.as_str() == kind)
    }
}

/// One thing to tell the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub kind: Kind,
    pub title: String,
    pub text: String,
    /// The place in the window it opens, as the window's links are ("match/12", "job_scan").
    pub link: String,
    /// Raised as a system notification too, not only kept for the bell.
    pub pops_up: bool,
}

/// How the system's notifications are raised: for real, or not at all (tests, and a Context
/// made without main), and who hears of a click.
pub struct Toasts {
    raised: bool,
    /// The window's ear for a click, while it is open.
    clicked: Mutex<Option<Clicked>>,
}

impl Default for Toasts {
    /// Toasts that are never raised: a test's Context mustn't pop up notifications.
    fn default() -> Self {
        Toasts {
            raised: false,
            clicked: Mutex::new(None),
        }
    }
}

impl Toasts {
    /// The system's own notifications.
    pub fn system() -> Self {
        Toasts {
            raised: true,
            ..Toasts::default()
        }
    }

    /// Gives the link of a notification clicked from now on to clicked.
    pub fn on_click(&self, clicked: Clicked) {
        *self.clicked.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(clicked);
    }

    fn clicked(&self) -> Option<Clicked> {
        self.clicked
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// Keeps notice for the bell and raises it when it pops up. What fails is logged: telling is
/// never why the work it tells of fails.
pub async fn tell(context: &Context, notice: Notice) {
    let now = chrono::Utc::now().timestamp();
    if let Err(e) = notification_model::add(context.db(), &notice, now).await {
        crate::log_line!("notifications: \"{}\" couldn't be kept for the bell: {e}", notice.title);
    }
    if notice.pops_up && context.toasts.raised {
        raise(&notice, context.toasts.clicked()).await;
    }
}

/// Tells each of notices (tell).
pub async fn tell_all(context: &Context, notices: Vec<Notice>) {
    for notice in notices {
        tell(context, notice).await;
    }
}

/// Raises notice as a system notification, off the runtime's threads (D-Bus and WinRT block).
async fn raise(notice: &Notice, clicked: Option<Clicked>) {
    let Notice { title, text, link, .. } = notice.clone();
    let raised = tokio::task::spawn_blocking(move || toast::show(&title, &text, &link, clicked)).await;
    match raised {
        Ok(Ok(())) => {}
        Ok(Err(why)) => crate::log_line!("notifications: {why}"),
        Err(e) => crate::log_line!("notifications: raising one stopped: {e}"),
    }
}

/// Removes what the system's notifications keep outside the database (cleanup): Windows' name
/// for JobFinder's toasts. Ok(false) when there was none.
pub fn forget_system_name() -> Result<bool, String> {
    toast::forget_name()
}

/// A Windows toast's XML: title and text in the generic template, link as its launch argument.
#[cfg_attr(not(windows), allow(dead_code))]
fn toast_xml(title: &str, text: &str, link: &str) -> String {
    let escaped = |s: &str| html_escape::encode_double_quoted_attribute(s).into_owned();
    format!(
        "<toast launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escaped(link),
        escaped(title),
        escaped(text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toasts_words_are_escaped_in_its_xml() {
        let xml = toast_xml("R&D <lead>", "\"Acme\" 82%", "match/12");
        assert_eq!(
            xml,
            "<toast launch=\"match/12\"><visual><binding template=\"ToastGeneric\"><text>R&amp;D &lt;lead&gt;</text><text>&quot;Acme&quot; 82%</text></binding></visual></toast>"
        );
    }

    #[test]
    fn a_kind_reads_back_as_it_is_kept() {
        for kind in [Kind::GoodMatch, Kind::ScanFinished, Kind::ScanFailed, Kind::JobsClosed] {
            assert_eq!(Kind::kept_as(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::kept_as("elsewhere"), None);
    }
}
