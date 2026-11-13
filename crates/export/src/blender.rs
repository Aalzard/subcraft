//! Send to Blender: the files of the Blender round trip (user guide: `docs/blender-bridge.md`).
//!
//! [`write_bridge`] writes, into one exchange folder:
//! * the low poly as a *welded* OBJ (+ MTL): vertices split only for UVs or normals in SubCraft
//!   share one position, so Blender gets a closed surface that can be sculpted without tearing
//!   along UV seams, while UVs and normals stay exactly as in SubCraft;
//! * optionally the textures, through the normal texture export and any template;
//! * `subcraft_bridge.py` ([`SCRIPT`]), run by `blender --python`, and `bridge.json`
//!   ([`Manifest`]), which tells the script what each texture is (role, colour space, packed
//!   component, inversion, DirectX normals) so the Principled BSDF is wired for every template.
//!
//! The way back: the script adds a "Send to SubCraft" button that writes the selected object(s)
//! to [`RETURN_FILE`] in [`RETURN_DIR`] (atomically, through a temporary file), in SubCraft's
//! axes and units, for the app to pick up as the bake high poly.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use subcraft_core::ChannelKind;
use subcraft_image::FileFormat;
use subcraft_mesh::Mesh;

use crate::template::{ExportTemplate, OutputMap, Source};
use crate::{ExportError, ExportSettings, ExportTextureSet, sanitize};

/// The Python script Blender runs.
pub const SCRIPT: &str = include_str!("blender_bridge.py");
/// Folder (inside the exchange folder) the sculpt comes back to.
pub const RETURN_DIR: &str = "from_blender";
/// File the sculpt comes back as.
pub const RETURN_FILE: &str = "sculpt_high.obj";
/// Version of [`Manifest`]; the script refuses newer ones.
pub const MANIFEST_VERSION: u32 = 1;

/// The default template of the Blender dialog: every map the Principled BSDF reads, as separate
/// files, OpenGL normals (Blender's convention), plus AO and opacity when the project has them.
/// Height is left out: its bumps are baked into the normal map (`height_to_normal`).
pub fn template() -> ExportTemplate {
    use ChannelKind::*;
    let rgb = |k| (0..3).map(|c| Source::ch(k, c)).collect::<Vec<_>>();
    ExportTemplate {
        name: "Blender bridge (Principled BSDF + AO + Opacity)".into(),
        maps: vec![
            OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
            OutputMap::new("Metallic", FileFormat::Png8, vec![Source::ch(Metallic, 0)], false),
            OutputMap::new("Roughness", FileFormat::Png8, vec![Source::ch(Roughness, 0)], false),
            OutputMap::new("Normal", FileFormat::Png8, (0..3).map(|c| Source::Normal { component: c, directx: false }).collect(), false),
            OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
            OutputMap::new("Opacity", FileFormat::Png8, vec![Source::Alpha], false),
            OutputMap::new("AO", FileFormat::Png8, vec![Source::ch(AmbientOcclusion, 0)], false),
        ],
        normal_directx: Some(false),
        ..Default::default()
    }
}

/// What a texture (or one component of a packed texture) means to the Principled BSDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    BaseColor,
    Alpha,
    Metallic,
    Roughness,
    Normal,
    Emission,
    AmbientOcclusion,
    Height,
    Specular,
    CoatWeight,
    CoatRoughness,
    SheenWeight,
    Anisotropy,
}

impl Role {
    fn of_channel(kind: ChannelKind) -> Option<Self> {
        use ChannelKind as K;
        Some(match kind {
            K::BaseColor => Role::BaseColor,
            K::Opacity => Role::Alpha,
            K::Metallic => Role::Metallic,
            K::Roughness => Role::Roughness,
            K::Normal => Role::Normal,
            K::Emissive => Role::Emission,
            K::AmbientOcclusion => Role::AmbientOcclusion,
            K::Height => Role::Height,
            K::Specular => Role::Specular,
            K::CoatOpacity => Role::CoatWeight,
            K::CoatRoughness => Role::CoatRoughness,
            K::SheenOpacity => Role::SheenWeight,
            K::AnisotropyLevel => Role::Anisotropy,
            _ => return None,
        })
    }
}

/// One use of an image: the whole RGB (`component: None`) or one component.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextureInput {
    /// Absolute path of the file; for UDIM sets the 1001 tile (the script loads it as tiled).
    pub path: String,
    pub role: Role,
    /// 0..=3 for one component (packed maps), `None` for the image's colour.
    pub component: Option<u8>,
    /// 1 − v (smoothness, glossiness).
    pub invert: bool,
    /// The file holds sRGB-encoded colour (else Non-Color data).
    pub srgb: bool,
    /// Normal map in DirectX convention (green flipped).
    pub directx: bool,
    /// The texture set is one UDIM tile.
    pub udim: bool,
}

/// One Blender material: a material slot of the mesh and the textures of its texture set(s).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ManifestMaterial {
    /// Name used in the OBJ (`usemtl`) and so in Blender.
    pub name: String,
    pub textures: Vec<TextureInput>,
}

