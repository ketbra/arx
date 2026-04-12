//! Convert LSP diagnostics into Arx property-map intervals.

use std::sync::Arc;

use arx_buffer::{
    ByteRange, Diagnostic as ArxDiag, Face, Interval, PropertyValue, Rope, Severity,
    StickyBehavior, UnderlineStyle,
};

use crate::position;

/// Convert a batch of LSP diagnostics into Arx property-map intervals
/// ready to be inserted into the `"diagnostics"` layer. Diagnostics
/// whose ranges can't be mapped (e.g. line out of range) are skipped.
pub fn convert(
    rope: &Rope,
    diagnostics: &[lsp_types::Diagnostic],
) -> Vec<(ByteRange, Interval)> {
    diagnostics
        .iter()
        .filter_map(|d| {
            let range = position::lsp_range_to_bytes(rope, &d.range)?;
            if range.start >= range.end {
                return None;
            }
            let severity = d.severity.map_or(Severity::Hint, convert_severity);
            let arx_diag = ArxDiag {
                severity,
                message: Arc::from(d.message.as_str()),
                code: d.code.as_ref().map(|c| match c {
                    lsp_types::NumberOrString::Number(n) => Arc::from(n.to_string().as_str()),
                    lsp_types::NumberOrString::String(s) => Arc::from(s.as_str()),
                }),
                source: d.source.as_deref().map(Arc::from),
            };
            let face = diagnostic_face(severity);
            // We insert TWO intervals per diagnostic: one Diagnostic
            // (for programmatic access / modeline display) and one
            // Decoration (for the visual underline). The render
            // pipeline merges both.
            // Actually, the StyledRun pipeline applies Diagnostic
            // values and carries them for the renderer, but the
            // underline face comes from a Decoration. Let's use just
            // the Diagnostic variant and set the face there.
            //
            // Looking at the existing code: `apply_value` for
            // `PropertyValue::Diagnostic` only pushes into the
            // diagnostics vec and sets the DIAGNOSTIC flag. It does
            // NOT merge a face. So we need a Decoration for the
            // underline.
            //
            // Return a tuple of (byte_range, decoration_interval) —
            // the caller inserts both the Diagnostic and Decoration.
            let diag_iv = Interval::new(
                range.clone(),
                PropertyValue::Diagnostic(Arc::new(arx_diag)),
                StickyBehavior::Shrink,
            );
            let deco_iv = Interval::new(
                range.clone(),
                PropertyValue::Decoration(face),
                StickyBehavior::Shrink,
            );
            // Return as two intervals packed into a single entry.
            // The caller unpacks.
            Some((range, diag_iv, deco_iv))
        })
        .flat_map(|(range, diag, deco)| vec![(range.clone(), diag), (range, deco)])
        .collect()
}

fn convert_severity(s: lsp_types::DiagnosticSeverity) -> Severity {
    match s {
        lsp_types::DiagnosticSeverity::ERROR => Severity::Error,
        lsp_types::DiagnosticSeverity::WARNING => Severity::Warning,
        lsp_types::DiagnosticSeverity::INFORMATION => Severity::Info,
        _ => Severity::Hint,
    }
}

fn diagnostic_face(severity: Severity) -> Face {
    let (underline_color, style) = match severity {
        Severity::Error => (0xFF_0000, UnderlineStyle::Curly),
        Severity::Warning => (0xFF_CC00, UnderlineStyle::Curly),
        Severity::Info => (0x61_AFEF, UnderlineStyle::Straight),
        Severity::Hint => (0x88_88_88, UnderlineStyle::Straight),
    };
    Face {
        fg: Some(underline_color),
        underline: Some(style),
        priority: 10, // above syntax highlighting
        ..Face::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_buffer::{Buffer, BufferId};

    fn rope(text: &str) -> Rope {
        Buffer::from_str(BufferId(1), text).rope().clone()
    }

    fn lsp_diag(
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
        severity: lsp_types::DiagnosticSeverity,
        message: &str,
    ) -> lsp_types::Diagnostic {
        lsp_types::Diagnostic {
            range: lsp_types::Range::new(
                lsp_types::Position::new(start_line, start_char),
                lsp_types::Position::new(end_line, end_char),
            ),
            severity: Some(severity),
            message: message.to_owned(),
            ..lsp_types::Diagnostic::default()
        }
    }

    #[test]
    fn converts_error_diagnostic() {
        let r = rope("fn main() {\n    let x = 1;\n}\n");
        let diags = vec![lsp_diag(
            1,
            8,
            1,
            9,
            lsp_types::DiagnosticSeverity::ERROR,
            "unused variable",
        )];
        let intervals = convert(&r, &diags);
        // Two intervals per diagnostic (Diagnostic + Decoration).
        assert_eq!(intervals.len(), 2);
        let (range, _iv) = &intervals[0];
        // Line 1, chars 8..9 → "x" in "    let x = 1;".
        // Line 1 starts at byte 12, char 8 = byte 20.
        assert_eq!(range.start, 20);
        assert_eq!(range.end, 21);
    }

    #[test]
    fn skips_out_of_range_diagnostics() {
        let r = rope("hi\n");
        let diags = vec![lsp_diag(
            99,
            0,
            99,
            1,
            lsp_types::DiagnosticSeverity::WARNING,
            "out of range",
        )];
        let intervals = convert(&r, &diags);
        assert!(intervals.is_empty());
    }
}
