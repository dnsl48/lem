//! The terminal's own default colours, for `Hello`.
//!
//! Lem judges light or dark theme mode from the background it is told
//! (`display-background-mode`, `src/interface.lisp`), and falls back to the
//! default colours for attributes that name none. Without these, the relay
//! assumes a dark `#111111`, as ncurses assumes black.
//!
//! Asked with OSC 10 and 11 through `terminal-colorsaurus`, which follows
//! the query with DA1, a request every terminal answers: a terminal that
//! answers DA1 first does not support the colour query, and nobody waits
//! for a timeout. It writes only to a standard stream that is a terminal,
//! or to `/dev/tty`, never to this process's stdout, which is the wire.

use terminal_colorsaurus::{Color, QueryOptions, color_palette};

/// Packed `0xRRGGBB`, or `None` for each colour the terminal did not give.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub foreground: Option<u32>,
    pub background: Option<u32>,
}

/// Ask the terminal. Any failure (no terminal, no support, no answer
/// within the library's one-second timeout) is no colours: the relay's
/// defaults then stand, as before this existed.
pub fn query() -> Colors {
    match color_palette(QueryOptions::default()) {
        Ok(palette) => Colors {
            foreground: Some(pack(&palette.foreground)),
            background: Some(pack(&palette.background)),
        },
        Err(_) => Colors::default(),
    }
}

fn pack(color: &Color) -> u32 {
    pack_channels(color.r, color.g, color.b)
}

/// 16-bit channels as packed 8-bit `0xRRGGBB`: the high byte of each.
fn pack_channels(r: u16, g: u16, b: u16) -> u32 {
    let byte = |channel: u16| u32::from(channel >> 8);
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_bit_channels_pack_to_their_high_bytes() {
        assert_eq!(pack_channels(0x1c1c, 0xffff, 0x0000), 0x1cff00);
    }
}
