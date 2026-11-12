//! Serializable filter parameters. Kernels live in materials, never in widgets.
use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutlinePosition { Inner, Outer, Both }

/// One color stop; `color` is linear RGB, `position` is in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop { pub position: f32, pub color: [f32; 3] }

pub const MAX_GRADIENT_STOPS: usize = 8;
/// Median cost grows with the window, so it is capped below the Gaussian radii.
pub const MAX_MEDIAN_RADIUS: f32 = 16.0;
/// Warp noise cells across the texture.
pub const MAX_WARP_SCALE: u32 = 64;
/// Largest Mosaic block edge in pixels (also bounds its tile margin).
pub const MAX_MOSAIC_SIZE: u32 = 64;
/// Posterize steps: 2 is a binary split, 64 is nearly continuous.
pub const MAX_POSTERIZE_LEVELS: u32 = 64;

/// Up to [`MAX_GRADIENT_STOPS`] stops in a fixed array so `Filter` stays `Copy`.
/// Serialized as a plain list of stops; unused slots never reach the file.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "Vec<GradientStop>", try_from = "Vec<GradientStop>")]
pub struct Gradient { len: u8, stops: [GradientStop; MAX_GRADIENT_STOPS] }

impl Gradient {
    /// None when `stops` is empty or longer than [`MAX_GRADIENT_STOPS`].
    pub fn new(stops: &[GradientStop]) -> Option<Self> {
        if stops.is_empty() || stops.len() > MAX_GRADIENT_STOPS { return None; }
        let mut out = Self { len: stops.len() as u8, stops: [GradientStop { position: 0.0, color: [0.0; 3] }; MAX_GRADIENT_STOPS] };
        out.stops[..stops.len()].copy_from_slice(stops);
        Some(out)
    }
    /// Black to white in linear space.
    pub const fn black_white() -> Self {
        const Z: GradientStop = GradientStop { position: 0.0, color: [0.0; 3] };
        let mut stops = [Z; MAX_GRADIENT_STOPS];
        stops[1] = GradientStop { position: 1.0, color: [1.0; 3] };
        Self { len: 2, stops }
    }
    pub fn stops(&self) -> &[GradientStop] { &self.stops[..self.len as usize] }
    pub fn stops_mut(&mut self) -> &mut [GradientStop] { &mut self.stops[..self.len as usize] }
    /// Adds a stop (kept sorted by position); false when full.
    pub fn insert(&mut self, stop: GradientStop) -> bool {
        let n = self.len as usize;
        if n >= MAX_GRADIENT_STOPS { return false; }
        let at = self.stops[..n].iter().position(|s| s.position > stop.position).unwrap_or(n);
        self.stops.copy_within(at..n, at + 1);
        self.stops[at] = stop; self.len += 1; true
    }
    /// Removes a stop; the last remaining one cannot be removed.
    pub fn remove(&mut self, index: usize) -> bool {
        let n = self.len as usize;
        if n <= 1 || index >= n { return false; }
        self.stops.copy_within(index + 1..n, index);
        self.len -= 1; true
    }
    /// Re-sorts after a position edit (stable, so equal positions keep their order).
    pub fn sort(&mut self) {
        let n = self.len as usize;
        self.stops[..n].sort_by(|a, b| a.position.total_cmp(&b.position));
    }
    pub fn valid(&self) -> bool {
        let s = self.stops();
        !s.is_empty() && s.iter().all(|s| s.position.is_finite() && (0.0..=1.0).contains(&s.position) && s.color.iter().all(|c| c.is_finite() && *c >= 0.0))
            && s.windows(2).all(|w| w[0].position <= w[1].position)
    }
    /// Linear interpolation between neighbours in linear RGB; clamps outside the first/last stop.
    /// Equal positions make a hard step (the later stop wins at and above the position).
    pub fn eval(&self, t: f32) -> [f32; 3] {
        let s = self.stops();
        let t = if t.is_nan() { 0.0 } else { t };
        let Some(i) = s.iter().position(|s| s.position > t) else { return s[s.len() - 1].color };
        if i == 0 { return s[0].color; }
        let (a, b) = (s[i - 1], s[i]);
        let f = (t - a.position) / (b.position - a.position);
        std::array::from_fn(|c| a.color[c] + (b.color[c] - a.color[c]) * f)
    }
}

impl From<Gradient> for Vec<GradientStop> { fn from(g: Gradient) -> Self { g.stops().to_vec() } }
impl TryFrom<Vec<GradientStop>> for Gradient {
    type Error = String;
    fn try_from(v: Vec<GradientStop>) -> Result<Self, String> {
        Self::new(&v).ok_or_else(|| format!("a gradient needs 1 to {MAX_GRADIENT_STOPS} stops, found {}", v.len()))
    }
}

/// Built-in gradients as (name, sRGB stops); converted to linear storage on use.
pub const GRADIENT_PRESETS: [(&str, &[(f32, [f32; 3])]); 6] = [
    ("Black to White", &[(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 1.0, 1.0])]),
    ("Heat", &[(0.0, [0.0, 0.0, 0.0]), (0.35, [0.7, 0.05, 0.0]), (0.7, [1.0, 0.65, 0.0]), (1.0, [1.0, 1.0, 0.85])]),
    ("Rust", &[(0.0, [0.12, 0.06, 0.04]), (0.5, [0.55, 0.25, 0.1]), (1.0, [0.85, 0.5, 0.2])]),
    ("Sepia", &[(0.0, [0.1, 0.06, 0.03]), (1.0, [0.95, 0.88, 0.7])]),
    ("Sky", &[(0.0, [0.05, 0.1, 0.35]), (0.6, [0.35, 0.6, 0.9]), (1.0, [1.0, 1.0, 1.0])]),
    ("Terrain", &[(0.0, [0.1, 0.25, 0.5]), (0.3, [0.25, 0.55, 0.2]), (0.6, [0.5, 0.4, 0.25]), (1.0, [1.0, 1.0, 1.0])]),
];

impl Gradient {
    pub fn preset(index: usize) -> Option<Self> {
        let (_, stops) = GRADIENT_PRESETS.get(index)?;
        let stops: Vec<_> = stops.iter().map(|&(position, c)| GradientStop { position, color: subcraft_core::color::srgb3_to_linear(c) }).collect();
        Self::new(&stops)
    }
}

/// Comparison of a Threshold filter: the result is 1 where `value <op> threshold`, else 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdMode { Greater, GreaterOrEqual, Lower, LowerOrEqual }

impl ThresholdMode {
    pub const ALL: [Self; 4] = [Self::Greater, Self::GreaterOrEqual, Self::Lower, Self::LowerOrEqual];
    pub fn label(self) -> &'static str {
        match self { Self::Greater => "Greater", Self::GreaterOrEqual => "Greater or equal", Self::Lower => "Lower", Self::LowerOrEqual => "Lower or equal" }
    }
    pub fn test(self, value: f32, threshold: f32) -> bool {
        match self { Self::Greater => value > threshold, Self::GreaterOrEqual => value >= threshold, Self::Lower => value < threshold, Self::LowerOrEqual => value <= threshold }
    }
}

