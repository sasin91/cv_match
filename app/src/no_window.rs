//! Programs the app starts, started without a console window of their own. The desktop window
//! has no console (main hides the one it was double-clicked with), so on Windows each console
//! program it starts would otherwise open one: schtasks.exe on every status reload, llama-server
//! for as long as it runs, the PDF reader on every upload.

/// Windows' CREATE_NO_WINDOW process flag.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// command (tokio's), set to start without a console window.
pub fn hide(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
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
