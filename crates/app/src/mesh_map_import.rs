//! Importing already-baked mesh maps from image files (ADR-0003 storage, see `docs/progress.md`).
//!
//! Users who bake elsewhere bring Normal, World normal, AO, Curvature, Position, Thickness, ID,
//! Height and Bent normal maps as images. The kind is recognised from the file-name suffix
//! (`crate_normal.png`, `Body_ambient_occlusion.1002.exr`, `prop_id.tga`...), the texture set
//! from the UDIM number in the name and/or the set name inside it. The images go straight into
//! the texture set's mesh-map slots: no re-bake. Values are taken as stored (mesh maps are data,
//! never color-transformed); only a DirectX normal map gets its green channel flipped into the
//! OpenGL (+Y) convention used internally.

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use subcraft_core::{MeshMapKind, TextureSetId};
use subcraft_image::{TextureFilter, TiledImage};

use crate::Session;

/// One image chosen for import, with what was recognised about it (the user may change `kind`,
/// `set` and `directx` before importing).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshMapFile {
    pub path: PathBuf,
    pub kind: Option<MeshMapKind>,
    /// Index into the texture-set list the file was matched against.
    pub set: Option<usize>,
    /// UDIM tile number found in the name (1001...).
    pub udim: Option<u32>,
    /// Normal / bent-normal map stored with a green channel pointing down (DirectX).
    pub directx: bool,
}

/// A texture set as the matcher sees it: its name and UDIM tile.
#[derive(Clone, Debug, PartialEq)]
pub struct SetKey {
    pub name: String,
    pub udim: Option<u32>,
}

/// Recognised suffixes, written without separators (`ambient_occlusion` -> `ambientocclusion`).
const SUFFIXES: &[(&str, MeshMapKind)] = &[
    ("worldspacenormals", MeshMapKind::WorldSpaceNormal),
    ("worldspacenormal", MeshMapKind::WorldSpaceNormal),
    ("worldnormals", MeshMapKind::WorldSpaceNormal),
    ("worldnormal", MeshMapKind::WorldSpaceNormal),
    ("normalworld", MeshMapKind::WorldSpaceNormal),
    ("wsnormal", MeshMapKind::WorldSpaceNormal),
    ("wsn", MeshMapKind::WorldSpaceNormal),
    ("bentnormals", MeshMapKind::BentNormal),
    ("bentnormal", MeshMapKind::BentNormal),
    ("normalbase", MeshMapKind::Normal),
    ("tangentnormal", MeshMapKind::Normal),
    ("normalmap", MeshMapKind::Normal),
    ("normals", MeshMapKind::Normal),
    ("normal", MeshMapKind::Normal),
    ("norm", MeshMapKind::Normal),
    ("nrm", MeshMapKind::Normal),
    ("nor", MeshMapKind::Normal),
    ("ambientocclusion", MeshMapKind::AmbientOcclusion),
    ("occlusion", MeshMapKind::AmbientOcclusion),
    ("ao", MeshMapKind::AmbientOcclusion),
    ("curvature", MeshMapKind::Curvature),
    ("curv", MeshMapKind::Curvature),
    ("position", MeshMapKind::Position),
    ("pos", MeshMapKind::Position),
    ("thickness", MeshMapKind::Thickness),
    ("thick", MeshMapKind::Thickness),
    ("materialid", MeshMapKind::Id),
    ("colorid", MeshMapKind::Id),
    ("colourid", MeshMapKind::Id),
    ("matid", MeshMapKind::Id),
    ("id", MeshMapKind::Id),
    ("height", MeshMapKind::Height),
    ("displacement", MeshMapKind::Height),
    ("disp", MeshMapKind::Height),
];

/// Tokens that only describe the normal-map convention.
const DIRECTX: &[&str] = &["dx", "directx", "d3d"];
const OPENGL: &[&str] = &["gl", "opengl", "ogl"];

fn tokens(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = stem.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            continue;
        }
        // camelCase boundary: "crateNormal" -> crate, normal.
        if c.is_uppercase() && i > 0 && chars[i - 1].is_lowercase() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

fn udim_token(t: &str) -> Option<u32> {
    (t.len() == 4).then(|| t.parse::<u32>().ok()).flatten().filter(|n| (1001..=1999).contains(n))
}

