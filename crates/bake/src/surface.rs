//! Low-poly surface samples per texel of a texture set (what every baker starts from).

use glam::{Vec2, Vec3, Vec4};
use subcraft_core::IRect;
use subcraft_mesh::Mesh;
use subcraft_mesh::uv_raster::raster_triangle;

/// Rim (texels) baked around islands so bilinear filtering never reads unbaked gutter.
pub const BAKE_EXPAND: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct Texel {
    pub index: u32,
    pub tri: u32,
    pub position: Vec3,
    pub normal: Vec3,
    /// xyz tangent, w bitangent sign (MikkTSpace).
    pub tangent: Vec4,
    pub geometric_normal: Vec3,
}

/// All covered texels of `material_slot` at `size`² (inside texels win over rim texels).
pub fn texels(mesh: &Mesh, material_slot: u32, size: u32) -> Vec<Texel> {
    let n = (size * size) as usize;
    let mut best: Vec<(f32, u32, [f32; 3])> = vec![(f32::INFINITY, u32::MAX, [0.0; 3]); n];
    let full = IRect::from_size(size, size);
    for t in mesh.material_triangles(material_slot) {
        let uv: [Vec2; 3] = mesh.tri_uvs(t);
        raster_triangle(uv, size, size, full, BAKE_EXPAND, |h| {
            let i = (h.y as u32 * size + h.x as u32) as usize;
            let d = h.distance.max(0.0);
            if d < best[i].0 {
                best[i] = (d, t as u32, h.bary);
            }
        });
    }
    best.iter()
        .enumerate()
        .filter(|(_, b)| b.1 != u32::MAX)
        .map(|(i, &(_, tri, bary))| {
            let sp = mesh.interpolate(tri as usize, bary[1], bary[2]);
            Texel {
                index: i as u32,
                tri,
                position: sp.position,
                normal: sp.normal,
                tangent: sp.tangent,
                geometric_normal: sp.geometric_normal,
            }
        })
        .collect()
}

/// Deterministic per-texel random number in [0, 1).
#[inline]
pub fn hash01(a: u32, b: u32) -> f32 {
    let mut x = (a as u64) << 32 | b as u64;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Cosine-weighted hemisphere directions around `n`: Hammersley points rotated per texel.
pub fn cosine_directions(n: Vec3, count: u32, rotation: f32, out: &mut Vec<Vec3>) {
    out.clear();
    let (t, b) = n.any_orthonormal_pair();
    for i in 0..count {
        let u = (i as f32 + 0.5) / count as f32;
        let v = (i.reverse_bits() as f32 / 4_294_967_296.0 + rotation).fract();
        let r = u.sqrt();
        let phi = std::f32::consts::TAU * v;
        let d = t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u).max(0.0).sqrt();
        out.push(d.normalize_or(n));
    }
}
