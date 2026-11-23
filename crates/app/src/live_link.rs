//! Live link to a game engine: textures are re-exported into a Unity, Unreal or Godot project
//! whenever the painting settles, and the engine picks the changed files up by itself (all three
//! watch their asset folders). The model goes along once as OBJ. Godot also gets a ready
//! StandardMaterial3D per texture set (`.tres`, a plain-text resource) wired to the textures.
//!
//! The send runs on any thread from a [`LiveLinkSnapshot`]: owned copies of the finished
//! composites (tiles are shared copy-on-write, so taking one is cheap).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_export::{ExportSettings, ExportTemplate, ExportTextureSet};
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;

use crate::session::Session;

pub use subcraft_doc::{GameEngine as Engine, LiveLinkTarget};

/// The export template whose maps `engine`'s standard material reads.
pub fn template(engine: Engine) -> ExportTemplate {
    match engine {
        Engine::Unity => ExportTemplate::unity_urp(),
        Engine::Unreal => ExportTemplate::unreal(),
        Engine::Godot => ExportTemplate::godot(),
    }
}

struct OwnedSet {
    name: String,
    material_slot: u32,
    udim: Option<u32>,
    channels: BTreeMap<ChannelKind, TiledImage>,
    mesh_maps: Option<BTreeMap<MeshMapKind, TiledImage>>,
    mixing: subcraft_doc::MixingSettings,
}

/// Everything one send needs, detached from the session.
pub struct LiveLinkSnapshot {
    target: LiveLinkTarget,
    mesh: Arc<Mesh>,
    name: String,
    sets: Vec<OwnedSet>,
    settings: ExportSettings,
    /// Write the model too (first send, or the mesh changed since the last one).
    pub with_model: bool,
    /// The session revision the snapshot shows.
    pub revision: u64,
}

/// What a send wrote.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SendReport {
    pub dir: PathBuf,
    pub textures: usize,
    pub model: Option<PathBuf>,
    pub materials: Vec<PathBuf>,
}

impl SendReport {
    pub fn summary(&self, engine: Engine) -> String {
        let mut s = format!("Sent {} texture(s) to {}", self.textures, engine.label());
        if self.model.is_some() {
            s += " with the model";
        }
        if !self.materials.is_empty() {
            s += &format!(" and {} material(s)", self.materials.len());
        }
        s
    }
}

impl Session {
    /// Take what a live-link send needs from the finished composites (call when
    /// [`Session::composites_pending`] is false, so the export shows the current painting).
    pub fn live_link_snapshot(&self, target: &LiveLinkTarget, with_model: bool) -> LiveLinkSnapshot {
        let mut settings = self.effective_export_settings(&ExportSettings::default());
        settings.template = template(target.engine);
        settings.selection = subcraft_export::MapSelection::default();
        let sets = self
            .document()
            .texture_sets
            .iter()
            .filter_map(|ts| {
                let c = self.composite(ts.id)?;
                Some(OwnedSet {
                    name: ts.name.clone(),
                    material_slot: ts.material_slot,
                    udim: ts.udim_number(),
                    channels: c.channels.clone(),
                    mesh_maps: self.mesh_maps(ts.id).map(|m| m.as_map().clone()),
                    mixing: ts.mixing,
                })
            })
            .collect();
        LiveLinkSnapshot { target: target.clone(), mesh: self.scene().mesh.clone(), name: self.export_name(), sets, settings, with_model, revision: self.revision() }
    }

    /// The project's saved live-link target.
    pub fn live_link_target(&self) -> Option<&LiveLinkTarget> {
        self.settings.live_link.as_ref()
    }

    /// Remember (or forget, with `None`) where this project sends to. Saved with the project.
    pub fn set_live_link_target(&mut self, target: Option<LiveLinkTarget>) {
        self.settings.live_link = target;
    }
}

