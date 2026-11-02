//! Windows' signals: its "do not disturb" state, the Xbox Game Bar's list of games, and Steam's
//! RunningAppID, all read from the signed-in user's own session and registry.

use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HWND, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::{Playing, steam};

/// The Game Bar's games, one key each, under the user's registry.
const GAME_BAR_GAMES: &str = r"System\GameConfigStore\Children";
/// Each game's program file, in its key.
const GAME_PROGRAM: &str = "MatchedExeFullPath";
/// Steam's key under the user's registry.
const STEAM_KEY: &str = r"Software\Valve\Steam";
/// The game Steam runs, in its key.
const STEAM_RUNNING_APP: &str = "RunningAppID";
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
    let mut games = HKEY::default();
    // SAFETY: games outlives the call, and is closed below when it opened.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(GAME_BAR_GAMES),
            None,
            KEY_READ,
            &mut games,
        )
    };
    if opened == ERROR_FILE_NOT_FOUND {
        return Ok(Vec::new());
    }
    win32(opened).map_err(|e| format!("the Game Bar's games couldn't be read: {}", described(e)))?;
    let programs = programs_under(games);
    // SAFETY: games was opened above and isn't used after.
    let _ = unsafe { RegCloseKey(games) };
    programs.map_err(|e| format!("the Game Bar's games couldn't be read: {}", described(e)))
}

/// GAME_PROGRAM of each key under games; keys without one are skipped.
fn programs_under(games: HKEY) -> Result<Vec<String>, WIN32_ERROR> {
    let mut programs = Vec::new();
    for index in 0.. {
        let mut name = [0u16; LONGEST_KEY_NAME];
        let mut length = LONGEST_KEY_NAME as u32;
        // SAFETY: name holds length units, and the call writes at most that many.
        let found = unsafe {
            RegEnumKeyExW(
                games,
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
        let game = String::from_utf16_lossy(&name[..length as usize]);
        if let Some(program) = read_string(games, &game, GAME_PROGRAM)? {
            programs.push(program);
        }
    }
    Ok(programs)
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
