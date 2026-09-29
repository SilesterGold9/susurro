//! Per-app privacy routing (v0.3.0, issue 21).
//!
//! Blocklisted apps always use local STT, regardless of network state.
//! Matching is case-insensitive substring on the focused app id or class,
//! so `terminal` covers `org.gnome.Terminal` and `1password` covers the
//! desktop client. Unknown or empty app names never match, so a broken
//! detector fails open to the normal chain instead of blocking cloud.

/// Built-in local-only apps: password managers plus terminals.
pub const DEFAULT_BLOCKLIST: &[&str] = &[
    "1password",
    "bitwarden",
    "keepass",
    "keepassxc",
    "lastpass",
    "dashlane",
    "enpass",
    "gopass",
    "alacritty",
    "kitty",
    "wezterm",
    "konsole",
    "foot",
    "xterm",
    "terminal",
    "tilix",
    "terminator",
    "ghostty",
];

#[derive(Debug, Clone, Default)]
pub struct PrivacyPolicy {
    entries: Vec<String>,
}

impl PrivacyPolicy {
    pub fn new(entries: &[String]) -> Self {
        let mut out = Vec::new();
        for e in entries {
            let norm = e.trim().to_lowercase();
            if !norm.is_empty() && !out.contains(&norm) {
                out.push(norm);
            }
        }
        Self { entries: out }
    }

    pub fn with_defaults(extra: &[String]) -> Self {
        let mut all: Vec<String> = DEFAULT_BLOCKLIST.iter().map(|s| s.to_string()).collect();
        all.extend(extra.iter().cloned());
        Self::new(&all)
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when the app must stay local. None or empty never matches.
    pub fn is_local_only(&self, app: Option<&str>) -> bool {
        self.matched_entry(app).is_some()
    }

    /// The blocklist entry that matched, for the transparent indicator.
    pub fn matched_entry(&self, app: Option<&str>) -> Option<&str> {
        let app = app?.trim().to_lowercase();
        if app.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .find(|e| app.contains(e.as_str()))
            .map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> PrivacyPolicy {
        PrivacyPolicy::with_defaults(&[])
    }

    #[test]
    fn blocks_password_managers_case_insensitively() {
        let p = policy();
        assert!(p.is_local_only(Some("1Password")));
        assert!(p.is_local_only(Some("bitwarden-desktop")));
        assert!(p.is_local_only(Some("org.keepassxc.KeePassXC")));
    }

    #[test]
    fn blocks_terminals_by_substring() {
        let p = policy();
        assert!(p.is_local_only(Some("org.gnome.Terminal")));
        assert!(p.is_local_only(Some("kitty")));
        assert!(p.is_local_only(Some("Alacritty")));
    }

    #[test]
    fn allows_regular_apps_and_unknown() {
        let p = policy();
        assert!(!p.is_local_only(Some("firefox")));
        assert!(!p.is_local_only(Some("code")));
        assert!(!p.is_local_only(None));
        assert!(!p.is_local_only(Some("")));
        assert!(!p.is_local_only(Some("   ")));
    }

    #[test]
    fn custom_entries_extend_defaults() {
        let p = PrivacyPolicy::with_defaults(&["mybank".to_string()]);
        assert!(p.is_local_only(Some("mybank-app")));
        assert!(p.is_local_only(Some("kitty")));
        assert_eq!(p.matched_entry(Some("mybank-app")), Some("mybank"));
    }

    #[test]
    fn duplicates_and_blanks_collapse() {
        let p = PrivacyPolicy::new(&["Kitty".to_string(), " kitty ".to_string(), "".to_string()]);
        assert_eq!(p.entries(), &["kitty".to_string()]);
    }
}