/// HSL filter color model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HslMode {
    /// Hue, chroma and lightness of a perceptually uniform space (OKLCH): a hue shift keeps the
    /// perceived brightness.
    Perceptive,
    /// Classic HSL of the sRGB-encoded color.
    Regular,
}

impl HslMode {
    pub const ALL: [Self; 2] = [Self::Perceptive, Self::Regular];
    pub fn label(self) -> &'static str { match self { Self::Perceptive => "Perceptive", Self::Regular => "Regular" } }
}

/// What a Transform filter shows outside the transformed image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode { None, Horizontal, Vertical, Both }

impl RepeatMode {
    pub const ALL: [Self; 4] = [Self::Both, Self::Horizontal, Self::Vertical, Self::None];
    pub fn label(self) -> &'static str {
        match self { Self::None => "No repeat", Self::Horizontal => "Repeat horizontally", Self::Vertical => "Repeat vertically", Self::Both => "Repeat" }
    }
    /// Whether the image repeats along `[U, V]`.
    pub fn repeats(self) -> [bool; 2] {
        match self { Self::None => [false, false], Self::Horizontal => [true, false], Self::Vertical => [false, true], Self::Both => [true, true] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipMode { None, Horizontal, Vertical, Both }

impl FlipMode {
    pub const ALL: [Self; 4] = [Self::None, Self::Horizontal, Self::Vertical, Self::Both];
    pub fn label(self) -> &'static str {
        match self { Self::None => "None", Self::Horizontal => "Horizontal", Self::Vertical => "Vertical", Self::Both => "Both" }
    }
    /// Whether `[U, V]` is flipped.
    pub fn flips(self) -> [bool; 2] {
        match self { Self::None => [false, false], Self::Horizontal => [true, false], Self::Vertical => [false, true], Self::Both => [true, true] }
    }
}

/// Which axes a Mirror filter reflects across.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorMode {
    /// Across a vertical line: the left half is copied onto the right.
    AxisX,
    /// Across a horizontal line: the top half is copied onto the bottom.
    AxisY,
    /// Across both: one corner fills the whole image.
    Corner,
}

impl MirrorMode {
    pub const ALL: [Self; 3] = [Self::AxisX, Self::AxisY, Self::Corner];
    pub fn label(self) -> &'static str { match self { Self::AxisX => "Mirror axis X", Self::AxisY => "Mirror axis Y", Self::Corner => "Mirror corner" } }
}

/// The corner a Mirror filter in [`MirrorMode::Corner`] keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorCorner { TopLeft, TopRight, BottomLeft, BottomRight }

impl MirrorCorner {
    pub const ALL: [Self; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];
    pub fn label(self) -> &'static str {
        match self { Self::TopLeft => "Top left", Self::TopRight => "Top right", Self::BottomLeft => "Bottom left", Self::BottomRight => "Bottom right" }
    }
    /// Whether the kept side is the far one (right, bottom) along `[U, V]`.
    pub fn far(self) -> [bool; 2] {
        match self { Self::TopLeft => [false, false], Self::TopRight => [true, false], Self::BottomLeft => [false, true], Self::BottomRight => [true, true] }
    }
}

