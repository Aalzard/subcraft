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
/// Anisotropic Kuwahara limits: Painter's Radius maximum (32 px), sector sharpness exponent,
/// structure tensor smoothing (px) and ellipse stretch.
pub const MAX_KUWAHARA_RADIUS: f32 = 32.0;
pub const MAX_KUWAHARA_SHARPNESS: f32 = 16.0;
pub const MAX_KUWAHARA_TENSOR: f32 = 8.0;
pub const MAX_KUWAHARA_ANISOTROPY: f32 = 4.0;

/// Stylization filters: largest paint smoothing (px), border width (px), stroke width (px),
/// stroke length (in stroke widths) and watercolor edge spread (px).
pub const MAX_STYLE_PAINT: f32 = 16.0;
pub const MAX_COMIC_BORDER: f32 = 4.0;
pub const MAX_COMIC_TONES: u32 = 16;
pub const MAX_STROKE_SIZE: f32 = 32.0;
pub const MAX_STROKE_LENGTH: f32 = 4.0;
pub const MAX_WATERCOLOR_SPREAD: f32 = 16.0;

/// Median cost grows with the window, so it is capped below the Gaussian radii.
pub const MAX_MEDIAN_RADIUS: f32 = 16.0;
/// Warp noise cells across the texture.
pub const MAX_WARP_SCALE: u32 = 64;
/// Most reads per texel of a Blur Slope (its Quality).
pub const MAX_SLOPE_SAMPLES: u32 = 32;
/// Largest Mosaic block edge in pixels (also bounds its tile margin).
pub const MAX_MOSAIC_SIZE: u32 = 64;
/// Posterize steps: 2 is a binary split, 64 is nearly continuous.
pub const MAX_POSTERIZE_LEVELS: u32 = 64;
/// Quantize palette size (each texel compares against every colour, so it stays small).
pub const MAX_QUANTIZE_COLORS: u32 = 64;
/// Quantize Contour Smoothing: blur radius in pixels before the palette mapping.
pub const MAX_QUANTIZE_SMOOTHING: f32 = 8.0;

/// Dithering Pattern of the Quantize filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherPattern {
    /// Ordered 4 x 4 Bayer matrix.
    Bayer4,
    /// Ordered 8 x 8 Bayer matrix (finer, 64 levels).
    Bayer8,
    /// Per-texel white noise.
    Noise,
}

impl DitherPattern {
    pub const ALL: [Self; 3] = [Self::Bayer4, Self::Bayer8, Self::Noise];
    pub fn label(self) -> &'static str { match self { Self::Bayer4 => "Bayer 4x4", Self::Bayer8 => "Bayer 8x8", Self::Noise => "Noise" } }
}

/// Where the colour of a Fill Area Color fill comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillColorMode {
    /// The filter's Fill Color.
    Color,
    /// The average colour of the outline around each area.
    Border,
}

impl FillColorMode {
    pub const ALL: [Self; 2] = [Self::Color, Self::Border];
    pub fn label(self) -> &'static str { match self { Self::Color => "Fill color", Self::Border => "Outline color" } }
}

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

/// Where a Blur Slope filter takes its slope from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeSource {
    /// The built-in noise field.
    #[default]
    Noise,
    /// The image being filtered: samples walk along its own gradient.
    Input,
    /// A grayscale image picked by the user, tiled like the noise: reads go uphill on it.
    Custom,
}
impl SlopeSource {
    pub const ALL: [Self; 3] = [Self::Noise, Self::Input, Self::Custom];
    pub fn label(self) -> &'static str {
        match self { Self::Noise => "Default noise", Self::Input => "Previous input", Self::Custom => "Custom noise" }
    }
}

/// How a Blur Slope filter combines the reads along its slope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlopeMode {
    #[default]
    Blur,
    /// The darkest read: dark areas eat into bright ones (chipped edges).
    Min,
    /// The brightest read: bright areas grow into dark ones.
    Max,
}
impl SlopeMode {
    pub const ALL: [Self; 3] = [Self::Blur, Self::Min, Self::Max];
    pub fn label(self) -> &'static str {
        match self { Self::Blur => "Blur", Self::Min => "Min", Self::Max => "Max" }
    }
}

/// What sits in an image slot of a filter: an imported image, or an anchor point of the stack
/// below (ADR-0045). In files an image is its id alone, an anchor is `{anchor, channel}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageSource {
    Image(subcraft_core::ResourceId),
    Anchor(crate::anchor::AnchorInput),
}

impl ImageSource {
    pub fn resource(self) -> Option<subcraft_core::ResourceId> {
        match self { Self::Image(id) => Some(id), Self::Anchor(_) => None }
    }
    pub fn anchor(self) -> Option<crate::anchor::AnchorInput> {
        match self { Self::Anchor(a) => Some(a), Self::Image(_) => None }
    }
}

/// Every Blur Slope parameter, whichever variant holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlopeParts {
    pub intensity: f32,
    pub scale: u32,
    pub seed: u32,
    pub samples: u32,
    pub source: SlopeSource,
    pub mode: SlopeMode,
    /// The Custom noise image.
    pub image: Option<ImageSource>,
}

/// What sets the radius of a Non-uniform Blur at each texel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlurMap {
    /// The texel's own value (luminance on colour): bright areas blur more.
    #[default]
    Value,
    /// One minus the value: dark areas blur more.
    InvertedValue,
    /// A smooth noise: patches of sharp and blurred.
    Noise,
    /// A grayscale image picked by the user, stretched over the texture: white blurs most.
    Image,
}
impl BlurMap {
    pub const ALL: [Self; 4] = [Self::Value, Self::InvertedValue, Self::Noise, Self::Image];
    pub fn label(self) -> &'static str {
        match self { Self::Value => "Bright areas", Self::InvertedValue => "Dark areas", Self::Noise => "Noise", Self::Image => "Image" }
    }
}

/// Where a Color Match filter takes the colour it moves from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    /// The average colour of the whole input.
    #[default]
    Average,
    /// The Source Color parameter.
    Parameter,
    /// A colour image picked by the user, stretched over the texture: each texel moves from
    /// the image's colour there.
    Image,
}
impl MatchSource {
    pub const ALL: [Self; 3] = [Self::Average, Self::Parameter, Self::Image];
    pub fn label(self) -> &'static str {
        match self { Self::Average => "Average", Self::Parameter => "Parameter", Self::Image => "Input" }
    }
}

/// Blur radius, in multiples of the intensity, of the smoothed copy of the image a Blur Slope
/// on Previous input measures its slope on.
pub const SLOPE_SMOOTHING: f32 = 1.5;

