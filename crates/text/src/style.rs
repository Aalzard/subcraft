use serde::{Deserialize, Serialize};

use crate::TextError;

pub const MAX_TEXT_CHARS: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Base paragraph direction. `Auto` follows the first strong character of each line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Auto,
    Ltr,
    Rtl,
}

/// Everything about the text except which font (see `FontRef`) and where it is placed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub text: String,
    /// Extra advance per glyph in em (0 = font default). Not applied inside cursive scripts
    /// (Arabic, Syriac, ...), where it would break the joins.
    pub letter_spacing: f32,
    /// Line pitch multiplier (1 = the font's own line height).
    pub line_spacing: f32,
    pub align: Align,
    pub direction: Direction,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { text: "Text".into(), letter_spacing: 0.0, line_spacing: 1.0, align: Align::Left, direction: Direction::Auto }
    }
}

impl TextStyle {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    pub fn validate(&self) -> Result<(), TextError> {
        if self.text.chars().count() > MAX_TEXT_CHARS {
            return Err(TextError::TooLong(MAX_TEXT_CHARS));
        }
        if !self.letter_spacing.is_finite() || !(-0.5..=2.0).contains(&self.letter_spacing) {
            return Err(TextError::Invalid("letter spacing must be within -0.5..2 em"));
        }
        if !self.line_spacing.is_finite() || !(0.2..=4.0).contains(&self.line_spacing) {
            return Err(TextError::Invalid("line spacing must be within 0.2..4"));
        }
        Ok(())
    }
}
