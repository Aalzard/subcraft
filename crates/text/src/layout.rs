use glam::Vec2;
use rustybuzz::{BufferClusterLevel, UnicodeBuffer};
use unicode_bidi::{BidiInfo, Level};

use crate::style::{Align, Direction, TextStyle};
use crate::{Font, TextError};

/// One positioned glyph: the outline origin (pen position plus the shaper's mark offset) in em, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedGlyph {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub line: u32,
    /// Byte offset of the source character within its line (visual order ≠ logical order for RTL).
    pub cluster: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub glyphs: Vec<PlacedGlyph>,
    /// Widest line advance, em.
    pub width: f32,
    /// First line's ascent to last line's descent, em.
    pub height: f32,
    pub lines: u32,
    pub line_height: f32,
    /// Glyphs that mapped to the font's `.notdef` (the font lacks the character).
    pub missing: u32,
    /// Tight ink rectangle (min, max) in em, `None` for blank text.
    pub ink: Option<(Vec2, Vec2)>,
}

impl TextLayout {
    /// Ink rectangle, or the layout box when the text has no visible glyphs.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        self.ink.unwrap_or((Vec2::ZERO, Vec2::new(self.width.max(0.01), self.height.max(0.01))))
    }
}

fn is_cursive(script: Option<rustybuzz::Script>) -> bool {
    use rustybuzz::script::*;
    script.is_some_and(|s| [ARABIC, SYRIAC, MONGOLIAN, NKO, MANDAIC, PHAGS_PA].contains(&s))
}

struct Shaped {
    glyph: u16,
    advance: f32,
    dx: f32,
    dy: f32,
    cluster: u32,
    cursive: bool,
}

fn shape_line(face: &rustybuzz::Face<'_>, line: &str, base: Option<Level>) -> Vec<Shaped> {
    let upem = face.units_per_em() as f32;
    let mut out = Vec::new();
    if line.is_empty() {
        return out;
    }
    let bidi = BidiInfo::new(line, base);
    for para in &bidi.paragraphs {
        let (levels, runs) = bidi.visual_runs(para, para.range.clone());
        for run in runs {
            let level = levels[run.start];
            let mut buf = UnicodeBuffer::new();
            buf.push_str(&line[run.clone()]);
            buf.set_direction(if level.is_rtl() { rustybuzz::Direction::RightToLeft } else { rustybuzz::Direction::LeftToRight });
            buf.set_cluster_level(BufferClusterLevel::MonotoneCharacters);
            buf.guess_segment_properties();
            let cursive = is_cursive(Some(buf.script()));
            let glyphs = rustybuzz::shape(face, &[], buf);
            for (info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
                out.push(Shaped {
                    glyph: info.glyph_id.min(u16::MAX as u32) as u16,
                    advance: pos.x_advance as f32 / upem,
                    dx: pos.x_offset as f32 / upem,
                    dy: pos.y_offset as f32 / upem,
                    cluster: run.start as u32 + info.cluster,
                    cursive,
                });
            }
        }
    }
    out
}

/// Shape and lay out `style.text` with `font`. Deterministic for a given font file.
pub fn layout(font: &Font, style: &TextStyle) -> Result<TextLayout, TextError> {
    style.validate()?;
    font.with_face(|face| lay_out(face, style)).ok_or(TextError::BadFont)
}

fn lay_out(face: &rustybuzz::Face<'_>, style: &TextStyle) -> TextLayout {
    let upem = face.units_per_em() as f32;
    let ascent = face.ascender() as f32 / upem;
    let descent = -(face.descender() as f32) / upem;
    let gap = face.line_gap() as f32 / upem;
    let line_height = (ascent + descent + gap) * style.line_spacing;
    let base = match style.direction {
        Direction::Auto => None,
        Direction::Ltr => Some(Level::ltr()),
        Direction::Rtl => Some(Level::rtl()),
    };
    // Tabs become spaces; other control characters are dropped.
    let cleaned: String = style.text.chars().filter_map(|c| match c {
        '\t' => Some(' '),
        '\n' => Some('\n'),
        c if c.is_control() => None,
        c => Some(c),
    }).collect();
    let lines: Vec<&str> = cleaned.split('\n').collect();
    let shaped: Vec<Vec<Shaped>> = lines.iter().map(|l| shape_line(face, l, base)).collect();
    let advance_of = |g: &Shaped| g.advance + if g.cursive { 0.0 } else { style.letter_spacing };
    let widths: Vec<f32> = shaped.iter().map(|l| l.iter().map(advance_of).sum::<f32>()).collect();
    let width = widths.iter().copied().fold(0.0, f32::max);

    let mut glyphs = Vec::new();
    let mut missing = 0;
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for (i, line) in shaped.iter().enumerate() {
        let baseline = ascent + i as f32 * line_height;
        let mut pen = match style.align {
            Align::Left => 0.0,
            Align::Center => (width - widths[i]) * 0.5,
            Align::Right => width - widths[i],
        };
        for g in line {
            if g.glyph == 0 {
                missing += 1;
            }
            let (x, y) = (pen + g.dx, baseline - g.dy);
            glyphs.push(PlacedGlyph { id: g.glyph, x, y, line: i as u32, cluster: g.cluster });
            if let Some(b) = face.glyph_bounding_box(rustybuzz::ttf_parser::GlyphId(g.glyph)) {
                lo = lo.min(Vec2::new(x + b.x_min as f32 / upem, y - b.y_max as f32 / upem));
                hi = hi.max(Vec2::new(x + b.x_max as f32 / upem, y - b.y_min as f32 / upem));
            }
            pen += advance_of(g);
        }
    }
    let ink = (lo.x <= hi.x).then_some((lo, hi));
    let height = ascent + descent + (lines.len() as f32 - 1.0) * line_height;
    TextLayout { glyphs, width, height, lines: lines.len() as u32, line_height, missing, ink }
}
