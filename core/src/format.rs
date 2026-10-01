//! Per-app formatting profiles (v0.8.0, issue 40).
//!
//! Tone follows the app being dictated into: formal in docs, casual in
//! messages, verbatim where the transcript must stay untouched. Matching
//! uses the same case-insensitive substring rule as the privacy policy,
//! so `doc` covers `libreoffice-writer` and `chat` covers `telegram`.

/// How dictation into one app should sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Punctuation plus capitalization via the polish chain.
    Formal,
    /// Light whitespace tidy only, never the LLM.
    Casual,
    /// Raw transcript, no cleanup at all.
    Verbatim,
}

impl Style {
    /// Parse a style name at the boundary. Lowercase, trimmed.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "formal" => Ok(Self::Formal),
            "casual" => Ok(Self::Casual),
            "verbatim" => Ok(Self::Verbatim),
            other => Err(format!(
                "unknown style '{other}'. Use formal, casual, or verbatim."
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Formal => "formal",
            Self::Casual => "casual",
            Self::Verbatim => "verbatim",
        }
    }

    /// Which cleanup adapter this style runs.
    pub fn cleanup(&self) -> &'static str {
        match self {
            Self::Formal => "ollama",
            Self::Casual => "regex",
            Self::Verbatim => "none",
        }
    }
}

/// One app pattern plus its style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatProfile {
    pub app: String,
    pub style: Style,
}

impl FormatProfile {
    pub fn new(app: &str, style: Style) -> Result<Self, String> {
        let app = app.trim().to_lowercase();
        if app.is_empty() {
            return Err("empty app name. Name the app the profile applies to.".into());
        }
        Ok(Self { app, style })
    }
}

/// The profile that matches, for the transparent indicator.
pub fn matched_profile<'a>(
    profiles: &'a [FormatProfile],
    app: Option<&str>,
) -> Option<&'a FormatProfile> {
    let app = app?.trim().to_lowercase();
    if app.is_empty() {
        return None;
    }
    profiles.iter().find(|p| app.contains(p.app.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<FormatProfile> {
        vec![
            FormatProfile::new("doc", Style::Formal).unwrap(),
            FormatProfile::new("chat", Style::Casual).unwrap(),
            FormatProfile::new("terminal", Style::Verbatim).unwrap(),
        ]
    }

    #[test]
    fn styles_parse_and_map_to_cleanup() {
        assert_eq!(Style::parse("formal").unwrap(), Style::Formal);
        assert_eq!(Style::parse(" Casual ").unwrap(), Style::Casual);
        assert_eq!(Style::parse("VERBATIM").unwrap(), Style::Verbatim);
        assert!(Style::parse("fancy").is_err());
        assert_eq!(Style::Formal.cleanup(), "ollama");
        assert_eq!(Style::Casual.cleanup(), "regex");
        assert_eq!(Style::Verbatim.cleanup(), "none");
    }

    #[test]
    fn matching_is_substring_and_case_insensitive() {
        let p = profiles();
        assert_eq!(
            matched_profile(&p, Some("LibreOffice-Writer-doc"))
                .unwrap()
                .style,
            Style::Formal
        );
        assert_eq!(
            matched_profile(&p, Some("Telegram-CHAT")).unwrap().style,
            Style::Casual
        );
        assert_eq!(
            matched_profile(&p, Some("kitty-terminal")).unwrap().style,
            Style::Verbatim
        );
        assert!(matched_profile(&p, Some("firefox")).is_none());
        assert!(matched_profile(&p, None).is_none());
        assert!(matched_profile(&p, Some("   ")).is_none());
    }

    #[test]
    fn blank_apps_rejected() {
        assert!(FormatProfile::new("  ", Style::Formal).is_err());
    }
}