/// Most reads per texel of a Non-uniform Blur.
pub const MAX_BLUR_SAMPLES: u32 = 64;

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
    /// Painter's Anisotropic Kuwahara: an edge-keeping painterly smoothing. Each texel takes the
    /// mean of the calmest of 8 overlapping sectors of an ellipse stretched along the local edge
    /// direction (from a structure tensor smoothed over `tensor_smoothness` pixels, or the fixed
    /// `angle` in turns when `extract_direction` is off). `sharpness` favours calm sectors,
    /// `smoothness` widens the sectors, `anisotropy` stretches the ellipse along edges.
    AnisotropicKuwahara { radius: f32, smoothness: f32, sharpness: f32, tensor_smoothness: f32, anisotropy: f32, angle: f32, extract_direction: bool },
    /// MatFX Comic Book look: the image smoothed into flat painted areas (a Kuwahara pass of
    /// `paint` pixels), saturated by `color_intensity`, with ink borders where the smoothed image
    /// changes. A border starts at an edge strength of `border_tolerance`, ramps over
    /// `border_smoothness`, is `border_width` pixels wide and `border_opacity` dark.
    /// `input_contrast` / `input_luminosity` adjust the image first; `tones` cuts brightness into
    /// that many flat steps (cel shading, 0 = off), whose borders are inked too.
    ComicBook { tones: u32, paint: f32, color_intensity: f32, border_opacity: f32, border_tolerance: f32, border_smoothness: f32, border_width: f32, input_contrast: f32, input_luminosity: f32 },
    /// MatFX Watercolor look: a soft wash (`bleed` pixels of smoothing) thinned towards white
    /// paper by `1 - paint_opacity`, with pigment gathering at the edges of each wash
    /// (`borders_spread` pixels wide, `borders_density` strong) and soaking unevenly into the
    /// paper grain (`paper_absorption`). `hide_paper` removes the paper's own texture.
    Watercolor { paint_opacity: f32, paper_absorption: f32, borders_spread: f32, borders_density: f32, bleed: f32, hide_paper: bool, seed: u32 },
    /// MatFX Oil Paint strokes: the image repainted with overlapping brush strokes. Each stroke
    /// is `stroke_size` pixels wide and `stroke_length` widths long, takes the colour under its
    /// centre, follows the local edge direction (`angle` in turns where the image is flat),
    /// and shows bristle streaks (`bristles`). `size_random` and `color_random` vary strokes;
    /// `effect_intensity` mixes the result with the input.
    OilPaint { effect_intensity: f32, stroke_size: f32, stroke_length: f32, size_random: f32, color_random: f32, bristles: f32, angle: f32, seed: u32 },
    /// Painter's Fill Area Mask: areas of the mask closed off by a painted outline (values of
    /// `threshold` or more) become white. With `closed_borders` the edges of a UV island close
    /// an area too (the largest area of an island is taken as its outside); otherwise an area
    /// that reaches the island edge stays empty. Masks only; reads the whole texture.
    FillAreaMask { threshold: f32, closed_borders: bool },
    /// Painter's Fill Area Color: the same areas, found from the paint coverage (alpha) of the
    /// content, painted with `color` or with the average colour of their outline.
    FillAreaColor { threshold: f32, closed_borders: bool, mode: FillColorMode, color: [f32; 3] },
    /// Painter's Quantize: the image reduced to a palette of `colors` colours picked from the
    /// whole texture (median cut refined by k-means). `contour_smoothing` blurs the input first
    /// (pixels) for smoother region borders; `dithering` (0..1) mixes neighbouring colours with
    /// `pattern`; `lab` compares colours in CIELAB (colour channels only, data channels always
    /// use their values); `apply_to_alpha` turns coverage into 0 / 1 at `alpha_threshold`.
    Quantize { colors: u32, contour_smoothing: f32, dithering: f32, pattern: DitherPattern, lab: bool, apply_to_alpha: bool, alpha_threshold: f32 },
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
    /// Painter's Gradient Dynamic: each value (luminance on colour channels) picks the colour of
    /// `source` at that position along one row (`vertical`: one column), `position` across it.
    /// Masks and scalar channels use the colour's luminance.
    GradientDynamic { source: subcraft_core::ResourceId, vertical: bool, position: f32 },
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
    /// Painter's Gradient Curve: each value (0..=1) goes through the `preset` curve, is
    /// optionally inverted, then gets Balance (0..=1, 0.5 neutral) and Contrast (0..=1, 0 neutral):
    /// `(v - (1 - balance)) / (1 - contrast) + 0.5`.
    GradientCurve { preset: CurvePreset, invert: bool, balance: f32, contrast: f32 },
    /// Painter's Bevel Smooth: a ramp `radius` (Distance) pixels wide across the contour of the
    /// shape `threshold` (Mask Threshold) cuts out, going inward, outward or both ways.
    /// Curve Offset (-0.9..=0.9) delays (above 0) or shortens (below 0) the ramp, Curve Shape
    /// (-1 pinched ..= 1 rounded) raises it to the power `4^-shape`, Smoothing eases it toward a
    /// smoothstep.
    BevelSmooth { radius: f32, direction: BevelDirection, smoothing: f32, curve_offset: f32, curve_shape: f32, threshold: f32 },
    /// Painter's Directional Distance: bright values spread `distance` pixels along `angle_deg`
    /// (0 = right, counter-clockwise), fading with the distance; Contrast (0..=1) steepens the fade.
    DirectionalDistance { distance: f32, angle_deg: f32, contrast: f32 },
    /// Painter's Blur Slope: each texel is the average of `samples` reads along the offset the
    /// Warp noise field (`scale` cells across the texture, `seed`) gives it, up to `intensity`
    /// pixels per axis: a blur whose direction and length follow the noise.
    BlurSlope { intensity: f32, scale: u32, seed: u32, samples: u32 },
    /// [`Filter::BlurSlope`] with Painter's Source Type and Blending Mode (same panel).
    /// `source` Input puts the reads on the line across the nearest edge of the image itself, half
    /// of them uphill and half downhill, up to `intensity` pixels each way (`scale` and `seed` are then
    /// unused). `mode` keeps the average, the darkest or the brightest read.
    /// `source` Custom reads `image` (grayscale, `scale` repetitions across the texture) as the
    /// slope: each texel reads uphill on it. Without an image it does nothing.
    BlurSlopeMode {
        intensity: f32,
        scale: u32,
        seed: u32,
        samples: u32,
        source: SlopeSource,
        mode: SlopeMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<ImageSource>,
    },
    /// Blur whose radius changes per texel: `radius` pixels where the map is 1, none where it
    /// is 0. The map is the texel's own value, its inverse, or a noise with `scale` cells across
    /// the texture (`seed`). `samples` reads per texel on a spiral, Gaussian weighted.
    /// Map Image reads `image` (grayscale, stretched over the texture); without one nothing is
    /// blurred.
    NonUniformBlur {
        radius: f32,
        map: BlurMap,
        scale: u32,
        seed: u32,
        samples: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<ImageSource>,
    },
    /// Painter's Color Match: moves the colours near `source` (the Source Color, or the average
    /// of the whole input) so that the source colour becomes `target`, keeping the variation
    /// around it. Works in OKLab. `hue`, `chroma` and `luma` (0..=1) are how far from the source
    /// a colour may be and still move; 1 is no limit. Color channels only.
    /// Source mode Image takes the source colour per texel from `source_image`; a
    /// `target_image` gives the target colour per texel instead of `target` (both stretched over
    /// the texture). A missing source image falls back to `source`.
    ColorMatch {
        source_mode: MatchSource,
        source: [f32; 3],
        target: [f32; 3],
        hue: f32,
        chroma: f32,
        luma: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_image: Option<ImageSource>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_image: Option<ImageSource>,
    },
    /// [`Filter::Grayscale`] with Painter's Grayscale Conversion options (same panel): how the
    /// colour becomes gray (`mode`), then Invert, Balance (0.5 neutral) and Contrast (0 neutral)
    /// as in [`Filter::GradientCurve`].
    /// `weights` (Painter's Red / Green / Blue): when set, the gray is that weighted sum of the
    /// colour instead of `mode`. Not written when unset.
    GrayscaleConvert {
        mode: crate::mask::GrayscaleType,
        invert: bool,
        balance: f32,
        contrast: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<[f32; 3]>,
    },
}

/// Every Grayscale parameter, whichever variant holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrayParts {
    pub mode: crate::mask::GrayscaleType,
    pub invert: bool,
    pub balance: f32,
    pub contrast: f32,
    /// Custom Red / Green / Blue weights (instead of `mode`).
    pub weights: Option<[f32; 3]>,
}

impl GrayParts {
    /// Range of each custom weight.
    pub const WEIGHT: std::ops::RangeInclusive<f32> = 0.0..=1.0;
    /// The weights a new custom mix starts from (Rec. 709 luminance, as Luma).
    pub const LUMA_WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

    /// The gray value of `c`: the custom weighted sum (clamped to 0..=1), or the mode's.
    pub fn gray(&self, c: [f32; 3]) -> f32 {
        match self.weights {
            Some(w) => (c[0] * w[0] + c[1] * w[1] + c[2] * w[2]).clamp(0.0, 1.0),
            None => self.mode.convert(c),
        }
    }
}

impl Default for GrayParts {
    fn default() -> Self {
        Self { mode: crate::mask::GrayscaleType::Luma, invert: false, balance: 0.5, contrast: 0.0, weights: None }
    }
}

/// Curve of a Gradient Curve filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurvePreset {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    SCurve,
    Bell,
    Valley,
}

