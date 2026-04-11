//! Key representations: [`Key`], [`KeyModifiers`], [`KeyChord`].
//!
//! These are intentionally distinct from `crossterm::event::KeyEvent` and
//! friends so that the user-facing keymap config format stays stable when
//! terminal back-end crates change. A lossless `From<crossterm::KeyEvent>`
//! conversion is provided for the driver.

use std::fmt;

/// A logical key. Abstracts over character input, navigation keys, and
/// semantic markers like `<leader>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A literal character. Case matters; `Shift` is encoded in
    /// [`KeyModifiers::shift`] only for non-printable keys.
    Char(char),
    /// A named key (Enter, Escape, F1, …).
    Named(NamedKey),
    /// The `<leader>` sentinel. At keymap-resolution time the engine
    /// substitutes whatever the active profile declares as its leader.
    Leader,
}

/// Named non-character keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    Space,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    /// Function keys F1..F24. KEDIT users will want the full 24.
    F(u8),
}

impl fmt::Display for NamedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enter => write!(f, "Enter"),
            Self::Escape => write!(f, "Esc"),
            Self::Backspace => write!(f, "Backspace"),
            Self::Tab => write!(f, "Tab"),
            Self::BackTab => write!(f, "BackTab"),
            Self::Space => write!(f, "Space"),
            Self::Delete => write!(f, "Delete"),
            Self::Insert => write!(f, "Insert"),
            Self::Home => write!(f, "Home"),
            Self::End => write!(f, "End"),
            Self::PageUp => write!(f, "PageUp"),
            Self::PageDown => write!(f, "PageDown"),
            Self::Left => write!(f, "Left"),
            Self::Right => write!(f, "Right"),
            Self::Up => write!(f, "Up"),
            Self::Down => write!(f, "Down"),
            Self::F(n) => write!(f, "F{n}"),
        }
    }
}

/// Modifier flags for a key chord.
// We use four parallel bools rather than bitflags for config clarity —
// writing `KeyModifiers { ctrl: true, shift: true, .. }` is more
// readable in call sites than flag bit-ors. `meta` is rare in terminals
// but preserved so the representation doesn't lose info.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// `Super`/`Cmd`/`Windows` key. Rarely used in terminal editors but
    /// recognised when crossterm reports it.
    pub meta: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };

    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };

    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };

    pub const fn is_empty(self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.meta)
    }

    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
}

