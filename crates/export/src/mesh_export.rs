//! File > Export mesh: the mesh alone (no textures) as OBJ, glTF, GLB or USDA, as modelled or
//! with the Height channel applied as displacement (Painter's "With displacement/tessellation").
//!
//! SubCraft keeps meshes as triangles, so every export is triangulated (Painter's "Apply
//! triangulation" is always on). Displacement subdivides each triangle `subdivisions` times
//! (each level splits a triangle into four at its edge midpoints), then moves every vertex along
//! its normal by height × scale, sampling the Height composite of the texture set that owns the
//! vertex's material slot (UDIM sets by tile). Vertices split along a UV seam sample each side's
//! UVs, so a seam whose two sides have different heights opens, as in other displacement bakes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3, Vec4};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Aabb, Mesh, Submesh};

use crate::scene::{SceneFormat, SceneMaterial, glb_bytes, gltf_files, mtl_text, obj_text, usda_text};
use crate::{ExportError, ExportTextureSet, sanitize};

/// Painter's "With displacement/tessellation" options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Displacement {
    /// World units per unit of height (the viewport's height scale).
    pub scale: f32,
    /// Subdivision levels before displacing (0..=4; each multiplies the triangle count by 4).
    pub subdivisions: u32,
    /// Recompute smooth vertex normals from the displaced surface (Painter's "Recompute vertex
    /// normals"); otherwise the original normals are kept.
    pub recompute_normals: bool,
}

impl Default for Displacement {
    fn default() -> Self {
        Self { scale: 0.02, subdivisions: 1, recompute_normals: true }
    }
}

/// What File > Export mesh writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshExport {
    pub format: SceneFormat,
    /// `None` = without displacement (the mesh as modelled).
    pub displacement: Option<Displacement>,
}

/// Highest subdivision level offered (4 levels = 256 triangles per input triangle).
pub const MAX_SUBDIVISIONS: u32 = 4;

/// Split every triangle into four at its edge midpoints. Edge midpoints are shared between the
/// two triangles of an edge, so the surface stays closed; submesh ranges scale by four.
pub fn subdivide(mesh: &Mesh) -> Mesh {
    let mut out = mesh.clone();
    out.indices.clear();
    out.tri_material.clear();
    out.tri_object.clear();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut mid = |out: &mut Mesh, a: u32, b: u32| -> u32 {
        let key = (a.min(b), a.max(b));
        *mids.entry(key).or_insert_with(|| {
            let (a, b) = (a as usize, b as usize);
            out.positions.push((mesh.positions[a] + mesh.positions[b]) * 0.5);
            if mesh.normals.len() == mesh.positions.len() {
                out.normals.push(((mesh.normals[a] + mesh.normals[b]) * 0.5).normalize_or(mesh.normals[a]));
            }
            if mesh.uvs.len() == mesh.positions.len() {
                out.uvs.push((mesh.uvs[a] + mesh.uvs[b]) * 0.5);
            }
            if mesh.tangents.len() == mesh.positions.len() {
                let (ta, tb) = (mesh.tangents[a], mesh.tangents[b]);
                let xyz = ((ta.truncate() + tb.truncate()) * 0.5).normalize_or(ta.truncate());
                out.tangents.push(Vec4::new(xyz.x, xyz.y, xyz.z, ta.w));
            }
            (out.positions.len() - 1) as u32
        })
    };
    for t in 0..mesh.triangle_count() {
        let [a, b, c] = mesh.tri(t);
        let (ab, bc, ca) = (mid(&mut out, a, b), mid(&mut out, b, c), mid(&mut out, c, a));
        for tri in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
            out.indices.extend_from_slice(&tri);
            out.tri_material.push(mesh.tri_material.get(t).copied().unwrap_or(0));
            out.tri_object.push(mesh.tri_object.get(t).copied().flatten());
        }
    }
    out.submeshes = mesh.submeshes.iter().map(|s| Submesh { material: s.material, first_triangle: s.first_triangle * 4, triangle_count: s.triangle_count * 4 }).collect();
    out
}

/// Bilinear sample of a one-component image at `uv` (wrapping into the 0..1 tile).
fn sample(img: &TiledImage, uv: Vec2) -> f32 {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let u = uv.x - uv.x.floor();
    let v = uv.y - uv.y.floor();
    let (x, y) = ((u * w - 0.5).clamp(0.0, w - 1.0), (v * h - 0.5).clamp(0.0, h - 1.0));
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (tx, ty) = (x.fract(), y.fract());
    let at = |xx: u32, yy: u32| img.get(xx, yy)[0];
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    top + (bottom - top) * ty
}

