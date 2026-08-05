use std::ffi::OsStr;
use std::process::Command;

#[cfg(target_os = "macos")]
pub mod macos;

const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn background_process_creation_flags() -> u32 {
    if cfg!(target_os = "windows") {
        WINDOWS_CREATE_NO_WINDOW
    } else {
        0
    }
}

/// Creates a command that cannot allocate a transient console window on Windows.
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        let mut command = command;
        use std::os::windows::process::CommandExt;
        command.creation_flags(background_process_creation_flags());
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        command
    }
}
