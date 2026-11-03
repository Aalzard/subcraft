//! Text shaping, layout and resolution-independent rasterization.
//!
//! Pure CPU code with no UI or GPU dependency. The pipeline is
//! `TextStyle` + font → [`layout`] (bidi runs, HarfBuzz-compatible shaping through `rustybuzz`,
//! line breaking on `\n`, alignment, spacing) → [`rasterize`] (analytic area coverage of the glyph
//! outlines under any affine transform). Layout is in em units (1.0 = font size, +x right,
//! +y down, first line's top at y = 0), so the same layout rasterizes identically at any size.
//!
//! Fonts come from three places ([`FontRef`]): bundled fonts, installed system fonts, and font
//! files embedded in the project (identified by resource UUID, never by path).

mod font;
mod layout;
mod raster;
mod style;

pub use font::{BUNDLED_FONTS, Font, FontLibrary, FontRef, SystemFontInfo};
pub use layout::{PlacedGlyph, TextLayout, layout};
pub use raster::{Coverage, rasterize, rasterize_fit};
pub use style::{Align, Direction, TextStyle, MAX_TEXT_CHARS};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TextError {
    #[error("not a usable font file")]
    BadFont,
    #[error("font not found: {0}")]
    FontNotFound(String),
    #[error("text is too long (at most {0} characters)")]
    TooLong(usize),
    #[error("invalid text parameters: {0}")]
    Invalid(&'static str),
    #[error("raster size is too large")]
    TooLarge,
}

#[cfg(test)]
mod tests;
