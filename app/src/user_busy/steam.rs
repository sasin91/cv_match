//! What Steam says about the game it runs: its RunningAppID, 0 while none runs. Windows keeps it
//! in the registry (windows), Linux in Steam's registry.vdf (linux reads the file, running_app_id
//! reads the number from it).

use super::Busy;

/// Steam's app ID while no game runs.
const NO_APP: u32 = 0;

/// Steam apps that run all day without being games, so their app ID doesn't count as gaming:
/// Wallpaper Engine (431960), which draws the desktop background.
const NOT_GAMES: [u32; 1] = [431_960];

/// The Busy app_id says: None for NO_APP and NOT_GAMES.
pub fn busy_for(app_id: u32) -> Option<Busy> {
    let gaming = app_id != NO_APP && !NOT_GAMES.contains(&app_id);
    gaming.then_some(Busy::Steam { app_id })
}

/// The RunningAppID in a registry.vdf's text (`"RunningAppID" "570"`, tabs between), or None
/// without one.
#[cfg(any(target_os = "linux", test))]
pub fn running_app_id(vdf: &str) -> Option<u32> {
    vdf.lines().find_map(|line| {
        let mut quoted = line.split('"').skip(1).step_by(2);
        let name = quoted.next()?;
        if !name.eq_ignore_ascii_case("RunningAppID") {
            return None;
        }
        quoted.next()?.trim().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed registry.vdf as Steam on Linux writes it.
    const REGISTRY_VDF: &str = r#""Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"language"		"english"
					"RunningAppID"		"570"
					"SourceModInstallPath"		"/home/jonas/.steam/steam/steamapps/sourcemods"
				}
			}
		}
	}
}"#;

    #[test]
    fn the_running_game_is_read_from_registry_vdf() {
        assert_eq!(running_app_id(REGISTRY_VDF), Some(570));
        assert_eq!(running_app_id(&REGISTRY_VDF.replace("\"570\"", "\"0\"")), Some(0));
        assert_eq!(running_app_id("\"Registry\"\n{\n}"), None);
    }

    #[test]
    fn only_a_game_counts() {
        assert_eq!(busy_for(570), Some(Busy::Steam { app_id: 570 }));
        assert_eq!(busy_for(NO_APP), None);
        assert_eq!(busy_for(431_960), None);
    }
}
