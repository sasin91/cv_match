//! The games the Epic Games launcher and Battle.net installed, as their folders: a program in
//! one of them is that game (or its own helper), so the app in front counts as a game when its
//! program file is inside one. Neither launcher says which game runs, as Steam does.
//!
//! - Epic Games keeps one manifest per installed game, a JSON `.item` file under
//!   %ProgramData%\Epic\EpicGamesLauncher\Data\Manifests, naming DisplayName and
//!   InstallLocation (epic_game reads one).
//! - Battle.net registers each Blizzard game with Windows' installed programs, publisher
//!   "Blizzard Entertainment", with its InstallLocation (blizzard_game weighs one); the
//!   Battle.net app itself is registered the same way and isn't a game.
//!
//! windows reads the files and the registry.

use std::path::Path;

use serde::Deserialize;

/// What the launcher is called in the log.
pub const EPIC_GAMES: &str = "Epic Games";
/// What the launcher is called in the log.
pub const BATTLE_NET: &str = "Battle.net";

/// The publisher Battle.net registers Blizzard's games under.
const BLIZZARD_PUBLISHER: &str = "Blizzard Entertainment";
/// What Battle.net registers itself as, with the same publisher.
const BATTLE_NET_APP: &str = "Battle.net";

/// A game a launcher installed: its name, and the folder it's in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGame {
    pub launcher: &'static str,
    pub name: String,
    pub folder: String,
}

/// The fields of an Epic Games manifest that say what and where the game is.
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicManifest {
    display_name: String,
    install_location: String,
}

/// The game an Epic Games manifest's text names, or None when it isn't a manifest or names no
/// folder.
pub fn epic_game(manifest: &str) -> Option<InstalledGame> {
    let manifest: EpicManifest = serde_json::from_str(manifest).ok()?;
    installed(EPIC_GAMES, manifest.display_name, manifest.install_location)
}

/// The Blizzard game an installed program's entry is, or None for another publisher's program,
/// the Battle.net app itself, or one without a folder.
pub fn blizzard_game(publisher: &str, name: &str, folder: &str) -> Option<InstalledGame> {
    let blizzards = publisher.trim().eq_ignore_ascii_case(BLIZZARD_PUBLISHER);
    if !blizzards || name.trim().eq_ignore_ascii_case(BATTLE_NET_APP) {
        return None;
    }
    installed(BATTLE_NET, name.trim().to_string(), folder.to_string())
}

fn installed(launcher: &'static str, name: String, folder: String) -> Option<InstalledGame> {
    let folder = folder.trim().trim_end_matches(['\\', '/']).to_string();
    (!folder.is_empty()).then_some(InstalledGame { launcher, name, folder })
}

/// The game whose folder holds program (a full path), comparing as Windows does, without
/// regard to case or to / and \.
pub fn game_holding<'a>(program: &str, games: &'a [InstalledGame]) -> Option<&'a InstalledGame> {
    let program = comparable(program);
    games.iter().find(|game| {
        let folder = comparable(&game.folder);
        program.len() > folder.len() && program.starts_with(&folder) && program[folder.len()..].starts_with('\\')
    })
}

/// path in lower case with \ between its parts.
fn comparable(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// Whether path is an Epic Games manifest (a .item file).
pub fn is_epic_manifest(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed manifest as the Epic Games launcher writes it.
    const FORTNITE_MANIFEST: &str = r#"{
        "FormatVersion": 0,
        "DisplayName": "Fortnite",
        "InstallLocation": "C:\\Program Files\\Epic Games\\Fortnite",
        "LaunchExecutable": "FortniteGame/Binaries/Win64/FortniteLauncher.exe",
        "AppName": "Fortnite"
    }"#;

    #[test]
    fn an_epic_manifest_names_its_game_and_folder() {
        let game = epic_game(FORTNITE_MANIFEST).unwrap();
        assert_eq!(game.launcher, EPIC_GAMES);
        assert_eq!(game.name, "Fortnite");
        assert_eq!(game.folder, r"C:\Program Files\Epic Games\Fortnite");
        assert_eq!(epic_game("not json"), None);
        assert_eq!(epic_game(r#"{"DisplayName": "x", "InstallLocation": ""}"#), None);
    }

    #[test]
    fn only_item_files_are_manifests() {
        assert!(is_epic_manifest(Path::new(r"Manifests\0A1B2C.item")));
        assert!(!is_epic_manifest(Path::new(r"Manifests\Pending")));
    }

    #[test]
    fn only_blizzards_games_count_not_battle_net_itself() {
        let wow = blizzard_game(
            "Blizzard Entertainment",
            "World of Warcraft",
            r"C:\Games\World of Warcraft\",
        );
        assert_eq!(wow.unwrap().folder, r"C:\Games\World of Warcraft");
        assert_eq!(
            blizzard_game("Blizzard Entertainment", "Battle.net", r"C:\Battle.net"),
            None
        );
        assert_eq!(blizzard_game("Valve", "Steam", r"C:\Steam"), None);
    }

    #[test]
    fn a_program_in_a_games_folder_is_that_game() {
        let games = [epic_game(FORTNITE_MANIFEST).unwrap()];
        let program =
            r"c:\program files\epic games\fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe";
        assert_eq!(
            game_holding(program, &games).map(|game| game.name.as_str()),
            Some("Fortnite")
        );
        // A folder whose name only starts the same isn't the game's.
        let beside = r"C:\Program Files\Epic Games\FortniteTools\tool.exe";
        assert_eq!(game_holding(beside, &games), None);
    }
}
