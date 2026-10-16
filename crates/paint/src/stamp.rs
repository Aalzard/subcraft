//! Stamps: brush dabs resolved onto the surface, and their footprint.

use glam::{Vec2, Vec3};
use subcraft_image::{Bitmap, Wrap};
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
    Some(from_surface(dab, camera_index, camera, mesh, hit.tri, hit.u, hit.v, None))
}

fn from_surface(dab: &Dab, camera_index: usize, camera: &Camera, mesh: &Mesh, tri: u32, u: f32, v: f32, center: Option<Vec3>) -> Stamp {
    let sp = mesh.interpolate(tri as usize, u, v);
    let n = sp.normal;
    let right = camera.right();
    let mut t = right - n * right.dot(n);
    if t.length_squared() < 1e-8 {
        t = n.any_orthonormal_vector();
    }
    let t = t.normalize();
    let b = t.cross(n);
    let p = mesh.tri_positions(tri as usize);
    let uv = mesh.tri_uvs(tri as usize);
    let world_area = (p[1] - p[0]).cross(p[2] - p[0]).length();
    let uv_area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs();
    let uv_per_unit = if world_area > 1e-12 { (uv_area / world_area).sqrt() } else { 0.0 };
    let center = center.unwrap_or(sp.position);
    let depth = camera.view_depth(center);
    // Extrapolate the chart for UV alignment when the cursor lies beyond its surface edge.
    let chart_uv = if center == sp.position { sp.uv } else {
        let (a,b,w) = (p[1]-p[0],p[2]-p[0],center-p[0]);
        let (aa,ab,bb) = (a.dot(a),a.dot(b),b.dot(b)); let det = aa*bb-ab*ab;
        if det.abs() > 1e-20 { let u=(bb*w.dot(a)-ab*w.dot(b))/det; let v=(aa*w.dot(b)-ab*w.dot(a))/det; uv[0]+u*(uv[1]-uv[0])+v*(uv[2]-uv[0]) } else { sp.uv }
    };
    Stamp {
        camera: camera_index,
        screen: dab.screen,
        center,
        normal: n,
        uv: chart_uv,
        tri,
        radius: dab.radius,
        radius_px: dab.radius * camera.pixels_per_unit(depth),
        radius_uv: dab.radius * uv_per_unit,
        flow: dab.flow,
        angle: dab.angle,
        roundness: dab.roundness,
        hardness: dab.hardness,
        tangent: t,
        bitangent: b,
    }
}

/// Resolve a full footprint, including when its center ray misses. The center remains on the
/// cursor ray, at the nearest touched surface's depth. Final coverage/occlusion stays in kernel.
pub fn resolve_overlap(dab: &Dab, camera_index: usize, camera: &Camera, mesh: &Mesh, bvh: &Bvh,
    alignment: Alignment, has_alpha: bool, accept: impl Fn(u32) -> bool) -> Option<Stamp> {
    if !dab.screen.is_finite() || !dab.radius.is_finite() || dab.radius <= 0.0 { return None; }
    if let Some(stamp) = resolve(dab, camera_index, camera, mesh, bvh) && accept(stamp.tri) { return Some(stamp); }
    let (origin, dir) = camera.ray(dab.screen);
    let extent = reach(has_alpha) * if alignment == Alignment::TangentPlanar { std::f32::consts::SQRT_2 } else { 1.0 };
    let radius = dab.radius * extent;
    let vp = camera.view_proj();
    let mut best: Option<(f32, f32, u32, Vec3)> = None;
    bvh.query(|bounds| expanded_ray(bounds.min, bounds.max, origin, dir, radius), |tri| {
        if !accept(tri) { return; }
        let p = mesh.tri_positions(tri as usize);
        let clip = p.map(|v| vp * v.extend(1.0));
        if clip.iter().any(|v| v.w <= 1e-6 || v.z < 0.0) { return; }
        let screen = clip.map(|v| camera.ndc_to_screen(v.truncate().truncate()/v.w));
        let Some(weights) = closest_weights(dab.screen, screen) else { return };
        let projected = screen[0]*weights.x + screen[1]*weights.y + screen[2]*weights.z;
        let w = weights / Vec3::new(clip[0].w,clip[1].w,clip[2].w);
        let w = w / w.element_sum();
        let point = p[0]*w.x+p[1]*w.y+p[2]*w.z;
        let depth = camera.view_depth(point);
        let r_px = dab.radius * camera.pixels_per_unit(depth);
        let score = projected.distance_squared(dab.screen) / (r_px*r_px).max(1e-20);
        if score >= extent*extent || !score.is_finite() { return; }
        if best.as_ref().is_none_or(|b| score < b.0 || (score == b.0 && depth < b.1)) { best = Some((score, depth, tri, w)); }
    });
    let (_,depth,tri,w) = best?;
    let along = camera.forward().dot(dir);
    if along <= 1e-6 { return None; }
    let center = origin + dir * ((depth-camera.view_depth(origin))/along);
    Some(from_surface(dab, camera_index, camera, mesh, tri, w.y, w.z, Some(center)))
}

