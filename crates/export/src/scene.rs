//! Scene export: the mesh plus its textured PBR materials as OBJ+MTL, glTF, GLB or USDA.
//!
//! Textures come from the normal map exporter (same packing, padding and height-to-normal), so a
//! scene file always agrees with the texture files next to it. glTF and USD use the packed
//! occlusion/roughness/metallic map (R/G/B); OBJ uses separate maps because MTL has no channel
//! selection. Normals are OpenGL (+Y) everywhere, the glTF and USD convention.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use subcraft_image::FileFormat;
use subcraft_mesh::Mesh;

use crate::{ExportError, ExportSettings, ExportTemplate, ExportTextureSet, output_path, render_maps, sanitize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneFormat {
    ObjMtl,
    Gltf,
    Glb,
    Usda,
}

impl SceneFormat {
    pub const ALL: [SceneFormat; 4] = [SceneFormat::ObjMtl, SceneFormat::Gltf, SceneFormat::Glb, SceneFormat::Usda];

    pub fn extension(self) -> &'static str {
        match self {
            SceneFormat::ObjMtl => "obj",
            SceneFormat::Gltf => "gltf",
            SceneFormat::Glb => "glb",
            SceneFormat::Usda => "usda",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SceneFormat::ObjMtl => "OBJ + MTL",
            SceneFormat::Gltf => "glTF 2.0 (separate files)",
            SceneFormat::Glb => "GLB (single file)",
            SceneFormat::Usda => "USD (USDA, UsdPreviewSurface)",
        }
    }
}

/// One texture of a material: its file name next to the scene file and its PNG bytes.
#[derive(Clone, Debug, Default)]
pub struct SceneTexture {
    pub file: String,
    pub png: Vec<u8>,
}

/// Textures of one mesh material slot. `orm` packs occlusion (R), roughness (G), metallic (B).
#[derive(Clone, Debug, Default)]
pub struct SceneMaterial {
    pub name: String,
    pub base_color: Option<SceneTexture>,
    pub normal: Option<SceneTexture>,
    pub orm: Option<SceneTexture>,
    pub emissive: Option<SceneTexture>,
    pub roughness: Option<SceneTexture>,
    pub metallic: Option<SceneTexture>,
    pub occlusion: Option<SceneTexture>,
}

/// Triangles of one material slot with their own compact vertex arrays.
struct Part {
    material: usize,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tangents: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

fn parts(mesh: &Mesh) -> Vec<Part> {
    let mut out = Vec::new();
    for s in &mesh.submeshes {
        if s.triangle_count == 0 {
            continue;
        }
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut p = Part { material: s.material as usize, positions: vec![], normals: vec![], uvs: vec![], tangents: vec![], indices: vec![] };
        for t in s.first_triangle..s.first_triangle + s.triangle_count {
            for v in mesh.tri(t as usize) {
                let i = *remap.entry(v).or_insert_with(|| {
                    let vi = v as usize;
                    p.positions.push(mesh.positions[vi].to_array());
                    p.normals.push(mesh.normals.get(vi).map_or([0., 1., 0.], |n| n.to_array()));
                    p.uvs.push(mesh.uvs.get(vi).map_or([0., 0.], |u| u.to_array()));
                    p.tangents.push(mesh.tangents.get(vi).map_or([1., 0., 0., 1.], |t| t.to_array()));
                    (p.positions.len() - 1) as u32
                });
                p.indices.push(i);
            }
        }
        out.push(p);
    }
    out
}

fn material_name(mesh: &Mesh, slot: usize) -> String {
    sanitize(mesh.materials.get(slot).map_or("Material", |m| m.name.as_str()))
}

/// One valid, distinct prim name per material slot: two slots called "Material" (or names that
/// only differ in dropped characters) would otherwise merge into one USD prim.
fn usd_material_names(mesh: &Mesh, slots: usize) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    (0..slots)
        .map(|slot| {
            let base = usd_identifier(&material_name(mesh, slot));
            let mut name = base.clone();
            let mut n = 1;
            while !taken.insert(name.clone()) {
                n += 1;
                name = format!("{base}_{n}");
            }
            name
        })
        .collect()
}