/// What a file name says: the map kind, the UDIM tile, whether it is a DirectX normal map, and
/// the name part before the suffix (lowercase letters and digits only, for set matching).
pub fn parse_file_name(path: &Path) -> (Option<MeshMapKind>, Option<u32>, bool, String) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    // `name.1001` keeps the tile in the stem; `name.png` with a `.1001` before it is handled too.
    let mut toks = tokens(stem);
    let mut udim = None;
    let mut directx = false;
    // Strip trailing UDIM numbers and convention markers (in any order).
    while let Some(last) = toks.last() {
        if let Some(n) = udim_token(last) { udim = Some(n); toks.pop(); continue; }
        if DIRECTX.contains(&last.as_str()) { directx = true; toks.pop(); continue; }
        if OPENGL.contains(&last.as_str()) { toks.pop(); continue; }
        break;
    }
    if udim.is_none() {
        udim = toks.iter().find_map(|t| udim_token(t));
    }
    let mut kind = None;
    let mut prefix_len = toks.len();
    'outer: for take in (1..=3.min(toks.len())).rev() {
        let joined: String = toks[toks.len() - take..].concat();
        for (suffix, k) in SUFFIXES {
            if joined == *suffix {
                kind = Some(*k);
                prefix_len = toks.len() - take;
                break 'outer;
            }
        }
    }
    // Words in the middle also count ("crate_normal_map_dx" handled above; "T_Crate_N" is not
    // guessed: single letters are too ambiguous).
    if kind.is_none() {
        for (i, t) in toks.iter().enumerate().rev() {
            if let Some((_, k)) = SUFFIXES.iter().find(|(s, _)| s.len() >= 4 && *s == t) {
                kind = Some(*k);
                prefix_len = i;
                break;
            }
        }
    }
    let prefix: String = toks[..prefix_len].iter().filter(|t| udim_token(t).is_none()).cloned().collect();
    (kind, udim, directx, prefix)
}

/// Letters and digits of a set name, lowercase, without a trailing `.1001` tile.
fn set_key(name: &str) -> String {
    let toks = tokens(name);
    toks.into_iter().filter(|t| udim_token(t).is_none()).collect()
}

/// Recognise each file and match it to one of `sets`: by UDIM tile first, then by the set name
/// appearing in the file name (longest name wins), else the only candidate.
pub fn plan_import(paths: &[PathBuf], sets: &[SetKey]) -> Vec<MeshMapFile> {
    paths.iter().map(|path| {
        let (kind, udim, directx, prefix) = parse_file_name(path);
        let candidates: Vec<usize> = (0..sets.len()).filter(|&i| udim.is_none() || sets[i].udim.is_none() && sets.iter().all(|s| s.udim.is_none()) || sets[i].udim == udim).collect();
        let by_name = candidates.iter().copied()
            .filter(|&i| { let k = set_key(&sets[i].name); !k.is_empty() && prefix.contains(&k) })
            .max_by_key(|&i| set_key(&sets[i].name).len());
        let set = by_name.or_else(|| {
            // Without a name match: the only candidate, or the only set of this tile when every
            // candidate shares the file's tile.
            (candidates.len() == 1).then(|| candidates[0])
        });
        MeshMapFile { path: path.clone(), kind, set, udim, directx }
    }).collect()
}

/// Read an image as mesh map `kind` at `resolution`² (resampled when the file differs; ID maps
/// with nearest-neighbour so no in-between colors appear). Values are kept as stored.
pub fn load_mesh_map(path: &Path, kind: MeshMapKind, resolution: u32, directx: bool) -> Result<TiledImage, String> {
    let bitmap = subcraft_image::load_bitmap(path).map_err(|e| e.to_string())?;
    let (w, h) = (bitmap.width, bitmap.height);
    if w == 0 || h == 0 {
        return Err("the image is empty".into());
    }
    let comps = kind.components();
    let mut dense = vec![0.0f32; (w * h) as usize * comps];
    let flip_green = directx && matches!(kind, MeshMapKind::Normal | MeshMapKind::BentNormal);
    dense.par_chunks_mut(w as usize * comps).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let t = bitmap.texel(x as u32, y as u32, false);
            let px = &mut row[x * comps..(x + 1) * comps];
            if comps == 1 {
                px[0] = t[0];
            } else {
                px.copy_from_slice(&t[..3]);
                if flip_green { px[1] = 1.0 - px[1]; }
            }
        }
    });
    let neutral = kind.neutral_value();
    let mut fill = [0.0f32; 4];
    fill[..comps].copy_from_slice(&neutral[..comps]);
    let image = TiledImage::from_dense(w, h, comps, fill, &dense);
    if (w, h) == (resolution, resolution) {
        return Ok(image);
    }
    let filter = if kind == MeshMapKind::Id { TextureFilter::Nearest } else { TextureFilter::Bilinear };
    Ok(image.resampled_filtered(resolution, resolution, filter))
}

