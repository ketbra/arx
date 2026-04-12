//! Map tree-sitter capture names (`@keyword`, `@function`, etc.) to
//! [`arx_buffer::Face`] values.

use arx_buffer::Face;

/// A syntax theme: an ordered list of `(capture_pattern, Face)` rules.
/// When multiple rules match, the most specific (longest) wins.
#[derive(Debug, Clone)]
pub struct Theme {
    rules: Vec<(String, Face)>,
}

impl Theme {
    /// Look up the face for `capture_name`. Tries the full name first
    /// (`@function.method`), then each parent prefix (`@function`).
    /// Returns `None` if no rule matches.
    pub fn face_for_capture(&self, capture_name: &str) -> Option<Face> {
        // Try the full name, then progressively shorter prefixes.
        let mut name = capture_name;
        loop {
            for (pattern, face) in &self.rules {
                if pattern == name {
                    return Some(face.clone());
                }
            }
            match name.rfind('.') {
                Some(pos) => name = &name[..pos],
                None => return None,
            }
        }
    }

    /// The default dark theme. Covers the standard tree-sitter capture
    /// groups with colours loosely inspired by One Dark / VS Code Dark+.
    #[allow(clippy::unreadable_literal)]
    pub fn default_dark() -> Self {
        let mut rules = Vec::new();

        let mut r = |name: &str, fg: u32| {
            rules.push((
                name.to_owned(),
                Face {
                    fg: Some(fg),
                    ..Face::default()
                },
            ));
        };

        // Keywords, control flow.
        r("keyword", 0xC678DD);
        r("keyword.return", 0xC678DD);
        r("keyword.function", 0xC678DD);
        r("keyword.operator", 0xC678DD);

        // Types.
        r("type", 0xE5C07B);
        r("type.builtin", 0xE5C07B);

        // Functions / methods.
        r("function", 0x61AFEF);
        r("function.method", 0x61AFEF);
        r("function.builtin", 0x56B6C2);
        r("function.macro", 0x61AFEF);

        // Variables, parameters.
        r("variable", 0xABB2BF);
        r("variable.parameter", 0xE06C75);
        r("variable.builtin", 0xE06C75);
        r("property", 0xE06C75);

        // Constants, numbers, booleans.
        r("constant", 0xD19A66);
        r("constant.builtin", 0xD19A66);
        r("number", 0xD19A66);
        r("boolean", 0xD19A66);

        // Strings, characters.
        r("string", 0x98C379);
        r("string.escape", 0x56B6C2);
        r("character", 0x98C379);

        // Comments.
        r("comment", 0x5C6370);

        // Operators, punctuation.
        r("operator", 0x56B6C2);
        r("punctuation", 0xABB2BF);
        r("punctuation.bracket", 0xABB2BF);
        r("punctuation.delimiter", 0xABB2BF);
        r("punctuation.special", 0xC678DD);

        // Attributes / annotations.
        r("attribute", 0xE5C07B);

        // Labels.
        r("label", 0xE06C75);

        // Namespace / module.
        r("namespace", 0xE5C07B);

        Self { rules }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let theme = Theme::default_dark();
        assert!(theme.face_for_capture("keyword").is_some());
    }

    #[test]
    fn hierarchical_fallback() {
        let theme = Theme::default_dark();
        // `keyword.control` isn't a rule, but `keyword` is.
        let face = theme.face_for_capture("keyword.control");
        assert!(face.is_some());
        assert_eq!(face.unwrap().fg, Some(0x00C6_78DD));
    }

    #[test]
    fn unknown_capture_returns_none() {
        let theme = Theme::default_dark();
        assert!(theme.face_for_capture("totally.unknown").is_none());
    }
}