/// `bridge.json`: everything the Blender script needs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Manifest {
    pub version: u32,
    /// The welded OBJ.
    pub mesh: String,
    /// Name of the Blender object.
    pub object_name: String,
    /// False: mesh only, prepared for sculpting (smooth shading, no custom normals).
    pub include_textures: bool,
    pub materials: Vec<ManifestMaterial>,
    /// Folder and file the "Send to SubCraft" button writes.
    pub return_dir: String,
    pub return_file: String,
}

/// Files written by [`write_bridge`].
#[derive(Clone, Debug, PartialEq)]
pub struct BridgeFiles {
    pub mesh: PathBuf,
    pub script: PathBuf,
    pub manifest: PathBuf,
    pub textures: Vec<PathBuf>,
    /// Where the sculpt comes back ([`RETURN_DIR`]/[`RETURN_FILE`]).
    pub return_file: PathBuf,
}

/// OBJ material names per slot (as `obj_text` writes them).
fn material_names(mesh: &Mesh) -> Vec<String> {
    let slots = mesh.submeshes.iter().map(|sm| sm.material as usize + 1).max().unwrap_or(0).max(mesh.materials.len()).max(1);
    crate::scene::obj_material_names(mesh, slots)
}

/// The mesh as one OBJ object whose corners index shared positions: SubCraft vertices with the
/// same position (bit-exact) become one OBJ vertex, UVs and normals stay per SubCraft vertex.
/// Returns (OBJ text, MTL text).
pub fn welded_obj(mesh: &Mesh, object_name: &str, mtl_name: &str) -> (String, String) {
    let mut s = String::with_capacity(mesh.positions.len() * 96);
    let _ = writeln!(s, "# SubCraft Blender bridge\nmtllib {mtl_name}\no {}", sanitize(object_name));
    let mut index: HashMap<[u32; 3], u32> = HashMap::with_capacity(mesh.positions.len());
    let mut pos_of = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        let next = index.len() as u32;
        let i = *index.entry(key).or_insert_with(|| {
            let _ = writeln!(s, "v {} {} {}", p.x, p.y, p.z);
            next
        });
        pos_of.push(i + 1);
    }
    let has_uv = mesh.uvs.len() == mesh.positions.len();
    let has_n = mesh.normals.len() == mesh.positions.len();
    if has_uv {
        for uv in &mesh.uvs {
            let _ = writeln!(s, "vt {} {}", uv.x, 1.0 - uv.y);
        }
    }
    if has_n {
        for n in &mesh.normals {
            let _ = writeln!(s, "vn {} {} {}", n.x, n.y, n.z);
        }
    }
    let names = material_names(mesh);
    let corner = |i: u32| {
        let (v, t) = (pos_of[i as usize], i + 1);
        match (has_uv, has_n) {
            (true, true) => format!("{v}/{t}/{t}"),
            (true, false) => format!("{v}/{t}"),
            (false, true) => format!("{v}//{t}"),
            (false, false) => format!("{v}"),
        }
    };
    let _ = writeln!(s, "s 1");
    for sm in &mesh.submeshes {
        let _ = writeln!(s, "usemtl {}", names[sm.material as usize]);
        for t in sm.first_triangle..sm.first_triangle + sm.triangle_count {
            let [a, b, c] = mesh.tri(t as usize);
            let _ = writeln!(s, "f {} {} {}", corner(a), corner(b), corner(c));
        }
    }
    let mut mtl = String::new();
    for name in &names {
        let _ = writeln!(mtl, "newmtl {name}\nKd 0.8 0.8 0.8\nKs 0.5 0.5 0.5\nd 1\nillum 2\n");
    }
    (s, mtl)
}

/// How each file of `map` feeds the BSDF. Colour maps (3+ components of one colour channel) give
/// one colour input; everything else one input per component. Unknown sources are skipped.
pub fn inputs_of_map(map: &OutputMap, settings: &ExportSettings) -> Vec<(Role, Option<u8>, bool, bool)> {
    let c = &map.components;
    // (role, component, invert, directx)
    let mut out = Vec::new();
    if c.len() >= 3 {
        if let Source::Normal { directx, .. } = c[0] {
            out.push((Role::Normal, None, false, crate::normal_directx(map, settings, directx)));
        } else if let Some(kind) = c[0].channel().filter(|k| k.is_color())
            && c[..3].iter().enumerate().all(|(i, s)| matches!(s, Source::Channel { kind: k, component, invert: false } if *k == kind && *component as usize == i))
            && let Some(role) = Role::of_channel(kind)
        {
            out.push((role, None, false, false));
        }
        if !out.is_empty() {
            if c.len() == 4 && matches!(c[3], Source::Alpha) {
                out.push((Role::Alpha, Some(3), false, false));
            }
            return out;
        }
    }
    for (i, s) in c.iter().enumerate() {
        let (role, invert) = match *s {
            Source::Channel { kind: ChannelKind::Glossiness, invert, .. } | Source::Gray { kind: ChannelKind::Glossiness, invert } => (Role::Roughness, !invert),
            Source::Channel { kind, invert, .. } | Source::Gray { kind, invert } => match Role::of_channel(kind) {
                Some(r) => (r, invert),
                None => continue,
            },
            Source::Alpha => (Role::Alpha, false),
            Source::Height => (Role::Height, false),
            Source::MeshMap { kind: subcraft_core::MeshMapKind::AmbientOcclusion, invert, .. } => (Role::AmbientOcclusion, invert),
            Source::Converted { map: crate::Converted::Glossiness, .. } => (Role::Roughness, true),
            _ => continue,
        };
        // A colour role read from one component of a grey file is still that role.
        let component = if c.len() == 1 { None } else { Some(i as u8) };
        out.push((role, component, invert, false));
    }
    out
}

