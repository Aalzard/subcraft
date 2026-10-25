//! Mesh importers (OBJ, glTF/GLB, FBX). Each normalizes into [`MeshBuilder`] corners using the
//! canonical conventions; importer data structures never escape this module.

use std::path::Path;

use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::mesh::{BuildError, BuildStats, Corner, Mesh, MeshBuilder};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("unsupported mesh format '.{0}' (supported: obj, gltf, glb, fbx)")]
    UnsupportedFormat(String),
    #[error("failed to read OBJ: {0}")]
    Obj(String),
    #[error("failed to read glTF: {0}")]
    Gltf(String),
    #[error("failed to read FBX: {0}")]
    Fbx(String),
    #[error("assbin: {0}")]
    Assbin(String),
    #[error(transparent)]
    Build(#[from] BuildError),
}

#[derive(Debug)]
pub struct ImportReport {
    pub mesh: Mesh,
    pub stats: BuildStats,
    pub warnings: Vec<String>,
}

pub const SUPPORTED_EXTENSIONS: [&str; 4] = ["obj", "gltf", "glb", "fbx"];

pub fn import_mesh(path: &Path) -> Result<ImportReport, ImportError> {
    import(path, false)
}

/// Import a high-poly ray-cast source, accepting meshes without UV coordinates.
pub fn import_mesh_for_baking(path: &Path) -> Result<ImportReport, ImportError> {
    import(path, true)
}

fn import(path: &Path, bake_source: bool) -> Result<ImportReport, ImportError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Mesh").to_string();
    let mut b = MeshBuilder::new(name);
    let mut warnings = Vec::new();
    match ext.as_str() {
        "obj" => load_obj(path, &mut b, &mut warnings)?,
        "gltf" | "glb" => load_gltf(path, &mut b, &mut warnings)?,
        "fbx" => load_fbx(path, &mut b, &mut warnings)?,
        other => return Err(ImportError::UnsupportedFormat(other.to_string())),
    }
    let stats_pre = b.stats;
    let mesh = if bake_source { b.build_for_baking()? } else { b.build()? };
    let mut stats = stats_pre;
    stats.generated_normals |= mesh.normals.is_empty();
    if stats.degenerate_dropped > 0 {
        warnings.push(format!("dropped {} degenerate triangles", stats.degenerate_dropped));
    }
    if stats.non_finite_dropped > 0 {
        warnings.push(format!("dropped {} triangles with invalid coordinates", stats.non_finite_dropped));
    }
    if stats.missing_uv_triangles > 0 {
        warnings.push(format!("{} triangles have no UVs and cannot be painted", stats.missing_uv_triangles));
    }
    let problems = mesh.validate();
    if !problems.is_empty() {
        return Err(ImportError::Build(BuildError::NoTriangles)).inspect_err(|_| log::error!("{problems:?}"));
    }
    Ok(ImportReport { mesh, stats, warnings })
}