fn expanded_ray(min: Vec3, max: Vec3, origin: Vec3, dir: Vec3, radius: f32) -> bool {
    let (min,max) = (min-Vec3::splat(radius),max+Vec3::splat(radius));
    let (mut near,mut far) = (0.0_f32,f32::INFINITY);
    for axis in 0..3 {
        if dir[axis].abs() < 1e-12 { if origin[axis] < min[axis] || origin[axis] > max[axis] { return false; } }
        else { let a=(min[axis]-origin[axis])/dir[axis]; let b=(max[axis]-origin[axis])/dir[axis]; near=near.max(a.min(b)); far=far.min(a.max(b)); if far<near { return false; } }
    }
    true
}

/// Closest point on a projected triangle, expressed as screen-linear barycentrics. Analytic
/// edges handle thin pieces and corners without sampling a ring of rays or a depth-image disc.
fn closest_weights(p: Vec2, q: [Vec2;3]) -> Option<Vec3> {
    let area = (q[1]-q[0]).perp_dot(q[2]-q[0]);
    if area.abs() < 1e-9 { return None; }
    let b = (q[2]-q[1]).perp_dot(p-q[1])/area;
    let c = (q[0]-q[2]).perp_dot(p-q[2])/area;
    let w = Vec3::new(b,c,1.0-b-c);
    if w.min_element() >= 0.0 { return Some(w); }
    let mut best=(f32::INFINITY,Vec3::ZERO);
    for (a,b) in [(0,1),(1,2),(2,0)] {
        let edge=q[b]-q[a]; let t=((p-q[a]).dot(edge)/edge.length_squared().max(1e-20)).clamp(0.0,1.0);
        let distance=(q[a]+edge*t).distance_squared(p);
        if distance<best.0 { let mut w=Vec3::ZERO; w[a]=1.0-t; w[b]=t; best=(distance,w); }
    }
    Some(best.1)
}

impl Stamp {
    /// Brush strength (0..1) at surface point `p` / texel UV `uv`. With a brush `alpha`, the
    /// footprint is the alpha image stretched over the square [-r, r]² (luminance × alpha);
    /// otherwise a round falloff.
    #[inline]
    pub fn footprint(&self, alignment: Alignment, camera: &Camera, p: Vec3, uv: Vec2, alpha: Option<&Bitmap>) -> f32 {
        let reach = reach(alpha.is_some());
        let local = match alignment {
            Alignment::TangentWrap => {
                let w = p - self.center;
                let dist = w.length();
                if dist >= self.radius * reach {
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
                if w.dot(self.normal).abs() >= self.radius * reach {
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
        match alpha {
            None => falloff(r.length(), self.hardness),
            Some(bmp) => {
                if r.x.abs() >= 1.0 || r.y.abs() >= 1.0 {
                    return 0.0;
                }
                let t = bmp.sample(r.x * 0.5 + 0.5, r.y * 0.5 + 0.5, Wrap::Clamp, false);
                (t[0] * t[3]).clamp(0.0, 1.0)
            }
        }
    }
}

/// How far (in radii) a footprint can reach: the corners of the alpha square.
#[inline]
pub fn reach(has_alpha: bool) -> f32 {
    if has_alpha { std::f32::consts::SQRT_2 } else { 1.0 }
}
