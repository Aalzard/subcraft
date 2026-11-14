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
    /// Planar along `planar_axis` through a grid of movable points ([`Projection::warp`]);
    /// nothing is projected outside the grid.
    Warp,
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
                let mut n = normal;
                if mirrored {
                    // The same reflection for the normal (across the line at half of
                    // `theta + target`). Rotated instead, its tangential part had the wrong sign,
                    // so backface culling and tri-planar weights were inverted in flipped copies.
                    let (s, co) = (target + theta).sin_cos();
                    n[b] = normal[b] * co + normal[c] * s;
                    n[c] = normal[b] * s - normal[c] * co;
                } else {
                    let (s, co) = (target - theta).sin_cos();
                    n[b] = normal[b] * co - normal[c] * s;
                    n[c] = normal[b] * s + normal[c] * co;
                }
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

/// Placement of a 3D projection's box in the model's bounding cube (Painter's "3D projection"
/// group): the box is scaled about its centre, rotated (X, then Y, then Z, in degrees) and moved
/// by `offset` (bounding-cube units, 0 = centred).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionTransform {
    pub offset: [f32; 3],
    pub rotation_deg: [f32; 3],
    pub scale: [f32; 3],
}

impl Default for ProjectionTransform {
    fn default() -> Self {
        Self { offset: [0.0; 3], rotation_deg: [0.0; 3], scale: [1.0; 3] }
    }
}

impl ProjectionTransform {
    pub const MIN_SCALE: f32 = 0.01;
    pub const MAX_SCALE: f32 = 100.0;
    pub const MAX_OFFSET: f32 = 8.0;

    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    pub fn valid(&self) -> bool {
        self.offset.iter().all(|v| v.is_finite() && v.abs() <= Self::MAX_OFFSET)
            && self.rotation_deg.iter().all(|v| v.is_finite() && v.abs() <= 360.0)
            && self.scale.iter().all(|v| v.is_finite() && (Self::MIN_SCALE..=Self::MAX_SCALE).contains(v))
    }

