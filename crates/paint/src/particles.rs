//! Particle brush: each stamp releases particles that run over the surface, pulled by gravity
//! (rain streaks, drips, leaks) or pushed up by a negative gravity (burn marks, soot), and paint
//! a thinning trail. Painter calls these physics / particle brushes.
//!
//! The simulation walks on the mesh: every step moves the particle along the surface tangent and
//! snaps it back to the nearest surface point ([`Bvh::closest_point`]), so a trail follows
//! curves, wraps over convex edges and stops where it would leave the model. A particle under a
//! surface that faces the gravity (the underside of an overhang) drops off, as a drip does.
//! Everything random comes from the stroke seed, so a stroke repeats exactly.
//!
//! Positions are object space with +Y up (ADR-0005); gravity pulls toward -Y.

use glam::Vec3;
use subcraft_mesh::{Bvh, Mesh};

use crate::path::rand01;
use crate::settings::BrushSettings;

/// Most particles a stamp releases.
pub const MAX_PARTICLES: u32 = 16;
/// Most steps (painted dabs) in one particle's trail.
pub const MAX_TRAIL_STEPS: u32 = 96;
/// A surface whose normal points this much along the gravity is an underside: particles fall off.
const DETACH_COS: f32 = 0.75;

/// One painted point of a trail: object-space position and its size as a fraction of the
/// particle's starting size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailPoint {
    pub pos: Vec3,
    pub size: f32,
}

/// Interpolated shading normal of triangle `tri` at barycentric `(u, v)`, falling back to the
/// face normal.
fn normal_at(mesh: &Mesh, tri: u32, u: f32, v: f32) -> Vec3 {
    let [a, b, c] = mesh.tri(tri as usize);
    let face = {
        let [p0, p1, p2] = mesh.tri_positions(tri as usize);
        (p1 - p0).cross(p2 - p0).normalize_or_zero()
    };
    let n = mesh.normals.get(a as usize).zip(mesh.normals.get(b as usize)).zip(mesh.normals.get(c as usize))
        .map(|((&na, &nb), &nc)| (na * (1.0 - u - v) + nb * u + nc * v).normalize_or_zero())
        .unwrap_or(Vec3::ZERO);
    if n == Vec3::ZERO { face } else { n }
}

fn point_at(mesh: &Mesh, tri: u32, u: f32, v: f32) -> Vec3 {
    let [p0, p1, p2] = mesh.tri_positions(tri as usize);
    p0 * (1.0 - u - v) + p1 * u + p2 * v
}

/// Any unit vector perpendicular to `n`.
fn perpendicular(n: Vec3) -> Vec3 {
    let helper = if n.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    n.cross(helper).normalize_or_zero()
}

