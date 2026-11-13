//! Fill sources and projections (shared by fill layers, mask fills and brush materials).

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelValue, MeshMapKind, ResourceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionMode {
    /// Use mesh UVs.
    #[default]
    Uv,
    /// Blend three world-axis planar projections by surface normal.
    Triplanar,
    /// Planar projection along an object axis.
    Planar,
    /// Wrapped around a sphere at the centre of the model's bounding cube; `planar_axis` is the
    /// pole (u = longitude, v = latitude from the pole).
    Spherical,
    /// Wrapped around a cylinder along `planar_axis` through the centre of the bounding cube
    /// (u = angle around it over [`Projection::cylinder_angle`], v = position along it).
    Cylindrical,
}

/// Symmetry of a 3D projection: the model position is folded before it is projected, so the
/// projected texture repeats mirrored or around an axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FillSymmetry {
    #[default]
    None,
    /// The side above `position` (bounding-cube coordinate, 0.5 = the middle) along `axis` is
    /// mirrored onto the other.
    Mirror { axis: u8, position: f32 },
    /// `count` copies around `axis` through `center`; `flip` mirrors every other copy.
    Radial { axis: u8, count: u32, flip: bool, center: [f32; 3] },
}

impl FillSymmetry {
    pub const MAX_COPIES: u32 = 64;

    pub fn is_none(&self) -> bool {
        *self == Self::None
    }

    pub fn mirror() -> Self {
        Self::Mirror { axis: 0, position: 0.5 }
    }

    pub fn radial() -> Self {
        Self::Radial { axis: 1, count: 4, flip: false, center: [0.5; 3] }
    }

    pub fn valid(&self) -> bool {
        match *self {
            Self::None => true,
            Self::Mirror { axis, position } => axis < 3 && position.is_finite(),
            Self::Radial { axis, count, center, .. } => axis < 3 && (1..=Self::MAX_COPIES).contains(&count) && center.iter().all(|c| c.is_finite()),
        }
    }

    /// The position and normal the projection reads for a texel at `pos` with `normal`.
    pub fn fold(&self, pos: [f32; 3], normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        match *self {
            Self::None => (pos, normal),
            Self::Mirror { axis, position } => {
                let a = (axis as usize).min(2);
                let (mut p, mut n) = (pos, normal);
                if p[a] < position {
                    p[a] = 2.0 * position - p[a];
                    n[a] = -n[a];
                }
                (p, n)
            }
            Self::Radial { axis, count, flip, center } => {
                let a = (axis as usize).min(2);
                let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                let sector = std::f32::consts::TAU / count.clamp(1, Self::MAX_COPIES) as f32;
                let (x, y) = (pos[b] - center[b], pos[c] - center[c]);
                let theta = y.atan2(x).rem_euclid(std::f32::consts::TAU);
                let k = (theta / sector).floor();
                let local = theta - k * sector;
                let mirrored = flip && (k as i64) % 2 == 1;
                let target = if mirrored { sector - local } else { local };
                // Rotate about the axis by `target - theta`; a flipped copy is also mirrored,
                // which for the angle is the reflection `theta -> target` (same radius).
                let r = (x * x + y * y).sqrt();
                let mut p = pos;
                p[b] = center[b] + r * target.cos();
                p[c] = center[c] + r * target.sin();
                let (s, co) = (target - theta).sin_cos();
                let mut n = normal;
                n[b] = normal[b] * co - normal[c] * s;
                n[c] = normal[b] * s + normal[c] * co;
                (p, n)
            }
        }
    }
}

/// Texture reconstruction of a projected bitmap (Painter's Filtering dropdown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillFiltering {
    /// Bilinear with mipmaps: a minified texture is averaged instead of aliasing.
    #[default]
    BilinearHq,
    /// Bilinear on the full-size image only (no mipmaps).
    BilinearSharp,
    /// Nearest texel, no interpolation.
    Nearest,
}

impl FillFiltering {
    pub const ALL: [FillFiltering; 3] = [FillFiltering::BilinearHq, FillFiltering::BilinearSharp, FillFiltering::Nearest];