/// Support of filters that read the whole texture (Transform, Mirror, wrapped Blur): larger than
/// any texture set, so every tile depends on every other.
pub const FULL_SUPPORT: u32 = 1 << 16;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filter {
    /// Maps luminance (color) or the raw value (scalar; signed Height is remapped from -1..1)
    /// through the gradient. Scalar channels and masks receive the result's luminance.
    GradientMap { gradient: Gradient, to_gray: bool },
    /// Soft threshold: a linear ramp centred on `position`, `1 - contrast` wide (contrast 1 is a
    /// hard threshold). Evaluated per channel; `invert` returns 1 - result.
    HistogramScan { position: f32, contrast: f32, invert: bool },
    /// Band select: 1 inside `position +- range / 2`, 0 outside, with edge ramps that take up
    /// `softness` of the half-range. Evaluated per channel; `invert` returns 1 - result.
    HistogramRange { position: f32, range: f32, softness: f32, invert: bool },
    /// Exact median over the square window `(2r + 1)^2`, r = radius rounded to whole pixels.
    /// Per channel; paint coverage (alpha) is kept as is. Edges clamp.
    Median { radius: f32 },
    /// Noise-driven UV displacement: each texel reads the input up to `intensity` pixels away,
    /// along a smooth periodic noise field with `scale` cells across the texture. Deterministic
    /// for a given `seed`; resolution independent (noise is evaluated in UV space).
    Warp { intensity: f32, scale: u32, seed: u32 },
    Contrast { gain: f32, pivot: f32 },
    /// Hue in degrees, saturation multiplier, additive HSL lightness. Color channels only.
    Hsl { hue: f32, saturation: f32, lightness: f32 },
    /// Color inputs use linear Rec.709 luminance; scalar inputs use their raw value.
    Threshold { cutoff: f32 },
    Clamp { low: f32, high: f32 },
    /// Gaussian support radius in working texture pixels.
    Blur { radius: f32 },
    Sharpen { radius: f32, amount: f32 },
    /// Symmetric Gaussian line along a UV axis. Positive angle follows image +v (down).
    DirectionalBlur { radius: f32, angle_deg: f32 },
    /// Source minus Gaussian low frequencies; neutral 0.5, or 0 for signed Height.
    Highpass { radius: f32, amount: f32 },
    /// Raised inward ramp from a thresholded contour; Amount is the plateau height.
    Bevel { radius: f32, amount: f32, threshold: f32 },
    /// Euclidean border of a thresholded contour, in UV pixels on each chosen side.
    MaskOutline { radius: f32, threshold: f32, position: OutlinePosition },
    /// Rec.709 luminance copied to all three components. Color channels only.
    Grayscale,
    /// `1 - v` (signed Height: `-v`). Any channel except Normal; use Normal Invert there.
    Invert,
    /// Quantises every component to `levels` evenly spaced steps (round to nearest).
    Posterize { levels: u32 },
    /// Adds `shift` to every value (signed Height keeps its sign), then clamps to the channel range.
    HistogramShift { shift: f32 },
    /// Per tonal range (shadows, midtones, highlights) additive offset of R, G, B in -1..=1.
    /// Weights follow luminance; the three weights always sum to 1. Color channels only.
    ColorBalance { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3] },
    /// Colours within `tolerance` (linear RGB distance) of `from` move toward `to`, fading out over
    /// `softness` extra distance; the pixel keeps its offset from `from`. Color channels only.
    ReplaceColor { from: [f32; 3], to: [f32; 3], tolerance: f32, softness: f32 },
    /// Flips selected axes of an encoded tangent-space normal (v -> 1 - v). Normal channel only.
    NormalInvert { flip_x: bool, flip_y: bool, flip_z: bool },
    /// Sobel gradient magnitude of the luminance (scalars: the value), sampled `radius` pixels
    /// apart, times `strength`, clamped to 0..=1. Grayscale result.
    EdgeDetect { radius: f32, strength: f32 },
    /// Adds blurred highlights: `amount * max(blur(v) - threshold, 0) / (1 - threshold)`.
    Glow { radius: f32, amount: f32, threshold: f32 },
    /// Self-driven displacement along `angle_deg`: each texel reads from `intensity * (2v - 1)`
    /// pixels away (v = luminance or raw scalar; signed Height uses `v` directly).
    DirectionalWarp { intensity: f32, angle_deg: f32 },
    /// Every `size` x `size` pixel block takes the value at its centre pixel.
    Mosaic { size: u32 },
    /// Hermite ramp `t * t * (3 - 2t)` with `t = (v - min) / (max - min)` clamped to 0..=1, per
    /// component. `min > max` gives the inverted ramp; `min == max` is a step at `min`.
    Smoothstep { min: f32, max: f32 },
    /// [`Filter::Threshold`] with a comparison other than greater-or-equal (same panel, Mode row).
    ThresholdCompare { cutoff: f32, mode: ThresholdMode },
    /// [`Filter::Blur`] that wraps around the texture borders instead of clamping (Wrap toggle).
    BlurWrap { radius: f32 },
    /// [`Filter::HistogramShift`] whose values wrap around the range limits instead of clamping.
    HistogramShiftWrap { shift: f32 },
    /// [`Filter::Hsl`] in [`HslMode::Perceptive`]: hue rotation in degrees, chroma multiplier and
    /// additive lightness in OKLCH. Color channels only.
    HslPerceptive { hue: f32, saturation: f32, lightness: f32 },
    /// Contrast and Luminosity, each -1..=1 with 0 neutral. Positive contrast stretches around
    /// 0.5 by `1 / (1 - contrast)` (1 is a hard step), negative compresses by `1 + contrast`
    /// (-1 is flat mid gray); luminosity is then added. Signed Height pivots on 0.
    ContrastLuminosity { contrast: f32, luminosity: f32 },
    /// Per tonal range `[contrast, luminosity, saturation]`, each -1..=1 with 0 neutral, blended
    /// by the luminance weights of [`Filter::ColorBalance`]. Color channels only.
    ColorCorrect { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3] },
    /// 2D transform of the image in UV space about its centre: scale, then rotation (degrees,
    /// counter-clockwise on screen), then offset (UV units, +v down). `tile_safe` snaps the
    /// rotation to quarter turns and the scale to whole repetitions so a tiling image still tiles.
    Transform { repeat: RepeatMode, flip: FlipMode, scale: [f32; 2], offset: [f32; 2], rotation_deg: f32, tile_safe: bool },
    /// Reflects one side of the image onto the other across an axis at `offset` (UV, 0.5 = the
    /// centre). The near side (left / top) is kept unless inverted; `corner` picks the kept
    /// corner in [`MirrorMode::Corner`], the invert flags then swap it per axis.
    Mirror { mode: MirrorMode, offset: [f32; 2], invert_x: bool, invert_y: bool, corner: MirrorCorner },
    /// [`Filter::Bevel`] with Painter's Corner Type and Smoothing (same panel). Angular corners
    /// measure the distance to the contour as the larger axis distance (square corners);
    /// Smoothing 0..=1 eases the ramp from linear toward a smoothstep.
    BevelShape { radius: f32, amount: f32, threshold: f32, corner: BevelCorner, smoothing: f32 },
    /// [`Filter::MaskOutline`] with Painter's Start From Black, Blur and Curve Shape (same panel).
    /// Blur (pixels) fades the band's far edge out over that width; Start From Black ramps the
    /// band from 0 at the contour to 1 at its far edge; Curve Shape (-1..=1) raises the result to
    /// the power `4^curve`.
    MaskOutlineShape { radius: f32, threshold: f32, position: OutlinePosition, start_black: bool, blur: f32, curve: f32 },
    /// Painter's Height Adjust: optional invert, then `v * multiply + offset`.
    HeightAdjust { invert: bool, offset: f32, multiply: f32 },
    /// Painter's FXAA: smooths stair-stepped edges along their direction (luminance contrast
    /// with the four neighbours). `alpha` also smooths the coverage.
    Fxaa { alpha: bool },
    /// Painter's Drop Shadow (colour content): the layer's coverage offset by `distance` pixels
    /// along `angle_deg` (0 = right, counter-clockwise), blurred over `size` pixels, widened by
    /// `spread` (0..1), at `opacity`, in `color` (linear), under the content.
    DropShadow { angle_deg: f32, distance: f32, size: f32, spread: f32, opacity: f32, color: [f32; 3] },
    /// Painter's Glow: an outer glow around the content's coverage (or, on a mask or a scalar
    /// channel, around its bright values) `width` pixels wide, widened by `spread`, at `opacity`,
    /// in `color` (linear; scalar channels use its luminance). `precise` doubles the falloff
    /// steepness (SubCraft's approximation of Painter's Precise mode).
    OuterGlow { width: f32, spread: f32, opacity: f32, color: [f32; 3], precise: bool },
}

/// Corner Type of a Bevel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BevelCorner {
    #[default]
    Rounded,
    Angular,
}

impl BevelCorner {
    pub const ALL: [Self; 2] = [Self::Rounded, Self::Angular];
    pub fn label(self) -> &'static str {
        match self {
            Self::Rounded => "Rounded",
            Self::Angular => "Angular",
        }
    }
}

/// Every Bevel parameter, whichever variant holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BevelParts {
    pub radius: f32,
    pub amount: f32,
    pub threshold: f32,
    pub corner: BevelCorner,
    pub smoothing: f32,
}

/// Every Mask Outline parameter, whichever variant holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineParts {
    pub radius: f32,
    pub threshold: f32,
    pub position: OutlinePosition,
    pub start_black: bool,
    pub blur: f32,
    pub curve: f32,
}

