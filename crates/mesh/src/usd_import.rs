//! USD importer (`.usd`, `.usda`, `.usdc`, `.usdz`) on the pure-Rust `openusd` crate. Walks the composed stage
//! (references, payloads, sublayers, variants and instances are resolved by the library) and emits one
//! object per `Mesh` prim with the prim's world transform baked in. Static geometry only: time samples are
//! read at the earliest sample, skinning, animation, point instancers and shading networks are not imported.

use std::collections::HashSet;
use std::path::Path;

use glam::{Mat4, Vec2, Vec3};
use openusd::{sdf, usd};
use openusd_schemas::geom::XformCache;

use crate::import::ImportError;
use crate::mesh::{Corner, MeshBuilder};

type R<T> = Result<T, ImportError>;

/// Deepest prim nesting the walk accepts (the same limit as the Alembic reader).
const MAX_DEPTH: usize = 512;

fn err(e: impl std::fmt::Display) -> ImportError {
    ImportError::Usd(e.to_string())
}

/// How a primvar/normal array maps onto the mesh.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Interp {
    Constant,
    Uniform,
    Vertex,
    FaceVarying,
}

struct Walk {
    xforms: XformCache,
    /// World-space fix-up for up axis and units, applied after the prim's own transform.
    root: Mat4,
    warnings: Vec<String>,
    meshes: usize,
    hidden: usize,
    instancers: usize,
    no_uv: Vec<String>,
    skipped_bad: usize,
    seen_names: HashSet<String>,
}

/// Deepest `{ ... }` nesting of a text USD file (strings and `#` comments skipped), or `None` for
/// a binary file.
fn text_nesting(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(b"#usda") {
        return None;
    }
    let (mut depth, mut deepest, mut i) = (0usize, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' { i += 1; }
                    i += 1;
                }
            }
            b'#' => while i < bytes.len() && bytes[i] != b'\n' { i += 1; },
            b'{' => { depth += 1; deepest = deepest.max(depth); }
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    Some(deepest)
}

pub fn load_usd(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> R<()> {
    let path_str = path.to_str().ok_or_else(|| ImportError::Usd("non UTF-8 path".into()))?;
    // The text parser recurses once per nested prim with no limit of its own, and a stack
    // overflow cannot be caught: it ends the whole app. Refuse such a file before parsing it
    // (the walk below has the same limit, but it only runs after the parser).
    if let Ok(bytes) = std::fs::read(path)
        && text_nesting(&bytes).is_some_and(|d| d > MAX_DEPTH)
    {
        return Err(ImportError::Usd(format!("prim tree is nested deeper than {MAX_DEPTH} levels")));
    }
    let stage = usd::Stage::builder()
        .schema_registry(openusd_schemas::schema_registry())
        .open(path_str)
        .map_err(|e| ImportError::Usd(format!("{}: {e}", path.display())))?;
    for e in stage.composition_errors() {
        warnings.push(format!("USD composition: {e}"));
    }

    let up_z = matches!(stage.stage_metadata("upAxis").ok().flatten(), Some(sdf::Value::Token(t)) if t.as_str().eq_ignore_ascii_case("z"));
    // Root-layer field read: an unauthored `metersPerUnit` means meters here, not the 0.01 schema fallback.
    let mpu = match stage.field::<sdf::Value>(sdf::Path::abs_root(), "metersPerUnit").ok().flatten() {
        Some(sdf::Value::Double(d)) if d.is_finite() && d > 0.0 => d as f32,
        Some(sdf::Value::Float(d)) if d.is_finite() && d > 0.0 => d,
        _ => 1.0,
    };
    // SubCraft is +Y up in meters; Z-up USD maps (x, y, z) -> (x, z, -y).
    let up = if up_z { Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2) } else { Mat4::IDENTITY };
    let root = Mat4::from_scale(Vec3::splat(mpu)) * up;

    let mut w = Walk {
        xforms: XformCache::new(None),
        root,
        warnings: Vec::new(),
        meshes: 0,
        hidden: 0,
        instancers: 0,
        no_uv: Vec::new(),
        skipped_bad: 0,
        seen_names: HashSet::new(),
    };
    let pseudo_root = stage.prim("/").map_err(err)?;
    w.visit(&pseudo_root, b, None, true)?;

    if w.instancers > 0 {
        w.warnings.push(format!("skipped {} USD point instancer(s) (instancing is not supported)", w.instancers));
    }
    if w.hidden > 0 {
        w.warnings.push(format!("skipped {} invisible or guide/proxy USD prim(s)", w.hidden));
    }
    if w.skipped_bad > 0 {
        w.warnings.push(format!("skipped {} USD mesh(es) with inconsistent topology", w.skipped_bad));
    }
    for n in &w.no_uv {
        w.warnings.push(format!("USD mesh '{n}' has no UVs"));
    }
    if w.meshes == 0 {
        w.warnings.push("no Mesh prims found in the USD stage (only meshes are imported, not curves, volumes or instancers)".into());
    }
    w.warnings.push("USD materials are imported by name only; shading networks and textures are not read".into());
    warnings.extend(w.warnings);
    Ok(())
}