    pub fn label(self) -> &'static str {
        match self {
            FillFiltering::BilinearHq => "Bilinear | HQ",
            FillFiltering::BilinearSharp => "Bilinear | Sharp",
            FillFiltering::Nearest => "Nearest",
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a projected texture does outside its 0..1 square (Painter's UV Wrap dropdown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UvWrap {
    /// Drawn once; transparent everywhere else.
    None,
    /// Repeats along U only.
    RepeatHorizontal,
    /// Repeats along V only.
    RepeatVertical,
    #[default]
    Repeat,
}

impl UvWrap {
    pub const ALL: [UvWrap; 4] = [UvWrap::None, UvWrap::RepeatHorizontal, UvWrap::RepeatVertical, UvWrap::Repeat];

    pub fn label(self) -> &'static str {
        match self {
            UvWrap::None => "None",
            UvWrap::RepeatHorizontal => "Repeat horizontally",
            UvWrap::RepeatVertical => "Repeat vertically",
            UvWrap::Repeat => "Repeat",
        }
    }

    /// Whether the texture repeats along `[U, V]`.
    #[inline]
    pub fn repeats(self) -> [bool; 2] {
        match self {
            UvWrap::None => [false, false],
            UvWrap::RepeatHorizontal => [true, false],
            UvWrap::RepeatVertical => [false, true],
            UvWrap::Repeat => [true, true],
        }
    }

    /// Whether the texture exists at the (transformed) coordinate `p`: an axis that does not
    /// repeat only covers 0..=1.
    #[inline]
    pub fn covers(self, p: [f32; 2]) -> bool {
        let r = self.repeats();
        (r[0] || (0.0..=1.0).contains(&p[0])) && (r[1] || (0.0..=1.0).contains(&p[1]))
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Whether a 3D projection is limited to its projection box (Painter's Shape crop dropdown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShapeCrop {
    /// Nothing is projected outside the projection box.
    Crop,
    #[default]
    Extend,
}

impl ShapeCrop {
    pub const ALL: [ShapeCrop; 2] = [ShapeCrop::Crop, ShapeCrop::Extend];

    pub fn label(self) -> &'static str {
        match self {
            ShapeCrop::Crop => "Project cropped to shape",
            ShapeCrop::Extend => "Projection extends outside shape",
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub mode: ProjectionMode,
    /// Tiling (UV mode) or world units per tile inverse (triplanar/planar).
    pub scale: [f32; 2],
    pub rotation_deg: f32,
    pub offset: [f32; 2],
    /// Triplanar blend exponent (1 = soft; [`TRIPLANAR_HARD`] and above = hard cut). The panel
    /// edits it as Painter's Hardness, see [`Projection::hardness`].
    pub triplanar_sharpness: f32,
    /// Planar projection axis (0 = X, 1 = Y, 2 = Z).
    pub planar_axis: u8,
    /// Bitmap reconstruction. Not written at its default, so older projects serialize unchanged.
    #[serde(default, skip_serializing_if = "FillFiltering::is_default")]
    pub filtering: FillFiltering,
    /// Repetition outside 0..1 (UV and planar; tri-planar always repeats). Not written at its default.
    #[serde(default, skip_serializing_if = "UvWrap::is_default")]
    pub uv_wrap: UvWrap,
    /// Limit a tri-planar / planar projection to its box. Not written at its default.
    #[serde(default, skip_serializing_if = "ShapeCrop::is_default")]
    pub shape_crop: ShapeCrop,
    /// Cylindrical: degrees of the perimeter the texture spans once (360 = all the way round).
    #[serde(default = "full_turn", skip_serializing_if = "is_full_turn")]
    pub cylinder_angle: f32,
    /// Mirror / radial symmetry of a 3D projection. Not written when off.
    #[serde(default, skip_serializing_if = "FillSymmetry::is_none")]
    pub symmetry: FillSymmetry,
}

fn full_turn() -> f32 {
    360.0
}
fn is_full_turn(v: &f32) -> bool {
    *v == 360.0
}

/// Triplanar exponent stored for Hardness 1.0: the three planes are cut, not blended.
pub const TRIPLANAR_HARD: f32 = 1.0e6;

impl Default for Projection {
    fn default() -> Self {
        Self {
            mode: ProjectionMode::Uv,
            scale: [1.0, 1.0],
            rotation_deg: 0.0,
            offset: [0.0, 0.0],
            triplanar_sharpness: 8.0,
            planar_axis: 2,
            filtering: FillFiltering::default(),
            uv_wrap: UvWrap::default(),
            shape_crop: ShapeCrop::default(),
            cylinder_angle: 360.0,
            symmetry: FillSymmetry::None,
        }
    }
}

impl Projection {
    /// Tri-planar Hardness 0..=1 as Painter shows it: 0 blends the three planes over the widest
    /// band, 1 is a hard cut. It is the stored exponent `s` seen as `1 - 1 / s`.
    pub fn hardness(&self) -> f32 {
        let s = self.triplanar_sharpness;
        if s >= TRIPLANAR_HARD { 1.0 } else { (1.0 - 1.0 / s.max(1.0)).clamp(0.0, 1.0) }
    }

    /// Set the tri-planar blend from a Hardness 0..=1 (inverse of [`Projection::hardness`]).
    pub fn set_hardness(&mut self, hardness: f32) {
        let h = hardness.clamp(0.0, 1.0);
        let s = 1.0 / (1.0 - h);
        self.triplanar_sharpness = if s >= TRIPLANAR_HARD { TRIPLANAR_HARD } else { s };
    }

    /// Whether the projection reads the model position (everything but UV).
    #[inline]
    pub fn is_3d(&self) -> bool {
        self.mode != ProjectionMode::Uv
    }

    /// Values a projection computes with are numbers and the symmetry is in range: a NaN angle
    /// or an infinite mirror position made every texel of the fill NaN, and a huge radial
    /// count multiplied the work per texel.
    pub fn valid(&self) -> bool {
        let floats = [self.scale[0], self.scale[1], self.rotation_deg, self.offset[0], self.offset[1], self.triplanar_sharpness, self.cylinder_angle];
        floats.iter().all(|v| v.is_finite()) && self.symmetry.valid()
    }

    /// The symmetry in effect: 3D projections only.
    #[inline]
    pub fn effective_symmetry(&self) -> FillSymmetry {
        if self.is_3d() { self.symmetry } else { FillSymmetry::None }
    }

    /// Unprojected 2D coordinates of a bounding-cube position for the spherical and cylindrical
    /// modes: u around `planar_axis` (0.5 where the next axis points), v from the pole (sphere)
    /// or along the axis (cylinder, top = 0).
    pub fn wrapped_coords(&self, pos: [f32; 3]) -> [f32; 2] {
        let a = (self.planar_axis as usize).min(2);
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        let d = [pos[0] - 0.5, pos[1] - 0.5, pos[2] - 0.5];
        let theta = d[c].atan2(d[b]);
        if self.mode == ProjectionMode::Spherical {
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let v = if len <= 1e-12 { 0.5 } else { (d[a] / len).clamp(-1.0, 1.0).acos() / std::f32::consts::PI };
            [theta / std::f32::consts::TAU + 0.5, v]
        } else {
            let span = self.cylinder_angle.clamp(1.0, 360.0).to_radians();
            [theta / span + 0.5, 1.0 - pos[a]]
        }
    }

    /// The UV wrap in effect: tri-planar has no UV Wrap setting and always repeats.
    #[inline]
    pub fn effective_wrap(&self) -> UvWrap {
        if self.mode == ProjectionMode::Triplanar { UvWrap::Repeat } else { self.uv_wrap }
    }

    /// Whether Shape crop applies (3D projections only).
    #[inline]
    pub fn crops_to_shape(&self) -> bool {
        self.shape_crop == ShapeCrop::Crop && self.mode != ProjectionMode::Uv
    }

    /// Whether some texels can fall outside the projected texture (UV wrap or shape crop).
    #[inline]
    pub fn clips(&self) -> bool {
        self.effective_wrap() != UvWrap::Repeat || self.crops_to_shape()
    }

    /// Apply scale/rotation/offset to a 2D coordinate.
    pub fn transform_2d(&self, p: [f32; 2]) -> [f32; 2] {
        self.transform_2d_sc(self.rotation_sin_cos(), p)
    }

    /// `(sin, cos)` of the rotation, for [`Projection::transform_2d_sc`] in per-texel loops.
    #[inline]
    pub fn rotation_sin_cos(&self) -> (f32, f32) {
        self.rotation_deg.to_radians().sin_cos()
    }

    /// [`Projection::transform_2d`] with the rotation's `(sin, cos)` computed once by the caller.
    #[inline]
    pub fn transform_2d_sc(&self, (s, c): (f32, f32), p: [f32; 2]) -> [f32; 2] {
        let x = p[0] * self.scale[0];
        let y = p[1] * self.scale[1];
        [x * c - y * s + self.offset[0], x * s + y * c + self.offset[1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projections_with_non_numbers_or_out_of_range_symmetry_are_invalid() {
        let p = Projection::default();
        assert!(p.valid());
        assert!(!Projection { cylinder_angle: f32::NAN, ..p }.valid());
        assert!(!Projection { offset: [0.0, f32::INFINITY], ..p }.valid());
        assert!(!Projection { symmetry: FillSymmetry::Mirror { axis: 0, position: f32::INFINITY }, ..p }.valid());
        assert!(!Projection { symmetry: FillSymmetry::Radial { axis: 1, count: FillSymmetry::MAX_COPIES + 1, flip: false, center: [0.5; 3] }, ..p }.valid());
        assert!(Projection { symmetry: FillSymmetry::radial(), cylinder_angle: 90.0, ..p }.valid());
    }

    #[test]
    fn spherical_and_cylindrical_coordinates_and_symmetry_folds() {
        let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5;
        // Sphere with the Y pole: the equator point on +Z is u 0.5, v 0.5; the pole is v 0.
        let s = Projection { mode: ProjectionMode::Spherical, planar_axis: 1, ..Projection::default() };
        assert!(near(s.wrapped_coords([0.5, 0.5, 1.0]), [0.5, 0.5]));
        assert!((s.wrapped_coords([0.5, 1.0, 0.5])[1]).abs() < 1e-5);
        assert!((s.wrapped_coords([0.5, 0.0, 0.5])[1] - 1.0).abs() < 1e-5);
        // A quarter turn toward +X is a quarter of the longitude.
        assert!(near(s.wrapped_coords([1.0, 0.5, 0.5]), [0.75, 0.5]));
        // Cylinder along Y: v runs down from the top; Angle 180 spans half the perimeter per tile.
        let c = Projection { mode: ProjectionMode::Cylindrical, planar_axis: 1, ..Projection::default() };
        assert!(near(c.wrapped_coords([1.0, 0.25, 0.5]), [0.75, 0.75]));
        let half = Projection { cylinder_angle: 180.0, ..c };
        assert!(near(half.wrapped_coords([1.0, 0.25, 0.5]), [1.0, 0.75]));
        // Mirror on X at the middle: the low side reads the high side, its normal flipped.
        let m = FillSymmetry::mirror();
        let (p, n) = m.fold([0.2, 0.3, 0.4], [1.0, 0.0, 0.0]);
        assert!((p[0] - 0.8).abs() < 1e-6 && p[1] == 0.3 && p[2] == 0.4 && n == [-1.0, 0.0, 0.0], "{p:?} {n:?}");
        assert_eq!(m.fold([0.8, 0.3, 0.4], [1.0, 0.0, 0.0]), ([0.8, 0.3, 0.4], [1.0, 0.0, 0.0]));
        // Radial, 4 copies about Y: the same spot in every quarter reads the first one.
        let r = FillSymmetry::radial();
        let at = |angle: f32| [0.5 + 0.4 * angle.sin(), 0.1, 0.5 + 0.4 * angle.cos()];
        let quarter = std::f32::consts::FRAC_PI_2;
        let first = at(0.2);
        for k in 0..4 {
            let p = r.fold(at(0.2 + k as f32 * quarter), [0.0, 0.0, 1.0]).0;
            assert!((0..3).all(|i| (p[i] - first[i]).abs() < 1e-5), "quarter {k}: {p:?}");
        }
        // Flip mirrors every other copy: a point just inside the second sector reads the far
        // end of the first.
        let f = FillSymmetry::Radial { axis: 1, count: 4, flip: true, center: [0.5; 3] };
        let t = 0.1f32;
        let (p, q) = (f.fold(at(quarter + t), [0.0; 3]).0, at(quarter - t));
        assert!((0..3).all(|i| (p[i] - q[i]).abs() < 1e-5), "{p:?} {q:?}");
        // Symmetry does nothing on a UV projection; defaults are not written.
        let uv = Projection { symmetry: m, ..Projection::default() };
        assert!(uv.effective_symmetry().is_none() && Projection { mode: ProjectionMode::Planar, ..uv }.effective_symmetry() == m);
        let json = serde_json::to_string(&Projection::default()).unwrap();
        assert!(!json.contains("symmetry") && !json.contains("cylinder_angle"), "{json}");
        let back: Projection = serde_json::from_str(&serde_json::to_string(&Projection { symmetry: f, cylinder_angle: 90.0, ..s }).unwrap()).unwrap();
        assert_eq!((back.symmetry, back.cylinder_angle, back.mode), (f, 90.0, ProjectionMode::Spherical));
        assert!(!FillSymmetry::Radial { axis: 1, count: 0, flip: false, center: [0.5; 3] }.valid());
    }

    #[test]
    fn transform_scales_x_and_y_separately_then_offsets() {
        let p = Projection { scale: [2.0, 3.0], offset: [0.25, -0.5], ..Projection::default() };
        assert_eq!(p.transform_2d([1.0, 1.0]), [2.25, 2.5]);
        let r = Projection { rotation_deg: 90.0, ..Projection::default() };
        let q = r.transform_2d([1.0, 0.0]);
        assert!(q[0].abs() < 1e-6 && (q[1] - 1.0).abs() < 1e-6, "{q:?}");
    }

    #[test]
    fn hardness_maps_to_the_stored_exponent_and_back() {
        let mut p = Projection::default();
        assert_eq!(p.hardness(), 0.875, "the default exponent 8");
        for (h, s) in [(0.0, 1.0), (0.5, 2.0), (0.75, 4.0), (0.875, 8.0)] {
            p.set_hardness(h);
            assert_eq!(p.triplanar_sharpness, s);
            assert_eq!(p.hardness(), h);
        }
        p.set_hardness(1.0);
        assert_eq!(p.triplanar_sharpness, TRIPLANAR_HARD);
        assert_eq!(p.hardness(), 1.0);
        p.set_hardness(7.0);
        assert_eq!(p.hardness(), 1.0, "clamped");
        p.triplanar_sharpness = 32.0; // the old slider's maximum
        assert!((p.hardness() - 0.96875).abs() < 1e-6);
    }

    #[test]
    fn uv_wrap_covers_only_the_unit_square_on_axes_that_do_not_repeat() {
        let (inside, right, below) = ([0.5, 0.5], [1.5, 0.5], [0.5, -0.25]);
        assert!(UvWrap::ALL.iter().all(|w| w.covers(inside)));
        assert_eq!(UvWrap::ALL.map(|w| w.covers(right)), [false, true, false, true]);
        assert_eq!(UvWrap::ALL.map(|w| w.covers(below)), [false, false, true, true]);
        assert!(UvWrap::None.covers([0.0, 1.0]), "the border belongs to the texture");
    }

    #[test]
    fn projection_options_default_to_painters_and_stay_out_of_old_files() {
        let p = Projection::default();
        assert_eq!((p.filtering, p.uv_wrap, p.shape_crop), (FillFiltering::BilinearHq, UvWrap::Repeat, ShapeCrop::Extend));
        assert!(!p.clips());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("filtering") && !json.contains("uv_wrap") && !json.contains("shape_crop"), "{json}");
        let old: Projection = serde_json::from_str(r#"{"mode":"triplanar","scale":[1.0,1.0],"rotation_deg":0.0,"offset":[0.0,0.0],"triplanar_sharpness":8.0,"planar_axis":2}"#).unwrap();
        assert_eq!(old, Projection { mode: ProjectionMode::Triplanar, ..Projection::default() });
        let set = Projection { filtering: FillFiltering::Nearest, uv_wrap: UvWrap::RepeatHorizontal, shape_crop: ShapeCrop::Crop, ..p };
        let json = serde_json::to_string(&set).unwrap();
        assert!(json.ends_with(r#""filtering":"nearest","uv_wrap":"repeat_horizontal","shape_crop":"crop"}"#), "{json}");
        assert_eq!(serde_json::from_str::<Projection>(&json).unwrap(), set);
        // Tri-planar has no UV Wrap; UV projection has no shape.
        assert!(!Projection { mode: ProjectionMode::Triplanar, uv_wrap: UvWrap::None, ..p }.clips());
        assert!(!Projection { shape_crop: ShapeCrop::Crop, ..p }.clips());
        assert!(Projection { mode: ProjectionMode::Planar, shape_crop: ShapeCrop::Crop, ..p }.clips());
    }
}

/// Procedural pattern generators (original implementations in `subcraft-materials`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralKind {
    /// Fractal value/gradient noise.
    Clouds,
    /// Cellular (Worley) distance pattern.
    Cells,
    /// Sharp thresholded noise splotches.
    Grunge,
    /// Thin directional scratches.
    Scratches,
    /// Concentric rings with noise distortion.
    WoodRings,
    /// Anisotropic stretched noise.
    Brushed,
    Checker,
    /// Linear gradient along U.
    GradientU,
    /// Linear gradient along V.
    GradientV,
    /// Blotchy grime with fine specks and soft smears.
    Dirt,
    /// Thin cracks along cell borders (1 on a crack).
    Cracks,
    /// Vertical streaks, like rain or drips running down a surface.
    Streaks,
    /// Fine scattered dots.
    Speckles,
    /// Small pits: one dot per cell.
    Pores,
    /// Scratch generator: long hairlines and gouges with direction, length, width, depth,
    /// curvature, density, clustering and curvature-driven wear (`ProceduralParams::scratch`).
    Scratch,
}

impl ProceduralKind {
    pub const ALL: [ProceduralKind; 15] = [
        ProceduralKind::Clouds,
        ProceduralKind::Cells,
        ProceduralKind::Grunge,
        ProceduralKind::Scratches,
        ProceduralKind::WoodRings,
        ProceduralKind::Brushed,
        ProceduralKind::Checker,
        ProceduralKind::GradientU,
        ProceduralKind::GradientV,
        ProceduralKind::Dirt,
        ProceduralKind::Cracks,
        ProceduralKind::Streaks,
        ProceduralKind::Speckles,
        ProceduralKind::Pores,
        ProceduralKind::Scratch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProceduralKind::Clouds => "Clouds",
            ProceduralKind::Cells => "Cells",
            ProceduralKind::Grunge => "Grunge",
            ProceduralKind::Scratches => "Scratches",
            ProceduralKind::WoodRings => "Wood Rings",
            ProceduralKind::Brushed => "Brushed",
            ProceduralKind::Checker => "Checker",
            ProceduralKind::GradientU => "Gradient U",
            ProceduralKind::GradientV => "Gradient V",
            ProceduralKind::Dirt => "Dirt",
            ProceduralKind::Cracks => "Cracks",
            ProceduralKind::Streaks => "Streaks",
            ProceduralKind::Speckles => "Speckles",
            ProceduralKind::Pores => "Pores",
            ProceduralKind::Scratch => "Scratch Generator",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProceduralParams {
    pub kind: ProceduralKind,
    /// Feature frequency multiplier.
    pub scale: f32,
    pub seed: u32,
    /// Remap: output = smoothstep(balance - contrast, balance + contrast, raw).
    pub balance: f32,
    pub contrast: f32,
    pub invert: bool,
    /// Each color component is its own pattern (seeds `seed`, `seed + 1`, `seed + 2`), mapped
    /// `low`..`high` per component: colored noise. Not written when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub colored: bool,
    /// Settings of [`ProceduralKind::Scratch`]; ignored by every other kind and not written
    /// while at its defaults, so older projects serialize unchanged.
    #[serde(default, skip_serializing_if = "ScratchParams::is_default")]
    pub scratch: ScratchParams,
}

impl ProceduralParams {
    pub fn new(kind: ProceduralKind) -> Self {
        Self { kind, scale: 4.0, seed: 1, balance: 0.5, contrast: 0.0, invert: false, colored: false, scratch: ScratchParams::default() }
    }
}

/// Scratch generator settings. Lengths and widths are in lattice cells (`ProceduralParams::scale`
/// cells per tile), so the pattern is the same at every resolution and tiles when `scale` is
/// an integer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScratchParams {
    /// Mean direction, degrees (0 = along U, 90 = along V).
    pub angle_deg: f32,
    /// Random deviation from the mean direction, +- degrees (90 = any direction).
    pub spread_deg: f32,
    /// Mean length in cells.
    pub length: f32,
    /// Random length variation 0..1.
    pub length_var: f32,
    /// Mean half-width in cells.
    pub width: f32,
    /// Random width variation 0..1.
    pub width_var: f32,
    /// Probability that a candidate scratch exists (up to 6 per cell), 0..1.
    pub density: f32,
    /// Sideways bend 0..1.
    pub curvature: f32,
    /// 0 = full depth along the whole scratch, 1 = pointed ends and shallow/deep variation.
    pub depth_falloff: f32,
    /// 0 = even distribution, 1 = scratches gather in patches.
    pub cluster: f32,
    /// 0 = ignore the baked curvature map, 1 = scratches only on edges (needs a bake).
    pub wear: f32,
}

impl Default for ScratchParams {
    fn default() -> Self {
        Self { angle_deg: 0.0, spread_deg: 90.0, length: 1.0, length_var: 0.5, width: 0.012, width_var: 0.5, density: 0.35, curvature: 0.1, depth_falloff: 0.6, cluster: 0.0, wear: 0.0 }
    }
}

impl ScratchParams {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Named starting points for the scratch generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScratchPreset {
    FineHairlines,
    HeavyGouges,
    BrushedMetal,
    ScuffedPlastic,
    Cracked,
}

impl ScratchPreset {
    pub const ALL: [ScratchPreset; 5] =
        [ScratchPreset::FineHairlines, ScratchPreset::HeavyGouges, ScratchPreset::BrushedMetal, ScratchPreset::ScuffedPlastic, ScratchPreset::Cracked];

    pub fn label(self) -> &'static str {
        match self {
            ScratchPreset::FineHairlines => "Fine hairlines",
            ScratchPreset::HeavyGouges => "Heavy gouges",
            ScratchPreset::BrushedMetal => "Brushed metal",
            ScratchPreset::ScuffedPlastic => "Scuffed plastic",
            ScratchPreset::Cracked => "Cracked",
        }
    }

    /// Scratch settings and cell scale of the preset.
    pub fn params(self) -> (f32, ScratchParams) {
        let d = ScratchParams::default();
        match self {
            ScratchPreset::FineHairlines => (6.0, ScratchParams { spread_deg: 90.0, length: 1.6, length_var: 0.6, width: 0.004, width_var: 0.4, density: 0.5, curvature: 0.15, depth_falloff: 0.8, cluster: 0.4, ..d }),
            ScratchPreset::HeavyGouges => (3.0, ScratchParams { angle_deg: 35.0, spread_deg: 40.0, length: 1.2, length_var: 0.7, width: 0.04, width_var: 0.6, density: 0.12, curvature: 0.05, depth_falloff: 0.5, cluster: 0.3, ..d }),
            ScratchPreset::BrushedMetal => (10.0, ScratchParams { angle_deg: 0.0, spread_deg: 3.0, length: 2.0, length_var: 0.6, width: 0.005, width_var: 0.5, density: 0.9, curvature: 0.0, depth_falloff: 0.3, cluster: 0.0, ..d }),
            ScratchPreset::ScuffedPlastic => (5.0, ScratchParams { angle_deg: 20.0, spread_deg: 60.0, length: 0.6, length_var: 0.8, width: 0.02, width_var: 0.8, density: 0.45, curvature: 0.5, depth_falloff: 1.0, cluster: 0.8, ..d }),
            ScratchPreset::Cracked => (3.0, ScratchParams { spread_deg: 90.0, length: 1.5, length_var: 0.5, width: 0.01, width_var: 0.3, density: 0.3, curvature: 0.7, depth_falloff: 0.9, cluster: 0.2, ..d }),
        }
    }

    /// Set the scale and scratch settings of `params` (kind becomes Scratch; seed is kept).
    pub fn apply(self, params: &mut ProceduralParams) {
        let (scale, scratch) = self.params();
        params.kind = ProceduralKind::Scratch;
        params.scale = scale;
        params.scratch = scratch;
    }
}

/// Where a channel's value comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FillSource {
    Uniform { value: ChannelValue },
    /// Imported bitmap resource (sampled through the projection).
    Bitmap { resource: ResourceId },
    /// Procedural pattern; grayscale result maps `low`..`high`.
    Procedural { params: ProceduralParams, low: ChannelValue, high: ChannelValue },
    /// Baked mesh map value.
    MeshMap { map: MeshMapKind },
    /// A channel of an anchor point below this layer (ADR-0045), as `(value, alpha)`.
    Anchor { input: crate::anchor::AnchorInput },
}

impl FillSource {
    pub fn uniform(v: ChannelValue) -> Self {
        FillSource::Uniform { value: v }
    }
}