fn load_obj(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> Result<(), ImportError> {
    let opts = tobj::LoadOptions { triangulate: true, single_index: true, ignore_points: true, ignore_lines: true };
    // Read through `vfs`; `.mtl` files resolve next to the OBJ.
    let bytes = subcraft_core::vfs::read(path).map_err(|e| ImportError::Obj(format!("{}: {e}", path.display())))?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let (models, materials) = tobj::load_obj_buf(&mut std::io::Cursor::new(bytes), &opts, |mtl| {
        let mtl_bytes = subcraft_core::vfs::read(&base.join(mtl)).map_err(|_| tobj::LoadError::OpenFileFailed)?;
        tobj::load_mtl_buf(&mut std::io::Cursor::new(mtl_bytes))
    })
    .map_err(|e| ImportError::Obj(e.to_string()))?;
    let materials = match materials {
        Ok(m) => m,
        Err(e) => {
            warnings.push(format!("material library not loaded ({e}); using default material"));
            Vec::new()
        }
    };
    for model in &models {
        b.object(&model.name);
        let m = &model.mesh;
        let mat_name = match m.material_id {
            Some(id) if id < materials.len() => materials[id].name.clone(),
            Some(id) => format!("material_{id}"),
            None => "DefaultMaterial".to_string(),
        };
        let mat = b.material(&mat_name);
        let has_n = !m.normals.is_empty();
        let has_uv = !m.texcoords.is_empty();
        let corner = |i: u32| {
            let i = i as usize;
            Corner {
                position: Vec3::new(m.positions[3 * i], m.positions[3 * i + 1], m.positions[3 * i + 2]),
                normal: has_n.then(|| Vec3::new(m.normals[3 * i], m.normals[3 * i + 1], m.normals[3 * i + 2])),
                // OBJ UVs have their origin at the bottom-left.
                uv: has_uv.then(|| Vec2::new(m.texcoords[2 * i], 1.0 - m.texcoords[2 * i + 1])),
            }
        };
        for tri in m.indices.chunks_exact(3) {
            b.push_triangle(mat, [corner(tri[0]), corner(tri[1]), corner(tri[2])]);
        }
    }
    Ok(())
}

fn load_gltf(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> Result<(), ImportError> {
    let (doc, buffers, _images) = gltf::import(path).map_err(|e| ImportError::Gltf(e.to_string()))?;
    let scene = doc.default_scene().or_else(|| doc.scenes().next());
    let roots: Vec<gltf::Node> = match &scene {
        Some(s) => s.nodes().collect(),
        None => doc.nodes().collect(),
    };
    let mut stack: Vec<(gltf::Node, Mat4)> = roots.into_iter().map(|n| (n, Mat4::IDENTITY)).collect();
    let mut skipped_modes = 0usize;
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            b.object(node.name().or_else(|| mesh.name()).unwrap_or("Object"));
            let normal_m = Mat3::from_mat4(world).inverse().transpose();
            let flip = world.determinant() < 0.0;
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    skipped_modes += 1;
                    continue;
                }
                let mat_name = match (prim.material().name(), prim.material().index()) {
                    (Some(n), _) => n.to_string(),
                    (None, Some(i)) => format!("material_{i}"),
                    (None, None) => "DefaultMaterial".to_string(),
                };
                let mat = b.material(&mat_name);
                let reader = prim.reader(|buf| Some(&buffers[buf.index()]));
                let Some(pos) = reader.read_positions() else { continue };
                let pos: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from(p))).collect();
                let nrm: Option<Vec<Vec3>> =
                    reader.read_normals().map(|it| it.map(|n| (normal_m * Vec3::from(n)).normalize_or_zero()).collect());
                let uv: Option<Vec<Vec2>> = reader.read_tex_coords(0).map(|t| t.into_f32().map(Vec2::from).collect());
                let idx: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..pos.len() as u32).collect(),
                };
                let corner = |i: u32| {
                    let i = i as usize;
                    Corner {
                        position: pos[i],
                        normal: nrm.as_ref().map(|n| n[i]),
                        uv: uv.as_ref().map(|u| u[i]),
                    }
                };
                for tri in idx.chunks_exact(3) {
                    if tri.iter().any(|&i| i as usize >= pos.len()) {
                        continue;
                    }
                    let c = [corner(tri[0]), corner(tri[1]), corner(tri[2])];
                    b.push_triangle(mat, if flip { [c[0], c[2], c[1]] } else { c });
                }
            }
        }
        for child in node.children() {
            stack.push((child, world));
        }
    }
    if skipped_modes > 0 {
        warnings.push(format!("skipped {skipped_modes} non-triangle primitives"));
    }
    Ok(())
}

