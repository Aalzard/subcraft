//! MikkTSpace tangent generation over un-indexed triangle corners.
//!
//! UVs are converted to OpenGL orientation (`v_gl = 1 - v`) for the computation so that the
//! resulting bitangent points toward +V in OpenGL texture space; tangent-space normal maps are
//! therefore OpenGL (+Y / "green up") convention, matching common bakers.

use glam::{Vec2, Vec3, Vec4};

struct Corners<'a> {
    positions: &'a [Vec3],
    normals: &'a [Vec3],
    uvs: &'a [Vec2],
    out: Vec<Vec4>,
}

impl mikktspace::Geometry for Corners<'_> {
    fn num_faces(&self) -> usize {
        self.positions.len() / 3
    }
    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }
    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.positions[face * 3 + vert].to_array()
    }
    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.normals[face * 3 + vert].to_array()
    }
    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        let uv = self.uvs[face * 3 + vert];
        [uv.x, 1.0 - uv.y]
    }
    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        self.out[face * 3 + vert] = Vec4::from(tangent);
    }
}

pub fn compute_corner_tangents(positions: &[Vec3], normals: &[Vec3], uvs: &[Vec2]) -> Vec<Vec4> {
    let mut g = Corners { positions, normals, uvs, out: vec![Vec4::ZERO; positions.len()] };
    let ok = mikktspace::generate_tangents(&mut g);
    let mut out = g.out;
    // Repair degenerate results with an arbitrary frame orthogonal to the normal.
    for (i, t) in out.iter_mut().enumerate() {
        let n = normals[i];
        let tv = t.truncate();
        if !ok || !tv.is_finite() || tv.length_squared() < 1e-10 {
            let fallback = n.any_orthonormal_vector();
            *t = fallback.extend(1.0);
        } else {
            // Gram-Schmidt to guarantee orthogonality after interpolation-free generation.
            let ortho = (tv - n * n.dot(tv)).normalize_or(n.any_orthonormal_vector());
            *t = ortho.extend(if t.w < 0.0 { -1.0 } else { 1.0 });
        }
    }
    out
}