impl CurvePreset {
    pub const ALL: [Self; 6] = [Self::Linear, Self::EaseIn, Self::EaseOut, Self::SCurve, Self::Bell, Self::Valley];
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease in",
            Self::EaseOut => "Ease out",
            Self::SCurve => "S-curve",
            Self::Bell => "Bell",
            Self::Valley => "Valley",
        }
    }
    /// The curve at `t` (clamped to 0..=1).
    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::SCurve => t * t * (3.0 - 2.0 * t),
            Self::Bell => (t * std::f32::consts::PI).sin(),
            Self::Valley => 1.0 - (t * std::f32::consts::PI).sin(),
        }
    }
}

/// Direction of a Bevel Smooth ramp from the contour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BevelDirection {
    #[default]
    Inward,
    Outward,
    Both,
}

impl BevelDirection {
    pub const ALL: [Self; 3] = [Self::Inward, Self::Outward, Self::Both];
    pub fn label(self) -> &'static str {
        match self {
            Self::Inward => "Inward",
            Self::Outward => "Outward",
            Self::Both => "Both",
        }
    }
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
    pub const ALL: [Self; 48] = [Self::Contrast { gain: 1.0, pivot: 0.5 },
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
        Self::OuterGlow { width: 8.0, spread: 0.0, opacity: 0.75, color: [1.0; 3], precise: false },
        Self::GradientCurve { preset: CurvePreset::SCurve, invert: false, balance: 0.5, contrast: 0.0 },
        Self::BevelSmooth { radius: 8.0, direction: BevelDirection::Inward, smoothing: 0.5, curve_offset: 0.0, curve_shape: 0.0, threshold: 0.5 },
        Self::DirectionalDistance { distance: 16.0, angle_deg: 0.0, contrast: 0.0 },
        Self::BlurSlope { intensity: 8.0, scale: 4, seed: 1, samples: 8 },
        // Painter documents no defaults (only Radius' maximum, 32).
        Self::AnisotropicKuwahara { radius: 6.0, smoothness: 0.5, sharpness: 8.0, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true },
        // Painter documents no defaults.
        Self::Quantize { colors: 8, contour_smoothing: 0.0, dithering: 0.0, pattern: DitherPattern::Bayer4, lab: true, apply_to_alpha: false, alpha_threshold: 0.5 },
        Self::NonUniformBlur { radius: 12.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: None },
        Self::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [0.5; 3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None },
        // Painter documents no defaults for the three stylization filters.
        Self::ComicBook { tones: 4, paint: 5.0, color_intensity: 1.3, border_opacity: 1.0, border_tolerance: 0.25, border_smoothness: 0.3, border_width: 1.5, input_contrast: 0.0, input_luminosity: 0.0 },
        Self::Watercolor { paint_opacity: 0.9, paper_absorption: 0.5, borders_spread: 4.0, borders_density: 0.5, bleed: 5.0, hide_paper: false, seed: 1 },
        Self::OilPaint { effect_intensity: 1.0, stroke_size: 10.0, stroke_length: 3.0, size_random: 0.3, color_random: 0.15, bristles: 0.4, angle: 0.125, seed: 1 },
        // Painter documents no defaults for the two Fill Area filters.
        Self::FillAreaMask { threshold: 0.5, closed_borders: false },
        Self::FillAreaColor { threshold: 0.5, closed_borders: false, mode: FillColorMode::Color, color: [0.5; 3] }];
    /// A Blur Slope filter: the plain variant when the options are the defaults.
    pub fn blur_slope(p: SlopeParts) -> Self {
        let SlopeParts { intensity, scale, seed, samples, source, mode, image } = p;
        // The picked image is kept while another source is selected, so switching back finds it.
        if source == SlopeSource::Noise && mode == SlopeMode::Blur && image.is_none() { Self::BlurSlope { intensity, scale, seed, samples } } else { Self::BlurSlopeMode { intensity, scale, seed, samples, source, mode, image } }
    }
    /// Parameters of either Blur Slope variant.
    pub fn slope_parts(self) -> Option<SlopeParts> {
        match self {
            Self::BlurSlope { intensity, scale, seed, samples } => Some(SlopeParts { intensity, scale, seed, samples, source: SlopeSource::Noise, mode: SlopeMode::Blur, image: None }),
            Self::BlurSlopeMode { intensity, scale, seed, samples, source, mode, image } => Some(SlopeParts { intensity, scale, seed, samples, source, mode, image }),
            _ => None,
        }
    }

    /// A Grayscale filter (the plain variant when the conversion options are at their defaults).
    pub fn grayscale(p: GrayParts) -> Self {
        if p == GrayParts::default() {
            Self::Grayscale
        } else {
            Self::GrayscaleConvert { mode: p.mode, invert: p.invert, balance: p.balance, contrast: p.contrast, weights: p.weights }
        }
    }
    pub fn grayscale_parts(self) -> Option<GrayParts> {
        match self {
            Self::Grayscale => Some(GrayParts::default()),
            Self::GrayscaleConvert { mode, invert, balance, contrast, weights } => Some(GrayParts { mode, invert, balance, contrast, weights }),
            _ => None,
        }
    }

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

    /// The image slots of a filter that is shown in its current mode: slot index (stable per
    /// variant), the button text when the slot is empty, and what is in it.
    pub fn image_slots(self) -> Vec<(usize, &'static str, Option<ImageSource>)> {
        match self {
            Self::GradientDynamic { source, .. } => vec![(0, "Choose gradient image…", (source != subcraft_core::ResourceId::nil()).then_some(ImageSource::Image(source)))],
            Self::BlurSlopeMode { source: SlopeSource::Custom, image, .. } => vec![(0, "Choose noise image…", image)],
            Self::NonUniformBlur { map: BlurMap::Image, image, .. } => vec![(0, "Choose blur map image…", image)],
            Self::ColorMatch { source_mode, source_image, target_image, .. } => {
                let mut slots = Vec::new();
                if source_mode == MatchSource::Image {
                    slots.push((0, "Choose source color image…", source_image));
                }
                slots.push((1, "Choose target color image…", target_image));
                slots
            }
            _ => Vec::new(),
        }
    }

    /// Whether a slot of [`Filter::image_slots`] can read an anchor point as well as an image.
    pub fn slot_accepts_anchor(self, slot: usize) -> bool {
        !matches!(self, Self::GradientDynamic { .. }) && self.image_slots().iter().any(|s| s.0 == slot)
    }

    /// Put `image` in a slot of [`Filter::image_slots`] (`None` empties it). False when the
    /// filter has no such slot.
    pub fn set_image(&mut self, slot: usize, image: Option<subcraft_core::ResourceId>) -> bool {
        if let (Self::GradientDynamic { source, .. }, 0) = (&mut *self, slot) {
            *source = image.unwrap_or(subcraft_core::ResourceId::nil());
            return true;
        }
        self.set_slot(slot, image.map(ImageSource::Image))
    }

    /// Make a slot read an anchor point. False when the slot does not exist or reads images only.
    pub fn set_anchor(&mut self, slot: usize, input: crate::anchor::AnchorInput) -> bool {
        self.set_slot(slot, Some(ImageSource::Anchor(input)))
    }

    fn set_slot(&mut self, slot: usize, to: Option<ImageSource>) -> bool {
        match (self, slot) {
            (Self::BlurSlopeMode { image: i, .. }, 0) | (Self::NonUniformBlur { image: i, .. }, 0) => *i = to,
            (Self::ColorMatch { source_image, .. }, 0) => *source_image = to,
            (Self::ColorMatch { target_image, .. }, 1) => *target_image = to,
            _ => return false,
        }
        true
    }

    /// Every slot, used in the current mode or not.
    fn all_slots(self) -> [Option<ImageSource>; 2] {
        match self {
            Self::BlurSlopeMode { image, .. } | Self::NonUniformBlur { image, .. } => [image, None],
            Self::ColorMatch { source_image, target_image, .. } => [source_image, target_image],
            _ => [None; 2],
        }
    }

    /// Every image the filter holds, used in its current mode or not (they are saved with it).
    pub fn resources(self) -> Vec<subcraft_core::ResourceId> {
        match self {
            Self::GradientDynamic { source, .. } if source != subcraft_core::ResourceId::nil() => vec![source],
            _ => self.all_slots().into_iter().flatten().filter_map(ImageSource::resource).collect(),
        }
    }

    /// The anchor points the filter reads in its current mode, by slot. An anchor kept in a slot
    /// the mode does not show is not read, so it is no reference.
    pub fn anchor_inputs(self) -> Vec<(usize, crate::anchor::AnchorInput)> {
        if matches!(self, Self::GradientDynamic { .. }) {
            return Vec::new();
        }
        self.image_slots().into_iter().filter_map(|(slot, _, s)| Some((slot, s?.anchor()?))).collect()
    }

    /// Whether the filter holds (shown or not) an anchor that is a key of `map`.
    pub fn reads_any_anchor(self, map: &std::collections::HashMap<subcraft_core::EffectId, subcraft_core::EffectId>) -> bool {
        self.all_slots().into_iter().flatten().filter_map(ImageSource::anchor).any(|i| map.contains_key(&i.anchor))
    }

    /// Point every anchor the filter holds (shown or not) that is a key of `map` at the mapped one.
    pub fn remap_anchors(&mut self, map: &std::collections::HashMap<subcraft_core::EffectId, subcraft_core::EffectId>) {
        let slots = self.all_slots();
        for (slot, s) in slots.into_iter().enumerate() {
            if let Some(ImageSource::Anchor(input)) = s && let Some(new) = map.get(&input.anchor) {
                self.set_slot(slot, Some(ImageSource::Anchor(crate::anchor::AnchorInput { anchor: *new, ..input })));
            }
        }
    }

    /// The first image a filter reads, when it has one.
    pub fn resource(self) -> Option<subcraft_core::ResourceId> {
        self.resources().first().copied()
    }

    /// Whether a Color Match reads images, which only the compositor can do.
    pub fn match_reads_images(self) -> bool {
        matches!(self, Self::ColorMatch { source_mode: MatchSource::Image, source_image: Some(_), .. } | Self::ColorMatch { target_image: Some(_), .. })
    }

    /// A Color Match that must be evaluated by the stage graph: the Average mode, or an anchor
    /// point in a slot it reads.
    pub fn match_is_staged(self) -> bool {
        matches!(self, Self::ColorMatch { source_mode, .. } if source_mode == MatchSource::Average || !self.anchor_inputs().is_empty())
    }

    /// A Gradient Dynamic filter whose source image is still to be picked.
    pub fn gradient_dynamic() -> Self {
        Self::GradientDynamic { source: subcraft_core::ResourceId::nil(), vertical: false, position: 0.5 }
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
            Self::Fxaa {..} => "FXAA (Anti-Aliasing)", Self::DropShadow {..} => "Drop Shadow", Self::OuterGlow {..} => "Glow", Self::GradientDynamic {..} => "Gradient Dynamic",
            Self::GradientCurve {..} => "Gradient Curve", Self::BevelSmooth {..} => "Bevel Smooth", Self::DirectionalDistance {..} => "Directional Distance",
            Self::BlurSlope {..} | Self::BlurSlopeMode {..} => "Blur Slope", Self::NonUniformBlur {..} => "Non-uniform Blur", Self::ColorMatch {..} => "Color Match", Self::GrayscaleConvert {..} => "Grayscale",
            Self::AnisotropicKuwahara {..} => "Anisotropic Kuwahara", Self::Quantize {..} => "Quantize",
            Self::ComicBook {..} => "MatFX Comic Book", Self::Watercolor {..} => "MatFX Watercolor", Self::OilPaint {..} => "MatFX Oil Paint",
            Self::FillAreaMask {..} => "Fill Area Mask", Self::FillAreaColor {..} => "Fill Area Color" }
    }

    pub fn supports_channel(self, channel: ChannelKind) -> bool {
        if matches!(self, Self::NormalInvert { .. }) { return channel == ChannelKind::Normal; }
        // Masks only.
        if matches!(self, Self::FillAreaMask { .. }) { return false; }
        channel != ChannelKind::Normal && (!self.color_only() || channel.is_color())
    }

    /// Filters that only make sense on RGB colour data.
    fn color_only(self) -> bool {
        matches!(self, Self::Hsl { .. } | Self::HslPerceptive { .. } | Self::Grayscale | Self::GrayscaleConvert { .. } | Self::ColorBalance { .. } | Self::ReplaceColor { .. } | Self::ColorCorrect { .. } | Self::DropShadow { .. } | Self::ColorMatch { .. })
    }

    /// Quantize works on content only, as in Painter.
    pub fn supports_mask(self) -> bool { !self.color_only() && !matches!(self, Self::NormalInvert { .. } | Self::Quantize { .. } | Self::FillAreaColor { .. }) }

    /// Copy with pixel-based parameters multiplied by `factor` (texture resolution ratio).
    pub fn scaled(self, factor: f32) -> Self {
        match self {
            Self::EdgeDetect { radius, strength } => Self::EdgeDetect { radius: radius * factor, strength },
            Self::Glow { radius, amount, threshold } => Self::Glow { radius: radius * factor, amount, threshold },
            Self::DirectionalWarp { intensity, angle_deg } => Self::DirectionalWarp { intensity: intensity * factor, angle_deg },
            Self::Mosaic { size } => Self::Mosaic { size: ((size as f32 * factor).round() as u32).clamp(1, MAX_MOSAIC_SIZE) },
            Self::DirectionalDistance { distance, angle_deg, contrast } => Self::DirectionalDistance { distance: distance * factor, angle_deg, contrast },
            Self::BlurSlopeMode { intensity, scale, seed, samples, source, mode, image } => Self::BlurSlopeMode { intensity: intensity * factor, scale, seed, samples, source, mode, image },
            Self::NonUniformBlur { radius, map, scale, seed, samples, image } => Self::NonUniformBlur { radius: radius * factor, map, scale, seed, samples, image },
            Self::Quantize { colors, contour_smoothing, dithering, pattern, lab, apply_to_alpha, alpha_threshold } => {
                Self::Quantize { colors, contour_smoothing: (contour_smoothing * factor).min(MAX_QUANTIZE_SMOOTHING), dithering, pattern, lab, apply_to_alpha, alpha_threshold }
            }
            Self::AnisotropicKuwahara { radius, smoothness, sharpness, tensor_smoothness, anisotropy, angle, extract_direction } => {
                Self::AnisotropicKuwahara { radius: (radius * factor).min(MAX_KUWAHARA_RADIUS), smoothness, sharpness, tensor_smoothness: (tensor_smoothness * factor).min(MAX_KUWAHARA_TENSOR), anisotropy, angle, extract_direction }
            }
            Self::ComicBook { tones, paint, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity } => {
                Self::ComicBook { tones, paint: (paint * factor).min(MAX_STYLE_PAINT), color_intensity, border_opacity, border_tolerance, border_smoothness, border_width: (border_width * factor).min(MAX_COMIC_BORDER), input_contrast, input_luminosity }
            }
            Self::Watercolor { paint_opacity, paper_absorption, borders_spread, borders_density, bleed, hide_paper, seed } => {
                Self::Watercolor { paint_opacity, paper_absorption, borders_spread: (borders_spread * factor).min(MAX_WATERCOLOR_SPREAD), borders_density, bleed: (bleed * factor).min(MAX_STYLE_PAINT), hide_paper, seed }
            }
            Self::OilPaint { effect_intensity, stroke_size, stroke_length, size_random, color_random, bristles, angle, seed } => {
                Self::OilPaint { effect_intensity, stroke_size: (stroke_size * factor).min(MAX_STROKE_SIZE), stroke_length, size_random, color_random, bristles, angle, seed }
            }
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
        matches!(self, Self::EdgeDetect { .. } | Self::DirectionalWarp { .. } | Self::Mosaic { .. } | Self::Transform { .. } | Self::Mirror { .. } | Self::Fxaa { .. } | Self::DirectionalDistance { .. } | Self::AnisotropicKuwahara { .. } | Self::Quantize { .. } | Self::BlurSlopeMode { .. } | Self::NonUniformBlur { .. }
            | Self::ComicBook { .. } | Self::Watercolor { .. } | Self::OilPaint { .. } | Self::FillAreaMask { .. } | Self::FillAreaColor { .. }) || self.match_is_staged()
    }

    /// The Kuwahara smoothing pass the Comic Book and Watercolor filters run before their own
    /// node (none under half a pixel).
    pub fn style_smoothing(self) -> Option<Self> {
        let (radius, smoothness, sharpness) = match self {
            Self::ComicBook { paint, .. } => (paint, 0.5, 8.0),
            // A wash is softer than flat comic paint: wide sectors, less edge keeping.
            Self::Watercolor { bleed, .. } => (bleed, 1.0, 4.0),
            _ => return None,
        };
        (radius >= 0.5).then_some(Self::AnisotropicKuwahara { radius, smoothness, sharpness, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true })
    }

    /// Farthest texel an Oil Paint stroke can reach from a texel it covers: half its (largest)
    /// length, the jitter of its centre, and the taps its direction is measured with.
    pub fn stroke_reach(stroke_size: f32, stroke_length: f32, size_random: f32) -> f32 {
        let size = stroke_size * (1.0 + 0.5 * size_random.clamp(0.0, 1.0));
        size * stroke_length.clamp(1.0, MAX_STROKE_LENGTH) * 0.5 + stroke_size * 0.75 + (stroke_size * 0.25).max(1.0) + 1.0
    }

    /// Filters whose every texel may read any other texel of the texture.
    pub fn reads_whole_texture(self) -> bool {
        // Quantize picks its palette from the whole texture; Fill Area flood-fills it.
        matches!(self, Self::Transform { .. } | Self::Mirror { .. } | Self::Quantize { .. } | Self::FillAreaMask { .. } | Self::FillAreaColor { .. } | Self::ColorMatch { source_mode: MatchSource::Average, .. }) || matches!(self, Self::BlurWrap { radius } if radius > 0.0)
    }

    pub fn radius(self) -> Option<f32> {
        match self { Self::Transform { .. } | Self::Mirror { .. } | Self::Quantize { .. } | Self::FillAreaMask { .. } | Self::FillAreaColor { .. } | Self::ColorMatch { source_mode: MatchSource::Average, .. } => Some(FULL_SUPPORT as f32),
            Self::Blur { radius } | Self::BlurWrap { radius } | Self::Sharpen { radius, .. } | Self::DirectionalBlur { radius, .. } | Self::Highpass { radius, .. } | Self::Median { radius } | Self::Warp { intensity: radius, .. } | Self::Bevel {radius,..} | Self::MaskOutline {radius,..} | Self::EdgeDetect { radius, .. } | Self::Glow { radius, .. } | Self::DirectionalWarp { intensity: radius, .. } => Some(radius),
            Self::Mosaic { size } => Some(size as f32),
            Self::BevelShape { radius, .. } | Self::BevelSmooth { radius, .. } => Some(radius),
            Self::DirectionalDistance { distance, .. } => Some(distance),
            Self::BlurSlope { intensity, .. } | Self::BlurSlopeMode { intensity, .. } => Some(intensity),
            Self::NonUniformBlur { radius, .. } => Some(radius),
            // Reads an anchor at the texel itself: staged, with the smallest neighbourhood.
            Self::ColorMatch { .. } if self.match_is_staged() => Some(1.0),
            Self::MaskOutlineShape { radius, blur, .. } => Some(radius + blur),
            Self::Fxaa { .. } => Some(1.0),
            Self::DropShadow { distance, size, .. } => Some(distance + size),
            Self::OuterGlow { width, .. } => Some(width),
            // Their own nodes only: the smoothing pass before them is a separate node.
            Self::ComicBook { border_width, .. } => Some(border_width.max(1.0)),
            Self::Watercolor { borders_spread, .. } => Some(borders_spread.max(1.0)),
            Self::OilPaint { effect_intensity: 0.0, .. } => Some(0.0),
            Self::OilPaint { stroke_size, stroke_length, size_random, .. } => Some(Self::stroke_reach(stroke_size, stroke_length, size_random)),
            // The ellipse reaches `radius * (1 + anisotropy)` along an edge; the tensor reads
            // its smoothing span plus the Sobel taps.
            Self::AnisotropicKuwahara { radius, anisotropy, tensor_smoothness, extract_direction, .. } => {
                Some(radius * (1.0 + anisotropy.clamp(0.0, MAX_KUWAHARA_ANISOTROPY)) + if extract_direction { tensor_smoothness + 1.0 } else { 0.0 })
            }
            _ => None }
    }

    pub fn is_contour(self) -> bool { matches!(self, Self::Bevel {..} | Self::MaskOutline {..} | Self::BevelShape {..} | Self::MaskOutlineShape {..} | Self::BevelSmooth {..}) }

    /// Contours need a half-pixel antialias band beyond their nominal radius.
    pub fn support(self) -> u32 {
        if let Self::Mosaic { size } = self { return size / 2 + 1; }
        if self.reads_whole_texture() { return FULL_SUPPORT; }
        // Edge Detect also reads one sample beyond its offset; Directional Warp interpolates.
        (self.radius().unwrap_or(0.0) + if self.is_contour() {0.5} else {0.0}
            + if matches!(self, Self::DirectionalWarp { .. } | Self::EdgeDetect { .. } | Self::Fxaa { .. } | Self::DropShadow { .. } | Self::DirectionalDistance { .. } | Self::NonUniformBlur { .. }) {1.0} else {0.0}
            + match self {
                // Previous input also reads the smoothed copy the slope is measured on.
                Self::BlurSlopeMode { intensity, source: SlopeSource::Input, .. } => intensity * SLOPE_SMOOTHING + 2.0,
                Self::BlurSlopeMode { .. } => 1.0,
                _ => 0.0,
            }).ceil() as u32
    }

    pub fn softens_coverage(self) -> bool {
        matches!(self, Self::Blur { .. } | Self::BlurWrap { .. } | Self::Transform { .. } | Self::Mirror { .. } | Self::DirectionalBlur { .. } | Self::Warp { .. } | Self::DirectionalWarp { .. } | Self::Mosaic { .. }
            | Self::Fxaa { alpha: true } | Self::DropShadow { .. } | Self::OuterGlow { .. } | Self::BlurSlope { .. } | Self::BlurSlopeMode { .. } | Self::NonUniformBlur { .. }
            // A filled area becomes covered.
            | Self::FillAreaColor { .. })
    }

    /// Highpass is deliberately excluded: radius/amount zero gives its neutral value.
    pub fn spatial_identity(self) -> bool {
        matches!(self, Self::Blur {radius:0.0} | Self::BlurWrap {radius:0.0} | Self::DirectionalBlur {radius:0.0,..}
            | Self::Sharpen {radius:0.0,..} | Self::Sharpen {amount:0.0,..} | Self::Warp {intensity:0.0,..}
            | Self::DirectionalWarp {intensity:0.0,..} | Self::Mosaic {size:1} | Self::Glow {radius:0.0,..} | Self::Glow {amount:0.0,..}
            | Self::DropShadow {opacity:0.0,..} | Self::OuterGlow {opacity:0.0,..} | Self::OuterGlow {width:0.0,..}
            | Self::DirectionalDistance {distance:0.0,..} | Self::BlurSlope {intensity:0.0,..} | Self::BlurSlopeMode {intensity:0.0,..} | Self::NonUniformBlur {radius:0.0,..})
            || matches!(self, Self::Median { radius } if radius < 0.5)
            || matches!(self, Self::AnisotropicKuwahara { radius, .. } if radius < 0.5)
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
            Self::Quantize { colors, contour_smoothing, dithering, alpha_threshold, .. } => {
                (2..=MAX_QUANTIZE_COLORS).contains(&colors)
                    && contour_smoothing.is_finite() && (0.0..=MAX_QUANTIZE_SMOOTHING).contains(&contour_smoothing)
                    && dithering.is_finite() && (0.0..=1.0).contains(&dithering)
                    && alpha_threshold.is_finite() && (0.0..=1.0).contains(&alpha_threshold)
            }
            // A threshold of 0 would make every texel outline.
            Self::FillAreaMask { threshold, .. } => threshold.is_finite() && (0.01..=1.0).contains(&threshold),
            Self::FillAreaColor { threshold, color, .. } => threshold.is_finite() && (0.01..=1.0).contains(&threshold) && color.iter().all(|v| v.is_finite() && *v >= 0.0),
            Self::ComicBook { tones, paint, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity } => {
                let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
                tones <= MAX_COMIC_TONES && tones != 1 && paint.is_finite() && (0.0..=MAX_STYLE_PAINT).contains(&paint)
                    && color_intensity.is_finite() && (0.0..=2.0).contains(&color_intensity)
                    && unit(border_opacity) && unit(border_tolerance) && unit(border_smoothness)
                    && border_width.is_finite() && (0.5..=MAX_COMIC_BORDER).contains(&border_width)
                    && [input_contrast, input_luminosity].iter().all(|v| v.is_finite() && (-1.0..=1.0).contains(v))
            }
            Self::Watercolor { paint_opacity, paper_absorption, borders_spread, borders_density, bleed, .. } => {
                [paint_opacity, paper_absorption, borders_density].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && borders_spread.is_finite() && (0.0..=MAX_WATERCOLOR_SPREAD).contains(&borders_spread)
                    && bleed.is_finite() && (0.0..=MAX_STYLE_PAINT).contains(&bleed)
            }
            Self::OilPaint { effect_intensity, stroke_size, stroke_length, size_random, color_random, bristles, angle, .. } => {
                [effect_intensity, size_random, color_random, bristles].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && stroke_size.is_finite() && (2.0..=MAX_STROKE_SIZE).contains(&stroke_size)
                    && stroke_length.is_finite() && (1.0..=MAX_STROKE_LENGTH).contains(&stroke_length)
                    && angle.is_finite()
            }
            Self::AnisotropicKuwahara { radius, smoothness, sharpness, tensor_smoothness, anisotropy, angle, .. } => {
                radius.is_finite() && (0.0..=MAX_KUWAHARA_RADIUS).contains(&radius)
                    && smoothness.is_finite() && (0.0..=1.0).contains(&smoothness)
                    && sharpness.is_finite() && (0.0..=MAX_KUWAHARA_SHARPNESS).contains(&sharpness)
                    && tensor_smoothness.is_finite() && (0.0..=MAX_KUWAHARA_TENSOR).contains(&tensor_smoothness)
                    && anisotropy.is_finite() && (0.0..=MAX_KUWAHARA_ANISOTROPY).contains(&anisotropy)
                    && angle.is_finite()
            }
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
            Self::GradientDynamic { position, .. } => position.is_finite() && (0.0..=1.0).contains(&position),
            Self::BevelShape { radius, amount, threshold, smoothing, .. } => {
                Self::Bevel { radius, amount, threshold }.valid() && smoothing.is_finite() && (0.0..=1.0).contains(&smoothing)
            }
            Self::MaskOutlineShape { radius, threshold, position, blur, curve, .. } => {
                Self::MaskOutline { radius, threshold, position }.valid() && blur.is_finite() && (0.0..=32.0).contains(&blur) && curve.is_finite() && (-1.0..=1.0).contains(&curve)
            }
            Self::GradientCurve { balance, contrast, .. } => [balance, contrast].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            Self::BevelSmooth { radius, smoothing, curve_offset, curve_shape, threshold, .. } => {
                Self::Blur { radius }.valid() && smoothing.is_finite() && (0.0..=1.0).contains(&smoothing)
                    && curve_offset.is_finite() && (-0.9..=0.9).contains(&curve_offset)
                    && curve_shape.is_finite() && (-1.0..=1.0).contains(&curve_shape)
                    && threshold.is_finite() && (-1.0..=1.0).contains(&threshold)
            }
            Self::DirectionalDistance { distance, angle_deg, contrast } => {
                Self::Blur { radius: distance }.valid() && angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg)
                    && contrast.is_finite() && (0.0..=1.0).contains(&contrast)
            }
            Self::GrayscaleConvert { balance, contrast, weights, .. } => {
                [balance, contrast].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    && weights.is_none_or(|w| w.iter().all(|v| v.is_finite() && GrayParts::WEIGHT.contains(v)))
            }
            Self::BlurSlope { intensity, scale, samples, .. } => {
                Self::Blur { radius: intensity }.valid() && (1..=MAX_WARP_SCALE).contains(&scale) && (2..=MAX_SLOPE_SAMPLES).contains(&samples)
            }
            Self::BlurSlopeMode { intensity, scale, seed, samples, .. } => Self::BlurSlope { intensity, scale, seed, samples }.valid(),
            Self::NonUniformBlur { radius, scale, samples, .. } => {
                Self::Blur { radius }.valid() && (1..=MAX_WARP_SCALE).contains(&scale) && (4..=MAX_BLUR_SAMPLES).contains(&samples)
            }
            Self::ColorMatch { source, target, hue, chroma, luma, .. } => {
                source.iter().chain(&target).all(|v| v.is_finite() && *v >= 0.0) && [hue, chroma, luma].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
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
    fn slope_modes_non_uniform_blur_and_color_match_rules() {
        // Blur Slope options: defaults give back the plain variant, others the option variant,
        // which shares the panel and so the name.
        let plain = Filter::BlurSlope { intensity: 8.0, scale: 4, seed: 1, samples: 8 };
        let parts = plain.slope_parts().unwrap();
        assert_eq!(Filter::blur_slope(parts), plain);
        let eat = Filter::blur_slope(SlopeParts { source: SlopeSource::Input, mode: SlopeMode::Min, ..parts });
        assert_eq!(eat, Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 8, source: SlopeSource::Input, mode: SlopeMode::Min, image: None });
        assert_eq!(Filter::blur_slope(eat.slope_parts().unwrap()), eat);
        assert!(eat.valid() && eat.label() == "Blur Slope" && eat.is_pixelwise() && eat.softens_coverage() && eat.supports_mask());
        assert_eq!((eat.radius(), eat.support()), (Some(8.0), 22), "the walk, the smoothed slope and its taps");
        assert_eq!(Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 8, source: SlopeSource::Noise, mode: SlopeMode::Min, image: None }.support(), 9);
        assert_eq!(eat.scaled(0.5).radius(), Some(4.0));
        assert!(Filter::BlurSlopeMode { intensity: 0.0, scale: 4, seed: 1, samples: 8, source: SlopeSource::Input, mode: SlopeMode::Max, image: None }.spatial_identity());
        assert!(!Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 1, source: SlopeSource::Input, mode: SlopeMode::Max, image: None }.valid());
        assert_eq!(serde_json::to_string(&eat).unwrap(), r#"{"kind":"blur_slope_mode","intensity":8.0,"scale":4,"seed":1,"samples":8,"source":"input","mode":"min"}"#);

        let nub = Filter::NonUniformBlur { radius: 12.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: None };
        assert!(Filter::ALL.contains(&nub) && nub.valid() && nub.label() == "Non-uniform Blur" && nub.is_pixelwise() && nub.softens_coverage());
        assert!(nub.supports_mask() && nub.supports_channel(ChannelKind::Height) && !nub.supports_channel(ChannelKind::Normal) && !nub.reads_whole_texture());
        assert_eq!((nub.radius(), nub.support(), nub.scaled(0.5).radius()), (Some(12.0), 13, Some(6.0)));
        assert!(Filter::NonUniformBlur { radius: 0.0, map: BlurMap::Noise, scale: 4, seed: 1, samples: 32, image: None }.spatial_identity());
        for bad in [
            Filter::NonUniformBlur { radius: 33.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: None },
            Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 0, seed: 1, samples: 32, image: None },
            Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 3, image: None },
            Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 4, seed: 1, samples: MAX_BLUR_SAMPLES + 1, image: None },
        ] {
            assert!(!bad.valid(), "{bad:?}");
        }
        assert_eq!(serde_json::to_string(&nub).unwrap(), r#"{"kind":"non_uniform_blur","radius":12.0,"map":"value","scale":4,"seed":1,"samples":32}"#);

        // Color Match: colour content only. Average reads the whole texture and is staged;
        // Parameter is a point filter.
        let average = Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [0.5; 3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None };
        let picked = Filter::ColorMatch { source_mode: MatchSource::Parameter, source: [0.5; 3], target: [0.1; 3], hue: 0.2, chroma: 1.0, luma: 1.0, source_image: None, target_image: None };
        assert!(Filter::ALL.contains(&average) && average.valid() && picked.valid() && average.label() == "Color Match");
        assert!(average.supports_channel(ChannelKind::BaseColor) && !average.supports_channel(ChannelKind::Roughness) && !average.supports_mask());
        assert!(average.is_pixelwise() && average.reads_whole_texture() && average.support() == FULL_SUPPORT && !average.softens_coverage());
        assert!(!picked.is_pixelwise() && !picked.reads_whole_texture() && picked.radius().is_none());
        assert!(!Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [0.5; 3], hue: 1.2, chroma: 1.0, luma: 1.0, source_image: None, target_image: None }.valid());
        assert!(!Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [-0.1, 0.5, 0.5], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None }.valid());
        assert_eq!(serde_json::to_string(&picked).unwrap(), r#"{"kind":"color_match","source_mode":"parameter","source":[0.5,0.5,0.5],"target":[0.1,0.1,0.1],"hue":0.2,"chroma":1.0,"luma":1.0}"#);
        for f in [eat, nub, average, picked] {
            assert_eq!(serde_json::from_str::<Filter>(&serde_json::to_string(&f).unwrap()).unwrap(), f);
        }
    }

    #[test]
    fn image_slots_follow_the_mode_and_old_files_load_without_them() {
        let (a, b) = (subcraft_core::ResourceId::new(), subcraft_core::ResourceId::new());
        // Blur Slope: a slot only on the custom noise source; the image survives a mode change.
        let mut f = Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 8, source: SlopeSource::Custom, mode: SlopeMode::Blur, image: None };
        assert_eq!(f.image_slots(), vec![(0, "Choose noise image…", None)]);
        assert!(f.set_image(0, Some(a)) && !f.set_image(1, Some(b)));
        assert_eq!((f.image_slots()[0].2, f.resources(), f.resource()), (Some(ImageSource::Image(a)), vec![a], Some(a)));
        let mut parts = f.slope_parts().unwrap();
        parts.source = SlopeSource::Noise;
        let noise = Filter::blur_slope(parts);
        assert!(matches!(noise, Filter::BlurSlopeMode { image: Some(ImageSource::Image(i)), .. } if i == a), "the image is kept: {noise:?}");
        assert!(noise.image_slots().is_empty() && noise.resources() == vec![a]);
        assert!(Filter::BlurSlope { intensity: 8.0, scale: 4, seed: 1, samples: 8 }.image_slots().is_empty());
        // Non-uniform Blur: a slot only on the Image map.
        let mut f = Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Image, scale: 4, seed: 1, samples: 32, image: None };
        assert!(f.set_image(0, Some(a)) && f.image_slots() == vec![(0, "Choose blur map image…", Some(ImageSource::Image(a)))]);
        assert!(Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: Some(ImageSource::Image(a)) }.image_slots().is_empty());
        // Color Match: the target slot always, the source slot in the Input mode.
        let mut f = Filter::ColorMatch { source_mode: MatchSource::Parameter, source: [0.5; 3], target: [0.1; 3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None };
        assert_eq!(f.image_slots().iter().map(|s| s.0).collect::<Vec<_>>(), vec![1]);
        assert!(!f.match_reads_images() && f.set_image(0, Some(a)) && !f.match_reads_images(), "a source image is unused outside the Input mode");
        assert!(f.set_image(1, Some(b)) && f.match_reads_images() && f.resources() == vec![a, b]);
        assert!(f.set_image(1, None) && f.resources() == vec![a]);
        if let Filter::ColorMatch { source_mode, .. } = &mut f { *source_mode = MatchSource::Image; }
        assert!(f.match_reads_images() && f.image_slots().iter().map(|s| s.0).collect::<Vec<_>>() == vec![0, 1]);
        // Files: images round-trip, and files written before the slots existed still load.
        for f in [f, noise] { assert_eq!(serde_json::from_str::<Filter>(&serde_json::to_string(&f).unwrap()).unwrap(), f); }
        let old: Filter = serde_json::from_str(r#"{"kind":"non_uniform_blur","radius":12.0,"map":"value","scale":4,"seed":1,"samples":32}"#).unwrap();
        assert_eq!(old, Filter::NonUniformBlur { radius: 12.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: None });
        let old: Filter = serde_json::from_str(r#"{"kind":"color_match","source_mode":"average","source":[0.5,0.5,0.5],"target":[0.1,0.1,0.1],"hue":1.0,"chroma":1.0,"luma":1.0}"#).unwrap();
        assert!(matches!(old, Filter::ColorMatch { source_image: None, target_image: None, .. }));
        // Gradient Dynamic keeps its one slot.
        let mut g = Filter::gradient_dynamic();
        assert!(g.image_slots()[0].2.is_none() && g.set_image(0, Some(a)) && g.resources() == vec![a] && g.set_image(0, None) && g.resources().is_empty());
    }

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
        // Gradient Curve, Bevel Smooth, Directional Distance.
        assert_eq!(CurvePreset::Linear.eval(0.3), 0.3);
        assert!((CurvePreset::EaseIn.eval(0.5) - 0.25).abs() < 1e-6 && (CurvePreset::EaseOut.eval(0.5) - 0.75).abs() < 1e-6);
        assert!((CurvePreset::SCurve.eval(0.25) - 0.15625).abs() < 1e-6);
        assert!((CurvePreset::Bell.eval(0.5) - 1.0).abs() < 1e-6 && CurvePreset::Valley.eval(0.5).abs() < 1e-6);
        assert_eq!(CurvePreset::EaseIn.eval(2.0), 1.0, "clamped");
        let smooth = Filter::BevelSmooth { radius: 8.0, direction: BevelDirection::Both, smoothing: 0.5, curve_offset: 0.0, curve_shape: 0.0, threshold: 0.5 };
        assert!(smooth.valid() && smooth.is_contour() && smooth.support() == 9 && smooth.label() == "Bevel Smooth");
        assert!(!Filter::BevelSmooth { radius: 8.0, direction: BevelDirection::Both, smoothing: 0.5, curve_offset: 1.0, curve_shape: 0.0, threshold: 0.5 }.valid());
        let far = Filter::DirectionalDistance { distance: 16.0, angle_deg: 0.0, contrast: 0.0 };
        assert!(far.valid() && far.is_pixelwise() && far.support() == 17 && !far.softens_coverage());
        assert_eq!(far.scaled(0.5), Filter::DirectionalDistance { distance: 8.0, angle_deg: 0.0, contrast: 0.0 });
        assert!(Filter::DirectionalDistance { distance: 0.0, angle_deg: 0.0, contrast: 0.0 }.spatial_identity());
        assert!(!Filter::GradientCurve { preset: CurvePreset::Bell, invert: false, balance: 1.5, contrast: 0.0 }.valid());
        for f in [smooth, far, Filter::GradientCurve { preset: CurvePreset::Bell, invert: true, balance: 0.5, contrast: 0.0 }] {
            assert!(Filter::ALL.iter().any(|a| a.label() == f.label()) && f.supports_mask() && f.supports_channel(ChannelKind::Height));
        }
        // Every entry of ALL has its own name.
        let mut names: Vec<_> = Filter::ALL.iter().map(|f| f.label()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Filter::ALL.len());
    }
    #[test]
    fn fill_area_filters_read_the_whole_texture_and_split_by_target() {
        let find = |name: &str| Filter::ALL.iter().copied().find(|f| f.label() == name).unwrap();
        let (mask, color) = (find("Fill Area Mask"), find("Fill Area Color"));
        for f in [mask, color] {
            assert!(f.valid() && f.is_pixelwise() && f.reads_whole_texture() && f.support() == FULL_SUPPORT && !f.spatial_identity());
            assert_eq!(f.scaled(4.0), f, "nothing in pixels");
        }
        // The mask filter is for masks only, the colour filter for content only (it adds coverage).
        assert!(mask.supports_mask() && ChannelKind::ALL.iter().all(|k| !mask.supports_channel(*k)));
        assert!(!color.supports_mask() && color.supports_channel(ChannelKind::BaseColor) && color.supports_channel(ChannelKind::Height) && !color.supports_channel(ChannelKind::Normal));
        assert!(color.softens_coverage() && !mask.softens_coverage());
        assert!(!Filter::FillAreaMask { threshold: 0.0, closed_borders: false }.valid());
        assert!(!Filter::FillAreaColor { threshold: 0.5, closed_borders: false, mode: FillColorMode::Border, color: [f32::NAN, 0.0, 0.0] }.valid());
    }

    #[test]
    fn stylization_filters_validate_scale_and_stage() {
        let find = |name: &str| Filter::ALL.iter().copied().find(|f| f.label() == name).unwrap();
        let (comic, wash, oil) = (find("MatFX Comic Book"), find("MatFX Watercolor"), find("MatFX Oil Paint"));
        for f in [comic, wash, oil] {
            assert!(f.valid() && f.is_pixelwise() && f.supports_channel(ChannelKind::BaseColor) && !f.supports_channel(ChannelKind::Normal));
            assert!(f.scaled(64.0).valid(), "pixel sizes stay in range at any resolution: {f:?}");
        }
        // Comic Book and Watercolor smooth first with a Kuwahara pass of their paint size.
        assert!(matches!(comic.style_smoothing(), Some(Filter::AnisotropicKuwahara { radius, .. }) if radius == 5.0));
        assert!(matches!(wash.scaled(2.0).style_smoothing(), Some(Filter::AnisotropicKuwahara { radius, .. }) if radius == 10.0));
        assert_eq!(oil.style_smoothing(), None);
        let Filter::ComicBook { tones, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity, .. } = comic else { unreachable!() };
        let unpainted = Filter::ComicBook { tones, paint: 0.2, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity };
        assert_eq!(unpainted.style_smoothing(), None);
        // One tone is no shading: rejected. Border width and stroke size have a floor.
        assert!(!Filter::ComicBook { tones: 1, paint: 0.2, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity }.valid());
        assert!(!Filter::ComicBook { tones, paint: 0.2, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width: 0.0, input_contrast, input_luminosity }.valid());
        let Filter::OilPaint { effect_intensity, stroke_length, size_random, color_random, bristles, angle, seed, .. } = oil else { unreachable!() };
        assert!(!Filter::OilPaint { effect_intensity, stroke_size: 1.0, stroke_length, size_random, color_random, bristles, angle, seed }.valid());
        // The oil node reads as far as its longest stroke can reach; none at intensity 0.
        assert!(oil.support() as f32 >= 10.0 * 1.15 * 3.0 * 0.5 + 7.5);
        assert_eq!(Filter::OilPaint { effect_intensity: 0.0, stroke_size: 10.0, stroke_length, size_random, color_random, bristles, angle, seed }.support(), 0);
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
        let slope = Filter::BlurSlope { intensity: 8.2, scale: 4, seed: 9, samples: 8 };
        assert!(slope.valid() && slope.softens_coverage() && slope.support() == 9 && slope.label() == "Blur Slope" && slope.supports_mask());
        assert!(Filter::BlurSlope { intensity: 0.0, scale: 4, seed: 9, samples: 8 }.spatial_identity());
        assert!(!Filter::BlurSlope { intensity: 8.0, scale: 4, seed: 9, samples: 1 }.valid());
        assert!(!Filter::BlurSlope { intensity: 8.0, scale: 0, seed: 9, samples: 8 }.valid());
        assert!(!Filter::BlurSlope { intensity: 33.0, scale: 4, seed: 9, samples: 8 }.valid());
        assert!(Filter::ALL.iter().any(|f| matches!(f, Filter::BlurSlope { .. })));
        assert!(!Filter::Warp { intensity: 8.0, scale: 0, seed: 0 }.valid());
        assert!(!Filter::Warp { intensity: 8.0, scale: 65, seed: 0 }.valid());
        assert!(!Filter::Warp { intensity: 33.0, scale: 4, seed: 0 }.valid());
        assert!(Filter::Warp { intensity: 0.0, scale: 4, seed: 9 }.spatial_identity());
        assert_eq!(Filter::Warp { intensity: 8.2, scale: 4, seed: 9 }.support(), 9);
        assert!(!Filter::Median { radius: 16.5 }.valid());
        let kuwahara = Filter::ALL.iter().copied().find(|f| matches!(f, Filter::AnisotropicKuwahara { .. })).unwrap();
        assert!(kuwahara.valid() && kuwahara.is_pixelwise() && kuwahara.supports_mask());
        assert_eq!(kuwahara.label(), "Anisotropic Kuwahara");
        // Default radius 6, anisotropy 1, tensor 2: 6 * 2 + 2 + 1 = 15 px of margin.
        assert_eq!(kuwahara.support(), 15);
        let Filter::AnisotropicKuwahara { radius, tensor_smoothness, .. } = kuwahara.scaled(2.0) else { unreachable!() };
        let quantize = Filter::ALL.iter().copied().find(|f| matches!(f, Filter::Quantize { .. })).unwrap();
        assert!(quantize.valid() && quantize.is_pixelwise() && quantize.reads_whole_texture() && quantize.support() == FULL_SUPPORT);
        assert!(!quantize.supports_mask() && quantize.supports_channel(ChannelKind::Roughness) && quantize.supports_channel(ChannelKind::BaseColor));
        assert_eq!(quantize.label(), "Quantize");
        let Filter::Quantize { pattern, .. } = quantize else { unreachable!() };
        let with = |colors: u32, contour_smoothing: f32, dithering: f32| Filter::Quantize { colors, contour_smoothing, dithering, pattern, lab: true, apply_to_alpha: false, alpha_threshold: 0.5 };
        assert!(!with(1, 0.0, 0.0).valid() && !with(65, 0.0, 0.0).valid() && !with(8, 8.5, 0.0).valid() && !with(8, 0.0, 1.5).valid());
        assert_eq!(with(8, 2.0, 0.5).scaled(2.0), with(8, 4.0, 0.5), "smoothing follows the resolution");
        assert_eq!(DitherPattern::ALL.len(), 3);
        assert_eq!((radius, tensor_smoothness), (12.0, 4.0), "pixel sizes follow the resolution");
        assert!(!Filter::AnisotropicKuwahara { radius: 33.0, smoothness: 0.5, sharpness: 8.0, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true }.valid());
        assert!(Filter::AnisotropicKuwahara { radius: 0.4, smoothness: 0.5, sharpness: 8.0, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true }.spatial_identity());
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
        // Grayscale's conversion options swap to the variant that stores them and back.
        let options = GrayParts { mode: crate::mask::GrayscaleType::Max, invert: true, balance: 0.6, contrast: 0.2, weights: None };
        let converted = Filter::grayscale(options);
        assert!(matches!(converted, Filter::GrayscaleConvert { .. }) && converted.label() == "Grayscale" && converted.valid());
        assert_eq!(converted.grayscale_parts(), Some(options));
        assert_eq!(Filter::grayscale(GrayParts::default()), Filter::Grayscale);
        assert_eq!(Filter::Grayscale.grayscale_parts(), Some(GrayParts::default()));
        assert!(!Filter::grayscale(GrayParts { balance: 1.5, ..options }).valid());
        for (f, color_only) in [(Filter::Grayscale, true), (converted, true), (Filter::ALL[19], true), (Filter::ALL[20], true)] {
            assert!(f.supports_channel(ChannelKind::BaseColor));
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

#[cfg(test)]
mod grayscale_weight_tests {
    use super::*;

    #[test]
    fn custom_weights_are_optional_in_files() {
        let plain = Filter::GrayscaleConvert { mode: crate::mask::GrayscaleType::Max, invert: true, balance: 0.5, contrast: 0.0, weights: None };
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("weights"), "{json}");
        assert_eq!(serde_json::from_str::<Filter>(&json).unwrap(), plain, "files without weights read them as unset");
        let custom = Filter::GrayscaleConvert { mode: crate::mask::GrayscaleType::Max, invert: true, balance: 0.5, contrast: 0.0, weights: Some([0.1, 0.2, 0.7]) };
        assert_eq!(serde_json::from_str::<Filter>(&serde_json::to_string(&custom).unwrap()).unwrap(), custom);
        assert_eq!(GrayParts { weights: Some([1.0, 0.0, 0.0]), ..Default::default() }.gray([0.8, 0.4, 0.2]), 0.8);
    }
}
