//! Stamps: brush dabs resolved onto the surface, and their footprint.

use glam::{Vec2, Vec3};
use subcraft_mesh::{Bvh, Mesh};

use crate::camera::Camera;
use crate::settings::{Alignment, falloff};

/// A brush dab placed on the surface.
#[derive(Clone, Copy, Debug)]
pub struct Stamp {
    /// Index of the camera (0 = view, others = symmetry mirrors) the stamp projects from.
    pub camera: usize,
    pub screen: Vec2,
    pub center: Vec3,
    /// Shading normal at the hit.
    pub normal: Vec3,
    pub uv: Vec2,
    pub tri: u32,
    /// Object-space radius.
    pub radius: f32,
    pub radius_px: f32,
    pub radius_uv: f32,
    pub flow: f32,
    pub angle: f32,
    pub roundness: f32,
    pub hardness: f32,
    /// Tangent frame: `tangent` points screen-right, `bitangent` screen-down.
    pub tangent: Vec3,
    pub bitangent: Vec3,
}

/// Per-stamp brush parameters before the surface hit is known.
#[derive(Clone, Copy, Debug)]
pub struct Dab {
    pub screen: Vec2,
    pub radius: f32,
    pub flow: f32,
    pub angle: f32,
    pub roundness: f32,
    pub hardness: f32,
}

/// Ray-cast a dab through `camera`; `None` if it misses the mesh.
pub fn resolve(dab: &Dab, camera_index: usize, camera: &Camera, mesh: &Mesh, bvh: &Bvh) -> Option<Stamp> {
    let (origin, dir) = camera.ray(dab.screen);
    let hit = bvh.intersect(origin, dir, 0.0, f32::INFINITY)?;
    let sp = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
    let n = sp.normal;
    let right = camera.right();
    let mut t = right - n * right.dot(n);
    if t.length_squared() < 1e-8 {
        t = n.any_orthonormal_vector();
    }
    let t = t.normalize();
    let b = t.cross(n);
    let p = mesh.tri_positions(hit.tri as usize);
    let uv = mesh.tri_uvs(hit.tri as usize);
    let world_area = (p[1] - p[0]).cross(p[2] - p[0]).length();
    let uv_area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs();
    let uv_per_unit = if world_area > 1e-12 { (uv_area / world_area).sqrt() } else { 0.0 };
    let depth = camera.view_depth(sp.position);
    Some(Stamp {
        camera: camera_index,
        screen: dab.screen,
        center: sp.position,
        normal: n,
        uv: sp.uv,
        tri: hit.tri,
        radius: dab.radius,
        radius_px: dab.radius * camera.pixels_per_unit(depth),
        radius_uv: dab.radius * uv_per_unit,
        flow: dab.flow,
        angle: dab.angle,
        roundness: dab.roundness,
        hardness: dab.hardness,
        tangent: t,
        bitangent: b,
    })
}

impl Stamp {
    /// Brush strength (0..1) at surface point `p` / texel UV `uv`.
    #[inline]
    pub fn footprint(&self, alignment: Alignment, camera: &Camera, p: Vec3, uv: Vec2) -> f32 {
        let local = match alignment {
            Alignment::TangentWrap => {
                let w = p - self.center;
                let dist = w.length();
                if dist >= self.radius {
                    return 0.0;
                }
                let inplane = w - self.normal * w.dot(self.normal);
                let l = inplane.length();
                if l < 1e-12 {
                    Vec2::ZERO
                } else {
                    Vec2::new(inplane.dot(self.tangent), inplane.dot(self.bitangent)) * (dist / (l * self.radius))
                }
            }
            Alignment::TangentPlanar => {
                let w = p - self.center;
                if w.dot(self.normal).abs() >= self.radius {
                    return 0.0;
                }
                Vec2::new(w.dot(self.tangent), w.dot(self.bitangent)) / self.radius
            }
            Alignment::Camera => {
                let Some(s) = camera.project(p) else { return 0.0 };
                (Vec2::new(s.x, s.y) - self.screen) / self.radius_px.max(1e-6)
            }
            Alignment::Uv => (uv - self.uv) / self.radius_uv.max(1e-9),
        };
        let (s, c) = self.angle.sin_cos();
        let r = Vec2::new(local.x * c + local.y * s, -local.x * s + local.y * c);
        let r = Vec2::new(r.x, r.y / self.roundness.clamp(0.01, 1.0));
        falloff(r.length(), self.hardness)
    }
}
