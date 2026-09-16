//! The seven palettes, and nothing else.
//!
//! Extracted from `tui.rs` because it is one of the few parts of that file with
//! a real seam: a colour table touches no application state, so keeping it in
//! the middle of 5.000 lines of layout and input handling only made both harder
//! to find. Everything here is `pub(super)` — this is an inside of `tui`, not a
//! new public surface.

use ratatui::style::Color;

pub(super) struct Theme {
    pub(super) fg: Color,
    pub(super) dim: Color,
    pub(super) accent: Color,
    pub(super) bg: Color,
    pub(super) run: Color,
    pub(super) idle: Color,
    /// Second agent (Codex). Deliberately off the theme's own hue in every
    /// palette so the two agents never read as the same family of rows —
    /// colour alone is not the whole signal (see the `◆` marker), but it is
    /// what makes them separable at a glance while scrolling.
    pub(super) alt: Color,
}

pub const THEME_NAMES: [&str; 7] =
    ["fosfori", "ambra", "ghiaccio", "synthwave", "matrix", "rosso", "blu"];
pub const THEME_COUNT: usize = THEME_NAMES.len();

pub fn theme_index(name: &str) -> usize {
    match name.to_lowercase().as_str() {
        "fosfori" | "green" | "verde" => 0,
        "ambra" | "amber" => 1,
        "ghiaccio" | "cyan" | "ice" => 2,
        "synthwave" | "wave" => 3,
        "matrix" => 4,
        "rosso" | "red" => 5,
        "blu" | "blue" => 6,
        _ => 0,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}
pub(super) fn theme(idx: usize) -> Theme {
    match idx % THEME_COUNT {
        1 => Theme { fg: rgb(255, 176, 0), dim: rgb(122, 85, 16), accent: rgb(255, 224, 138), bg: rgb(12, 8, 2), run: rgb(255, 210, 74), idle: rgb(185, 132, 42), alt: rgb(120, 200, 255) },
        2 => Theme { fg: rgb(120, 230, 255), dim: rgb(40, 110, 140), accent: rgb(190, 250, 255), bg: rgb(4, 10, 16), run: rgb(90, 235, 200), idle: rgb(255, 210, 120), alt: rgb(255, 190, 120) },
        3 => Theme { fg: rgb(255, 120, 200), dim: rgb(120, 50, 110), accent: rgb(120, 230, 255), bg: rgb(20, 7, 30), run: rgb(120, 255, 180), idle: rgb(255, 215, 120), alt: rgb(160, 255, 170) },
        4 => Theme { fg: rgb(0, 255, 70), dim: rgb(0, 110, 40), accent: rgb(170, 255, 120), bg: rgb(0, 8, 0), run: rgb(120, 255, 160), idle: rgb(200, 255, 120), alt: rgb(120, 210, 255) },
        5 => Theme { fg: rgb(255, 95, 85), dim: rgb(130, 42, 38), accent: rgb(255, 185, 120), bg: rgb(16, 4, 4), run: rgb(255, 145, 120), idle: rgb(255, 200, 120), alt: rgb(150, 220, 255) },
        6 => Theme { fg: rgb(120, 185, 255), dim: rgb(50, 80, 140), accent: rgb(190, 215, 255), bg: rgb(4, 8, 22), run: rgb(120, 255, 205), idle: rgb(255, 215, 120), alt: rgb(255, 190, 130) },
        _ => Theme { fg: rgb(59, 240, 106), dim: rgb(31, 122, 58), accent: rgb(200, 255, 50), bg: rgb(7, 10, 7), run: rgb(91, 255, 143), idle: rgb(255, 204, 51), alt: rgb(255, 170, 220) },
    }
}