/// Build the manifest's materials from the files the export planned (`written` = the files that
/// exist; maps not written are left out).
pub fn manifest_materials(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, dir: &Path, written: &[PathBuf]) -> Vec<ManifestMaterial> {
    let names = material_names(mesh);
    let mut by_slot: BTreeMap<u32, Vec<TextureInput>> = BTreeMap::new();
    let planned = crate::plan_all(mesh_name, sets, settings);
    for set in sets {
        // Only one tile of a UDIM material is listed; the script loads the others as tiles.
        if set.udim.is_some_and(|n| n != 1001) && sets.iter().any(|o| o.material_slot == set.material_slot && o.udim == Some(1001)) {
            continue;
        }
        let set_settings = &*settings.for_set(set.name);
        for file in planned.iter().filter(|f| f.set == set.name && f.enabled) {
            let path = dir.join(&file.path);
            if !written.iter().any(|w| w == &path) {
                continue;
            }
            let Some(map) = set_settings.template.maps.iter().find(|m| m.suffix == file.suffix) else { continue };
            let srgb = crate::color_space(map, crate::output_format(map, set_settings)) == "sRGB";
            for (role, component, invert, directx) in inputs_of_map(map, set_settings) {
                let srgb = srgb && component.is_none() && matches!(role, Role::BaseColor | Role::Emission);
                by_slot.entry(set.material_slot).or_default().push(TextureInput { path: path.display().to_string(), role, component, invert, srgb, directx, udim: set.udim.is_some() });
            }
        }
    }
    names
        .iter()
        .enumerate()
        .map(|(slot, name)| ManifestMaterial { name: name.clone(), textures: by_slot.remove(&(slot as u32)).unwrap_or_default() })
        .collect()
}

/// Write everything Blender needs into `dir` (see the module docs). `settings` is the texture
/// export's; textures are written only when `include_textures`.
pub fn write_bridge(mesh: &Mesh, mesh_name: &str, sets: &[ExportTextureSet], settings: &ExportSettings, include_textures: bool, dir: &Path) -> Result<BridgeFiles, ExportError> {
    if dir.as_os_str().is_empty() {
        return Err(ExportError::Folder(dir.to_path_buf(), std::io::Error::new(std::io::ErrorKind::InvalidInput, "choose an export folder")));
    }
    subcraft_core::vfs::create_dir_all(dir).map_err(|e| ExportError::Folder(dir.to_path_buf(), e))?;
    let write = |name: &str, bytes: &[u8]| -> Result<PathBuf, ExportError> {
        let path = dir.join(name);
        subcraft_core::vfs::write(&path, bytes).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
        Ok(path)
    };
    let stem = sanitize(mesh_name);
    let mtl_name = format!("{stem}_lowpoly.mtl");
    let (obj, mtl) = welded_obj(mesh, mesh_name, &mtl_name);
    let mesh_path = write(&format!("{stem}_lowpoly.obj"), obj.as_bytes())?;
    write(&mtl_name, mtl.as_bytes())?;
    let textures = if include_textures { crate::export(mesh, mesh_name, sets, settings, &dir.join("textures"))? } else { Vec::new() };
    let materials = if include_textures {
        manifest_materials(mesh, mesh_name, sets, settings, &dir.join("textures"), &textures)
    } else {
        material_names(mesh).into_iter().map(|name| ManifestMaterial { name, textures: Vec::new() }).collect()
    };
    let return_dir = dir.join(RETURN_DIR);
    subcraft_core::vfs::create_dir_all(&return_dir).map_err(|e| ExportError::Folder(return_dir.clone(), e))?;
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        mesh: mesh_path.display().to_string(),
        object_name: mesh_name.to_string(),
        include_textures,
        materials,
        return_dir: return_dir.display().to_string(),
        return_file: RETURN_FILE.into(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| ExportError::Write(dir.join("bridge.json"), e.to_string()))?;
    let manifest_path = write("bridge.json", json.as_bytes())?;
    let script = write("subcraft_bridge.py", SCRIPT.as_bytes())?;
    Ok(BridgeFiles { mesh: mesh_path, script, manifest: manifest_path, textures, return_file: return_dir.join(RETURN_FILE) })
}

#[cfg(test)]
mod tests;
