//! Convert alacritty's terminal grid into a renderer-agnostic
//! [`TerminalSnapshot`].

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use crate::{Listener, TerminalCell, TerminalSnapshot};

/// Snapshot the current terminal grid into a flat `rows × cols` matrix
/// of [`TerminalCell`] values.
pub fn snapshot(term: &Term<Listener>) -> TerminalSnapshot {
    let grid = term.grid();
    let cols = grid.columns();
    let total_rows = grid.screen_lines();

    let mut cells = Vec::with_capacity(total_rows);
    for row_idx in 0..total_rows {
        let mut row = Vec::with_capacity(cols);
        let line = alacritty_terminal::index::Line(row_idx as i32);
        for col_idx in 0..cols {
            let col = alacritty_terminal::index::Column(col_idx);
            let cell = &grid[line][col];
            let c = cell.c.to_string();
            let fg = convert_color(cell.fg);
            let bg = convert_color(cell.bg);
            let flags = cell.flags;
            row.push(TerminalCell {
                c,
                fg,
                bg,
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.contains(Flags::UNDERLINE),
            });
        }
        cells.push(row);
    }

    let cursor_point = term.grid().cursor.point;
    let cursor = Some((cursor_point.column.0 as u16, cursor_point.line.0 as u16));

    TerminalSnapshot {
        cells,
        cursor,
        cols: cols as u16,
        rows: total_rows as u16,
    }
}

fn convert_color(color: AnsiColor) -> u32 {
    match color {
        AnsiColor::Spec(rgb) => {
            u32::from(rgb.r) << 16 | u32::from(rgb.g) << 8 | u32::from(rgb.b)
        }
        AnsiColor::Named(named) => named_color_rgb(named),
        AnsiColor::Indexed(idx) => indexed_color_rgb(idx),
    }
}

/// Map the 16 standard ANSI named colours to a dark-theme palette.
fn named_color_rgb(c: NamedColor) -> u32 {
    match c {
        NamedColor::Black | NamedColor::Background => 0x28_2C34,
        NamedColor::Red | NamedColor::BrightRed => 0xE0_6C75,
        NamedColor::Green | NamedColor::BrightGreen => 0x98_C379,
        NamedColor::Yellow | NamedColor::BrightYellow => 0xE5_C07B,
        NamedColor::Blue | NamedColor::BrightBlue => 0x61_AFEF,
        NamedColor::Magenta | NamedColor::BrightMagenta => 0xC6_78DD,
        NamedColor::Cyan | NamedColor::BrightCyan => 0x56_B6C2,
        NamedColor::BrightBlack => 0x5C_6370,
        NamedColor::BrightWhite => 0xFF_FFFF,
        _ => 0xAB_B2BF, // White, Foreground, and others
    }
}

/// Map 256-colour index to RGB. Indices 0–15 are the named colours;
/// 16–231 are a 6×6×6 colour cube; 232–255 are a greyscale ramp.
fn indexed_color_rgb(idx: u8) -> u32 {
    if idx < 16 {
        // Use the named colour mapping for the first 16.
        return named_color_rgb(match idx {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            15 => NamedColor::BrightWhite,
            _ => unreachable!(),
        });
    }
    if idx < 232 {
        // 6×6×6 colour cube.
        let idx = idx - 16;
        let r = (idx / 36) * 51;
        let g = ((idx % 36) / 6) * 51;
        let b = (idx % 6) * 51;
        return u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    }
    // Greyscale ramp 232–255.
    let grey = 8 + (idx - 232) * 10;
    u32::from(grey) << 16 | u32::from(grey) << 8 | u32::from(grey)
}
