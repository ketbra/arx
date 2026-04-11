//! Fully-resolved rendering face.
//!
//! The [`arx_buffer::Face`] used by text property layers is intentionally
//! sparse — every attribute is `Option<T>` so that higher-priority layers
//! can override only the fields they care about. At the render layer we
//! need a concrete face where every attribute has a definite value, so
//! that [`Cell`] equality and diffing are well-defined.
//!
//! [`ResolvedFace::resolve`] collapses a sparse [`arx_buffer::Face`]
//! against a default theme face. This is what the view layer calls once
//! per styled run, then stamps the result onto each cell in that run.
//!
//! [`Cell`]: crate::cell::Cell

use arx_buffer::{Face as SparseFace, UnderlineStyle};

/// 24-bit RGB colour. Terminal backends that don't support truecolour
/// quantise to their nearest palette entry at write time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl Color {
    pub const BLACK: Self = Self(0x0000_0000);
    pub const WHITE: Self = Self(0x00ff_ffff);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    pub const fn r(self) -> u8 {
        ((self.0 >> 16) & 0xff) as u8
    }
    pub const fn g(self) -> u8 {
        ((self.0 >> 8) & 0xff) as u8
    }
    pub const fn b(self) -> u8 {
        (self.0 & 0xff) as u8
    }
}

/// A fully-concrete face ready for a [`crate::cell::Cell`].
///
/// Every attribute has a definite value — no `Option`s. Produced by
/// resolving a sparse [`SparseFace`] (from the buffer's property layers)
/// against a default face from the active theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolvedFace {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: bool,
}

impl ResolvedFace {
    /// Plain face: default terminal colours, no attributes.
    pub const DEFAULT: Self = Self {
        fg: Color::WHITE,
        bg: Color::BLACK,
        bold: false,
        italic: false,
        underline: None,
        strikethrough: false,
    };

    /// Resolve a sparse buffer face on top of a default. Fields set in
    /// `sparse` override the defaults; unset fields inherit.
    pub fn resolve(default: Self, sparse: &SparseFace) -> Self {
        Self {
            fg: sparse.fg.map_or(default.fg, Color),
            bg: sparse.bg.map_or(default.bg, Color),
            bold: sparse.bold.unwrap_or(default.bold),
            italic: sparse.italic.unwrap_or(default.italic),
            underline: sparse.underline.or(default.underline),
            strikethrough: sparse.strikethrough.unwrap_or(default.strikethrough),
        }
    }
}

impl Default for ResolvedFace {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_overrides_only_set_fields() {
        let default = ResolvedFace::DEFAULT;
        let sparse = SparseFace {
            fg: Some(0xff_00_00),
            bold: Some(true),
            ..Default::default()
        };
        let r = ResolvedFace::resolve(default, &sparse);
        assert_eq!(r.fg, Color(0xff_00_00));
        assert!(r.bold);
        // Unset fields inherit.
        assert_eq!(r.bg, default.bg);
        assert!(!r.italic);
    }

    #[test]
    fn rgb_roundtrip() {
        let c = Color::rgb(0x12, 0x34, 0x56);
        assert_eq!(c.r(), 0x12);
        assert_eq!(c.g(), 0x34);
        assert_eq!(c.b(), 0x56);
    }
}