/// The Height composite for a vertex of material `slot` at `uv`: the slot's set. A UDIM project
/// gives every tile its own slot with UVs moved into 0..1, so the slot alone names the set
/// (QA 2026-11: the tile was worked out from the UVs, always 1001 there, and every other tile
/// stayed flat). Only a slot shared by several sets falls back to the tile under `uv`.
fn height_image<'a>(sets: &'a [ExportTextureSet], slot: u32, uv: Vec2) -> Option<&'a TiledImage> {
    let of_slot: Vec<&ExportTextureSet> = sets.iter().filter(|s| s.material_slot == slot).collect();
    let set = match of_slot.as_slice() {
        [] => return None,
        [one] => *one,
        several => {
            let tile = subcraft_mesh::regroup::udim_number([uv.x.floor() as i32, uv.y.floor() as i32]);
            several.iter().find(|s| s.udim == Some(tile)).or_else(|| several.iter().find(|s| s.udim.is_none() || s.udim == Some(1001))).copied()?
        }
    };
    set.channels.get(&ChannelKind::Height)
}

/// Smooth normals of `mesh`: area-weighted face normals summed per position, so vertices split
/// along UV seams share one normal.
pub fn smooth_normals(mesh: &Mesh) -> Vec<Vec3> {
    let key = |p: Vec3| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
    let mut acc: HashMap<(u32, u32, u32), Vec3> = HashMap::new();
    for t in 0..mesh.triangle_count() {
        let [a, b, c] = mesh.tri_positions(t);
        let n = (b - a).cross(c - a); // length = 2 × area
        for p in [a, b, c] {
            *acc.entry(key(p)).or_insert(Vec3::ZERO) += n;
        }
    }
    mesh.positions
        .iter()
        .enumerate()
        .map(|(i, p)| acc.get(&key(*p)).copied().unwrap_or(Vec3::ZERO).normalize_or(mesh.normals.get(i).copied().unwrap_or(Vec3::Y)))
        .collect()
}

/// `mesh` subdivided and displaced by each texture set's Height channel.
pub fn displaced(mesh: &Mesh, sets: &[ExportTextureSet], d: &Displacement) -> Mesh {
    let mut out = mesh.clone();
    for _ in 0..d.subdivisions.min(MAX_SUBDIVISIONS) {
        out = subdivide(&out);
    }
    // Material slot of each vertex (from the first triangle using it).
    let mut slot = vec![None; out.positions.len()];
    for t in 0..out.triangle_count() {
        for v in out.tri(t) {
            slot[v as usize].get_or_insert(out.tri_material.get(t).copied().unwrap_or(0));
        }
    }
    let normals = if out.normals.len() == out.positions.len() { out.normals.clone() } else { smooth_normals(&out) };
    for (i, p) in out.positions.iter_mut().enumerate() {
        let (Some(slot), Some(uv)) = (slot[i], out.uvs.get(i).copied()) else { continue };
        if let Some(img) = height_image(sets, slot, uv) {
            *p += normals[i] * sample(img, uv) * d.scale;
        }
    }
    if d.recompute_normals {
        out.normals = smooth_normals(&out);
        // The tangents still followed the flat surface: glTF wrote TANGENTs no longer
        // perpendicular to NORMAL, skewing normal-mapped shading on slopes (QA 2026-11).
        // Gram-Schmidt them against the new normals, keeping their handedness.
        if out.tangents.len() == out.normals.len() {
            for (t, n) in out.tangents.iter_mut().zip(&out.normals) {
                let v = t.truncate();
                let ortho = (v - *n * n.dot(v)).normalize_or(n.any_orthonormal_vector());
                *t = ortho.extend(if t.w < 0.0 { -1.0 } else { 1.0 });
            }
        }
    }
    out.bounds = Aabb::from_points(out.positions.iter().copied());
    out
}

