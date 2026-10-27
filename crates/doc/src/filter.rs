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
}

impl Filter {
    pub const ALL: [Self; 13] = [Self::Contrast { gain: 1.0, pivot: 0.5 },
        Self::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 },
        Self::Threshold { cutoff: 0.5 }, Self::Clamp { low: 0.0, high: 1.0 }, Self::Blur { radius: 4.0 }, Self::Sharpen { radius: 2.0, amount: 1.0 }, Self::DirectionalBlur { radius: 8.0, angle_deg: 0.0 }, Self::Highpass { radius: 4.0, amount: 1.0 },
        Self::Bevel {radius:8.0,amount:1.0,threshold:0.5}, Self::MaskOutline {radius:4.0,threshold:0.5,position:OutlinePosition::Both},
        Self::GradientMap { gradient: Gradient::black_white(), to_gray: false },
        Self::HistogramScan { position: 0.5, contrast: 0.5, invert: false },
        Self::HistogramRange { position: 0.5, range: 0.2, softness: 0.5, invert: false }];

    pub fn label(self) -> &'static str {
        match self { Self::Contrast { .. } => "Contrast", Self::Hsl { .. } => "HSL",
            Self::Threshold { .. } => "Threshold", Self::Clamp { .. } => "Clamp", Self::Blur { .. } => "Blur", Self::Sharpen { .. } => "Sharpen", Self::DirectionalBlur { .. } => "Directional Blur", Self::Highpass { .. } => "Highpass", Self::Bevel {..} => "Bevel", Self::MaskOutline {..} => "Mask Outline", Self::GradientMap {..} => "Gradient Map", Self::HistogramScan {..} => "Histogram Scan", Self::HistogramRange {..} => "Histogram Range" }
    }

    pub fn supports_channel(self, channel: ChannelKind) -> bool {
        channel != ChannelKind::Normal && (!matches!(self, Self::Hsl { .. }) || channel.is_color())
    }

    pub fn supports_mask(self) -> bool { !matches!(self, Self::Hsl { .. }) }

    pub fn radius(self) -> Option<f32> {
        match self { Self::Blur { radius } | Self::Sharpen { radius, .. } | Self::DirectionalBlur { radius, .. } | Self::Highpass { radius, .. } | Self::Bevel {radius,..} | Self::MaskOutline {radius,..} => Some(radius), _ => None }
    }

    pub fn is_contour(self) -> bool { matches!(self, Self::Bevel {..} | Self::MaskOutline {..}) }

    /// Contours need a half-pixel antialias band beyond their nominal radius.
    pub fn support(self) -> u32 {
        (self.radius().unwrap_or(0.0) + if self.is_contour() {0.5} else {0.0}).ceil() as u32
    }

    pub fn softens_coverage(self) -> bool { matches!(self, Self::Blur { .. } | Self::DirectionalBlur { .. }) }

    /// Highpass is deliberately excluded: radius/amount zero gives its neutral value.
    pub fn spatial_identity(self) -> bool {
        matches!(self, Self::Blur {radius:0.0} | Self::DirectionalBlur {radius:0.0,..}
            | Self::Sharpen {radius:0.0,..} | Self::Sharpen {amount:0.0,..})
    }

    pub fn valid(self) -> bool {
        match self {
            Self::Contrast { gain, pivot } => gain.is_finite() && (0.0..=4.0).contains(&gain) && pivot.is_finite(),
            Self::Hsl { hue, saturation, lightness } => hue.is_finite() && (-180.0..=180.0).contains(&hue)
                && saturation.is_finite() && (0.0..=2.0).contains(&saturation)
                && lightness.is_finite() && (-1.0..=1.0).contains(&lightness),
            Self::Blur { radius } => radius.is_finite() && (0.0..=32.0).contains(&radius),
            Self::DirectionalBlur { radius, angle_deg } => Self::Blur {radius}.valid() && angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg),
            Self::Sharpen { radius, amount } | Self::Highpass { radius, amount } => Self::Blur { radius }.valid() && amount.is_finite() && (0.0..=4.0).contains(&amount),
            Self::Bevel {radius,amount,threshold} => Self::Blur {radius}.valid() && amount.is_finite() && (-1.0..=1.0).contains(&amount) && threshold.is_finite() && (-1.0..=1.0).contains(&threshold),
            Self::MaskOutline {radius,threshold,..} => Self::Blur {radius}.valid() && threshold.is_finite() && (-1.0..=1.0).contains(&threshold),
            Self::GradientMap { gradient, .. } => gradient.valid(),
            Self::HistogramScan { position, contrast, .. } => position.is_finite() && (-1.0..=1.0).contains(&position) && contrast.is_finite() && (0.0..=1.0).contains(&contrast),
            Self::HistogramRange { position, range, softness, .. } => position.is_finite() && (-1.0..=1.0).contains(&position)
                && range.is_finite() && (0.0..=2.0).contains(&range) && softness.is_finite() && (0.0..=1.0).contains(&softness),
            Self::Threshold { cutoff } => cutoff.is_finite(),
            Self::Clamp { low, high } => low.is_finite() && high.is_finite() && low <= high,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_parameters_and_domains_are_checked() {
        assert!(!Filter::Bevel {radius:2.0,amount:1.01,threshold:0.5}.valid());
        assert!(!Filter::MaskOutline {radius:2.0,threshold:f32::NAN,position:OutlinePosition::Both}.valid());
        for f in Filter::ALL { assert!(f.valid()); assert!(!f.supports_channel(ChannelKind::Normal)); }
        assert!(!Filter::DirectionalBlur {radius:2.0,angle_deg:181.0}.valid());
        assert!(!Filter::Highpass {radius:2.0,amount:f32::INFINITY}.valid());
        assert!(!Filter::Highpass {radius:0.0,amount:0.0}.spatial_identity());
        assert!(Filter::DirectionalBlur {radius:0.0,angle_deg:45.0}.spatial_identity());
        assert!(!Filter::Blur {radius:32.01}.valid());
        assert!(!Filter::Sharpen {radius:2.0,amount:f32::NAN}.valid());
        assert!(!Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 }.supports_mask());
        assert!(!Filter::Clamp { low: 0.8, high: 0.2 }.valid());
        assert!(!Filter::Threshold { cutoff: f32::NAN }.valid());
        assert!(!Filter::HistogramScan { position: 1.5, contrast: 0.5, invert: false }.valid());
        assert!(!Filter::HistogramScan { position: 0.5, contrast: f32::NAN, invert: false }.valid());
        assert!(!Filter::HistogramRange { position: 0.5, range: -0.1, softness: 0.0, invert: true }.valid());
        assert!(!Filter::HistogramRange { position: 0.5, range: 0.5, softness: 1.5, invert: true }.valid());
        assert!(!Filter::Contrast { gain: f32::INFINITY, pivot: 0.5 }.valid());
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
