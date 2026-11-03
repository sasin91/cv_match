//! Windows' signals: its "do not disturb" state, the Xbox Game Bar's list of games, the games
//! the Epic Games launcher and Battle.net installed, and Steam's RunningAppID, all read from the
//! signed-in user's own session, the registry and the launchers' files.

use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HWND, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW,
    RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use std::path::PathBuf;

use super::launchers::{self, InstalledGame};
use super::{Playing, steam};

/// The Game Bar's games, one key each, under the user's registry.
const GAME_BAR_GAMES: &str = r"System\GameConfigStore\Children";
/// Each game's program file, in its key.
const GAME_PROGRAM: &str = "MatchedExeFullPath";
/// Steam's key under the user's registry.
const STEAM_KEY: &str = r"Software\Valve\Steam";
/// The game Steam runs, in its key.
const STEAM_RUNNING_APP: &str = "RunningAppID";
/// Where Windows keeps the computer's shared program data, when %ProgramData% isn't set.
const PROGRAM_DATA: &str = r"C:\ProgramData";
/// The Epic Games launcher's manifests, one per installed game, under the program data.
const EPIC_MANIFESTS: &str = r"Epic\EpicGamesLauncher\Data\Manifests";
/// Windows' lists of installed programs: 32-bit programs', 64-bit programs', and the user's own.
const INSTALLED_PROGRAMS: [(HKEY, &str); 3] = [
    (
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
    (
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    ),
];
/// An installed program's publisher, name and folder, in its key.
const PUBLISHER: &str = "Publisher";
const DISPLAY_NAME: &str = "DisplayName";
const INSTALL_LOCATION: &str = "InstallLocation";
/// The longest path a program file has, in UTF-16 units (Windows' long paths).
const LONGEST_PATH: usize = 32_768;
/// The longest registry key name, in UTF-16 units, with its ending 0.
const LONGEST_KEY_NAME: usize = 256;

/// Whether Windows holds back notifications for a full-screen game, presentation or app.
pub fn full_screen() -> Result<Option<Playing>, String> {
    // SAFETY: takes nothing and returns the state by value.
    let state = unsafe { SHQueryUserNotificationState() }.map_err(|e| e.to_string())?;
    let busy = [QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE].contains(&state);
    Ok(busy.then_some(Playing::FullScreen))
}

/// Whether the app in front is one of the Game Bar's games; none without a window in front, or
/// one whose program can't be read (another user's, or the system's).
pub fn known_game_in_front() -> Result<Option<Playing>, String> {
    let Some(program) = program_in_front() else {
        return Ok(None);
    };
    let known = game_bar_programs()?
        .iter()
        .any(|game| game.eq_ignore_ascii_case(&program));
    Ok(known.then(|| Playing::KnownGame {
        program: file_name(&program),
    }))
}

/// The game Steam runs, by its RunningAppID; none without Steam.
pub fn steam_game() -> Result<Option<Playing>, String> {
    let app_id = read_dword(HKEY_CURRENT_USER, STEAM_KEY, STEAM_RUNNING_APP)
        .map_err(|e| format!("Steam's RunningAppID couldn't be read: {}", described(e)))?;
    Ok(app_id.and_then(steam::playing_for))
}

/// The program file of the window in front.
fn program_in_front() -> Option<String> {
    // SAFETY: takes nothing; a null window means none is in front.
    let window: HWND = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut process_id = 0;
    // SAFETY: window is a window handle and process_id outlives the call.
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    // SAFETY: opens the process to read its name only; closed below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut path = vec![0u16; LONGEST_PATH];
    let mut length = u32::try_from(path.len()).ok()?;
    // SAFETY: path holds length units, and the call writes at most that many.
    let read =
        unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut length) };
    // SAFETY: process was opened above and isn't used after.
    let _ = unsafe { CloseHandle(process) };
    read.ok()?;
    Some(String::from_utf16_lossy(&path[..length as usize]))
}

/// The program file of each game in the Game Bar's list; none without the list.
fn game_bar_programs() -> Result<Vec<String>, String> {
    let read = || -> Result<Vec<String>, WIN32_ERROR> {
        let mut programs = Vec::new();
        for game in sub_key_names(HKEY_CURRENT_USER, GAME_BAR_GAMES)? {
            let key = format!(r"{GAME_BAR_GAMES}\{game}");
            programs.extend(read_string(HKEY_CURRENT_USER, &key, GAME_PROGRAM)?);
        }
        Ok(programs)
    };
    read().map_err(|e| format!("the Game Bar's games couldn't be read: {}", described(e)))
}