impl Filter {
    pub const ALL: [Self; 35] = [Self::Contrast { gain: 1.0, pivot: 0.5 },
        Self::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 },
        Self::Threshold { cutoff: 0.5 }, Self::Clamp { low: 0.0, high: 1.0 }, Self::Blur { radius: 4.0 }, Self::Sharpen { radius: 2.0, amount: 1.0 }, Self::DirectionalBlur { radius: 8.0, angle_deg: 0.0 }, Self::Highpass { radius: 4.0, amount: 1.0 },
        Self::Bevel {radius:8.0,amount:1.0,threshold:0.5}, Self::MaskOutline {radius:4.0,threshold:0.5,position:OutlinePosition::Both},
        Self::GradientMap { gradient: Gradient::black_white(), to_gray: false },
        Self::HistogramScan { position: 0.5, contrast: 0.5, invert: false },
        Self::HistogramRange { position: 0.5, range: 0.2, softness: 0.5, invert: false },
        Self::Median { radius: 2.0 }, Self::Warp { intensity: 8.0, scale: 4, seed: 1 },
        Self::Grayscale, Self::Invert, Self::Posterize { levels: 4 }, Self::HistogramShift { shift: 0.1 },
        Self::ColorBalance { shadows: [0.0; 3], midtones: [0.0; 3], highlights: [0.0; 3] },
        Self::ReplaceColor { from: [0.5; 3], to: [0.5; 3], tolerance: 0.2, softness: 0.2 },
        Self::NormalInvert { flip_x: false, flip_y: true, flip_z: false },
        Self::EdgeDetect { radius: 1.0, strength: 1.0 }, Self::Glow { radius: 8.0, amount: 1.0, threshold: 0.6 },
        Self::DirectionalWarp { intensity: 8.0, angle_deg: 0.0 }, Self::Mosaic { size: 8 },
        Self::Smoothstep { min: 0.0, max: 1.0 }, Self::ContrastLuminosity { contrast: 0.0, luminosity: 0.0 },
        Self::ColorCorrect { shadows: [0.0; 3], midtones: [0.0; 3], highlights: [0.0; 3] },
        Self::Transform { repeat: RepeatMode::Both, flip: FlipMode::None, scale: [1.0; 2], offset: [0.0; 2], rotation_deg: 0.0, tile_safe: false },
        Self::Mirror { mode: MirrorMode::AxisX, offset: [0.5; 2], invert_x: false, invert_y: false, corner: MirrorCorner::TopLeft },
        Self::HeightAdjust { invert: false, offset: 0.0, multiply: 1.0 }, Self::Fxaa { alpha: false },
        Self::DropShadow { angle_deg: -45.0, distance: 4.0, size: 4.0, spread: 0.0, opacity: 0.75, color: [0.0; 3] },
        Self::OuterGlow { width: 8.0, spread: 0.0, opacity: 0.75, color: [1.0; 3], precise: false }];

    /// A Bevel (the plain variant when Corner Type and Smoothing are at their defaults).
    pub fn bevel(p: BevelParts) -> Self {
        if p.corner == BevelCorner::Rounded && p.smoothing == 0.0 {
            Self::Bevel { radius: p.radius, amount: p.amount, threshold: p.threshold }
        } else {
            Self::BevelShape { radius: p.radius, amount: p.amount, threshold: p.threshold, corner: p.corner, smoothing: p.smoothing }
        }
    }
    pub fn bevel_parts(self) -> Option<BevelParts> {
        match self {
            Self::Bevel { radius, amount, threshold } => Some(BevelParts { radius, amount, threshold, corner: BevelCorner::Rounded, smoothing: 0.0 }),
            Self::BevelShape { radius, amount, threshold, corner, smoothing } => Some(BevelParts { radius, amount, threshold, corner, smoothing }),
            _ => None,
        }
    }
    /// A Mask Outline (the plain variant when the shape options are at their defaults).
    pub fn mask_outline(p: OutlineParts) -> Self {
        if !p.start_black && p.blur == 0.0 && p.curve == 0.0 {
            Self::MaskOutline { radius: p.radius, threshold: p.threshold, position: p.position }
        } else {
            Self::MaskOutlineShape { radius: p.radius, threshold: p.threshold, position: p.position, start_black: p.start_black, blur: p.blur, curve: p.curve }
        }
    }
    pub fn outline_parts(self) -> Option<OutlineParts> {
        match self {
            Self::MaskOutline { radius, threshold, position } => Some(OutlineParts { radius, threshold, position, start_black: false, blur: 0.0, curve: 0.0 }),
            Self::MaskOutlineShape { radius, threshold, position, start_black, blur, curve } => {
                Some(OutlineParts { radius, threshold, position, start_black, blur, curve })
            }
            _ => None,
        }
    }
    /// Drop Shadow / Glow: the shadow's offset in pixels (+x right, +y down) and blur radius.
    pub fn shadow_geometry(self) -> Option<([f32; 2], f32)> {
        match self {
            Self::DropShadow { angle_deg, distance, size, .. } => {
                let a = angle_deg.to_radians();
                Some(([a.cos() * distance, -a.sin() * distance], size))
            }
            Self::OuterGlow { width, .. } => Some(([0.0; 2], width)),
            _ => None,
        }
    }

    /// A Threshold filter with `mode` (the greater-or-equal one is the plain [`Filter::Threshold`]).
    pub fn threshold(cutoff: f32, mode: ThresholdMode) -> Self {
        if mode == ThresholdMode::GreaterOrEqual { Self::Threshold { cutoff } } else { Self::ThresholdCompare { cutoff, mode } }
    }
    /// Cutoff and comparison of either Threshold variant.
    pub fn threshold_parts(self) -> Option<(f32, ThresholdMode)> {
        match self { Self::Threshold { cutoff } => Some((cutoff, ThresholdMode::GreaterOrEqual)), Self::ThresholdCompare { cutoff, mode } => Some((cutoff, mode)), _ => None }
    }
    /// A Blur filter that wraps around the texture borders or clamps at them.
    pub fn blur(radius: f32, wrap: bool) -> Self { if wrap { Self::BlurWrap { radius } } else { Self::Blur { radius } } }
    /// Radius and wrap of either Blur variant.
    pub fn blur_parts(self) -> Option<(f32, bool)> {
        match self { Self::Blur { radius } => Some((radius, false)), Self::BlurWrap { radius } => Some((radius, true)), _ => None }
    }
    pub fn histogram_shift(shift: f32, wrap: bool) -> Self { if wrap { Self::HistogramShiftWrap { shift } } else { Self::HistogramShift { shift } } }
    pub fn histogram_shift_parts(self) -> Option<(f32, bool)> {
        match self { Self::HistogramShift { shift } => Some((shift, false)), Self::HistogramShiftWrap { shift } => Some((shift, true)), _ => None }
    }
    pub fn hsl(mode: HslMode, hue: f32, saturation: f32, lightness: f32) -> Self {
        match mode { HslMode::Regular => Self::Hsl { hue, saturation, lightness }, HslMode::Perceptive => Self::HslPerceptive { hue, saturation, lightness } }
    }
    /// Mode, hue, saturation and lightness of either HSL variant.
    pub fn hsl_parts(self) -> Option<(HslMode, [f32; 3])> {
        match self {
            Self::Hsl { hue, saturation, lightness } => Some((HslMode::Regular, [hue, saturation, lightness])),
            Self::HslPerceptive { hue, saturation, lightness } => Some((HslMode::Perceptive, [hue, saturation, lightness])),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self { Self::Contrast { .. } => "Contrast", Self::Hsl { .. } => "HSL",
            Self::Threshold { .. } => "Threshold", Self::Clamp { .. } => "Clamp", Self::Blur { .. } => "Blur", Self::Sharpen { .. } => "Sharpen", Self::DirectionalBlur { .. } => "Directional Blur", Self::Highpass { .. } => "Highpass", Self::Bevel {..} => "Bevel", Self::MaskOutline {..} => "Mask Outline", Self::GradientMap {..} => "Gradient Map", Self::HistogramScan {..} => "Histogram Scan", Self::HistogramRange {..} => "Histogram Range", Self::Median {..} => "Median", Self::Warp {..} => "Warp",
            Self::Grayscale => "Grayscale", Self::Invert => "Invert", Self::Posterize {..} => "Posterize", Self::HistogramShift {..} => "Histogram Shift",
            Self::ColorBalance {..} => "Color Balance", Self::ReplaceColor {..} => "Replace Color", Self::NormalInvert {..} => "Normal Invert",
            // SubCraft's bloom of bright values; Painter's (outer) Glow is `OuterGlow`.
            Self::EdgeDetect {..} => "Edge Detect", Self::Glow {..} => "Bloom", Self::DirectionalWarp {..} => "Directional Warp", Self::Mosaic {..} => "Mosaic",
            // Mode / wrap variants share the panel (and so the name) of their base filter.
            Self::ThresholdCompare {..} => "Threshold", Self::BlurWrap {..} => "Blur", Self::HistogramShiftWrap {..} => "Histogram Shift", Self::HslPerceptive {..} => "HSL",
            Self::Smoothstep {..} => "Smoothstep", Self::ContrastLuminosity {..} => "Contrast Luminosity", Self::ColorCorrect {..} => "Color Correct",
            Self::Transform {..} => "Transform", Self::Mirror {..} => "Mirror",
            Self::BevelShape {..} => "Bevel", Self::MaskOutlineShape {..} => "Mask Outline", Self::HeightAdjust {..} => "Height Adjust",
            Self::Fxaa {..} => "FXAA (Anti-Aliasing)", Self::DropShadow {..} => "Drop Shadow", Self::OuterGlow {..} => "Glow" }
    }

    pub fn supports_channel(self, channel: ChannelKind) -> bool {
        if matches!(self, Self::NormalInvert { .. }) { return channel == ChannelKind::Normal; }
        channel != ChannelKind::Normal && (!self.color_only() || channel.is_color())
    }

    /// Filters that only make sense on RGB colour data.
    fn color_only(self) -> bool {
        matches!(self, Self::Hsl { .. } | Self::HslPerceptive { .. } | Self::Grayscale | Self::ColorBalance { .. } | Self::ReplaceColor { .. } | Self::ColorCorrect { .. } | Self::DropShadow { .. })
    }

    pub fn supports_mask(self) -> bool { !self.color_only() && !matches!(self, Self::NormalInvert { .. }) }

    /// Copy with pixel-based parameters multiplied by `factor` (texture resolution ratio).
    pub fn scaled(self, factor: f32) -> Self {
        match self {
            Self::EdgeDetect { radius, strength } => Self::EdgeDetect { radius: radius * factor, strength },
            Self::Glow { radius, amount, threshold } => Self::Glow { radius: radius * factor, amount, threshold },
            Self::DirectionalWarp { intensity, angle_deg } => Self::DirectionalWarp { intensity: intensity * factor, angle_deg },
            Self::Mosaic { size } => Self::Mosaic { size: ((size as f32 * factor).round() as u32).clamp(1, MAX_MOSAIC_SIZE) },
            // The outline shape draws its band from its own width and blur, not from the node
            // radius the other contour filters use: both follow the resolution scale.
            Self::MaskOutlineShape { radius, threshold, position, start_black, blur, curve } => {
                Self::MaskOutlineShape { radius: radius * factor, threshold, position, start_black, blur: blur * factor, curve }
            }
            other => other,
        }
    }

    /// Neighbourhood filters evaluated per texel by the compositor with direct pixel reads
    /// (no Gaussian staging). The node output is the final value.
    pub fn is_pixelwise(self) -> bool {
        matches!(self, Self::EdgeDetect { .. } | Self::DirectionalWarp { .. } | Self::Mosaic { .. } | Self::Transform { .. } | Self::Mirror { .. } | Self::Fxaa { .. })
    }

    /// Filters whose every texel may read any other texel of the texture.
    pub fn reads_whole_texture(self) -> bool {
        matches!(self, Self::Transform { .. } | Self::Mirror { .. }) || matches!(self, Self::BlurWrap { radius } if radius > 0.0)
    }

    pub fn radius(self) -> Option<f32> {
        match self { Self::Transform { .. } | Self::Mirror { .. } => Some(FULL_SUPPORT as f32),
            Self::Blur { radius } | Self::BlurWrap { radius } | Self::Sharpen { radius, .. } | Self::DirectionalBlur { radius, .. } | Self::Highpass { radius, .. } | Self::Median { radius } | Self::Warp { intensity: radius, .. } | Self::Bevel {radius,..} | Self::MaskOutline {radius,..} | Self::EdgeDetect { radius, .. } | Self::Glow { radius, .. } | Self::DirectionalWarp { intensity: radius, .. } => Some(radius),
            Self::Mosaic { size } => Some(size as f32),
            Self::BevelShape { radius, .. } => Some(radius),
            Self::MaskOutlineShape { radius, blur, .. } => Some(radius + blur),
            Self::Fxaa { .. } => Some(1.0),
            Self::DropShadow { distance, size, .. } => Some(distance + size),
            Self::OuterGlow { width, .. } => Some(width),
            _ => None }
    }

    pub fn is_contour(self) -> bool { matches!(self, Self::Bevel {..} | Self::MaskOutline {..} | Self::BevelShape {..} | Self::MaskOutlineShape {..}) }

    /// Contours need a half-pixel antialias band beyond their nominal radius.
    pub fn support(self) -> u32 {
        if let Self::Mosaic { size } = self { return size / 2 + 1; }
        if self.reads_whole_texture() { return FULL_SUPPORT; }
        // Edge Detect also reads one sample beyond its offset; Directional Warp interpolates.
        (self.radius().unwrap_or(0.0) + if self.is_contour() {0.5} else {0.0}
            + if matches!(self, Self::DirectionalWarp { .. } | Self::EdgeDetect { .. } | Self::Fxaa { .. } | Self::DropShadow { .. }) {1.0} else {0.0}).ceil() as u32
    }

    pub fn softens_coverage(self) -> bool {
        matches!(self, Self::Blur { .. } | Self::BlurWrap { .. } | Self::Transform { .. } | Self::Mirror { .. } | Self::DirectionalBlur { .. } | Self::Warp { .. } | Self::DirectionalWarp { .. } | Self::Mosaic { .. }
            | Self::Fxaa { alpha: true } | Self::DropShadow { .. } | Self::OuterGlow { .. })
    }

    /// Highpass is deliberately excluded: radius/amount zero gives its neutral value.
    pub fn spatial_identity(self) -> bool {
        matches!(self, Self::Blur {radius:0.0} | Self::BlurWrap {radius:0.0} | Self::DirectionalBlur {radius:0.0,..}
            | Self::Sharpen {radius:0.0,..} | Self::Sharpen {amount:0.0,..} | Self::Warp {intensity:0.0,..}
            | Self::DirectionalWarp {intensity:0.0,..} | Self::Mosaic {size:1} | Self::Glow {radius:0.0,..} | Self::Glow {amount:0.0,..}
            | Self::DropShadow {opacity:0.0,..} | Self::OuterGlow {opacity:0.0,..} | Self::OuterGlow {width:0.0,..})
            || matches!(self, Self::Median { radius } if radius < 0.5)
    }

    pub fn valid(self) -> bool {
        match self {
            Self::Contrast { gain, pivot } => gain.is_finite() && (0.0..=4.0).contains(&gain) && pivot.is_finite(),
            Self::Hsl { hue, saturation, lightness } => hue.is_finite() && (-180.0..=180.0).contains(&hue)
                && saturation.is_finite() && (0.0..=2.0).contains(&saturation)
                && lightness.is_finite() && (-1.0..=1.0).contains(&lightness),
            Self::Blur { radius } | Self::BlurWrap { radius } => radius.is_finite() && (0.0..=32.0).contains(&radius),
            Self::Smoothstep { min, max } => [min, max].iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v)),
            Self::ThresholdCompare { cutoff, .. } => cutoff.is_finite(),
            Self::HistogramShiftWrap { shift } => Self::HistogramShift { shift }.valid(),
            Self::HslPerceptive { hue, saturation, lightness } => Self::Hsl { hue, saturation, lightness }.valid(),
            Self::ContrastLuminosity { contrast, luminosity } => [contrast, luminosity].iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v)),
            Self::ColorCorrect { shadows, midtones, highlights } => [shadows, midtones, highlights].iter().all(|r| r.iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v))),
            Self::Transform { scale, offset, rotation_deg, .. } => scale.iter().all(|v| v.is_finite() && (0.01..=100.0).contains(v))
                && offset.iter().all(|v| v.is_finite() && (-8.0..=8.0).contains(v)) && rotation_deg.is_finite() && (-360.0..=360.0).contains(&rotation_deg),
            Self::Mirror { offset, .. } => offset.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            Self::Warp { intensity, scale, .. } => Self::Blur { radius: intensity }.valid() && (1..=MAX_WARP_SCALE).contains(&scale),
            Self::Median { radius } => radius.is_finite() && (0.0..=MAX_MEDIAN_RADIUS).contains(&radius),
            Self::DirectionalBlur { radius, angle_deg } => Self::Blur {radius}.valid() && angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg),
            Self::Sharpen { radius, amount } | Self::Highpass { radius, amount } => Self::Blur { radius }.valid() && amount.is_finite() && (0.0..=4.0).contains(&amount),
            Self::Bevel {radius,amount,threshold} => Self::Blur {radius}.valid() && amount.is_finite() && (-1.0..=1.0).contains(&amount) && threshold.is_finite() && (-1.0..=1.0).contains(&threshold),
            Self::MaskOutline {radius,threshold,..} => Self::Blur {radius}.valid() && threshold.is_finite() && (-1.0..=1.0).contains(&threshold),
            Self::GradientMap { gradient, .. } => gradient.valid(),
            Self::HistogramScan { position, contrast, .. } => position.is_finite() && (-1.0..=1.0).contains(&position) && contrast.is_finite() && (0.0..=1.0).contains(&contrast),
            Self::HistogramRange { position, range, softness, .. } => position.is_finite() && (-1.0..=1.0).contains(&position)
                && range.is_finite() && (0.0..=2.0).contains(&range) && softness.is_finite() && (0.0..=1.0).contains(&softness),
            Self::Threshold { cutoff } => cutoff.is_finite(),
            Self::Grayscale | Self::Invert | Self::NormalInvert { .. } => true,
            Self::Posterize { levels } => (2..=MAX_POSTERIZE_LEVELS).contains(&levels),
            Self::HistogramShift { shift } => shift.is_finite() && (-1.0..=1.0).contains(&shift),
            Self::ColorBalance { shadows, midtones, highlights } => [shadows, midtones, highlights].iter().all(|r| r.iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v))),
            Self::ReplaceColor { from, to, tolerance, softness } => from.iter().chain(&to).all(|v| v.is_finite() && *v >= 0.0)
                && tolerance.is_finite() && (0.0..=2.0).contains(&tolerance) && softness.is_finite() && (0.0..=2.0).contains(&softness),
            Self::EdgeDetect { radius, strength } => Self::Blur { radius }.valid() && strength.is_finite() && (0.0..=8.0).contains(&strength),
            Self::Glow { radius, amount, threshold } => Self::Blur { radius }.valid() && amount.is_finite() && (0.0..=4.0).contains(&amount) && threshold.is_finite() && (0.0..1.0).contains(&threshold),
            Self::DirectionalWarp { intensity, angle_deg } => Self::Blur { radius: intensity }.valid() && angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg),
            Self::Mosaic { size } => (1..=MAX_MOSAIC_SIZE).contains(&size),
            Self::Clamp { low, high } => low.is_finite() && high.is_finite() && low <= high,
            Self::BevelShape { radius, amount, threshold, smoothing, .. } => {
                Self::Bevel { radius, amount, threshold }.valid() && smoothing.is_finite() && (0.0..=1.0).contains(&smoothing)
            }
            Self::MaskOutlineShape { radius, threshold, position, blur, curve, .. } => {
                Self::MaskOutline { radius, threshold, position }.valid() && blur.is_finite() && (0.0..=32.0).contains(&blur) && curve.is_finite() && (-1.0..=1.0).contains(&curve)
            }
            Self::HeightAdjust { offset, multiply, .. } => offset.is_finite() && (-1.0..=1.0).contains(&offset) && multiply.is_finite() && (0.0..=4.0).contains(&multiply),
            Self::Fxaa { .. } => true,
            Self::DropShadow { angle_deg, distance, size, spread, opacity, color } => {
                angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg)
                    && [distance, size].iter().all(|v| v.is_finite() && (0.0..=32.0).contains(v))
                    && spread.is_finite() && (0.0..=0.99).contains(&spread)
                    && opacity.is_finite() && (0.0..=1.0).contains(&opacity)
                    && color.iter().all(|c| c.is_finite() && *c >= 0.0)
            }
            Self::OuterGlow { width, spread, opacity, color, .. } => {
                width.is_finite() && (0.0..=32.0).contains(&width)
                    && spread.is_finite() && (0.0..=0.99).contains(&spread)
                    && opacity.is_finite() && (0.0..=1.0).contains(&opacity)
                    && color.iter().all(|c| c.is_finite() && *c >= 0.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_shape_filters_have_parts_supports_and_checked_domains() {
        // Default shape options give back the plain variants.
        let p = Filter::Bevel { radius: 6.0, amount: 0.5, threshold: 0.4 }.bevel_parts().unwrap();
        assert_eq!(Filter::bevel(p), Filter::Bevel { radius: 6.0, amount: 0.5, threshold: 0.4 });
        let angular = Filter::bevel(BevelParts { corner: BevelCorner::Angular, smoothing: 0.5, ..p });
        assert!(matches!(angular, Filter::BevelShape { .. }) && angular.is_contour() && angular.label() == "Bevel" && angular.valid());
        assert_eq!(angular.support(), 7);
        let o = Filter::MaskOutline { radius: 3.0, threshold: 0.5, position: OutlinePosition::Both }.outline_parts().unwrap();
        let soft = Filter::mask_outline(OutlineParts { blur: 2.0, curve: 0.5, start_black: true, ..o });
        assert_eq!(soft.radius(), Some(5.0));
        assert_eq!(soft.support(), 6, "radius + blur + the half-pixel band");
        assert_eq!(Filter::mask_outline(soft.outline_parts().unwrap()), soft);
        assert!(!Filter::MaskOutlineShape { radius: 3.0, threshold: 0.5, position: OutlinePosition::Both, start_black: false, blur: 2.0, curve: 1.5 }.valid());
        // A scaled composite scales the outline's own width and blur, so its band matches the
        // radius the compositor reads (radius + blur, scaled).
        let half = soft.scaled(0.5);
        assert_eq!(half.radius(), Some(2.5));
        assert!(matches!(half, Filter::MaskOutlineShape { radius, blur, .. } if radius == 1.5 && blur == 1.0), "{half:?}");
        assert_eq!(soft.scaled(1.0), soft);
        // Drop Shadow: colour content only, offset along its angle, support covers offset + blur.
        let shadow = Filter::DropShadow { angle_deg: 90.0, distance: 4.0, size: 3.0, spread: 0.0, opacity: 0.5, color: [0.0; 3] };
        let ([dx, dy], size) = shadow.shadow_geometry().unwrap();
        assert!(dx.abs() < 1e-5 && (dy + 4.0).abs() < 1e-5 && size == 3.0, "90 degrees points up");
        assert_eq!(shadow.support(), 8);
        assert!(shadow.supports_channel(ChannelKind::BaseColor) && !shadow.supports_channel(ChannelKind::Height) && !shadow.supports_mask());
        assert!(shadow.softens_coverage() && Filter::DropShadow { angle_deg: 90.0, distance: 4.0, size: 3.0, spread: 0.0, opacity: 0.0, color: [0.0; 3] }.spatial_identity());
        assert!(!Filter::DropShadow { angle_deg: 90.0, distance: 4.0, size: 3.0, spread: 1.0, opacity: 0.5, color: [0.0; 3] }.valid());
        // Glow works on masks and every channel; FXAA is a per-texel neighbourhood filter.
        let glow = Filter::OuterGlow { width: 8.0, spread: 0.2, opacity: 1.0, color: [1.0; 3], precise: false };
        assert!(glow.supports_mask() && glow.supports_channel(ChannelKind::Roughness) && glow.label() == "Glow");
        assert_eq!(Filter::Glow { radius: 8.0, amount: 1.0, threshold: 0.6 }.label(), "Bloom");
        assert!(Filter::Fxaa { alpha: false }.is_pixelwise() && Filter::Fxaa { alpha: false }.support() == 2);
        assert!(Filter::Fxaa { alpha: true }.softens_coverage() && !Filter::Fxaa { alpha: false }.softens_coverage());
        assert!(Filter::HeightAdjust { invert: true, offset: 0.2, multiply: 2.0 }.valid());
        assert!(!Filter::HeightAdjust { invert: false, offset: 0.0, multiply: 5.0 }.valid());
        // Every entry of ALL has its own name.
        let mut names: Vec<_> = Filter::ALL.iter().map(|f| f.label()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Filter::ALL.len());
    }
    #[test]
    fn filter_parameters_and_domains_are_checked() {
        assert!(!Filter::Bevel {radius:2.0,amount:1.01,threshold:0.5}.valid());
        assert!(!Filter::MaskOutline {radius:2.0,threshold:f32::NAN,position:OutlinePosition::Both}.valid());
        for f in Filter::ALL { assert!(f.valid()); assert_eq!(f.supports_channel(ChannelKind::Normal), matches!(f, Filter::NormalInvert {..})); }
        assert!(!Filter::DirectionalBlur {radius:2.0,angle_deg:181.0}.valid());
        assert!(!Filter::Highpass {radius:2.0,amount:f32::INFINITY}.valid());
        assert!(!Filter::Highpass {radius:0.0,amount:0.0}.spatial_identity());
        assert!(Filter::DirectionalBlur {radius:0.0,angle_deg:45.0}.spatial_identity());
        assert!(!Filter::Blur {radius:32.01}.valid());
        assert!(!Filter::Sharpen {radius:2.0,amount:f32::NAN}.valid());
        assert!(!Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 }.supports_mask());
        assert!(!Filter::Clamp { low: 0.8, high: 0.2 }.valid());
        assert!(!Filter::Threshold { cutoff: f32::NAN }.valid());
        assert!(!Filter::Warp { intensity: 8.0, scale: 0, seed: 0 }.valid());
        assert!(!Filter::Warp { intensity: 8.0, scale: 65, seed: 0 }.valid());
        assert!(!Filter::Warp { intensity: 33.0, scale: 4, seed: 0 }.valid());
        assert!(Filter::Warp { intensity: 0.0, scale: 4, seed: 9 }.spatial_identity());
        assert_eq!(Filter::Warp { intensity: 8.2, scale: 4, seed: 9 }.support(), 9);
        assert!(!Filter::Median { radius: 16.5 }.valid());
        assert!(!Filter::Median { radius: f32::NAN }.valid());
        assert!(Filter::Median { radius: 0.4 }.spatial_identity());
        assert!(!Filter::Median { radius: 0.5 }.spatial_identity());
        assert_eq!(Filter::Median { radius: 2.2 }.support(), 3);
        assert!(!Filter::HistogramScan { position: 1.5, contrast: 0.5, invert: false }.valid());
        assert!(!Filter::HistogramScan { position: 0.5, contrast: f32::NAN, invert: false }.valid());
        assert!(!Filter::HistogramRange { position: 0.5, range: -0.1, softness: 0.0, invert: true }.valid());
        assert!(!Filter::HistogramRange { position: 0.5, range: 0.5, softness: 1.5, invert: true }.valid());
        assert!(!Filter::Contrast { gain: f32::INFINITY, pivot: 0.5 }.valid());
    }

    #[test]
    fn new_filters_have_checked_domains_and_channel_rules() {
        assert!(!Filter::Posterize { levels: 1 }.valid() && !Filter::Posterize { levels: 65 }.valid());
        assert!(!Filter::HistogramShift { shift: 1.5 }.valid());
        assert!(!Filter::ColorBalance { shadows: [0.0, f32::NAN, 0.0], midtones: [0.0; 3], highlights: [0.0; 3] }.valid());
        assert!(!Filter::ReplaceColor { from: [0.0; 3], to: [-0.1, 0.0, 0.0], tolerance: 0.1, softness: 0.1 }.valid());
        assert!(!Filter::Glow { radius: 4.0, amount: 1.0, threshold: 1.0 }.valid());
        assert!(!Filter::EdgeDetect { radius: 1.0, strength: 9.0 }.valid());
        assert!(!Filter::Mosaic { size: 0 }.valid() && !Filter::Mosaic { size: 65 }.valid());
        assert!(!Filter::DirectionalWarp { intensity: 4.0, angle_deg: 200.0 }.valid());
        for (f, color_only) in [(Filter::Grayscale, true), (Filter::ALL[19], true), (Filter::ALL[20], true)] {
            assert_eq!(f.supports_channel(ChannelKind::BaseColor), true);
            assert_eq!(f.supports_channel(ChannelKind::Roughness), !color_only, "{f:?}");
            assert!(!f.supports_mask());
        }
        let ni = Filter::NormalInvert { flip_x: true, flip_y: false, flip_z: false };
        assert!(ni.supports_channel(ChannelKind::Normal) && !ni.supports_channel(ChannelKind::BaseColor) && !ni.supports_mask());
        assert!(Filter::Invert.supports_mask() && Filter::Invert.supports_channel(ChannelKind::Height));
        assert_eq!(Filter::Mosaic { size: 8 }.support(), 5);
        assert_eq!(Filter::EdgeDetect { radius: 2.0, strength: 1.0 }.support(), 3);
        assert!(Filter::Mosaic { size: 1 }.spatial_identity() && Filter::Glow { radius: 0.0, amount: 1.0, threshold: 0.5 }.spatial_identity());
        assert_eq!(Filter::Mosaic { size: 8 }.scaled(2.0), Filter::Mosaic { size: 16 });
    }

    #[test]
    fn parity_filters_have_checked_domains_names_and_supports() {
        // One list entry per panel: the mode / wrap variants are reached from their base filter.
        let mut names: Vec<_> = Filter::ALL.iter().map(|f| f.label()).collect();
        names.sort(); names.dedup();
        assert_eq!(names.len(), Filter::ALL.len());
        for name in ["Smoothstep", "Contrast Luminosity", "Color Correct", "Transform", "Mirror"] { assert!(names.contains(&name), "{name}"); }
        assert_eq!(Filter::threshold(0.4, ThresholdMode::GreaterOrEqual), Filter::Threshold { cutoff: 0.4 });
        for mode in ThresholdMode::ALL {
            let f = Filter::threshold(0.4, mode);
            assert_eq!(f.threshold_parts(), Some((0.4, mode)));
            assert!(f.valid() && f.supports_mask() && f.label() == "Threshold" && f.radius().is_none());
        }
        assert!(ThresholdMode::Greater.test(0.5, 0.4) && !ThresholdMode::Greater.test(0.4, 0.4) && ThresholdMode::GreaterOrEqual.test(0.4, 0.4));
        assert!(ThresholdMode::Lower.test(0.3, 0.4) && !ThresholdMode::Lower.test(0.4, 0.4) && ThresholdMode::LowerOrEqual.test(0.4, 0.4));
        for wrap in [false, true] {
            assert_eq!(Filter::blur(4.0, wrap).blur_parts(), Some((4.0, wrap)));
            assert_eq!(Filter::histogram_shift(0.2, wrap).histogram_shift_parts(), Some((0.2, wrap)));
        }
        assert_eq!(Filter::blur(4.0, false), Filter::Blur { radius: 4.0 });
        let wrapped = Filter::blur(4.2, true);
        assert!(wrapped.valid() && wrapped.label() == "Blur" && wrapped.softens_coverage() && wrapped.reads_whole_texture());
        assert_eq!((wrapped.radius(), wrapped.support()), (Some(4.2), FULL_SUPPORT));
        assert!(Filter::blur(0.0, true).spatial_identity() && !Filter::blur(0.0, true).reads_whole_texture());
        assert!(!Filter::blur(33.0, true).valid());
        for mode in HslMode::ALL {
            let f = Filter::hsl(mode, 30.0, 1.2, -0.1);
            assert_eq!(f.hsl_parts(), Some((mode, [30.0, 1.2, -0.1])));
            assert!(f.valid() && f.label() == "HSL" && !f.supports_mask() && !f.supports_channel(ChannelKind::Roughness));
        }
        assert!(!Filter::hsl(HslMode::Perceptive, 181.0, 1.0, 0.0).valid());
        assert!(!Filter::Smoothstep { min: 0.0, max: 1.5 }.valid() && Filter::Smoothstep { min: 0.8, max: 0.2 }.valid());
        assert!(!Filter::ContrastLuminosity { contrast: 1.1, luminosity: 0.0 }.valid());
        let cc = Filter::ColorCorrect { shadows: [0.0; 3], midtones: [0.0, 2.0, 0.0], highlights: [0.0; 3] };
        assert!(!cc.valid() && !cc.supports_mask() && cc.supports_channel(ChannelKind::BaseColor) && !cc.supports_channel(ChannelKind::Height));
        let (transform, mirror) = (Filter::ALL[29], Filter::ALL[30]);
        for f in [transform, mirror] {
            assert!(f.is_pixelwise() && f.reads_whole_texture() && f.softens_coverage() && f.supports_mask());
            assert_eq!(f.support(), FULL_SUPPORT);
            assert_eq!(f.scaled(4.0), f, "UV-space parameters do not depend on the resolution");
        }
        assert!(!Filter::Transform { repeat: RepeatMode::Both, flip: FlipMode::None, scale: [0.0, 1.0], offset: [0.0; 2], rotation_deg: 0.0, tile_safe: false }.valid());
        assert!(!Filter::Mirror { mode: MirrorMode::Corner, offset: [0.5, 1.5], invert_x: false, invert_y: false, corner: MirrorCorner::TopLeft }.valid());
        assert_eq!(RepeatMode::Horizontal.repeats(), [true, false]);
        assert_eq!(FlipMode::Vertical.flips(), [false, true]);
        assert_eq!(MirrorCorner::BottomLeft.far(), [false, true]);
    }

    #[test]
    fn gradient_stops_are_bounded_sorted_and_checked() {
        let st = |position, color| GradientStop { position, color };
        assert!(Gradient::new(&[]).is_none());
        assert!(Gradient::new(&[st(0.0, [0.0; 3]); 9]).is_none());
        let mut g = Gradient::black_white();
        assert!(g.insert(st(0.5, [0.5; 3])));
        assert_eq!(g.stops().iter().map(|s| s.position).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
        while g.insert(st(0.7, [0.0; 3])) {}
        assert_eq!(g.stops().len(), MAX_GRADIENT_STOPS);
        assert!(g.valid());
        while g.remove(0) {}
        assert_eq!(g.stops().len(), 1);
        let mut g = Gradient::black_white();
        g.stops_mut()[0].position = 0.9; g.stops_mut()[1].position = 0.2; assert!(!g.valid()); g.sort(); assert!(g.valid());
        assert!(!Filter::GradientMap { gradient: Gradient::new(&[st(f32::NAN, [0.0; 3])]).unwrap(), to_gray: false }.valid());
        assert!(!Filter::GradientMap { gradient: Gradient::new(&[st(0.5, [-1.0, 0.0, 0.0])]).unwrap(), to_gray: false }.valid());
        for i in 0..GRADIENT_PRESETS.len() { assert!(Gradient::preset(i).unwrap().valid(), "preset {i}"); }
        assert!(Gradient::preset(GRADIENT_PRESETS.len()).is_none());
        assert!(Filter::GradientMap { gradient: Gradient::black_white(), to_gray: true }.supports_mask());
        assert!(!Filter::GradientMap { gradient: Gradient::black_white(), to_gray: true }.supports_channel(ChannelKind::Normal));
    }
}