fn load_fbx(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> Result<(), ImportError> {
    let opts = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        generate_missing_normals: true,
        ..Default::default()
    };
    let path_str = path.to_str().ok_or_else(|| ImportError::Fbx("non UTF-8 path".into()))?;
    let scene = ufbx::load_file(path_str, opts).map_err(|e| ImportError::Fbx(format!("{:?}", e.description)))?;
    let mut tri_indices: Vec<u32> = Vec::new();
    for node in scene.nodes.iter() {
        let Some(mesh) = node.mesh.as_ref() else { continue };
        b.object(&node.element.name);
        let m = &node.geometry_to_world;
        let nm = ufbx::matrix_for_normals(m);
        let to_v3 = |v: ufbx::Vec3| Vec3::new(v.x as f32, v.y as f32, v.z as f32);
        let has_uv = mesh.vertex_uv.exists;
        let has_n = mesh.vertex_normal.exists;
        if !has_uv {
            warnings.push(format!("FBX mesh '{}' has no UVs", node.element.name));
        }
        let flip = {
            let det = m.m00 * (m.m11 * m.m22 - m.m12 * m.m21) - m.m01 * (m.m10 * m.m22 - m.m12 * m.m20)
                + m.m02 * (m.m10 * m.m21 - m.m11 * m.m20);
            det < 0.0
        };
        for (fi, face) in mesh.faces.iter().enumerate() {
            let mat_name = if mesh.face_material.is_empty() || mesh.materials.is_empty() {
                "DefaultMaterial".to_string()
            } else {
                let mi = mesh.face_material[fi] as usize;
                mesh.materials.get(mi).map(|mat| mat.element.name.to_string()).unwrap_or_else(|| format!("material_{mi}"))
            };
            let mat = b.material(&mat_name);
            let n = ufbx::triangulate_face_vec(&mut tri_indices, mesh, *face);
            for t in 0..n as usize {
                let corner = |ix: u32| {
                    let ix = ix as usize;
                    Corner {
                        position: to_v3(ufbx::transform_position(m, mesh.vertex_position[ix])),
                        normal: has_n.then(|| to_v3(ufbx::transform_direction(&nm, mesh.vertex_normal[ix])).normalize_or_zero()),
                        // FBX UVs have their origin at the bottom-left.
                        uv: has_uv.then(|| {
                            let uv = mesh.vertex_uv[ix];
                            Vec2::new(uv.x as f32, 1.0 - uv.y as f32)
                        }),
                    }
                };
                let c = [corner(tri_indices[3 * t]), corner(tri_indices[3 * t + 1]), corner(tri_indices[3 * t + 2])];
                b.push_triangle(mat, if flip { [c[0], c[2], c[1]] } else { c });
            }
        }
    }
    Ok(())
}

