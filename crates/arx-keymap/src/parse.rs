//! Parser for key-sequence strings.
//!
//! Accepted grammar (informal):
//!
//! ```text
//! sequence := chord ( space+ chord )*
//! chord    := mod-prefix* ( char | bracketed )
//! bracketed:= '<' mod-prefix* ( char | name ) '>'
//! mod-prefix := 'C-' | 'M-' | 'S-' | 'A-'
//! name     := 'Enter' | 'Esc' | 'Escape' | 'Tab' | 'BackTab'
//!           | 'Space' | 'Backspace' | 'BS' | 'Delete' | 'Del'
//!           | 'Insert' | 'Ins' | 'Home' | 'End'
//!           | 'PageUp' | 'PgUp' | 'PageDown' | 'PgDn'
//!           | 'Left' | 'Right' | 'Up' | 'Down'
//!           | 'F' digits | 'PF' digits | 'leader'
//! ```
//!
//! Modifier prefixes are case-insensitive for their letters but a single
//! trailing `-` is required. Key names are case-insensitive.
//!
//! Examples:
//!
//! * `"C-x C-s"` — Emacs save.
//! * `"<C-x> <C-c>"` — same chord in bracketed form.
//! * `"<Esc>"` — bare Escape.
//! * `"g g"` — press `g` twice (Vim go-to-top).
//! * `"<leader> f f"` — leader, then `f`, then `f`.

use thiserror::Error;

use crate::key::{Key, KeyChord, KeyModifiers, NamedKey};

/// Errors returned by [`parse_sequence`] and [`parse_chord`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("empty key sequence")]
    Empty,
    #[error("unterminated bracketed key at byte {pos}")]
    UnterminatedBracket { pos: usize },
    #[error("unknown named key: {name:?}")]
    UnknownNamedKey { name: String },
    #[error("invalid function key number: {raw:?}")]
    InvalidFunctionKey { raw: String },
    #[error("empty chord at byte {pos}")]
    EmptyChord { pos: usize },
    #[error("stray modifier prefix at byte {pos}")]
    DanglingModifier { pos: usize },
}

/// Parse a whitespace-separated sequence of chords.
pub fn parse_sequence(input: &str) -> Result<Vec<KeyChord>, ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut out = Vec::new();
    let mut parser = Parser::new(trimmed);
    while !parser.eof() {
        parser.skip_whitespace();
        if parser.eof() {
            break;
        }
        let chord = parser.parse_chord()?;
        out.push(chord);
    }
    if out.is_empty() {
        Err(ParseError::Empty)
    } else {
        Ok(out)
    }
}

/// Parse a single chord. Convenience for callers that already know they
/// have exactly one chord.
pub fn parse_chord(input: &str) -> Result<KeyChord, ParseError> {
    let mut seq = parse_sequence(input)?;
    if seq.len() != 1 {
        return Err(ParseError::Empty);
    }
    Ok(seq.pop().unwrap())
}

struct Parser<'a> {
    src: &'a str,
    /// Byte index into `src`.
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn parse_chord(&mut self) -> Result<KeyChord, ParseError> {
        let start = self.pos;
        let mut modifiers = KeyModifiers::default();

        // Bracketed form: <...>. Only enter bracketed parsing if
        // there is actually a closing `>` — a bare `<` is the literal
        // less-than character (used e.g. for Vim's `<<` dedent).
        if self.peek() == Some('<') && self.src[self.pos..].contains('>') {
            return self.parse_bracketed();
        }

        // Bare form: modifier prefixes followed by a single char or
        // a bracketed key name.
        while let Some(m) = self.try_parse_modifier_prefix() {
            match m {
                'C' => modifiers.ctrl = true,
                'M' | 'A' => modifiers.alt = true,
                'S' => modifiers.shift = true,
                _ => unreachable!(),
            }
        }

        // After consuming modifier prefixes, a `<` starts a
        // bracketed name (e.g. `C-<Space>`, `M-<Enter>`). But a
        // bare `<` without a closing `>` is just the literal `<`
        // character (as in `M-<` for buffer-start).
        if self.peek() == Some('<') && self.src[self.pos..].contains('>') {
            let mut chord = self.parse_bracketed()?;
            chord.modifiers.ctrl |= modifiers.ctrl;
            chord.modifiers.alt |= modifiers.alt;
            chord.modifiers.shift |= modifiers.shift;
            return Ok(chord);
        }

        let ch = self.advance().ok_or(ParseError::EmptyChord { pos: start })?;
        if ch.is_whitespace() {
            return Err(ParseError::DanglingModifier { pos: start });
        }
        Ok(KeyChord {
            key: Key::Char(ch),
            modifiers,
        })
    }

    /// If the upcoming two bytes are `X-` where `X` is a modifier letter,
    /// consume them and return the letter. Otherwise leave the cursor.
    fn try_parse_modifier_prefix(&mut self) -> Option<char> {
        let bytes = self.src.as_bytes();
        if self.pos + 1 >= bytes.len() {
            return None;
        }
        let a = bytes[self.pos];
        let b = bytes[self.pos + 1];
        if b != b'-' {
            return None;
        }
        let letter = match a.to_ascii_uppercase() {
            b'C' => 'C',
            b'M' => 'M',
            b'A' => 'A',
            b'S' => 'S',
            _ => return None,
        };
        // Don't consume "C-" if only one char remains afterwards and it's
        // whitespace — that's a dangling modifier. The outer parser will
        // still error later, but we do the easy guard here.
        self.pos += 2;
        Some(letter)
    }

    fn parse_bracketed(&mut self) -> Result<KeyChord, ParseError> {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some('<'));
        self.advance(); // consume '<'

        let mut modifiers = KeyModifiers::default();
        while let Some(m) = self.try_parse_modifier_prefix() {
            match m {
                'C' => modifiers.ctrl = true,
                'M' | 'A' => modifiers.alt = true,
                'S' => modifiers.shift = true,
                _ => unreachable!(),
            }
        }

        // Read until '>'.
        let name_start = self.pos;
        let rel = self.src[name_start..]
            .find('>')
            .ok_or(ParseError::UnterminatedBracket { pos: start })?;
        let name = &self.src[name_start..name_start + rel];
        self.pos = name_start + rel + 1; // skip '>'
        if name.is_empty() {
            return Err(ParseError::EmptyChord { pos: start });
        }
        let key = parse_named_key(name)?;
        Ok(KeyChord { key, modifiers })
    }
}

