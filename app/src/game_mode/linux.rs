//! Linux's signals: Steam's registry.vdf, and Feral's gamemode on the session D-Bus.

use std::path::PathBuf;

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::names::BusName;

use super::{Playing, steam};

/// Where Steam keeps registry.vdf under the home folder: installed from the distribution's
/// packages, as a Flatpak, and as a Snap.
const STEAM_REGISTRY_FILES: [&str; 3] = [
    ".steam/registry.vdf",
    ".var/app/com.valvesoftware.Steam/.steam/registry.vdf",
    "snap/steam/common/.steam/registry.vdf",
];

/// gamemoded's name on the session bus.
const GAME_MODE_NAME: &str = "com.feralinteractive.GameMode";
/// gamemoded's object.
const GAME_MODE_PATH: &str = "/com/feralinteractive/GameMode";
/// gamemoded's interface, whose ClientCount is the number of games it runs for.
const GAME_MODE_INTERFACE: &str = "com.feralinteractive.GameMode";

/// The game Steam runs, by the first registry.vdf that names one; none without Steam.
pub fn steam_game() -> Result<Option<Playing>, String> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(None);
    };
    for file in STEAM_REGISTRY_FILES.map(|file| home.join(file)) {
        let Ok(vdf) = std::fs::read_to_string(&file) else {
            continue;
        };
        if let Some(playing) = steam::running_app_id(&vdf).and_then(steam::playing_for) {
            return Ok(Some(playing));
        }
    }
    Ok(None)
}

/// The games gamemode runs for; none when it isn't running, or there's no session bus (a
/// scheduled scan without a desktop). gamemoded is started by D-Bus when it's called, so it is
/// only asked once it is there.
pub fn feral_game_mode() -> Result<Option<Playing>, String> {
    let Ok(connection) = Connection::session() else {
        return Ok(None);
    };
    if !game_mode_runs(&connection)? {
        return Ok(None);
    }
    let proxy = Proxy::new(&connection, GAME_MODE_NAME, GAME_MODE_PATH, GAME_MODE_INTERFACE)
        .map_err(|e| format!("gamemode couldn't be asked: {e}"))?;
    let games: i32 = proxy
        .get_property("ClientCount")
        .map_err(|e| format!("gamemode didn't say how many games it runs: {e}"))?;
    Ok((games > 0).then_some(Playing::GameMode { games }))
}

/// Whether gamemoded is on the session bus now.
fn game_mode_runs(connection: &Connection) -> Result<bool, String> {
    let bus = DBusProxy::new(connection).map_err(|e| format!("the session bus couldn't be asked: {e}"))?;
    let name = BusName::try_from(GAME_MODE_NAME).map_err(|e| e.to_string())?;
    bus.name_has_owner(name)
        .map_err(|e| format!("the session bus didn't say whether gamemode runs: {e}"))
}