/// Write a mesh as Wavefront OBJ (+MTL with material names). Used for test fixtures and
/// "export mesh" round trips.
pub fn write_obj(mesh: &Mesh, path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let mtl_path = path.with_extension("mtl");
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(f, "# SubCraft OBJ export")?;
    writeln!(f, "mtllib {}", mtl_path.file_name().and_then(|s| s.to_str()).unwrap_or("mesh.mtl"))?;
    for p in &mesh.positions {
        writeln!(f, "v {} {} {}", p.x, p.y, p.z)?;
    }
    for uv in &mesh.uvs {
        writeln!(f, "vt {} {}", uv.x, 1.0 - uv.y)?;
    }
    for n in &mesh.normals {
        writeln!(f, "vn {} {} {}", n.x, n.y, n.z)?;
    }
    for s in &mesh.submeshes {
        writeln!(f, "usemtl {}", mesh.materials[s.material as usize].name)?;
        let mut last_object = None;
        for t in s.first_triangle..s.first_triangle + s.triangle_count {
            let object = mesh.tri_object.get(t as usize).copied().flatten();
            if t == s.first_triangle || object != last_object {
                writeln!(f, "o {}", object.map_or(mesh.name.as_str(), |o| mesh.objects[o as usize].name.as_str()))?;
                last_object = object;
            }
            let [a, b, c] = mesh.tri(t as usize).map(|i| i + 1);
            writeln!(f, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
        }
    }
    let mut m = std::io::BufWriter::new(std::fs::File::create(&mtl_path)?);
    for mat in &mesh.materials {
        writeln!(m, "newmtl {}\nKd 0.8 0.8 0.8\n", mat.name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-import-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn high_poly_import_accepts_no_uvs_but_paint_import_still_requires_them() {
        let dir = tmp("high-no-uv");
        let path = dir.join("high.obj");
        std::fs::write(&path, "v -1 0 -1\nv -1 0 1\nv 1 0.2 -1\nf 1 2 3\n").unwrap();
        assert!(matches!(import_mesh(&path), Err(ImportError::Build(BuildError::NoUvs))));
        let high = import_mesh_for_baking(&path).unwrap();
        assert_eq!(high.mesh.triangle_count(), 1);
        assert_eq!(high.stats.missing_uv_triangles, 1);
        assert!(high.mesh.validate().is_empty());
        assert!(high.mesh.normals.iter().all(|n| n.is_finite() && (n.length() - 1.).abs() < 1e-5));
        let bvh = crate::Bvh::build(&high.mesh);
        assert!(bvh.intersect(Vec3::new(-0.5, 1., -0.5), -Vec3::Y, 0., 2.).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn obj_roundtrip_preserves_materials_and_uvs() {
        let src = primitives::sample_crate();
        let dir = tmp("obj");
        let path = dir.join("crate.obj");
        write_obj(&src, &path).unwrap();
        let rep = import_mesh(&path).unwrap();
        let m = rep.mesh;
        assert_eq!(m.materials.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Body", "Trim"]);
        assert_eq!(m.triangle_count(), src.triangle_count());
        // UV orientation survives the OBJ v-flip round trip.
        let mut a: Vec<_> = src.uvs.iter().map(|u| ((u.x * 1e4).round() as i64, (u.y * 1e4).round() as i64)).collect();
        let mut b: Vec<_> = m.uvs.iter().map(|u| ((u.x * 1e4).round() as i64, (u.y * 1e4).round() as i64)).collect();
        a.sort();
        a.dedup();
        b.sort();
        b.dedup();
        assert_eq!(a, b);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gltf_minimal_triangle() {
        // One triangle with positions + uvs in an embedded data URI buffer.
        let positions: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let uvs: [f32; 6] = [0.0, 1.0, 1.0, 1.0, 0.0, 0.0];
        let mut bytes: Vec<u8> = Vec::new();
        for v in positions.iter().chain(uvs.iter()) {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let b64 = base64_encode(&bytes);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"translation":[0,0,1]}}],
            "materials":[{{"name":"Painted"}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"material":0}}]}}],
            "buffers":[{{"byteLength":{len},"uri":"data:application/octet-stream;base64,{b64}"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                         {{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}}]}}"#,
            len = bytes.len()
        );
        let dir = tmp("gltf");
        let path = dir.join("tri.gltf");
        std::fs::write(&path, json).unwrap();
        let rep = import_mesh(&path).unwrap();
        assert_eq!(rep.mesh.triangle_count(), 1);
        assert_eq!(rep.mesh.materials[0].name, "Painted");
        assert!((rep.mesh.bounds.min.z - 1.0).abs() < 1e-6, "node transform applied");
        assert!(rep.mesh.normals.iter().all(|n| (n.z - 1.0).abs() < 1e-5));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unsupported_extension() {
        assert!(matches!(import_mesh(Path::new("x.stl")), Err(ImportError::UnsupportedFormat(_))));
    }

    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            s.push(T[(n >> 18) as usize & 63] as char);
            s.push(T[(n >> 12) as usize & 63] as char);
            s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
            s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
        }
        s
    }

    #[test]
    fn obj_keeps_object_names_when_material_sort_changes_triangle_order() {
        let dir = tmp("object-names");
        let path = dir.join("parts.obj");
        std::fs::write(&path, "mtllib parts.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 2 0 0\nv 3 0 0\nv 2 1 0\nvt 0 0\nvt 1 0\nvt 0 1\no Blade\nusemtl Steel\nf 1/1 2/2 3/3\no Handle\nusemtl Wood\nf 4/1 5/2 6/3\n").unwrap();
        std::fs::write(dir.join("parts.mtl"), "newmtl Steel\nKd 1 1 1\nnewmtl Wood\nKd 1 1 1\n").unwrap();
        let mesh = import_mesh(&path).unwrap().mesh;
        assert_eq!(mesh.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["Blade", "Handle"]);
        for t in 0..mesh.triangle_count() {
            assert_eq!(mesh.objects[mesh.tri_object[t].unwrap() as usize].name, if mesh.tri_positions(t)[0].x < 1.0 { "Blade" } else { "Handle" });
        }
        assert_ne!(mesh.objects[0].id, mesh.objects[1].id);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