/// One import request: which file goes into which slot of which set.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshMapAssignment {
    pub texture_set: TextureSetId,
    pub kind: MeshMapKind,
    pub path: PathBuf,
    pub directx: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshMapImportReport {
    /// (texture set name, map) per imported image.
    pub imported: Vec<(String, MeshMapKind)>,
    /// Curvature maps derived from an imported normal map.
    pub derived: Vec<String>,
    pub errors: Vec<String>,
}

impl MeshMapImportReport {
    pub fn summary(&self) -> String {
        let mut s = format!("Imported {} mesh map{}", self.imported.len(), if self.imported.len() == 1 { "" } else { "s" });
        if !self.derived.is_empty() {
            s += &format!("; curvature derived from the normal map for {}", self.derived.join(", "));
        }
        if !self.errors.is_empty() {
            s += &format!("; {} could not be read: {}", self.errors.len(), self.errors.join("; "));
        }
        s
    }
}

impl Session {
    /// The texture sets as [`plan_import`] matches them, in document order.
    pub fn mesh_map_set_keys(&self) -> Vec<SetKey> {
        self.document().texture_sets.iter().map(|t| SetKey { name: t.name.clone(), udim: t.udim_number() }).collect()
    }

    /// Resolve a plan against this document (files without a kind or a set are skipped).
    pub fn mesh_map_assignments(&self, files: &[MeshMapFile]) -> Vec<MeshMapAssignment> {
        let sets = &self.document().texture_sets;
        files.iter().filter_map(|f| {
            let ts = sets.get(f.set?)?;
            Some(MeshMapAssignment { texture_set: ts.id, kind: f.kind?, path: f.path.clone(), directx: f.directx })
        }).collect()
    }

    /// Load the images into the mesh-map slots of their texture sets (at each set's
    /// resolution), replacing maps of the same kind. With `derive_curvature`, a set that got a
    /// normal map but no curvature map gets curvature measured from that normal map along the
    /// surface. Mesh maps are not part of the undo history (like a bake).
    pub fn import_mesh_maps(&mut self, assignments: &[MeshMapAssignment], derive_curvature: bool) -> MeshMapImportReport {
        let mut report = MeshMapImportReport::default();
        let jobs: Vec<(MeshMapAssignment, u32)> = assignments.iter().filter_map(|a| {
            let ts = self.document().texture_set(a.texture_set)?;
            Some((a.clone(), ts.resolution))
        }).collect();
        let loaded: Vec<(MeshMapAssignment, Result<TiledImage, String>)> = jobs.into_par_iter()
            .map(|(a, res)| { let r = load_mesh_map(&a.path, a.kind, res, a.directx); (a, r) })
            .collect();
        let mut by_set: std::collections::BTreeMap<TextureSetId, std::collections::BTreeMap<MeshMapKind, TiledImage>> = Default::default();
        for (a, result) in loaded {
            let name = self.document().texture_set(a.texture_set).map(|t| t.name.clone()).unwrap_or_default();
            let file = a.path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
            match result {
                Ok(img) => {
                    by_set.entry(a.texture_set).or_default().insert(a.kind, img);
                    report.imported.push((name, a.kind));
                }
                Err(e) => report.errors.push(format!("{file}: {e}")),
            }
        }
        for (ts, maps) in by_set {
            let derive = derive_curvature && maps.contains_key(&MeshMapKind::Normal) && !maps.contains_key(&MeshMapKind::Curvature);
            self.set_mesh_maps(ts, maps);
            if derive && self.derive_curvature_from_normal(ts).is_ok() {
                report.derived.push(self.document().texture_set(ts).map(|t| t.name.clone()).unwrap_or_default());
            }
        }
        report
    }

