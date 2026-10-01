//! Child process spawning without console flashes (issue 50-batch,
//! Windows audit). Console-subsystem binaries (curl, whisper-cli,
//! where) pop a visible terminal on Windows unless the spawn sets
//! CREATE_NO_WINDOW. Every `Command::new` in this workspace goes
//! through here so the flag cannot be forgotten per call site.

/// CREATE_NO_WINDOW value, named so review can check it.
#[cfg(target_os = "windows")]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a child command that never flashes a console window.
/// Identical to `Command::new` on other platforms.
pub fn silent_command(bin: &str) -> std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new(bin);
        // The child keeps no console at all.
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(bin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_for_any_binary_name() {
        let cmd = silent_command("curl");
        assert!(format!("{cmd:?}").contains("curl"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn no_window_flag_value_is_correct() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
    }
}
