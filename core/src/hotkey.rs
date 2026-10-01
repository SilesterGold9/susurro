//! Hotkey choice vocabulary (Windows audit): one name list shared by
//! the CLI flag, the daemon default, onboarding, and settings, so a
//! remap in any surface means the same combo everywhere. Mapping a
//! name to platform keys stays in the platform adapters; this module
//! only validates and labels names.

/// Canonical hotkey choice names, cheapest first.
pub const CHOICES: &[&str] = &["super_shift_r", "ctrl_shift_r", "shift_d"];

/// Default choice: Win+Shift+R, mirroring Hyprland SUPER_SHIFT+R.
pub const DEFAULT: &str = "super_shift_r";

/// Validate a choice name at the boundary. Lowercase, trimmed.
/// Unknown names name the choices instead of guessing.
pub fn normalize(name: &str) -> Result<String, String> {
    let norm = name.trim().to_lowercase();
    if CHOICES.contains(&norm.as_str()) {
        Ok(norm)
    } else {
        Err(format!(
            "unknown hotkey '{norm}'. Use {}.",
            CHOICES.join(", ")
        ))
    }
}

/// Human label for settings and onboarding screens.
pub fn display_name(name: &str) -> &str {
    match name.trim().to_lowercase().as_str() {
        "ctrl_shift_r" => "Ctrl + Shift + R",
        "shift_d" => "Shift + D",
        _ => "Super + Shift + R",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_normalize_and_reject_unknown() {
        assert_eq!(normalize("Super_Shift_R").unwrap(), "super_shift_r");
        assert_eq!(normalize(" shift_d ").unwrap(), "shift_d");
        assert!(normalize("fancy").is_err());
        assert!(normalize("").is_err());
    }

    #[test]
    fn labels_cover_every_choice() {
        assert_eq!(display_name("super_shift_r"), "Super + Shift + R");
        assert_eq!(display_name("ctrl_shift_r"), "Ctrl + Shift + R");
        assert_eq!(display_name("shift_d"), "Shift + D");
        assert_eq!(display_name("nope"), "Super + Shift + R");
    }
}