    /// Replace the set's curvature map with one measured from its normal map along the surface
    /// (see `subcraft_bake::normal_curvature`), at the set's resolution.
    pub fn derive_curvature_from_normal(&mut self, ts: TextureSetId) -> Result<(), String> {
        let set = self.document().texture_set(ts).ok_or("no such texture set")?;
        let (slot, res) = (set.material_slot, set.resolution);
        let normal = self.mesh_maps(ts).and_then(|m| m.get(MeshMapKind::Normal)).ok_or("the texture set has no normal map")?.clone();
        let curvature = subcraft_bake::normal_curvature::curvature_from_normal_map(
            &self.scene().mesh, slot, &normal, res, &Default::default(), None).map_err(|e| e.to_string())?;
        self.set_mesh_maps(ts, std::collections::BTreeMap::from([(MeshMapKind::Curvature, curvature)]));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str) -> Option<MeshMapKind> {
        parse_file_name(Path::new(name)).0
    }

    #[test]
    fn suffixes_name_the_map() {
        use MeshMapKind::*;
        for (name, want) in [
            ("crate_normal.png", Normal), ("crate_Normal_OpenGL.png", Normal), ("Crate_normal_base.png", Normal),
            ("crate_nrm.tga", Normal), ("crateNormal.png", Normal), ("crate_world_normal.png", WorldSpaceNormal),
            ("Body_world_space_normals.png", WorldSpaceNormal), ("crate_ao.png", AmbientOcclusion),
            ("crate_ambient_occlusion.exr", AmbientOcclusion), ("crate-curvature.png", Curvature),
            ("crate_position.png", Position), ("crate_thickness.png", Thickness), ("crate_id.png", Id),
            ("crate_material_id.png", Id), ("crate_height.exr", Height), ("crate_bent_normals.png", BentNormal),
            ("Body_normal.1002.png", Normal), ("Body.1003_ao.png", AmbientOcclusion), ("crate_normal_map_dx.png", Normal),
        ] {
            assert_eq!(kind(name), Some(want), "{name}");
        }
        assert_eq!(kind("crate_basecolor.png"), None);
        assert_eq!(kind("idea.png"), None, "a word merely starting with a suffix is not a map");
        let (_, udim, dx, prefix) = parse_file_name(Path::new("Hero_Body_normal_DX.1002.png"));
        assert_eq!((udim, dx, prefix.as_str()), (Some(1002), true, "herobody"));
    }

    #[test]
    fn files_match_sets_by_udim_tile_and_by_name() {
        let sets = vec![
            SetKey { name: "Body.1001".into(), udim: Some(1001) },
            SetKey { name: "Body.1002".into(), udim: Some(1002) },
            SetKey { name: "Head.1001".into(), udim: Some(1001) },
        ];
        let paths: Vec<PathBuf> = ["hero_Body_normal.1002.png", "hero_head_ao.1001.png", "hero_body_id.1001.png", "hero_curvature.1001.png", "hero_ao.1003.png"]
            .iter().map(PathBuf::from).collect();
        let plan = plan_import(&paths, &sets);
        assert_eq!(plan[0].set, Some(1));
        assert_eq!(plan[1].set, Some(2));
        assert_eq!(plan[2].set, Some(0));
        assert_eq!(plan[3].set, None, "two sets on tile 1001 and no name in the file: left to the user");
        assert_eq!(plan[4].set, None, "no set on tile 1003");
        // One set without tiles takes every file.
        let one = plan_import(&[PathBuf::from("anything_normal.1001.png")], &[SetKey { name: "Mat".into(), udim: None }]);
        assert_eq!(one[0].set, Some(0));
    }

    #[test]
    fn loading_keeps_data_values_flips_directx_green_and_resamples() {
        let dir = std::env::temp_dir().join(format!("subcraft-mmimport-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crate_normal.png");
        // 4×4 RGB8: (64, 200, 255) everywhere.
        let data: Vec<f32> = [64.0 / 255.0, 200.0 / 255.0, 1.0].repeat(16);
        subcraft_image::save_image(&path, 4, 4, 3, &data, subcraft_image::FileFormat::Png8).unwrap();
        let gl = load_mesh_map(&path, MeshMapKind::Normal, 4, false).unwrap();
        let px = gl.get(1, 1);
        assert!((px[0] - 64.0 / 255.0).abs() < 2e-3 && (px[1] - 200.0 / 255.0).abs() < 2e-3, "no color transform: {px:?}");
        let dx = load_mesh_map(&path, MeshMapKind::Normal, 8, true).unwrap();
        assert_eq!((dx.width(), dx.height()), (8, 8));
        assert!((dx.get(3, 3)[1] - (1.0 - 200.0 / 255.0)).abs() < 2e-3);
        let ao = load_mesh_map(&path, MeshMapKind::AmbientOcclusion, 4, false).unwrap();
        assert_eq!(ao.comps(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