    /// The rotation matrix `Rz * Ry * Rx` (rows).
    pub fn rotation(&self) -> [[f32; 3]; 3] {
        let (sx, cx) = self.rotation_deg[0].to_radians().sin_cos();
        let (sy, cy) = self.rotation_deg[1].to_radians().sin_cos();
        let (sz, cz) = self.rotation_deg[2].to_radians().sin_cos();
        [
            [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
            [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
            [-sy, cy * sx, cy * cx],
        ]
    }

    /// Euler angles in degrees (X, then Y, then Z) of a rotation matrix given as rows: the
    /// inverse of [`ProjectionTransform::rotation`]. At Y = +-90 degrees (where X and Z turn
    /// about the same axis) the whole turn is reported on X.
    pub fn euler_of(m: [[f32; 3]; 3]) -> [f32; 3] {
        let sy = (-m[2][0]).clamp(-1.0, 1.0);
        let y = sy.asin();
        let (x, z) = if sy.abs() > 0.99999 {
            ((m[0][1] * sy).atan2(m[1][1]), 0.0)
        } else {
            (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
        };
        [x.to_degrees(), y.to_degrees(), z.to_degrees()]
    }

    /// The rotation angles after turning the box by `deg` about its own axis `axis` (0 = X).
    pub fn rotated_about_local_axis(&self, axis: usize, deg: f32) -> [f32; 3] {
        let a = axis.min(2);
        let (b, d) = ((a + 1) % 3, (a + 2) % 3);
        let (s, c) = deg.to_radians().sin_cos();
        let mut local = [[0.0f32; 3]; 3];
        local[a][a] = 1.0;
        local[b][b] = c;
        local[b][d] = -s;
        local[d][b] = s;
        local[d][d] = c;
        let r = self.rotation();
        let mut m = [[0.0f32; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = r[i][0] * local[0][j] + r[i][1] * local[1][j] + r[i][2] * local[2][j];
            }
        }
        Self::euler_of(m)
    }

    /// A bounding-cube position and normal in the box's own frame (the box is the unit cube
    /// there): the inverse of scale, rotation and offset. The normal is renormalised.
    pub fn to_local(&self, pos: [f32; 3], normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        if self.is_identity() {
            return (pos, normal);
        }
        let r = self.rotation();
        let d = [pos[0] - 0.5 - self.offset[0], pos[1] - 0.5 - self.offset[1], pos[2] - 0.5 - self.offset[2]];
        let mut p = [0.0f32; 3];
        let mut n = [0.0f32; 3];
        for i in 0..3 {
            let scale = self.scale[i].clamp(Self::MIN_SCALE, Self::MAX_SCALE);
            p[i] = (r[0][i] * d[0] + r[1][i] * d[1] + r[2][i] * d[2]) / scale + 0.5;
            n[i] = (r[0][i] * normal[0] + r[1][i] * normal[1] + r[2][i] * normal[2]) * scale;
        }
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-12 {
            n = n.map(|c| c / len);
        }
        (p, n)
    }
}

/// Backface culling of a planar or cylindrical projection: surfaces turned more than
/// `angle_deg` away from the projection's facing direction get nothing. `hardness` 1 cuts at the
/// angle; below 1 the projection fades out over up to [`BackfaceCulling::SOFT_BAND_DEG`] degrees
/// before it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackfaceCulling {
    pub angle_deg: f32,
    pub hardness: f32,
}

impl Default for BackfaceCulling {
    fn default() -> Self {
        Self { angle_deg: 90.0, hardness: 0.5 }
    }
}

impl BackfaceCulling {
    /// Width in degrees of the fade at Hardness 0.
    pub const SOFT_BAND_DEG: f32 = 30.0;

    pub fn valid(&self) -> bool {
        self.angle_deg.is_finite() && (0.0..=180.0).contains(&self.angle_deg) && self.hardness.is_finite() && (0.0..=1.0).contains(&self.hardness)
    }

    /// Coverage 0..=1 of a surface whose normal makes the angle with cosine `cos` to the facing
    /// direction.
    pub fn weight(&self, cos: f32) -> f32 {
        let theta = cos.clamp(-1.0, 1.0).acos().to_degrees();
        let band = (1.0 - self.hardness.clamp(0.0, 1.0)) * Self::SOFT_BAND_DEG;
        if band <= 1e-6 {
            return if theta <= self.angle_deg { 1.0 } else { 0.0 };
        }
        ((self.angle_deg - theta) / band).clamp(0.0, 1.0)
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
    /// Warp mode's grid; `None` is the flat grid of [`Projection::warp_grid`]. Not written when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warp: Option<crate::warp::WarpGrid>,
    /// 3D Offset / Rotation / Scale of the projection box (tri-planar, planar, spherical,
    /// cylindrical). Not written at its default.
    #[serde(default, skip_serializing_if = "ProjectionTransform::is_identity")]
    pub transform3: ProjectionTransform,
    /// Backface culling (planar and cylindrical). Not written when off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backface: Option<BackfaceCulling>,
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
            warp: None,
            transform3: ProjectionTransform::default(),
            backface: None,
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
        floats.iter().all(|v| v.is_finite()) && self.symmetry.valid() && self.transform3.valid() && self.backface.is_none_or(|b| b.valid())
    }

    /// The backface culling in effect: planar and cylindrical projections only.
    #[inline]
    pub fn backface_culling(&self) -> Option<BackfaceCulling> {
        if matches!(self.mode, ProjectionMode::Planar | ProjectionMode::Cylindrical) { self.backface } else { None }
    }

    /// Coverage 0..=1 left by backface culling for a texel whose position and normal are in the
    /// projection box's frame ([`Projection::local`]). A planar projection faces the side its
    /// image reads unmirrored from (+Z, +Y, and -X for the X axis, see the planar frames); a
    /// cylindrical one faces outward from its axis and always cuts at 90 degrees. A texel
    /// without a normal is kept. The angle is the one in the scene: the box's scale is taken
    /// back out of the normal.
    pub fn facing_weight(&self, local_pos: [f32; 3], local_normal: [f32; 3]) -> f32 {
        let Some(culling) = self.backface_culling() else { return 1.0 };
        // The box frame's normal is tilted by the box's scale (a box squashed along its axis
        // turned a 60 degree surface into an 82 degree one, culled at a 70 degree limit). Undo
        // the scale on the normal and put it on the cylinder's outward direction instead.
        let s = self.effective_transform3().scale.map(|v| v.clamp(ProjectionTransform::MIN_SCALE, ProjectionTransform::MAX_SCALE));
        let n = [local_normal[0] / s[0], local_normal[1] / s[1], local_normal[2] / s[2]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len <= 1e-12 {
            return 1.0;
        }
        let a = (self.planar_axis as usize).min(2);
        if self.mode == ProjectionMode::Planar {
            let facing = if a == 0 { -1.0 } else { 1.0 };
            return culling.weight(n[a] * facing / len);
        }
        let mut d = [(local_pos[0] - 0.5) * s[0], (local_pos[1] - 0.5) * s[1], (local_pos[2] - 0.5) * s[2]];
        d[a] = 0.0;
        let dl = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if dl <= 1e-9 {
            return 1.0;
        }
        BackfaceCulling { angle_deg: 90.0, ..culling }.weight((n[0] * d[0] + n[1] * d[1] + n[2] * d[2]) / (len * dl))
    }

    /// Whether the mode has a 3D Offset / Rotation / Scale (Warp is placed by its grid).
    #[inline]
    pub fn has_transform3(&self) -> bool {
        matches!(self.mode, ProjectionMode::Triplanar | ProjectionMode::Planar | ProjectionMode::Spherical | ProjectionMode::Cylindrical)
    }

    /// The 3D transform in effect: identity for the modes without one.
    #[inline]
    pub fn effective_transform3(&self) -> ProjectionTransform {
        if self.has_transform3() { self.transform3 } else { ProjectionTransform::default() }
    }

    /// The position and normal the projection reads for a texel: folded by the symmetry, then
    /// brought into the projection box's frame.
    #[inline]
    pub fn local(&self, pos: [f32; 3], normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let (p, n) = self.effective_symmetry().fold(pos, normal);
        if self.has_transform3() { self.transform3.to_local(p, n) } else { (p, n) }
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
        self.effective_wrap() != UvWrap::Repeat || self.crops_to_shape() || self.mode == ProjectionMode::Warp || self.backface_culling().is_some()
    }

    /// The warp grid in use: the stored one, or the flat grid of `planar_axis`.
    pub fn warp_grid(&self) -> crate::warp::WarpGrid {
        self.warp.unwrap_or_else(|| crate::warp::WarpGrid::planar(self.planar_axis, crate::warp::DEFAULT_WARP_CELLS, crate::warp::DEFAULT_WARP_CELLS))
    }

    /// Warp mode: unprojected 2D coordinates of a bounding-cube position through the grid, and
    /// whether the grid covers it.
    pub fn warp_coords(&self, pos: [f32; 3]) -> ([f32; 2], bool) {
        let axis = self.planar_axis.min(2);
        match &self.warp {
            Some(grid) => grid.coords(axis, pos),
            // The flat grid is the unit square of the planar projection.
            None => {
                let q = crate::warp::plane_coords(pos, axis);
                (q, q.iter().all(|v| (-1e-5..=1.0 + 1e-5).contains(v)))
            }
        }
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
        // QA 2026-11: the normal is mirrored with the position. It was rotated, which flips its
        // tangential part: planar backface culling was inverted in every flipped copy.
        let close = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5);
        let n = f.fold(at(quarter + t), [0.0, 0.0, -1.0]).1;
        assert!(close(n, [0.0, 0.0, 1.0]), "a normal facing -Z just past the mirror faces +Z in the first copy: {n:?}");
        let outward = |angle: f32| [angle.sin(), 0.0, angle.cos()];
        for (sym, angle, read) in [(f, quarter + t, quarter - t), (f, 2.0 * quarter + t, t), (r, quarter + t, t)] {
            let n = sym.fold(at(angle), outward(angle)).1;
            assert!(close(n, outward(read)), "an outward normal stays outward at {read}: {n:?}");
            let along = |a: f32| [a.cos(), 0.0, -a.sin()];
            let m = sym.fold(at(angle), along(angle)).1;
            let expected = if sym == f && ((angle / quarter).floor() as i64) % 2 == 1 { along(read).map(|v| -v) } else { along(read) };
            assert!(close(m, expected), "the tangent direction reverses only in a flipped copy: {m:?} vs {expected:?}");
        }
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

    #[test]
    fn backface_culling_keeps_what_faces_the_projection() {
        let hard = BackfaceCulling { angle_deg: 90.0, hardness: 1.0 };
        assert!(hard.valid() && !BackfaceCulling { angle_deg: 200.0, hardness: 1.0 }.valid() && !BackfaceCulling { angle_deg: 90.0, hardness: 2.0 }.valid());
        // 60 degrees off: inside a 90 degree limit, outside a 45 degree one.
        assert_eq!(hard.weight(0.5), 1.0);
        assert_eq!(BackfaceCulling { angle_deg: 45.0, hardness: 1.0 }.weight(0.5), 0.0);
        assert_eq!(hard.weight(-0.5), 0.0);
        // Hardness 0 fades over the last 30 degrees: 75 degrees off is half way.
        let soft = BackfaceCulling { angle_deg: 90.0, hardness: 0.0 };
        assert!((soft.weight(75f32.to_radians().cos()) - 0.5).abs() < 1e-3);
        assert_eq!(soft.weight(1.0), 1.0);
        assert_eq!(soft.weight(-1.0), 0.0);
        let planar = |planar_axis| Projection { mode: ProjectionMode::Planar, planar_axis, backface: Some(hard), ..Projection::default() };
        let c = [0.5; 3];
        assert_eq!(planar(2).facing_weight(c, [0.0, 0.0, 1.0]), 1.0);
        assert_eq!(planar(2).facing_weight(c, [0.0, 0.0, -1.0]), 0.0);
        assert_eq!(planar(1).facing_weight(c, [0.0, 2.0, 0.0]), 1.0, "normals need not be unit length");
        assert_eq!(planar(0).facing_weight(c, [-1.0, 0.0, 0.0]), 1.0, "the X frame reads upright from -X");
        assert_eq!(planar(0).facing_weight(c, [1.0, 0.0, 0.0]), 0.0);
        assert_eq!(planar(2).facing_weight(c, [0.0; 3]), 1.0, "no normal: kept");
        // Cylindrical (axis Y): outward faces are kept, inward ones culled; its limit is 90 degrees.
        let cyl = Projection { mode: ProjectionMode::Cylindrical, planar_axis: 1, backface: Some(BackfaceCulling { angle_deg: 10.0, hardness: 1.0 }), ..Projection::default() };
        assert_eq!(cyl.facing_weight([1.0, 0.2, 0.5], [1.0, 0.0, 0.0]), 1.0);
        assert_eq!(cyl.facing_weight([1.0, 0.2, 0.5], [0.6, 0.0, 0.8]), 1.0);
        assert_eq!(cyl.facing_weight([1.0, 0.2, 0.5], [-1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cyl.facing_weight([0.5, 0.2, 0.5], [-1.0, 0.0, 0.0]), 1.0, "on the axis there is no outward direction");
        // Other modes ignore it; it makes the projection clip; it is not written when off.
        let uv = Projection { backface: Some(hard), ..Projection::default() };
        assert!(uv.backface_culling().is_none() && !uv.clips() && uv.facing_weight(c, [0.0, 0.0, -1.0]) == 1.0);
        assert!(planar(2).clips() && planar(2).valid());
        assert!(!Projection { backface: Some(BackfaceCulling { angle_deg: f32::NAN, hardness: 1.0 }), ..planar(2) }.valid());
        assert!(!serde_json::to_string(&Projection::default()).unwrap().contains("backface"));
        let back: Projection = serde_json::from_str(&serde_json::to_string(&planar(2)).unwrap()).unwrap();
        assert_eq!(back.backface, Some(hard));
    }

    /// QA 2026-11: the culling angle was measured on the normal in the box's frame, which the
    /// box's scale tilts: a box squashed along its axis culled a surface 60 degrees off at a 70
    /// degree limit, and a stretched one kept a surface 80 degrees off.
    #[test]
    fn the_culling_angle_is_the_angle_in_the_scene() {
        let limit = BackfaceCulling { angle_deg: 70.0, hardness: 1.0 };
        let at = |deg: f32| {
            let r = deg.to_radians();
            [r.sin(), 0.0, r.cos()]
        };
        let weight = |projection: &Projection, normal: [f32; 3]| {
            let (p, n) = projection.local([0.5, 0.5, 0.5], normal);
            projection.facing_weight(p, n)
        };
        for scale in [[1.0, 1.0, 1.0], [1.0, 1.0, 0.25], [1.0, 1.0, 4.0], [3.0, 0.5, 2.0]] {
            for rotation_deg in [[0.0; 3], [0.0, 30.0, 0.0]] {
                let transform3 = ProjectionTransform { scale, rotation_deg, ..ProjectionTransform::default() };
                let planar = Projection { mode: ProjectionMode::Planar, planar_axis: 2, backface: Some(limit), transform3, ..Projection::default() };
                // Angles from the box's own (rotated) Z axis.
                let off = rotation_deg[1];
                assert_eq!(weight(&planar, at(off + 60.0)), 1.0, "60 degrees off, scale {scale:?}, rotation {rotation_deg:?}");
                assert_eq!(weight(&planar, at(off - 60.0)), 1.0, "-60 degrees off, scale {scale:?}, rotation {rotation_deg:?}");
                assert_eq!(weight(&planar, at(off + 80.0)), 0.0, "80 degrees off, scale {scale:?}, rotation {rotation_deg:?}");
                assert_eq!(weight(&planar, at(off + 180.0)), 0.0, "facing away, scale {scale:?}");
            }
        }
        // A soft edge fades by the scene angle too: at hardness 0.5 the band is 15 degrees, half way at 62.5.
        let soft = Projection { mode: ProjectionMode::Planar, planar_axis: 2, backface: Some(BackfaceCulling { angle_deg: 70.0, hardness: 0.5 }), transform3: ProjectionTransform { scale: [1.0, 1.0, 0.25], ..ProjectionTransform::default() }, ..Projection::default() };
        assert!((weight(&soft, at(62.5)) - 0.5).abs() < 1e-3, "{}", weight(&soft, at(62.5)));
        // A stretched cylinder (axis Y, wide in X) still keeps its outside and culls its inside.
        let cyl = Projection { mode: ProjectionMode::Cylindrical, planar_axis: 1, backface: Some(limit), transform3: ProjectionTransform { scale: [4.0, 1.0, 1.0], ..ProjectionTransform::default() }, ..Projection::default() };
        let (p, n) = cyl.local([2.5, 0.5, 0.5], [1.0, 0.0, 0.0]);
        assert_eq!(cyl.facing_weight(p, n), 1.0);
        let (p, n) = cyl.local([2.5, 0.5, 0.5], [-1.0, 0.0, 0.0]);
        assert_eq!(cyl.facing_weight(p, n), 0.0);
    }

    #[test]
    fn rotation_angles_round_trip_and_compose_about_the_box_axes() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-3);
        let id = ProjectionTransform::default();
        for angles in [[0.0, 0.0, 0.0], [20.0, -35.0, 110.0], [-170.0, 60.0, -45.0], [0.0, 0.0, 90.0]] {
            let t = ProjectionTransform { rotation_deg: angles, ..id };
            assert!(close(ProjectionTransform::euler_of(t.rotation()), angles), "{angles:?}");
        }
        // At Y = 90 the angles are not unique: the matrix still round-trips.
        let locked = ProjectionTransform { rotation_deg: [30.0, 90.0, 40.0], ..id };
        let again = ProjectionTransform { rotation_deg: ProjectionTransform::euler_of(locked.rotation()), ..id };
        let (a, b) = (locked.rotation(), again.rotation());
        assert!((0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < 1e-3)), "{a:?} {b:?}");
        // Turning about the box's own axis: Z on an unrotated box is plain Z; X after a quarter
        // turn about Z is X first, then Z.
        assert!(close(id.rotated_about_local_axis(2, 30.0), [0.0, 0.0, 30.0]));
        let turned = ProjectionTransform { rotation_deg: [0.0, 0.0, 90.0], ..id };
        assert!(close(turned.rotated_about_local_axis(0, 90.0), [90.0, 0.0, 90.0]));
        assert!(close(turned.rotated_about_local_axis(2, -90.0), [0.0, 0.0, 0.0]));
    }

    #[test]
    fn projection_transform_brings_positions_into_the_box_frame() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5);
        let planar = |transform3| Projection { mode: ProjectionMode::Planar, transform3, ..Projection::default() };
        let id = ProjectionTransform::default();
        assert!(id.is_identity() && id.valid());
        assert_eq!(planar(id).local([0.2, 0.3, 0.4], [0.0, 0.0, 1.0]), ([0.2, 0.3, 0.4], [0.0, 0.0, 1.0]));
        // Offset: the box centre moves, so the texel a quarter to the right is its centre.
        let moved = ProjectionTransform { offset: [0.25, 0.0, 0.0], ..id };
        assert!(close(planar(moved).local([0.75, 0.5, 0.5], [0.0, 0.0, 1.0]).0, [0.5, 0.5, 0.5]));
        // Scale 2 on X: the box edge is at the cube's edge plus half a cube.
        let wide = ProjectionTransform { scale: [2.0, 1.0, 1.0], ..id };
        assert!(close(planar(wide).local([1.0, 0.5, 0.5], [0.0, 0.0, 1.0]).0, [0.75, 0.5, 0.5]));
        // A quarter turn about Z: the model's +Y is the box's +X, for positions and normals.
        let turned = ProjectionTransform { rotation_deg: [0.0, 0.0, 90.0], ..id };
        let (p, n) = planar(turned).local([0.5, 0.75, 0.5], [0.0, 1.0, 0.0]);
        assert!(close(p, [0.75, 0.5, 0.5]) && close(n, [1.0, 0.0, 0.0]), "{p:?} {n:?}");
        // Rotations apply X first, then Y, then Z: the box's +Y goes to +Z, then stays under Z.
        let both = ProjectionTransform { rotation_deg: [90.0, 0.0, 90.0], ..id };
        assert!(close(planar(both).local([0.5, 0.5, 0.75], [0.0, 0.0, 1.0]).0, [0.5, 0.75, 0.5]));
        // UV and Warp projections ignore it; invalid values are rejected; the default is not written.
        let uv = Projection { transform3: moved, ..Projection::default() };
        assert_eq!(uv.local([0.75, 0.5, 0.5], [0.0; 3]).0, [0.75, 0.5, 0.5]);
        assert!(uv.effective_transform3().is_identity() && planar(moved).effective_transform3() == moved);
        assert!(planar(moved).valid() && !planar(ProjectionTransform { scale: [0.0, 1.0, 1.0], ..id }).valid());
        assert!(!planar(ProjectionTransform { offset: [f32::NAN, 0.0, 0.0], ..id }).valid());
        assert!(!serde_json::to_string(&planar(id)).unwrap().contains("transform3"));
        let back: Projection = serde_json::from_str(&serde_json::to_string(&planar(turned)).unwrap()).unwrap();
        assert_eq!(back.transform3, turned);
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