impl LiveLinkSnapshot {
    pub fn engine(&self) -> Engine {
        self.target.engine
    }

    /// Write the textures (and the model / materials) into the engine project.
    pub fn send(&self) -> Result<SendReport, String> {
        let root = Path::new(&self.target.project);
        if !root.is_dir() {
            return Err(format!("the {} project folder is missing: {}", self.target.engine.label(), root.display()));
        }
        let dir = self.target.engine.asset_dir(root, &subcraft_export::sanitize(&self.name));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let sets: Vec<ExportTextureSet<'_>> = self
            .sets
            .iter()
            .map(|s| ExportTextureSet { name: &s.name, material_slot: s.material_slot, udim: s.udim, channels: &s.channels, mesh_maps: s.mesh_maps.as_ref(), mixing: s.mixing })
            .collect();
        let written = subcraft_export::export(&self.mesh, &self.name, &sets, &self.settings, &dir).map_err(|e| e.to_string())?;
        let mut report = SendReport { dir: dir.clone(), textures: written.len(), ..Default::default() };
        if self.with_model {
            let path = dir.join(format!("{}.obj", subcraft_export::sanitize(&self.name)));
            subcraft_mesh::import::write_obj(&self.mesh, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            report.model = Some(path);
        }
        if self.target.engine == Engine::Godot {
            for set in &self.sets {
                let path = dir.join(format!("{}.tres", subcraft_export::sanitize(&set.name)));
                let tres = godot_material(root, &set.name, &written);
                std::fs::write(&path, tres).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                report.materials.push(path);
            }
        }
        Ok(report)
    }
}

/// A Godot 4 StandardMaterial3D (text resource) using texture set `set`'s Godot (ORM) maps among
/// `written` (paths inside the Godot project at `root`).
pub fn godot_material(root: &Path, set: &str, written: &[PathBuf]) -> String {
    let set_key = format!("_{}_", subcraft_export::sanitize(set));
    let find = |suffix: &str| {
        written.iter().find(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            stem.ends_with(&format!("_{suffix}")) && format!("{stem}_").contains(&set_key)
        })
    };
    let res = |p: &PathBuf| format!("res://{}", p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/"));
    let mut ext = Vec::new();
    let mut props = Vec::new();
    let mut add = |suffix: &str, lines: &dyn Fn(usize) -> Vec<String>| {
        if let Some(p) = find(suffix) {
            ext.push(format!("[ext_resource type=\"Texture2D\" path=\"{}\" id=\"{}\"]", res(p), ext.len() + 1));
            props.extend(lines(ext.len()));
        }
    };
    add("BaseColor", &|i| vec![format!("albedo_texture = ExtResource(\"{i}\")")]);
    add("ORM", &|i| {
        vec![
            "ao_enabled = true".into(),
            format!("ao_texture = ExtResource(\"{i}\")"),
            "ao_texture_channel = 0".into(),
            "roughness = 1.0".into(),
            format!("roughness_texture = ExtResource(\"{i}\")"),
            "roughness_texture_channel = 1".into(),
            "metallic = 1.0".into(),
            format!("metallic_texture = ExtResource(\"{i}\")"),
            "metallic_texture_channel = 2".into(),
        ]
    });
    add("Normal", &|i| vec!["normal_enabled = true".into(), format!("normal_texture = ExtResource(\"{i}\")")]);
    add("Emissive", &|i| vec!["emission_enabled = true".into(), "emission = Color(1, 1, 1, 1)".into(), format!("emission_texture = ExtResource(\"{i}\")")]);
    let mut out = format!("[gd_resource type=\"StandardMaterial3D\" load_steps={} format=3]\n\n", ext.len() + 1);
    for line in &ext {
        out += line;
        out += "\n";
    }
    out += "\n[resource]\n";
    out += &format!("resource_name = \"{}\"\n", set.replace('"', "'"));
    for p in props {
        out += &p;
        out += "\n";
    }
    out
}