/// The trail of particle `index` released by stamp `stamp` at surface point `start`, for a brush
/// of object-space radius `radius`. Empty when particles are off or the start is not on an
/// accepted triangle.
pub fn trail(mesh: &Mesh, bvh: &Bvh, s: &BrushSettings, start: Vec3, radius: f32, stamp: u32, index: u32, accept: impl Fn(u32) -> bool + Copy) -> Vec<TrailPoint> {
    if s.particle_count == 0 || !(radius > 0.0) || !start.is_finite() {
        return Vec::new();
    }
    let seed = s.seed;
    let stream = 64 + index * 8;
    let r = |k: u32| rand01(seed, stamp, stream + k);
    let size = radius * s.particle_size.clamp(0.01, 1.0);
    let length = s.particle_length.clamp(0.0, 64.0) * 2.0 * radius;
    let steps = ((length / (size * 0.5)).ceil() as u32).clamp(1, MAX_TRAIL_STEPS);
    let step = length / steps as f32;
    let Some(hit) = bvh.closest_point(start, radius * 2.0 + 1e-6, accept) else { return Vec::new() };
    let mut n = normal_at(mesh, hit.tri, hit.u, hit.v);
    // Release point: a random spot inside the brush on the tangent plane.
    let t1 = perpendicular(n);
    let t2 = n.cross(t1);
    let (a, d) = (r(0) * std::f32::consts::TAU, r(1).sqrt() * s.particle_spread.clamp(0.0, 1.0) * radius);
    let released = point_at(mesh, hit.tri, hit.u, hit.v) + (t1 * a.cos() + t2 * a.sin()) * d;
    let Some(hit) = bvh.closest_point(released, radius * 2.0 + 1e-6, accept) else { return Vec::new() };
    let mut pos = point_at(mesh, hit.tri, hit.u, hit.v);
    n = normal_at(mesh, hit.tri, hit.u, hit.v);
    let gravity = Vec3::NEG_Y * s.particle_gravity.clamp(-1.0, 1.0);
    let pull = gravity.length();
    // Starting heading: a random direction on the surface (used where gravity does not decide).
    let b = r(2) * std::f32::consts::TAU;
    let mut heading = (perpendicular(n) * b.cos() + n.cross(perpendicular(n)) * b.sin()).normalize_or_zero();
    let end = s.particle_size_end.clamp(0.0, 1.0);
    let mut out = vec![TrailPoint { pos, size: 1.0 }];
    for k in 1..=steps {
        if pull > 0.0 && n.dot(gravity / pull) > DETACH_COS {
            break; // under an overhang: the particle drops off
        }
        let along = gravity - n * n.dot(gravity);
        let tangent_heading = (heading - n * n.dot(heading)).normalize_or_zero();
        // Wander: turn the heading by a random angle each step.
        let turn = (2.0 * rand01(seed, stamp, stream + 8 * MAX_PARTICLES + k) - 1.0) * s.particle_wander.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
        let side = n.cross(tangent_heading);
        let wandered = tangent_heading * turn.cos() + side * turn.sin();
        // Gravity leads where the surface slopes; the heading (with its wander) carries the
        // particle where gravity has no pull along the surface (a flat top) or is weak.
        let carry = if along.length() < 0.2 * pull { (1.0 - pull).max(0.15) } else { (1.0 - pull).max(0.0) };
        let dir = (along + wandered * carry).normalize_or_zero();
        if dir == Vec3::ZERO {
            break;
        }
        heading = dir;
        let Some(next) = bvh.closest_point(pos + dir * step, step * 3.0, accept) else { break };
        let q = point_at(mesh, next.tri, next.u, next.v);
        // Stuck (pressed into a concave corner): stop instead of stacking dabs on one spot.
        if q.distance(pos) < step * 0.2 {
            break;
        }
        pos = q;
        n = normal_at(mesh, next.tri, next.u, next.v);
        let t = k as f32 / steps as f32;
        out.push(TrailPoint { pos, size: 1.0 + (end - 1.0) * t });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::primitives;

    fn drips() -> BrushSettings {
        BrushSettings { particle_count: 1, particle_length: 4.0, particle_size: 0.3, particle_size_end: 0.2, particle_gravity: 1.0, particle_wander: 0.0, particle_spread: 0.0, ..BrushSettings::default() }
    }

    #[test]
    fn a_drip_on_a_wall_runs_straight_down_and_thins() {
        let cube = primitives::cube(2.0);
        let bvh = Bvh::build(&cube);
        // Front face (z = 1), upper half: falls toward -Y and stays on the face.
        let t = trail(&cube, &bvh, &drips(), Vec3::new(0.0, 0.8, 1.0), 0.1, 0, 0, |_| true);
        assert!(t.len() > 10, "{}", t.len());
        let last = t.last().unwrap();
        assert!((last.pos.y - (0.8 - 0.8)).abs() < 0.05, "4 diameters of 0.2 = 0.8 down: {:?}", last.pos);
        assert!(t.iter().all(|p| (p.pos.z - 1.0).abs() < 1e-4 && p.pos.x.abs() < 1e-4), "stays on the face, straight");
        assert!(t.windows(2).all(|w| w[1].pos.y < w[0].pos.y && w[1].size <= w[0].size));
        assert!((last.size - 0.2).abs() < 1e-5);
    }

    #[test]
    fn a_drip_follows_a_curved_surface_down() {
        let sphere = primitives::uv_sphere(1.0, 48, 24);
        let bvh = Bvh::build(&sphere);
        // Released near the top, off the pole: it runs down the sphere along a meridian.
        let start = Vec3::new(0.3, 0.9, 0.0).normalize();
        let s = BrushSettings { particle_length: 6.0, ..drips() };
        let t = trail(&sphere, &bvh, &s, start, 0.1, 0, 0, |_| true);
        let last = t.last().unwrap().pos;
        assert!(last.y < start.y - 0.6, "ran down: {last:?}");
        assert!(last.z.abs() < 0.05 && last.x > 0.0, "stayed on its meridian: {last:?}");
        assert!(t.iter().all(|p| (p.pos.length() - 1.0).abs() < 0.02), "on the surface");
    }

    #[test]
    fn negative_gravity_rises_and_undersides_drop_particles() {
        let cube = primitives::cube(2.0);
        let bvh = Bvh::build(&cube);
        let up = trail(&cube, &bvh, &BrushSettings { particle_gravity: -1.0, ..drips() }, Vec3::new(0.0, -0.8, 1.0), 0.1, 0, 0, |_| true);
        assert!(up.last().unwrap().pos.y > -0.1, "burn marks rise: {:?}", up.last());
        // Bottom face (normal -Y): a drip drops off at once.
        let under = trail(&cube, &bvh, &drips(), Vec3::new(0.0, -1.0, 0.0), 0.1, 0, 0, |_| true);
        assert_eq!(under.len(), 1);
    }

    #[test]
    fn trails_repeat_for_a_seed_and_differ_between_particles() {
        let sphere = primitives::uv_sphere(1.0, 32, 16);
        let bvh = Bvh::build(&sphere);
        let s = BrushSettings { particle_count: 4, particle_wander: 0.6, particle_spread: 1.0, ..drips() };
        let start = Vec3::new(0.0, 0.5, 0.866);
        let a = trail(&sphere, &bvh, &s, start, 0.1, 3, 0, |_| true);
        assert_eq!(a, trail(&sphere, &bvh, &s, start, 0.1, 3, 0, |_| true));
        assert_ne!(a, trail(&sphere, &bvh, &s, start, 0.1, 3, 1, |_| true));
        // On a sphere every trail point stays on the surface.
        for p in &a {
            assert!((p.pos.length() - 1.0).abs() < 0.02, "{:?}", p.pos);
        }
        // Off: no trail.
        assert!(trail(&sphere, &bvh, &BrushSettings { particle_count: 0, ..s }, start, 0.1, 0, 0, |_| true).is_empty());
    }
}
