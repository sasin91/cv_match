//! Programs the app starts, started without a console window of their own. The desktop window
//! has no console (main hides the one it was double-clicked with), so on Windows each console
//! program it starts would otherwise open one: schtasks.exe on every status reload, llama-server
//! for as long as it runs, the PDF reader on every upload. The heavy ones also start at
//! below-normal priority (in_background).

/// Windows' CREATE_NO_WINDOW process flag.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// command (tokio's), set to start without a console window.
pub fn hide(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Windows' BELOW_NORMAL_PRIORITY_CLASS process flag: the program gets the processor after
/// everything the user runs at normal priority.
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// command (tokio's), set to start without a console window and at below-normal priority, for
/// the heavy work the app does in the background (llama-server, the headless browser), so the
/// user's own programs come first. Elsewhere it is only started as it is.
pub fn in_background(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    command
}

/// command (std's), set to start without a console window.
pub fn hide_std(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
