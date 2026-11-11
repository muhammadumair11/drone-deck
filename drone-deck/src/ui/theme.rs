//! Design tokens from `Design/*.html`: black surfaces, one light ink, translucent rules,
//! uppercase DIN type (Bahnschrift stands in for D-DIN on Windows) and mono metadata.
use rune::{Color, El, Family, TextCase, TextStyle, text};
use std::cell::Cell;

thread_local! {
    /// Whether this frame draws with the light palette; set once per frame by `ui::view`
    /// from the theme preference and Windows' app mode.
    static LIGHT: Cell<bool> = const { Cell::new(false) };
}
/// Selects the palette the colour functions below return for the rest of this frame.
pub(crate) fn set_light(on: bool) {
    LIGHT.with(|l| l.set(on));
}
pub(crate) fn light() -> bool {
    LIGHT.with(Cell::get)
}

pub(crate) fn c(r: u8, g: u8, b: u8) -> Color {
    fn l(v: u8) -> f32 {
        let x = v as f32 / 255.;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    Color {
        r: l(r),
        g: l(g),
        b: l(b),
        a: 1.,
    }
}

/// `--bg` / `--surface`.
pub(crate) fn bg() -> Color {
    if light() {
        c(248, 248, 250)
    } else {
        c(0, 0, 0)
    }
}
/// `--fg` / `--accent`.
pub(crate) fn fg() -> Color {
    if light() {
        c(20, 20, 28)
    } else {
        c(240, 240, 250)
    }
}
/// `--accent-hover`.
pub(crate) fn fg_hover() -> Color {
    if light() {
        c(0, 0, 0)
    } else {
        c(255, 255, 255)
    }
}
/// `--muted`.
pub(crate) fn muted() -> Color {
    fg().alpha(0.7)
}
/// `--border`.
pub(crate) fn rule() -> Color {
    fg().alpha(0.35)
}
/// `--border-soft`, also the selected and hovered wash. Rules between rows inside a section.
pub(crate) fn soft() -> Color {
    fg().alpha(0.1)
}
/// Rule above a section heading: the strongest line on a page, so sections read as separate.
pub(crate) fn section_rule() -> Color {
    fg().alpha(0.45)
}
/// Rule above a sub-section and between side-by-side columns: between `section_rule` and `soft`.
pub(crate) fn divider() -> Color {
    fg().alpha(0.22)
}
/// Status colours; the light palette uses darker shades that keep their contrast on white.
pub(crate) fn success() -> Color {
    if light() {
        c(0x15, 0x80, 0x3d)
    } else {
        c(0x16, 0xa3, 0x4a)
    }
}
pub(crate) fn warn() -> Color {
    if light() {
        c(0xb4, 0x53, 0x09)
    } else {
        c(0xea, 0xb3, 0x08)
    }
}
pub(crate) fn danger() -> Color {
    if light() {
        c(0xb9, 0x1c, 0x1c)
    } else {
        c(0xdc, 0x26, 0x26)
    }
}

/// The design's `--font-display` / `--font-body` resolve to the app's sans family
/// (Bahnschrift, set in `AppSpec.families`).
pub(crate) const SANS: Family = Family::Sans;
pub(crate) const MONO: Family = Family::SystemMono;

/// Uppercase text with letter spacing given in em, like the design's CSS.
pub(crate) fn caps(t: impl Into<String>, size: f32, weight: f32, em: f32, color: Color) -> El {
    text(t)
        .font(
            TextStyle::new(size, weight)
                .family(SANS)
                .tracking(em * size),
        )
        .text_case(TextCase::Uppercase)
        .color(color)
        .min_w(0.)
}
/// `h1`: display 40px, line-height 1.1.
pub(crate) fn h1(t: impl Into<String>, size: f32) -> El {
    text(t)
        .font(
            TextStyle::new(size, 700.)
                .family(SANS)
                .tracking(0.02 * size)
                .lh(size * 1.08),
        )
        .text_case(TextCase::Uppercase)
        .color(fg())
        .min_w(0.)
        .wrap()
}
/// `h2` (22px on management screens, 28px in guided setup).
pub(crate) fn h2(t: impl Into<String>, size: f32) -> El {
    h1(t, size)
}
/// `h3` (16px management, 19–23px in setup).
pub(crate) fn h3(t: impl Into<String>, size: f32) -> El {
    text(t)
        .font(
            TextStyle::new(size, 700.)
                .family(SANS)
                .tracking(0.02 * size)
                .lh(size * 1.25),
        )
        .text_case(TextCase::Uppercase)
        .color(fg())
        .min_w(0.)
        .wrap()
}
/// Body copy: sentence case, muted, line-height 1.55. Headings, tags and the rail stay in
/// uppercase DIN; running text, gate reasons and errors are read, so they keep their case.
pub(crate) fn p(t: impl Into<String>, size: f32) -> El {
    body(t, size, muted())
}
pub(crate) fn body(t: impl Into<String>, size: f32, color: Color) -> El {
    text(t)
        .font(
            TextStyle::new(size, 400.)
                .family(SANS)
                .tracking(0.01 * size)
                .lh(size * 1.55),
        )
        .color(color)
        .min_w(0.)
        .wrap()
}
/// `.meta` / `.eyebrow`: mono 10px, `.08em`, muted, uppercase.
pub(crate) fn meta(t: impl Into<String>) -> El {
    meta_sized(t, 10., muted())
}
pub(crate) fn meta_sized(t: impl Into<String>, size: f32, color: Color) -> El {
    text(t)
        .font(
            TextStyle::new(size, 400.)
                .family(MONO)
                .tracking(0.08 * size),
        )
        .text_case(TextCase::Uppercase)
        .color(color)
        .min_w(0.)
}
/// Literal mono text that keeps its case: commands, paths, logs.
pub(crate) fn code(t: impl Into<String>, size: f32, color: Color) -> El {
    text(t)
        .font(TextStyle::new(size, 400.).family(MONO).lh(size * 1.6))
        .color(color)
        .min_w(0.)
}

/// Layout breakpoints, in logical (zoom-adjusted) pixels, matching the design's media queries.
pub(crate) struct Viewport {
    pub w: f32,
    pub h: f32,
}
impl Viewport {
    /// `@media (max-width:720px)`: the rail becomes a drawer, rows stack.
    pub fn compact(&self) -> bool {
        self.w <= 720.
    }
    /// `@media (max-width:1100px)`: narrower rail and gutters.
    pub fn medium(&self) -> bool {
        self.w <= 1100.
    }
    /// `@media (min-width:1600px)`.
    pub fn wide(&self) -> bool {
        self.w >= 1600.
    }
    pub fn rail_width(&self) -> f32 {
        if self.medium() { 156. } else { 190. }
    }
    pub fn gutter(&self) -> f32 {
        if self.compact() {
            20.
        } else if self.medium() {
            24.
        } else if self.wide() {
            64.
        } else {
            40.
        }
    }
    /// Width available to page content.
    pub fn content(&self) -> f32 {
        let rail = if self.compact() {
            0.
        } else {
            self.rail_width()
        };
        (self.w - rail - self.gutter() * 2.).max(280.)
    }
}
