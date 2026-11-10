//! Palette resolution: an escape-sequence colour to 0xRRGGBB.
use alacritty_terminal::term::color::Colors as Overrides;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// The theme colours the emulator resolves against, all 0xRRGGBB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    pub fg: u32,
    pub bg: u32,
    pub cursor: u32,
    pub ansi: [u32; 16],
}

fn pack(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

/// Resolve `color`. `overrides` are colours a program set at run time (OSC 4, 10, 11); they win
/// over the theme. Named colours beyond the 16 (dim, bright foreground) fall back to the theme.
pub fn resolve(color: Color, overrides: &Overrides, colors: &Colors) -> u32 {
    let idx = match color {
        Color::Spec(rgb) => return pack(rgb.r, rgb.g, rgb.b),
        Color::Named(n) => n as usize,
        Color::Indexed(i) => usize::from(i),
    };
    if let Some(rgb) = overrides[idx] {
        return pack(rgb.r, rgb.g, rgb.b);
    }
    match idx {
        0..=15 => colors.ansi[idx],
        16..=231 => {
            let i = idx - 16;
            let level = |v: usize| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            level(i / 36) << 16 | level((i / 6) % 6) << 8 | level(i % 6)
        }
        232..=255 => {
            let g = 8 + 10 * (idx as u32 - 232);
            g << 16 | g << 8 | g
        }
        _ => {
            const BG: usize = NamedColor::Background as usize;
            const CUR: usize = NamedColor::Cursor as usize;
            const DIM0: usize = NamedColor::DimBlack as usize;
            const DIM7: usize = NamedColor::DimWhite as usize;
            match idx {
                BG => colors.bg,
                CUR => colors.cursor,
                DIM0..=DIM7 => colors.ansi[idx - DIM0],
                _ => colors.fg,
            }
        }
    }
}