impl Walk {
    fn visit(&mut self, prim: &usd::Prim, b: &mut MeshBuilder, inherited_material: Option<String>, is_root: bool) -> R<()> {
        self.visit_at(prim, b, inherited_material, is_root, 0)
    }

    fn visit_at(&mut self, prim: &usd::Prim, b: &mut MeshBuilder, inherited_material: Option<String>, is_root: bool, depth: usize) -> R<()> {
        // The walk recurses once per nesting level; a hostile file with thousands of nested
        // prims would otherwise overflow the stack and abort the whole process.
        if depth > MAX_DEPTH {
            return Err(ImportError::Usd(format!("prim tree is nested deeper than {MAX_DEPTH} levels")));
        }
        let type_name = prim.type_name().map_err(err)?.map(|t| t.as_str().to_string()).unwrap_or_default();
        let mut material = inherited_material;
        if !is_root {
            if !prim.is_active().map_err(err)? || prim.is_abstract().map_err(err)? || !prim.is_defined().map_err(err)? {
                return Ok(());
            }
            if let Some(m) = bound_material(prim) {
                material = Some(m);
            }
            if token_attr(prim, "visibility").as_deref() == Some("invisible")
                || matches!(token_attr(prim, "purpose").as_deref(), Some("guide" | "proxy"))
            {
                if type_name == "Mesh" || type_name == "Xform" || type_name == "Scope" {
                    self.hidden += 1;
                }
                return Ok(());
            }
            match type_name.as_str() {
                "Mesh" => {
                    self.import_mesh(prim, b, material.clone())?;
                    // Mesh prims may nest children (GeomSubsets are consumed above, other meshes follow).
                }
                "PointInstancer" => {
                    self.instancers += 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        for child in prim.children().map_err(err)? {
            self.visit_at(&child, b, material.clone(), false, depth + 1)?;
        }
        Ok(())
    }

    fn import_mesh(&mut self, prim: &usd::Prim, b: &mut MeshBuilder, material: Option<String>) -> R<()> {
        let name = prim.path().name().unwrap_or("Mesh").to_string();
        let Some(points) = read_vec3(prim, "points") else { return Ok(()) };
        let counts = read_ints(prim, "faceVertexCounts").unwrap_or_default();
        let indices = read_ints(prim, "faceVertexIndices").unwrap_or_default();
        let corner_total: i64 = counts.iter().map(|&c| c as i64).sum();
        if points.is_empty()
            || counts.iter().any(|&c| c < 3)
            || corner_total != indices.len() as i64
            || indices.iter().any(|&i| i < 0 || i as usize >= points.len())
        {
            self.skipped_bad += 1;
            return Ok(());
        }

        let world = {
            let m = self.xforms.local_to_world_transform(prim).map_err(err)?;
            let rows: [f32; 16] = std::array::from_fn(|i| m.0[i] as f32);
            // USD matrices are row-major for row vectors; glam's column array of the same data is the transpose.
            self.root * Mat4::from_cols_array(&rows)
        };
        let normal_m = glam::Mat3::from_mat4(world).inverse().transpose();
        let left_handed = token_attr(prim, "orientation").as_deref() == Some("leftHanded");
        let flip = left_handed ^ (world.determinant() < 0.0);

        let nrm = read_vec3(prim, "normals")
            .map(|v| (v, interp_of(prim, "normals")))
            .or_else(|| read_vec3(prim, "primvars:normals").map(|v| (v, interp_of(prim, "primvars:normals"))));
        let nrm = nrm.map(|(v, i)| (v, i, read_ints(prim, "primvars:normals:indices")));
        let uv = read_uvs(prim);

        let nfaces = counts.len();
        let resolve = |len: usize, interp: Interp, idx: Option<&Vec<i32>>, face: usize, corner: usize, point: usize| -> Option<usize> {
            let slot = match interp {
                Interp::Constant => 0,
                Interp::Uniform => face,
                Interp::Vertex => point,
                Interp::FaceVarying => corner,
            };
            let slot = match idx {
                Some(ix) => *ix.get(slot)? as usize,
                None => slot,
            };
            (slot < len).then_some(slot)
        };

        // Per-face material: GeomSubsets with a material binding override the mesh-level one.
        let mut face_material: Vec<Option<String>> = vec![None; nfaces];
        for child in prim.children().map_err(err)? {
            if child.type_name().map_err(err)?.as_ref().map(|t| t.as_str()) != Some("GeomSubset") {
                continue;
            }
            if token_attr(&child, "elementType").as_deref().is_some_and(|t| t != "face") {
                continue;
            }
            let Some(m) = bound_material(&child) else { continue };
            for f in read_ints(&child, "indices").unwrap_or_default() {
                if let Some(slot) = face_material.get_mut(f as usize) {
                    *slot = Some(m.clone());
                }
            }
        }

        let display = self.unique_name(&name);
        b.object(&display);
        let default_mat = material.unwrap_or_else(|| "DefaultMaterial".into());
        if uv.is_none() {
            self.no_uv.push(display.clone());
        }
        self.meshes += 1;

        let mut corner0 = 0usize;
        for (fi, &n) in counts.iter().enumerate() {
            let n = n as usize;
            let mat = b.material(face_material[fi].as_deref().unwrap_or(&default_mat));
            let corner_at = |k: usize| -> Corner {
                let corner = corner0 + k;
                let point = indices[corner] as usize;
                let p = points[point];
                let position = world.transform_point3(p);
                let normal = nrm.as_ref().and_then(|(v, interp, ix)| {
                    let s = resolve(v.len(), *interp, ix.as_ref(), fi, corner, point)?;
                    Some((normal_m * v[s]).normalize_or_zero())
                });
                let uv = uv.as_ref().and_then(|(v, interp, ix)| {
                    let s = resolve(v.len(), *interp, ix.as_ref(), fi, corner, point)?;
                    // USD's st origin is bottom-left, like OBJ.
                    Some(Vec2::new(v[s].x, 1.0 - v[s].y))
                });
                Corner { position, normal, uv }
            };
            // Fan triangulation (exact for convex polygons).
            for k in 1..n - 1 {
                let c = [corner_at(0), corner_at(k), corner_at(k + 1)];
                b.push_triangle(mat, if flip { [c[0], c[2], c[1]] } else { c });
            }
            corner0 += n;
        }
        Ok(())
    }

    fn unique_name(&mut self, name: &str) -> String {
        if self.seen_names.insert(name.to_string()) {
            return name.to_string();
        }
        let mut i = 2;
        loop {
            let candidate = format!("{name}_{i}");
            if self.seen_names.insert(candidate.clone()) {
                return candidate;
            }
            i += 1;
        }
    }
}

/// Default value, else the earliest time sample (animated files import their first frame).
fn value(prim: &usd::Prim, name: &str) -> Option<sdf::Value> {
    let attr = prim.attribute(name);
    attr.get::<sdf::Value>().ok().flatten().or_else(|| attr.get_at::<sdf::Value>(usd::TimeCode::EARLIEST).ok().flatten())
}

fn token_attr(prim: &usd::Prim, name: &str) -> Option<String> {
    match value(prim, name)? {
        sdf::Value::Token(t) => Some(t.as_str().to_string()),
        sdf::Value::String(s) => Some(s),
        _ => None,
    }
}

fn read_ints(prim: &usd::Prim, name: &str) -> Option<Vec<i32>> {
    match value(prim, name)? {
        sdf::Value::IntVec(v) => Some(v),
        sdf::Value::UintVec(v) => Some(v.into_iter().map(|x| x as i32).collect()),
        sdf::Value::Int64Vec(v) => Some(v.into_iter().map(|x| x as i32).collect()),
        _ => None,
    }
}

fn read_vec3(prim: &usd::Prim, name: &str) -> Option<Vec<Vec3>> {
    match value(prim, name)? {
        sdf::Value::Vec3fVec(v) => Some(v.into_iter().map(|p| Vec3::new(p.x, p.y, p.z)).collect()),
        sdf::Value::Vec3dVec(v) => Some(v.into_iter().map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32)).collect()),
        _ => None,
    }
}

fn read_vec2(prim: &usd::Prim, name: &str) -> Option<Vec<Vec2>> {
    match value(prim, name)? {
        sdf::Value::Vec2fVec(v) => Some(v.into_iter().map(|p| Vec2::new(p.x, p.y)).collect()),
        sdf::Value::Vec2dVec(v) => Some(v.into_iter().map(|p| Vec2::new(p.x as f32, p.y as f32)).collect()),
        sdf::Value::Vec2hVec(v) => Some(v.into_iter().map(|p| Vec2::new(f32::from(p.x), f32::from(p.y))).collect()),
        _ => None,
    }
}

fn interp_of(prim: &usd::Prim, attr: &str) -> Interp {
    let raw = prim.attribute(attr).get_metadata::<sdf::Value>("interpolation").ok().flatten();
    let s = match raw {
        Some(sdf::Value::Token(t)) => t.as_str().to_string(),
        Some(sdf::Value::String(s)) => s,
        // `normals` is vertex-interpolated unless authored otherwise; primvars default to constant.
        _ => String::new(),
    };
    match s.as_str() {
        "constant" => Interp::Constant,
        "uniform" => Interp::Uniform,
        "faceVarying" => Interp::FaceVarying,
        "vertex" | "varying" => Interp::Vertex,
        _ if attr == "normals" => Interp::Vertex,
        _ => Interp::Constant,
    }
}

/// The UV set: `primvars:st`, else the first other `texCoord2f`-looking primvar (`primvars:st0`, `primvars:UVMap`).
fn read_uvs(prim: &usd::Prim) -> Option<(Vec<Vec2>, Interp, Option<Vec<i32>>)> {
    let mut names = vec!["primvars:st".to_string(), "primvars:st0".to_string(), "primvars:uv".to_string(), "primvars:UVMap".to_string()];
    if let Ok(props) = prim.property_names() {
        for p in props {
            let p = p.as_str();
            if p.starts_with("primvars:") && !p.ends_with(":indices") && !names.iter().any(|n| n == p) {
                names.push(p.to_string());
            }
        }
    }
    for name in names {
        let Some(v) = read_vec2(prim, &name) else { continue };
        if v.is_empty() {
            continue;
        }
        let idx = read_ints(prim, &format!("{name}:indices"));
        return Some((v, interp_of(prim, &name), idx));
    }
    None
}

/// Name of the material a prim binds directly (`material:binding`), without its namespace path.
fn bound_material(prim: &usd::Prim) -> Option<String> {
    let targets = prim.relationship("material:binding").targets().ok()?;
    targets.first().and_then(|p| p.name()).map(str::to_string)
}
