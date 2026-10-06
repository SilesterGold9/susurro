//! Named rewrites over a past dictation (issue 56).
//!
//! Dictation is verbatim by default. This module names the shapes a
//! user can ask for afterwards and says honestly which of them the
//! bundled engine can do and which need the opt-in LLM tier.
//!
//! The distinction is the whole point of this module. Tidying
//! punctuation is local, free, and reversible, so the app can offer it
//! to everyone. Organizing, shortening, and formalizing throw words
//! away, which no punctuation model can do, so they need the LLM and
//! must be labelled as rewrites rather than tidies. A UI that showed
//! both as "cleanup" would be lying about the second one.
//!
//! All pure. The engine lives in the adapters, the preview flow lives
//! in the app.

/// One named rewrite, in the order a user reaches for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transform {
    /// Punctuation and capitalisation only. The bundled ONNX tier.
    Tidy,
    /// Restructure rambling notes into ordered points.
    Organize,
    /// Cut length while keeping every remaining point.
    Shorten,
    /// Raise the register without changing what was said.
    Formalize,
}

/// Every transform, in menu order.
pub const ALL: [Transform; 4] = [
    Transform::Tidy,
    Transform::Organize,
    Transform::Shorten,
    Transform::Formalize,
];

impl Transform {
    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Transform::Tidy => "Tidy",
            Transform::Organize => "Organize",
            Transform::Shorten => "Shorten",
            Transform::Formalize => "Formalize",
        }
    }

    /// One line saying what the user gets, for the button tooltip.
    pub fn blurb(self) -> &'static str {
        match self {
            Transform::Tidy => "Punctuation and capitalisation. On device, nothing discarded.",
            Transform::Organize => "Turn rambling notes into ordered points. Rewrites the text.",
            Transform::Shorten => "Cut length, keep every point. Rewrites the text.",
            Transform::Formalize => "Raise the register. Rewrites the text.",
        }
    }

    /// True when the transform discards or reorders words, which is
    /// what needs the LLM tier and what the UI must label honestly.
    pub fn rewrites(self) -> bool {
        !matches!(self, Transform::Tidy)
    }

    /// Name of the cleanup tier that can run this one. Tidy runs on
    /// the bundled default; the rest are opt-in.
    pub fn tier(self) -> &'static str {
        if self.rewrites() {
            "ollama"
        } else {
            "onnx"
        }
    }

    /// Parse a name from the CLI or the wire. Case-insensitive.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "tidy" => Ok(Transform::Tidy),
            "organize" | "organise" => Ok(Transform::Organize),
            "shorten" => Ok(Transform::Shorten),
            "formalize" | "formalise" => Ok(Transform::Formalize),
            other => Err(format!(
                "unknown transform '{other}'. Use tidy, organize, shorten, or formalize."
            )),
        }
    }

    /// The instruction sent to the LLM tier. Only meaningful for the
    /// rewriting transforms; Tidy never reaches a model prompt.
    pub fn prompt(self, raw: &str) -> String {
        let task = match self {
            Transform::Tidy => "Fix punctuation and capitalisation only.",
            Transform::Organize => "Restructure this into short ordered points, one idea each.",
            Transform::Shorten => {
                "Cut this to about half its length, keeping every distinct point."
            }
            Transform::Formalize => {
                "Rewrite this in a more formal register without changing what was said."
            }
        };
        format!(
            "You are transforming a speech transcript. {task}\n\
             Output the transformed text and nothing else: no preamble, no quotes, \
             no explanation, no markdown fences.\n\
             Transcript: {raw}"
        )
    }
}

/// What happened to the text, so the UI can label it rather than
/// guessing from the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The bundled local tier tidied it. Nothing was discarded.
    Tidied,
    /// An LLM rewrote it.
    Rewritten,
    /// The transform wanted a rewrite but the engine was unavailable
    /// or failed open, so the text is unchanged.
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tidy_runs_on_the_bundled_tier() {
        assert!(!Transform::Tidy.rewrites());
        assert_eq!(Transform::Tidy.tier(), "onnx");
        for t in [
            Transform::Organize,
            Transform::Shorten,
            Transform::Formalize,
        ] {
            assert!(t.rewrites(), "{t:?} must be labelled a rewrite");
            assert_eq!(t.tier(), "ollama", "{t:?}");
        }
    }

    #[test]
    fn names_parse_both_spellings() {
        assert_eq!(Transform::parse("Tidy").unwrap(), Transform::Tidy);
        assert_eq!(Transform::parse(" organize ").unwrap(), Transform::Organize);
        assert_eq!(Transform::parse("organise").unwrap(), Transform::Organize);
        assert_eq!(Transform::parse("SHORTEN").unwrap(), Transform::Shorten);
        assert_eq!(Transform::parse("formalise").unwrap(), Transform::Formalize);
        let err = Transform::parse("nope").unwrap_err();
        assert!(err.contains("unknown transform"), "{err}");
        // The error names the alternatives, because a user typed it.
        for name in ["tidy", "organize", "shorten", "formalize"] {
            assert!(err.contains(name), "{err}");
        }
    }

    #[test]
    fn prompts_carry_the_transcript_and_forbid_preamble() {
        for t in ALL {
            let p = t.prompt("buy milk later");
            assert!(p.contains("buy milk later"), "{t:?}");
            assert!(p.contains("nothing else"), "{t:?}");
        }
        // Each rewriting transform asks for something different.
        assert!(Transform::Organize.prompt("x").contains("ordered points"));
        assert!(Transform::Shorten.prompt("x").contains("half"));
        assert!(Transform::Formalize.prompt("x").contains("formal"));
    }

    #[test]
    fn every_transform_has_a_label_and_blurb() {
        for t in ALL {
            assert!(!t.label().is_empty(), "{t:?}");
            assert!(t.blurb().contains('.') || t.blurb().ends_with('.'), "{t:?}");
            // Rewrites say so in the blurb, so the UI cannot present
            // them as tidies by accident.
            if t.rewrites() {
                assert!(t.blurb().contains("Rewrites"), "{t:?}");
            }
        }
    }
}
