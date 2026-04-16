//! Translate `winit` keyboard events into `crossterm::event::Event`s so
//! they can reuse [`arx_driver::InputTask`]'s existing dispatch path.
//!
//! The translation is intentionally narrow:
//!
//! * Only `ElementState::Pressed` events produce output. Releases are
//!   dropped (the existing keymap engine is press-driven).
//! * Logical (post-IME, post-layout) keys are the source of truth.
//! * Modifier state (Ctrl / Shift / Alt / Super) is carried in on the
//!   winit `Modifiers` event and applied at translation time.
//! * We aim to round-trip `KeyChord::from(&crossterm::KeyEvent)` — so
//!   printable characters map via `KeyCode::Char`, and a curated list
//!   of named keys maps 1:1.
//! * Anything we don't recognise returns `None`; the caller just
//!   drops the event.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton as XtermMouseButton, MouseEvent,
    MouseEventKind,
};
use winit::event::{ElementState, KeyEvent as WinitKeyEvent, MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Convert a winit key event to a `crossterm::event::Event`.
///
/// Returns `None` for events we don't care to forward — modifier-only
/// presses, releases, unmappable keys.
pub fn translate_key(ev: &WinitKeyEvent, mods: ModifiersState) -> Option<Event> {
    if ev.state != ElementState::Pressed {
        return None;
    }
    let code = translate_logical_key(&ev.logical_key)?;
    let mut kmods = translate_mods(mods);

    // Printable characters encode Shift in the glyph itself
    // (Shift+a → 'A', Shift+, → '<'), so strip the Shift bit to match
    // the normalisation in `arx_keymap::key::KeyChord::from`. Named
    // keys (Tab, F-keys) keep Shift — `S-Tab` is a legitimate chord.
    if matches!(code, KeyCode::Char(c) if c.is_ascii_graphic()) {
        kmods.remove(KeyModifiers::SHIFT);
    }
    Some(Event::Key(KeyEvent::new(code, kmods)))
}

/// Project winit's four-bit modifier state onto crossterm's.
pub(crate) fn translate_mods(mods: ModifiersState) -> KeyModifiers {
    let mut out = KeyModifiers::empty();
    if mods.control_key() {
        out |= KeyModifiers::CONTROL;
    }
    if mods.alt_key() {
        out |= KeyModifiers::ALT;
    }
    if mods.shift_key() {
        out |= KeyModifiers::SHIFT;
    }
    if mods.super_key() {
        out |= KeyModifiers::SUPER;
    }
    out
}

/// Map a winit logical key into a crossterm key code.
pub(crate) fn translate_logical_key(key: &Key) -> Option<KeyCode> {
    match key {
        Key::Named(named) => translate_named_key(*named),
        Key::Character(s) => {
            // winit hands us the composed character post-layout. We
            // take the first grapheme as the canonical character
            // (multi-codepoint clusters become a single Char), because
            // crossterm KeyCode::Char is a single `char`.
            s.chars().next().map(KeyCode::Char)
        }
        // Unidentified / modifier-only / dead keys: drop.
        _ => None,
    }
}

/// 1:1 named-key translation table. Keep narrow — anything we haven't
/// explicitly listed returns `None` and is silently dropped.
pub(crate) fn translate_named_key(named: NamedKey) -> Option<KeyCode> {
    use NamedKey as N;
    Some(match named {
        N::Enter => KeyCode::Enter,
        N::Escape => KeyCode::Esc,
        N::Backspace => KeyCode::Backspace,
        N::Tab => KeyCode::Tab,
        N::Delete => KeyCode::Delete,
        N::Insert => KeyCode::Insert,
        N::Home => KeyCode::Home,
        N::End => KeyCode::End,
        N::PageUp => KeyCode::PageUp,
        N::PageDown => KeyCode::PageDown,
        N::ArrowLeft => KeyCode::Left,
        N::ArrowRight => KeyCode::Right,
        N::ArrowUp => KeyCode::Up,
        N::ArrowDown => KeyCode::Down,
        N::Space => KeyCode::Char(' '),
        N::F1 => KeyCode::F(1),
        N::F2 => KeyCode::F(2),
        N::F3 => KeyCode::F(3),
        N::F4 => KeyCode::F(4),
        N::F5 => KeyCode::F(5),
        N::F6 => KeyCode::F(6),
        N::F7 => KeyCode::F(7),
        N::F8 => KeyCode::F(8),
        N::F9 => KeyCode::F(9),
        N::F10 => KeyCode::F(10),
        N::F11 => KeyCode::F(11),
        N::F12 => KeyCode::F(12),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Mouse translation
// ---------------------------------------------------------------------------

/// Translate a winit mouse-button press/release into a crossterm
/// [`MouseEvent`]. Coordinates must already be in cell space.
pub fn translate_mouse_button(
    button: MouseButton,
    state: ElementState,
    col: u16,
    row: u16,
    mods: ModifiersState,
) -> Option<Event> {
    let xbutton = match button {
        MouseButton::Left => XtermMouseButton::Left,
        MouseButton::Right => XtermMouseButton::Right,
        MouseButton::Middle => XtermMouseButton::Middle,
        _ => return None,
    };
    let kind = match state {
        ElementState::Pressed => MouseEventKind::Down(xbutton),
        ElementState::Released => MouseEventKind::Up(xbutton),
    };
    Some(Event::Mouse(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: translate_mods(mods),
    }))
}

/// Translate a winit cursor-move into a crossterm drag event.
///
/// Only generates a drag event for the left button (the editor only
/// handles left-drag). Caller should only call this while the left
/// button is held.
pub fn translate_mouse_drag(col: u16, row: u16, mods: ModifiersState) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(XtermMouseButton::Left),
        column: col,
        row,
        modifiers: translate_mods(mods),
    })
}