// ───────────────────────────── OBJ + MTL ─────────────────────────────

/// One distinct OBJ / MTL material name per slot. Two slots with the same name (or names that
/// only differ in characters a file name cannot hold) used to share one `newmtl`, so the importing
/// program merged them and the second slot's textures were lost.
fn obj_material_names(mesh: &Mesh, slots: usize) -> Vec<String> {
    let mut taken = std::collections::HashSet::new();
    (0..slots)
        .map(|slot| {
            let base = material_name(mesh, slot);
            let mut name = base.clone();
            let mut n = 1;
            while !taken.insert(name.clone()) {
                n += 1;
                name = format!("{base}_{n}");
            }
            name
        })
        .collect()
}

/// OBJ text for `mesh` referencing `mtl_name`. UV v is flipped (OBJ origin is bottom-left).
pub fn obj_text(mesh: &Mesh, mtl_name: &str) -> String {
    let mut s = String::from("# SubCraft OBJ export\n");
    let _ = writeln!(s, "mtllib {mtl_name}");
    for p in &mesh.positions {
        let _ = writeln!(s, "v {} {} {}", p.x, p.y, p.z);
    }
    for uv in &mesh.uvs {
        let _ = writeln!(s, "vt {} {}", uv.x, 1.0 - uv.y);
    }
    for n in &mesh.normals {
        let _ = writeln!(s, "vn {} {} {}", n.x, n.y, n.z);
    }
    let slots = mesh.submeshes.iter().map(|sm| sm.material as usize + 1).max().unwrap_or(0).max(mesh.materials.len());
    let names = obj_material_names(mesh, slots);
    for sm in &mesh.submeshes {
        let _ = writeln!(s, "usemtl {}", names[sm.material as usize]);
        let mut last = None;
        for t in sm.first_triangle..sm.first_triangle + sm.triangle_count {
            let object = mesh.tri_object.get(t as usize).copied().flatten();
            if t == sm.first_triangle || object != last {
                let name = object.map_or(mesh.name.as_str(), |o| mesh.objects[o as usize].name.as_str());
                let _ = writeln!(s, "o {}", sanitize(name));
                last = object;
            }
            let [a, b, c] = mesh.tri(t as usize).map(|i| i + 1);
            let _ = writeln!(s, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
    }
    s
}

pub fn mtl_text(mesh: &Mesh, mats: &[SceneMaterial]) -> String {
    let mut s = String::new();
    let names = obj_material_names(mesh, mats.len().max(mesh.materials.len()));
    for (slot, m) in mats.iter().enumerate() {
        let _ = writeln!(s, "newmtl {}", names[slot]);
        let _ = writeln!(s, "Ka 1 1 1\nKd 1 1 1\nKs 0.5 0.5 0.5\nd 1\nillum 2");
        if m.emissive.is_some() {
            let _ = writeln!(s, "Ke 1 1 1");
        }
        let mut map = |key: &str, t: &Option<SceneTexture>| {
            if let Some(t) = t {
                let _ = writeln!(s, "{key} {}", t.file);
            }
        };
        map("map_Kd", &m.base_color);
        map("map_Ke", &m.emissive);
        map("map_Pr", &m.roughness);
        map("map_Pm", &m.metallic);
        map("map_Ka", &m.occlusion);
        map("norm", &m.normal);
        s.push('\n');
    }
    s
}

// ───────────────────────────── glTF / GLB ─────────────────────────────

fn pad4(v: &mut Vec<u8>, byte: u8) {
    while v.len() % 4 != 0 {
        v.push(byte);
    }
}

fn f32s(v: impl IntoIterator<Item = f32>) -> Vec<u8> {
    v.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Build the glTF JSON and binary buffer. With `embed` the PNGs go into the buffer (GLB);
/// otherwise images are referenced by file name and the buffer by `bin_uri`.
fn gltf_document(mesh: &Mesh, mats: &[SceneMaterial], embed: bool, bin_uri: &str) -> (Value, Vec<u8>) {
    let parts = parts(mesh);
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut view = |bin: &mut Vec<u8>, data: &[u8], target: Option<u32>| -> usize {
        pad4(bin, 0);
        let offset = bin.len();
        bin.extend_from_slice(data);
        let mut v = json!({"buffer": 0, "byteOffset": offset, "byteLength": data.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        views.push(v);
        views.len() - 1
    };
    let mut meshes_prims = Vec::new();
    for p in &parts {
        let mut attrs = serde_json::Map::new();
        let mut add = |bin: &mut Vec<u8>, accessors: &mut Vec<Value>, name: &str, data: Vec<u8>, count: usize, ty: &str, minmax: Option<([f32; 3], [f32; 3])>| {
            let bv = view(bin, &data, Some(34962));
            let mut a = json!({"bufferView": bv, "componentType": 5126, "count": count, "type": ty});
            if let Some((lo, hi)) = minmax {
                a["min"] = json!(lo);
                a["max"] = json!(hi);
            }
            accessors.push(a);
            attrs.insert(name.into(), json!(accessors.len() - 1));
        };
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for q in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        let n = p.positions.len();
        add(&mut bin, &mut accessors, "POSITION", f32s(p.positions.iter().flatten().copied()), n, "VEC3", Some((lo, hi)));
        add(&mut bin, &mut accessors, "NORMAL", f32s(p.normals.iter().flatten().copied()), n, "VEC3", None);
        add(&mut bin, &mut accessors, "TEXCOORD_0", f32s(p.uvs.iter().flatten().copied()), n, "VEC2", None);
        add(&mut bin, &mut accessors, "TANGENT", f32s(p.tangents.iter().flatten().copied()), n, "VEC4", None);
        let idx: Vec<u8> = p.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let bv = view(&mut bin, &idx, Some(34963));
        accessors.push(json!({"bufferView": bv, "componentType": 5125, "count": p.indices.len(), "type": "SCALAR"}));
        meshes_prims.push(json!({"attributes": attrs, "indices": accessors.len() - 1, "material": p.material, "mode": 4}));
    }

    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut by_file: HashMap<String, usize> = HashMap::new();
    let mut tex = |bin: &mut Vec<u8>, t: &SceneTexture| -> usize {
        if let Some(&i) = by_file.get(&t.file) {
            return i;
        }
        let img = if embed {
            let bv = view(bin, &t.png, None);
            json!({"bufferView": bv, "mimeType": "image/png", "name": t.file})
        } else {
            json!({"uri": t.file})
        };
        images.push(img);
        textures.push(json!({"source": images.len() - 1, "sampler": 0}));
        by_file.insert(t.file.clone(), textures.len() - 1);
        textures.len() - 1
    };
    let mut materials = Vec::new();
    for slot in 0..mesh.materials.len().max(mats.len()) {
        let m = mats.get(slot).cloned().unwrap_or_default();
        let mut pbr = json!({"baseColorFactor": [1, 1, 1, 1], "metallicFactor": if m.orm.is_some() { 1.0 } else { 0.0 }, "roughnessFactor": 1.0});
        let mut mat = json!({"name": material_name(mesh, slot), "doubleSided": false});
        if let Some(t) = &m.base_color {
            pbr["baseColorTexture"] = json!({"index": tex(&mut bin, t)});
        }
        if let Some(t) = &m.orm {
            let i = tex(&mut bin, t);
            pbr["metallicRoughnessTexture"] = json!({"index": i});
            mat["occlusionTexture"] = json!({"index": i});
        }
        mat["pbrMetallicRoughness"] = pbr;
        if let Some(t) = &m.normal {
            mat["normalTexture"] = json!({"index": tex(&mut bin, t)});
        }
        if let Some(t) = &m.emissive {
            mat["emissiveTexture"] = json!({"index": tex(&mut bin, t)});
            mat["emissiveFactor"] = json!([1, 1, 1]);
        }
        materials.push(mat);
    }
    pad4(&mut bin, 0);

    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "SubCraft"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": sanitize(&mesh.name)}],
        "meshes": [{"name": sanitize(&mesh.name), "primitives": meshes_prims}],
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    if !textures.is_empty() {
        doc["samplers"] = json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
        doc["textures"] = json!(textures);
        doc["images"] = json!(images);
    }
    if !embed {
        doc["buffers"][0]["uri"] = json!(bin_uri);
    }
    (doc, bin)
}

/// A complete `.glb` file (textures embedded).
pub fn glb_bytes(mesh: &Mesh, mats: &[SceneMaterial]) -> Vec<u8> {
    let (doc, bin) = gltf_document(mesh, mats, true, "");
    let mut json_bytes = serde_json::to_vec(&doc).expect("json");
    pad4(&mut json_bytes, b' ');
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// `.gltf` JSON text and its `.bin` buffer.
pub fn gltf_files(mesh: &Mesh, mats: &[SceneMaterial], bin_name: &str) -> (String, Vec<u8>) {
    let (doc, bin) = gltf_document(mesh, mats, false, bin_name);
    (serde_json::to_string_pretty(&doc).expect("json"), bin)
}

// ───────────────────────────── USDA ─────────────────────────────

fn usd_array3(v: impl Iterator<Item = [f32; 3]>) -> String {
    v.map(|p| format!("({}, {}, {})", p[0], p[1], p[2])).collect::<Vec<_>>().join(", ")
}

/// A USD prim name: ASCII letters, digits and `_`, not starting with a digit. `sanitize` keeps
/// `-`, leading digits and non-ASCII letters, none of which a USD reader accepts in a prim path.
fn usd_identifier(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// USD ASCII with one `Mesh` per material slot, bound to a `UsdPreviewSurface` material.
/// UVs are written as `st` with v flipped (USD origin is bottom-left).
pub fn usda_text(mesh: &Mesh, mats: &[SceneMaterial]) -> String {
    let root = usd_identifier(&sanitize(&mesh.name));
    let names = usd_material_names(mesh, mats.len().max(mesh.materials.len()));
    let mut s = String::new();
    let _ = writeln!(s, "#usda 1.0\n(\n    defaultPrim = \"{root}\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n    doc = \"Exported by SubCraft\"\n)\n");
    let _ = writeln!(s, "def Xform \"{root}\"\n{{");
    for (i, p) in parts(mesh).iter().enumerate() {
        let mname = names.get(p.material).cloned().unwrap_or_else(|| usd_identifier(&material_name(mesh, p.material)));
        let _ = writeln!(s, "    def Mesh \"Mesh_{i}_{mname}\" (\n        prepend apiSchemas = [\"MaterialBindingAPI\"]\n    )\n    {{");
        let _ = writeln!(s, "        int[] faceVertexCounts = [{}]", vec!["3"; p.indices.len() / 3].join(", "));
        let _ = writeln!(s, "        int[] faceVertexIndices = [{}]", p.indices.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
        let _ = writeln!(s, "        point3f[] points = [{}]", usd_array3(p.positions.iter().copied()));
        let _ = writeln!(s, "        normal3f[] normals = [{}] (\n            interpolation = \"vertex\"\n        )", usd_array3(p.normals.iter().copied()));
        let st = p.uvs.iter().map(|u| format!("({}, {})", u[0], 1.0 - u[1])).collect::<Vec<_>>().join(", ");
        let _ = writeln!(s, "        texCoord2f[] primvars:st = [{st}] (\n            interpolation = \"vertex\"\n        )");
        let _ = writeln!(s, "        uniform token subdivisionScheme = \"none\"");
        let _ = writeln!(s, "        rel material:binding = </{root}/Materials/{mname}>\n    }}\n");
    }
    let _ = writeln!(s, "    def Scope \"Materials\"\n    {{");
    for (slot, m) in mats.iter().enumerate() {
        let n = names[slot].clone();
        let base = format!("/{root}/Materials/{n}");
        let _ = writeln!(s, "        def Material \"{n}\"\n        {{");
        let _ = writeln!(s, "            token outputs:surface.connect = <{base}/Surface.outputs:surface>");
        let _ = writeln!(s, "            def Shader \"Surface\"\n            {{\n                uniform token info:id = \"UsdPreviewSurface\"");
        if m.base_color.is_some() {
            let _ = writeln!(s, "                color3f inputs:diffuseColor.connect = <{base}/BaseColor.outputs:rgb>");
        }
        if m.orm.is_some() {
            let _ = writeln!(s, "                float inputs:roughness.connect = <{base}/ORM.outputs:g>");
            let _ = writeln!(s, "                float inputs:metallic.connect = <{base}/ORM.outputs:b>");
            let _ = writeln!(s, "                float inputs:occlusion.connect = <{base}/ORM.outputs:r>");
        }
        if m.normal.is_some() {
            let _ = writeln!(s, "                normal3f inputs:normal.connect = <{base}/Normal.outputs:rgb>");
        }
        if m.emissive.is_some() {
            let _ = writeln!(s, "                color3f inputs:emissiveColor.connect = <{base}/Emissive.outputs:rgb>");
        }
        let _ = writeln!(s, "                token outputs:surface\n            }}");
        let _ = writeln!(s, "            def Shader \"UVReader\"\n            {{\n                uniform token info:id = \"UsdPrimvarReader_float2\"\n                token inputs:varname = \"st\"\n                float2 outputs:result\n            }}");
        let tex_node = |s: &mut String, node: &str, t: &SceneTexture, space: &str, normal: bool| {
            let _ = writeln!(s, "            def Shader \"{node}\"\n            {{\n                uniform token info:id = \"UsdUVTexture\"");
            let _ = writeln!(s, "                asset inputs:file = @{}@", t.file);
            let _ = writeln!(s, "                float2 inputs:st.connect = <{base}/UVReader.outputs:result>");
            let _ = writeln!(s, "                token inputs:sourceColorSpace = \"{space}\"");
            let _ = writeln!(s, "                token inputs:wrapS = \"repeat\"\n                token inputs:wrapT = \"repeat\"");
            if normal {
                let _ = writeln!(s, "                float4 inputs:scale = (2, 2, 2, 1)\n                float4 inputs:bias = (-1, -1, -1, 0)");
            }
            let _ = writeln!(s, "                float3 outputs:rgb\n                float outputs:r\n                float outputs:g\n                float outputs:b\n            }}");
        };
        if let Some(t) = &m.base_color {
            tex_node(&mut s, "BaseColor", t, "sRGB", false);
        }
        if let Some(t) = &m.orm {
            tex_node(&mut s, "ORM", t, "raw", false);
        }
        if let Some(t) = &m.normal {
            tex_node(&mut s, "Normal", t, "raw", true);
        }
        if let Some(t) = &m.emissive {
            tex_node(&mut s, "Emissive", t, "sRGB", false);
        }
        let _ = writeln!(s, "        }}");
    }
    let _ = writeln!(s, "    }}\n}}");
    s
}

// ───────────────────────────── driver ─────────────────────────────

/// Export textures and a scene file into `dir`. Returns every written file (scene file first).
/// Texture sets are matched to mesh material slots (`material_slot`); UDIM tiles other than
/// the first of a slot are ignored.
pub fn export_scene(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, format: SceneFormat, dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    subcraft_core::vfs::create_dir_all(dir).map_err(|e| ExportError::Folder(dir.to_path_buf(), e))?;
    let obj = format == SceneFormat::ObjMtl;
    let mut tex_settings = settings.clone();
    tex_settings.template = if obj { ExportTemplate::pbr_metal_rough() } else { ExportTemplate::gltf() };
    tex_settings.normal_directx = Some(false);
    tex_settings.format_override = Some(FileFormat::Png8);
    tex_settings.udim_folders = false;

    // path -> (slot, suffix) for every map the template can write.
    let mut owner: HashMap<String, (u32, String)> = HashMap::new();
    for set in sets {
        for map in &tex_settings.template.maps {
            owner.entry(output_path(mesh_name, set, &map.suffix, "png", &tex_settings)).or_insert((set.material_slot, map.suffix.clone()));
        }
    }
    let slots = mesh.materials.len();
    let mut mats: Vec<SceneMaterial> = (0..slots).map(|i| SceneMaterial { name: material_name(mesh, i), ..Default::default() }).collect();
    let mut taken: HashMap<(u32, String), ()> = HashMap::new();
    let mut tex_files: Vec<(String, Vec<u8>)> = Vec::new();
    render_maps(mesh, mesh_name, sets, &tex_settings, |name, bytes| {
        let Some((slot, suffix)) = owner.get(name).cloned() else { return Ok(()) };
        if slot as usize >= slots || taken.insert((slot, suffix.clone()), ()).is_some() {
            return Ok(());
        }
        let t = Some(SceneTexture { file: name.to_string(), png: bytes.to_vec() });
        let m = &mut mats[slot as usize];
        match suffix.as_str() {
            "BaseColor" => m.base_color = t,
            "Normal" => m.normal = t,
            "OcclusionRoughnessMetallic" => m.orm = t,
            "Emissive" => m.emissive = t,
            "Roughness" => m.roughness = t,
            "Metallic" => m.metallic = t,
            "AO" => m.occlusion = t,
            _ => {}
        }
        tex_files.push((name.to_string(), bytes.to_vec()));
        Ok(())
    })?;

    let stem = sanitize(mesh_name);
    let scene_name = format!("{stem}.{}", format.extension());
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    match format {
        SceneFormat::ObjMtl => {
            let mtl = format!("{stem}.mtl");
            files.push((scene_name, obj_text(mesh, &mtl).into_bytes()));
            files.push((mtl, mtl_text(mesh, &mats).into_bytes()));
            files.extend(tex_files);
        }
        SceneFormat::Gltf => {
            let bin = format!("{stem}.bin");
            let (json, data) = gltf_files(mesh, &mats, &bin);
            files.push((scene_name, json.into_bytes()));
            files.push((bin, data));
            files.extend(tex_files);
        }
        SceneFormat::Glb => files.push((scene_name, glb_bytes(mesh, &mats))),
        SceneFormat::Usda => {
            files.push((scene_name, usda_text(mesh, &mats).into_bytes()));
            files.extend(tex_files);
        }
    }
    let mut written = Vec::new();
    for (name, bytes) in files {
        let path = dir.join(&name);
        subcraft_core::vfs::write(&path, &bytes).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod usd_name_tests {
    use super::*;

    /// QA 2026-11: prim names were written with `-`, a leading digit or non-ASCII letters, which
    /// are not valid in a USD prim path (the file does not open in USD readers).
    #[test]
    fn usd_prim_names_are_identifiers() {
        let mut mesh = subcraft_mesh::primitives::sample_crate();
        mesh.name = "2024 sci-fi-crate é".into();
        mesh.materials[0].name = "1-Body".into();
        let text = usda_text(&mesh, &[SceneMaterial::default(), SceneMaterial::default()]);
        let names: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_prefix("def ")).filter_map(|l| l.split('"').nth(1)).collect();
        assert!(names.len() >= 4, "{names:?}");
        for n in &names {
            assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !n.starts_with(|c: char| c.is_ascii_digit()), "invalid prim name {n:?}");
        }
        assert!(text.contains("defaultPrim = \"_2024_sci_fi_crate__\""), "{text}");
        assert!(text.contains("</_2024_sci_fi_crate__/Materials/_1_Body>"), "the binding names the material prim");
        assert_eq!(usd_identifier("Body_01"), "Body_01", "valid names are unchanged");
    }
}

#[cfg(test)]
mod obj_name_tests {
    use super::*;

    /// QA 2026-11: #174 kept same-named material slots apart for USD only; OBJ / MTL still wrote
    /// one `newmtl` name for both.
    #[test]
    fn obj_and_mtl_keep_same_named_material_slots_apart() {
        let mut mesh = subcraft_mesh::primitives::sample_crate();
        mesh.materials[0].name = "Mat 1".into();
        mesh.materials[1].name = "Mat_1".into();
        let mtl = mtl_text(&mesh, &[SceneMaterial::default(), SceneMaterial::default()]);
        let defined: Vec<&str> = mtl.lines().filter_map(|l| l.strip_prefix("newmtl ")).collect();
        assert_eq!(defined, ["Mat_1", "Mat_1_2"]);
        let obj = obj_text(&mesh, "crate.mtl");
        let used: std::collections::BTreeSet<&str> = obj.lines().filter_map(|l| l.strip_prefix("usemtl ")).collect();
        assert_eq!(used.into_iter().collect::<Vec<_>>(), ["Mat_1", "Mat_1_2"], "the OBJ uses the names the MTL defines");
    }
}