/// Write the mesh (no textures) into `dir`; returns the written files (mesh file first).
pub fn export_mesh(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], options: &MeshExport, dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    if dir.as_os_str().is_empty() {
        return Err(ExportError::Folder(dir.to_path_buf(), std::io::Error::new(std::io::ErrorKind::InvalidInput, "choose an export folder")));
    }
    subcraft_core::vfs::create_dir_all(dir).map_err(|e| ExportError::Folder(dir.to_path_buf(), e))?;
    let shaped;
    let mesh = match &options.displacement {
        Some(d) => {
            shaped = displaced(mesh, sets, d);
            &shaped
        }
        None => mesh,
    };
    let mats: Vec<SceneMaterial> = mesh.materials.iter().map(|m| SceneMaterial { name: sanitize(&m.name), ..Default::default() }).collect();
    let stem = sanitize(mesh_name);
    let file = format!("{stem}.{}", options.format.extension());
    let files: Vec<(String, Vec<u8>)> = match options.format {
        SceneFormat::ObjMtl => {
            let mtl = format!("{stem}.mtl");
            vec![(file, obj_text(mesh, &mtl).into_bytes()), (mtl, mtl_text(mesh, &mats).into_bytes())]
        }
        SceneFormat::Gltf => {
            let bin = format!("{stem}.bin");
            let (json, data) = gltf_files(mesh, &mats, &bin);
            vec![(file, json.into_bytes()), (bin, data)]
        }
        SceneFormat::Glb => vec![(file, glb_bytes(mesh, &mats))],
        SceneFormat::Usda => vec![(file, usda_text(mesh, &mats).into_bytes())],
    };
    let mut written = Vec::new();
    for (name, bytes) in files {
        let path = dir.join(&name);
        subcraft_core::vfs::write(&path, &bytes).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use subcraft_mesh::primitives;

    #[test]
    fn subdivision_quadruples_triangles_and_keeps_the_surface_closed() {
        let mesh = primitives::plane(2.0, 1);
        let s = subdivide(&mesh);
        assert_eq!(s.triangle_count(), mesh.triangle_count() * 4);
        assert_eq!(s.submeshes.iter().map(|m| m.triangle_count).sum::<u32>() as usize, s.triangle_count());
        assert_eq!(s.tri_material.len(), s.triangle_count());
        // A shared edge gets one midpoint: 4 corners + 5 edges of two triangles = 9 vertices.
        assert_eq!(s.positions.len(), mesh.positions.len() + 5);
        assert!(s.validate().is_empty(), "{:?}", s.validate());
        // Same area before and after.
        let area = |m: &Mesh| (0..m.triangle_count()).map(|t| { let [a, b, c] = m.tri_positions(t); (b - a).cross(c - a).length() * 0.5 }).sum::<f32>();
        assert!((area(&s) - area(&mesh)).abs() < 1e-4);
    }

    #[test]
    fn displacement_moves_vertices_along_normals_by_height_times_scale() {
        let mesh = primitives::plane(2.0, 1);
        let mut ch = BTreeMap::new();
        ch.insert(ChannelKind::Height, TiledImage::new(16, 16, 1, [0.5; 4]));
        let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
        let d = Displacement { scale: 0.2, subdivisions: 2, recompute_normals: true };
        let out = displaced(&mesh, &sets, &d);
        assert_eq!(out.triangle_count(), mesh.triangle_count() * 16);
        for (p, n) in out.positions.iter().zip(&out.normals) {
            let base = mesh.normals[0];
            // A flat plane with constant height: every vertex moves by 0.1 along the plane normal.
            assert!((p.dot(base) - 0.1).abs() < 1e-5, "{p:?}");
            assert!((n.dot(base) - 1.0).abs() < 1e-5, "recomputed normals stay flat");
        }
        assert!((out.bounds.min.dot(mesh.normals[0]) - 0.1).abs() < 1e-5 || (out.bounds.max.dot(mesh.normals[0]) - 0.1).abs() < 1e-5);
        // QA 2026-11: a UDIM tile other than 1001 (its own slot, UVs in 0..1) is displaced too.
        let tile = [ExportTextureSet { name: "P.1002", material_slot: 0, udim: Some(1002), channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
        let moved = displaced(&mesh, &tile, &Displacement { subdivisions: 0, ..d });
        assert!(moved.positions.iter().all(|p| (p.dot(mesh.normals[0]) - 0.1).abs() < 1e-5), "tile 1002 displaced");
        // QA 2026-11: on a slope the recomputed normals tilt; the tangents follow them.
        let mut ramp = TiledImage::new(16, 16, 1, [0.0; 4]);
        for y in 0..16 {
            for x in 0..16 {
                ramp.set(x, y, [x as f32 / 15.0, 1.0, 0.0, 0.0]);
            }
        }
        let mut sloped = BTreeMap::new();
        sloped.insert(ChannelKind::Height, ramp);
        let slope = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &sloped, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
        let tilted = displaced(&mesh, &slope, &Displacement { scale: 1.0, subdivisions: 2, recompute_normals: true });
        assert_eq!(tilted.tangents.len(), tilted.normals.len());
        assert!(tilted.normals.iter().any(|n| (n.dot(mesh.normals[0]) - 1.0).abs() > 1e-3), "the slope tilts the normals");
        for (t, n) in tilted.tangents.iter().zip(&tilted.normals) {
            assert!(t.truncate().dot(*n).abs() < 1e-4 && (t.truncate().length() - 1.0).abs() < 1e-4, "{t:?} vs {n:?}");
        }
        // No Height channel: nothing moves.
        let empty = BTreeMap::new();
        let none = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &empty, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
        let flat = displaced(&mesh, &none, &Displacement { subdivisions: 0, ..d });
        assert_eq!(flat.positions, mesh.positions);
    }

    #[test]
    fn mesh_files_are_written_without_textures() {
        let mesh = primitives::sample_crate();
        let dir = std::env::temp_dir().join(format!("subcraft-mesh-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for format in SceneFormat::ALL {
            let files = export_mesh(&mesh, "Crate", &[], &MeshExport { format, displacement: None }, &dir).unwrap();
            assert!(files[0].extension().is_some_and(|e| e == format.extension()), "{files:?}");
            let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
            assert!(names.iter().all(|n| !n.ends_with(".png")), "mesh only: {names:?}");
        }
        let obj = std::fs::read_to_string(dir.join("Crate.obj")).unwrap();
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), mesh.triangle_count(), "triangulated");
        let glb = std::fs::read(dir.join("Crate.glb")).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        assert!(export_mesh(&mesh, "Crate", &[], &MeshExport { format: SceneFormat::Glb, displacement: None }, Path::new("")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