/// Translate a winit scroll event into crossterm scroll events.
///
/// Winit reports scroll in fractional pixels or lines depending on
/// the device. We collapse into `ScrollUp` / `ScrollDown` lines.
pub fn translate_scroll(
    delta: MouseScrollDelta,
    col: u16,
    row: u16,
    mods: ModifiersState,
) -> Option<Event> {
    let lines = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(pos) => {
            // Convert pixels to approximate lines (16px ≈ 1 line).
            (pos.y / 16.0) as f32
        }
    };
    if lines.abs() < 0.1 {
        return None;
    }
    let kind = if lines > 0.0 {
        MouseEventKind::ScrollUp
    } else {
        MouseEventKind::ScrollDown
    };
    Some(Event::Mouse(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: translate_mods(mods),
    }))
}

#[cfg(test)]
mod tests {
    // We test the pure helpers directly — `winit::event::KeyEvent` has
    // a non-exhaustive `platform_specific` field that isn't portably
    // constructible from user code, so `translate_key`'s top-level
    // behaviour is covered indirectly through the helpers it delegates
    // to.

    use super::*;

    #[test]
    fn logical_char_maps_to_char_code() {
        assert_eq!(
            translate_logical_key(&Key::Character("a".into())),
            Some(KeyCode::Char('a'))
        );
        assert_eq!(
            translate_logical_key(&Key::Character("A".into())),
            Some(KeyCode::Char('A'))
        );
        assert_eq!(
            translate_logical_key(&Key::Character("/".into())),
            Some(KeyCode::Char('/'))
        );
    }

    #[test]
    fn named_keys_translate() {
        assert_eq!(translate_named_key(NamedKey::Enter), Some(KeyCode::Enter));
        assert_eq!(translate_named_key(NamedKey::Escape), Some(KeyCode::Esc));
        assert_eq!(translate_named_key(NamedKey::Tab), Some(KeyCode::Tab));
        assert_eq!(
            translate_named_key(NamedKey::Backspace),
            Some(KeyCode::Backspace)
        );
        assert_eq!(
            translate_named_key(NamedKey::ArrowLeft),
            Some(KeyCode::Left)
        );
        assert_eq!(
            translate_named_key(NamedKey::PageDown),
            Some(KeyCode::PageDown)
        );
        assert_eq!(
            translate_named_key(NamedKey::Space),
            Some(KeyCode::Char(' '))
        );
        assert_eq!(translate_named_key(NamedKey::F1), Some(KeyCode::F(1)));
        assert_eq!(translate_named_key(NamedKey::F12), Some(KeyCode::F(12)));
    }

    #[test]
    fn unmapped_named_keys_return_none() {
        // Shift is a modifier, not a chord on its own — should drop.
        assert_eq!(translate_named_key(NamedKey::Shift), None);
        assert_eq!(translate_named_key(NamedKey::Control), None);
        assert_eq!(translate_named_key(NamedKey::Alt), None);
    }

    #[test]
    fn mods_ctrl_only() {
        let out = translate_mods(ModifiersState::CONTROL);
        assert!(out.contains(KeyModifiers::CONTROL));
        assert!(!out.contains(KeyModifiers::SHIFT));
        assert!(!out.contains(KeyModifiers::ALT));
    }

    #[test]
    fn mods_all_four() {
        let state = ModifiersState::CONTROL
            | ModifiersState::SHIFT
            | ModifiersState::ALT
            | ModifiersState::SUPER;
        let out = translate_mods(state);
        assert!(out.contains(KeyModifiers::CONTROL));
        assert!(out.contains(KeyModifiers::SHIFT));
        assert!(out.contains(KeyModifiers::ALT));
        assert!(out.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn mods_empty_translates_empty() {
        assert_eq!(translate_mods(ModifiersState::empty()), KeyModifiers::empty());
    }
}