/// Whether the app in front is a game the Epic Games launcher or Battle.net installed.
pub fn launcher_game_in_front() -> Result<Option<Playing>, String> {
    let Some(program) = program_in_front() else {
        return Ok(None);
    };
    let mut games = epic_games();
    games.extend(blizzard_games().map_err(|e| format!("Blizzard's games couldn't be read: {}", described(e)))?);
    Ok(
        launchers::game_holding(&program, &games).map(|game| Playing::LauncherGame {
            launcher: game.launcher,
            game: game.name.clone(),
        }),
    )
}

/// The games the Epic Games launcher installed, by its manifests; none without it. A manifest
/// that can't be read is skipped (the launcher may be writing it).
fn epic_games() -> Vec<InstalledGame> {
    let program_data = std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(PROGRAM_DATA), PathBuf::from);
    let Ok(manifests) = std::fs::read_dir(program_data.join(EPIC_MANIFESTS)) else {
        return Vec::new();
    };
    manifests
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| launchers::is_epic_manifest(path))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|manifest| launchers::epic_game(&manifest))
        .collect()
}

/// The Blizzard games installed, by Windows' list of installed programs (both registry views,
/// and the user's own).
fn blizzard_games() -> Result<Vec<InstalledGame>, WIN32_ERROR> {
    let mut games = Vec::new();
    for (root, programs) in INSTALLED_PROGRAMS {
        for program in sub_key_names(root, programs)? {
            let key = format!(r"{programs}\{program}");
            let value = |name| read_string(root, &key, name).map(Option::unwrap_or_default);
            let (publisher, name, folder) = (value(PUBLISHER)?, value(DISPLAY_NAME)?, value(INSTALL_LOCATION)?);
            games.extend(launchers::blizzard_game(&publisher, &name, &folder));
        }
    }
    Ok(games)
}

/// The names of the keys under root\path; none when there's no such key.
fn sub_key_names(root: HKEY, path: &str) -> Result<Vec<String>, WIN32_ERROR> {
    let mut key = HKEY::default();
    // SAFETY: key outlives the call, and is closed below when it opened.
    let opened = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) };
    if !found(opened)? {
        return Ok(Vec::new());
    }
    let names = names_under(key);
    // SAFETY: key was opened above and isn't used after.
    let _ = unsafe { RegCloseKey(key) };
    names
}

/// The names of the keys under key.
fn names_under(key: HKEY) -> Result<Vec<String>, WIN32_ERROR> {
    let mut names = Vec::new();
    for index in 0.. {
        let mut name = [0u16; LONGEST_KEY_NAME];
        let mut length = LONGEST_KEY_NAME as u32;
        // SAFETY: name holds length units, and the call writes at most that many.
        let found = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if found == ERROR_NO_MORE_ITEMS {
            break;
        }
        win32(found)?;
        names.push(String::from_utf16_lossy(&name[..length as usize]));
    }
    Ok(names)
}

/// The DWORD value under key\sub_key, or None when there's no such key or value.
fn read_dword(key: HKEY, sub_key: &str, value: &str) -> Result<Option<u32>, WIN32_ERROR> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: data holds size bytes, and the call writes at most that many.
    let read = unsafe {
        RegGetValueW(
            key,
            &HSTRING::from(sub_key),
            &HSTRING::from(value),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&mut size),
        )
    };
    found(read).map(|found| found.then_some(data))
}

/// The string value under key\sub_key, or None when there's no such key or value.
fn read_string(key: HKEY, sub_key: &str, value: &str) -> Result<Option<String>, WIN32_ERROR> {
    let sub_key = HSTRING::from(sub_key);
    let value = HSTRING::from(value);
    let mut text = vec![0u16; LONGEST_PATH];
    let mut size = (text.len() * size_of::<u16>()) as u32;
    // SAFETY: text holds size bytes, and the call writes at most that many.
    let read = unsafe {
        RegGetValueW(
            key,
            PCWSTR(sub_key.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(text.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if !found(read)? {
        return Ok(None);
    }
    // size counts bytes, with the ending 0.
    let units = (size as usize / size_of::<u16>()).saturating_sub(1);
    Ok(Some(String::from_utf16_lossy(&text[..units])))
}

/// Whether a registry read found its value: Ok(false) for ERROR_FILE_NOT_FOUND.
fn found(read: WIN32_ERROR) -> Result<bool, WIN32_ERROR> {
    if read == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    win32(read).map(|()| true)
}

/// What Windows says a registry error is.
fn described(e: WIN32_ERROR) -> String {
    windows::core::Error::from_hresult(e.to_hresult()).message()
}

fn win32(result: WIN32_ERROR) -> Result<(), WIN32_ERROR> {
    if result == ERROR_SUCCESS { Ok(()) } else { Err(result) }
}

/// "Cyberpunk2077.exe" of "C:\Games\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe".
fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}