fn parse_named_key(name: &str) -> Result<Key, ParseError> {
    // Single character: treat as Char.
    if name.chars().count() == 1 {
        return Ok(Key::Char(name.chars().next().unwrap()));
    }
    let lowered = name.to_ascii_lowercase();
    let named = match lowered.as_str() {
        "enter" | "return" | "ret" => NamedKey::Enter,
        "esc" | "escape" => NamedKey::Escape,
        "backspace" | "bs" => NamedKey::Backspace,
        "tab" => NamedKey::Tab,
        "backtab" | "btab" | "s-tab" => NamedKey::BackTab,
        "space" | "spc" => NamedKey::Space,
        "delete" | "del" => NamedKey::Delete,
        "insert" | "ins" => NamedKey::Insert,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" | "pgup" | "prior" => NamedKey::PageUp,
        "pagedown" | "pgdn" | "next" => NamedKey::PageDown,
        "left" => NamedKey::Left,
        "right" => NamedKey::Right,
        "up" => NamedKey::Up,
        "down" => NamedKey::Down,
        "leader" => return Ok(Key::Leader),
        _ => {
            // F1..F24 / PF1..PF24.
            if let Some(rest) = lowered
                .strip_prefix("pf")
                .or_else(|| lowered.strip_prefix('f'))
            {
                return rest
                    .parse::<u8>()
                    .ok()
                    .filter(|n| (1..=24).contains(n))
                    .map(|n| Key::Named(NamedKey::F(n)))
                    .ok_or_else(|| ParseError::InvalidFunctionKey {
                        raw: name.to_string(),
                    });
            }
            return Err(ParseError::UnknownNamedKey {
                name: name.to_string(),
            });
        }
    };
    Ok(Key::Named(named))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: char) -> KeyChord {
        KeyChord::char(c)
    }
    fn ctrl(c: char) -> KeyChord {
        KeyChord::ctrl(c)
    }
    fn named(n: NamedKey) -> KeyChord {
        KeyChord::named(n)
    }

    #[test]
    fn bare_chars() {
        assert_eq!(parse_sequence("a").unwrap(), vec![ch('a')]);
        assert_eq!(parse_sequence("a b c").unwrap(), vec![ch('a'), ch('b'), ch('c')]);
    }

    #[test]
    fn emacs_ctrl_sequence() {
        assert_eq!(
            parse_sequence("C-x C-s").unwrap(),
            vec![ctrl('x'), ctrl('s')]
        );
    }

    #[test]
    fn meta_and_alt_are_same() {
        assert_eq!(parse_sequence("M-x").unwrap(), parse_sequence("A-x").unwrap());
    }

    #[test]
    fn bracketed_named_keys() {
        assert_eq!(parse_sequence("<Enter>").unwrap(), vec![named(NamedKey::Enter)]);
        assert_eq!(parse_sequence("<Esc>").unwrap(), vec![named(NamedKey::Escape)]);
        assert_eq!(parse_sequence("<F5>").unwrap(), vec![named(NamedKey::F(5))]);
        assert_eq!(
            parse_sequence("<PF12>").unwrap(),
            vec![named(NamedKey::F(12))]
        );
    }

    #[test]
    fn bracketed_with_modifiers() {
        let chord = parse_chord("<C-Enter>").unwrap();
        assert_eq!(
            chord,
            KeyChord {
                key: Key::Named(NamedKey::Enter),
                modifiers: KeyModifiers::CTRL,
            }
        );
    }

    #[test]
    fn combined_modifiers() {
        let chord = parse_chord("C-M-S-a").unwrap();
        assert!(chord.modifiers.ctrl);
        assert!(chord.modifiers.alt);
        assert!(chord.modifiers.shift);
        assert_eq!(chord.key, Key::Char('a'));
    }

    #[test]
    fn vim_style_sequence() {
        let seq = parse_sequence("g g").unwrap();
        assert_eq!(seq, vec![ch('g'), ch('g')]);
    }

    #[test]
    fn leader_sequence() {
        let seq = parse_sequence("<leader> f f").unwrap();
        assert_eq!(seq.len(), 3);
        assert_eq!(seq[0].key, Key::Leader);
        assert_eq!(seq[1], ch('f'));
    }

    #[test]
    fn unknown_named_key_errors() {
        let err = parse_sequence("<NoSuchKey>").unwrap_err();
        assert!(matches!(err, ParseError::UnknownNamedKey { .. }));
    }

    #[test]
    fn unterminated_bracket_errors() {
        let err = parse_sequence("<Enter").unwrap_err();
        assert!(matches!(err, ParseError::UnterminatedBracket { .. }));
    }

    #[test]
    fn empty_input_errors() {
        assert_eq!(parse_sequence("").unwrap_err(), ParseError::Empty);
        assert_eq!(parse_sequence("   ").unwrap_err(), ParseError::Empty);
    }

    #[test]
    fn invalid_function_key_errors() {
        assert!(matches!(
            parse_sequence("<F99>").unwrap_err(),
            ParseError::InvalidFunctionKey { .. }
        ));
    }
}
