//! The desktop's notifications on Linux: org.freedesktop.Notifications on the session D-Bus,
//! which GNOME, KDE and the rest answer. A click only closes it.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

use super::Clicked;

/// The notification server's name on the session bus.
const NOTIFICATIONS_NAME: &str = "org.freedesktop.Notifications";
/// Its object.
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
/// Its interface.
const NOTIFICATIONS_INTERFACE: &str = "org.freedesktop.Notifications";
/// The app's name on its notifications.
const APP_NAME: &str = "JobFinder";
/// A new notification, not one in place of an earlier one.
const REPLACES_NONE: u32 = 0;
/// The server's own time before the notification goes away.
const SERVERS_TIMEOUT: i32 = -1;

/// Raises a notification saying title and text.
pub fn show(title: &str, text: &str, _link: &str, _clicked: Option<Clicked>) -> Result<(), String> {
    let failed = |e: zbus::Error| format!("The desktop didn't show the notification: {e}");
    let bus = Connection::session().map_err(failed)?;
    let notifications =
        Proxy::new(&bus, NOTIFICATIONS_NAME, NOTIFICATIONS_PATH, NOTIFICATIONS_INTERFACE).map_err(failed)?;
    let no_actions: Vec<&str> = Vec::new();
    let no_hints: HashMap<&str, Value<'_>> = HashMap::new();
    let _id: u32 = notifications
        .call(
            "Notify",
            &(
                APP_NAME,
                REPLACES_NONE,
                "",
                title,
                text,
                no_actions,
                no_hints,
                SERVERS_TIMEOUT,
            ),
        )
        .map_err(failed)?;
    Ok(())
}

/// Nothing is kept outside the database on Linux for notifications.
pub fn name_registered() -> bool {
    false
}

/// Nothing is kept outside the database on Linux for notifications.
pub fn forget_name() -> Result<bool, String> {
    Ok(false)
}