/// A single key press with its active modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub key: Key,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    pub const fn new(key: Key, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    /// Convenience: a bare character with no modifiers.
    pub fn char(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: KeyModifiers::NONE,
        }
    }

    /// Convenience: Ctrl+char.
    pub fn ctrl(c: char) -> Self {
        Self {
            key: Key::Char(c),
            modifiers: KeyModifiers::CTRL,
        }
    }

    /// Convenience: a bare named key.
    pub fn named(k: NamedKey) -> Self {
        Self {
            key: Key::Named(k),
            modifiers: KeyModifiers::NONE,
        }
    }

    /// Whether this chord is a "printable" keystroke — a bare character
    /// with at most Shift held. Used by the input dispatcher to decide
    /// between "execute unbound fallback" and "ignore".
    pub fn is_printable_char(&self) -> bool {
        matches!(self.key, Key::Char(_)) && !self.modifiers.ctrl && !self.modifiers.alt
    }

    /// If this chord represents a single printable character (letter,
    /// digit, punctuation), return it. Respects shift/no-shift via the
    /// character's case.
    pub fn as_printable_char(&self) -> Option<char> {
        if self.is_printable_char() {
            if let Key::Char(c) = self.key {
                return Some(c);
            }
        }
        None
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Emacs-like: "C-M-x", "<F5>", "<C-Enter>"
        let needs_brackets = matches!(&self.key, Key::Named(_) | Key::Leader)
            || !self.modifiers.is_empty()
                && matches!(&self.key, Key::Named(_) | Key::Leader);
        if needs_brackets || matches!(&self.key, Key::Named(_) | Key::Leader) {
            f.write_str("<")?;
            if self.modifiers.ctrl {
                f.write_str("C-")?;
            }
            if self.modifiers.alt {
                f.write_str("M-")?;
            }
            if self.modifiers.shift {
                f.write_str("S-")?;
            }
            match &self.key {
                Key::Char(c) => write!(f, "{c}")?,
                Key::Named(n) => write!(f, "{n}")?,
                Key::Leader => f.write_str("leader")?,
            }
            f.write_str(">")?;
        } else {
            if self.modifiers.ctrl {
                f.write_str("C-")?;
            }
            if self.modifiers.alt {
                f.write_str("M-")?;
            }
            if self.modifiers.shift {
                f.write_str("S-")?;
            }
            if let Key::Char(c) = &self.key {
                write!(f, "{c}")?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Crossterm conversion
// ---------------------------------------------------------------------------

impl From<&crossterm::event::KeyEvent> for KeyChord {
    fn from(ev: &crossterm::event::KeyEvent) -> Self {
        use crossterm::event::{KeyCode, KeyModifiers as XtermMods};

        let key = match ev.code {
            KeyCode::Char(c) => Key::Char(c),
            KeyCode::Enter => Key::Named(NamedKey::Enter),
            KeyCode::Esc => Key::Named(NamedKey::Escape),
            KeyCode::Backspace => Key::Named(NamedKey::Backspace),
            KeyCode::Tab => Key::Named(NamedKey::Tab),
            KeyCode::BackTab => Key::Named(NamedKey::BackTab),
            KeyCode::Delete => Key::Named(NamedKey::Delete),
            KeyCode::Insert => Key::Named(NamedKey::Insert),
            KeyCode::Home => Key::Named(NamedKey::Home),
            KeyCode::End => Key::Named(NamedKey::End),
            KeyCode::PageUp => Key::Named(NamedKey::PageUp),
            KeyCode::PageDown => Key::Named(NamedKey::PageDown),
            KeyCode::Left => Key::Named(NamedKey::Left),
            KeyCode::Right => Key::Named(NamedKey::Right),
            KeyCode::Up => Key::Named(NamedKey::Up),
            KeyCode::Down => Key::Named(NamedKey::Down),
            KeyCode::F(n) => Key::Named(NamedKey::F(n)),
            // Anything we don't recognise becomes a best-effort unknown
            // char so the keymap layer can still fall through cleanly.
            _ => Key::Char('\u{0}'),
        };
        let modifiers = KeyModifiers {
            ctrl: ev.modifiers.contains(XtermMods::CONTROL),
            alt: ev.modifiers.contains(XtermMods::ALT),
            shift: ev.modifiers.contains(XtermMods::SHIFT),
            meta: ev.modifiers.contains(XtermMods::META)
                || ev.modifiers.contains(XtermMods::SUPER),
        };
        Self { key, modifiers }
    }
}

impl From<crossterm::event::KeyEvent> for KeyChord {
    fn from(ev: crossterm::event::KeyEvent) -> Self {
        Self::from(&ev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_helpers() {
        assert!(KeyModifiers::NONE.is_empty());
        assert!(!KeyModifiers::CTRL.is_empty());
        let m = KeyModifiers::NONE.with_ctrl().with_shift();
        assert!(m.ctrl && m.shift && !m.alt);
    }

    #[test]
    fn printable_char_detection() {
        assert!(KeyChord::char('a').is_printable_char());
        assert_eq!(KeyChord::char('a').as_printable_char(), Some('a'));
        assert!(!KeyChord::ctrl('a').is_printable_char());
        assert!(!KeyChord::named(NamedKey::Enter).is_printable_char());
    }

    #[test]
    fn from_crossterm_char() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as XM};
        let ev = KeyEvent::new(KeyCode::Char('x'), XM::CONTROL);
        let chord = KeyChord::from(&ev);
        assert_eq!(chord, KeyChord::ctrl('x'));
    }

    #[test]
    fn from_crossterm_named() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as XM};
        let ev = KeyEvent::new(KeyCode::F(5), XM::NONE);
        let chord = KeyChord::from(&ev);
        assert_eq!(chord, KeyChord::named(NamedKey::F(5)));
    }
}
