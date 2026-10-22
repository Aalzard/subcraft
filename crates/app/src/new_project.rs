//! New Project: what to start a project with. The dialog edits a [`NewProjectSettings`]; the app
//! turns a mesh plus those settings into a [`Session`] (texture sets, channels, resolution, UDIM
//! tiles, normal-map format). Nothing here knows about widgets.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;
use subcraft_doc::{ChannelDef, Document, Layer, NormalFormat, ProjectSettings, ResourceStore, TextureSet};
use subcraft_mesh::Mesh;
use subcraft_mesh::regroup::{Tile, is_valid_udim, udim_number};

use crate::Session;

/// Channel presets (Painter's "template"): which channels a project starts with. More can be added
/// later from Properties > Channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplate {
    #[default]
    PbrMetalRough,
    UnrealOrm,
    Unity,
    Godot,
    Stylized,
}

impl ProjectTemplate {
    pub const ALL: [ProjectTemplate; 5] = [Self::PbrMetalRough, Self::UnrealOrm, Self::Unity, Self::Godot, Self::Stylized];

    pub fn label(self) -> &'static str {
        match self {
            Self::PbrMetalRough => "PBR Metallic Roughness",
            Self::UnrealOrm => "Unreal Engine (ORM)",
            Self::Unity => "Unity (HDRP / URP)",
            Self::Godot => "Godot 4",
            Self::Stylized => "Stylized (Base Color only)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::PbrMetalRough => "Base Color, Height, Roughness, Metallic, Normal. A good start for anything.",
            Self::UnrealOrm => "Adds Ambient Occlusion so Occlusion / Roughness / Metallic can be packed. DirectX normals on export.",
            Self::Unity => "Adds Ambient Occlusion and Emission for the Mask Map / Metallic + Occlusion exports.",
            Self::Godot => "Adds Ambient Occlusion and Emission for Godot's ORM texture.",
            Self::Stylized => "Only Base Color: hand-painted looks, the lightest project.",
        }
    }

    pub fn channels(self) -> Vec<ChannelKind> {
        use ChannelKind::*;
        match self {
            Self::PbrMetalRough => vec![BaseColor, Height, Roughness, Metallic, Normal],
            Self::UnrealOrm => vec![BaseColor, Height, Roughness, Metallic, Normal, AmbientOcclusion],
            Self::Unity | Self::Godot => vec![BaseColor, Height, Roughness, Metallic, Normal, AmbientOcclusion, Emissive],
            Self::Stylized => vec![BaseColor],
        }
    }

    /// Normal-map convention this engine expects.
    pub fn normal_format(self) -> NormalFormat {
        match self {
            Self::UnrealOrm => NormalFormat::DirectX,
            _ => NormalFormat::OpenGl,
        }
    }

    /// Name of the matching export template (see `subcraft_export::ExportTemplate`).
    pub fn export_template(self) -> &'static str {
        match self {
            Self::PbrMetalRough | Self::Stylized => "PBR Metallic Roughness",
            Self::UnrealOrm => "Unreal Engine (ORM packed)",
            Self::Unity => "Unity URP (Metallic + Occlusion)",
            Self::Godot => "Godot 4 (ORM)",
        }
    }
}

/// How the model's triangles are grouped into texture sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitRule {
    /// One texture set per material (the usual choice).
    #[default]
    ByMaterial,
    /// One texture set per object name in the file.
    ByObject,
    /// Everything in one texture set.
    Single,
}

impl SplitRule {
    pub const ALL: [SplitRule; 3] = [Self::ByMaterial, Self::ByObject, Self::Single];

    pub fn label(self) -> &'static str {
        match self {
            Self::ByMaterial => "By material",
            Self::ByObject => "By object",
            Self::Single => "One texture set",
        }
    }
}

/// Resolutions offered for a new project and for a single texture set.
pub const PROJECT_RESOLUTIONS: [u32; 5] = [256, 512, 1024, 2048, 4096];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewProjectSettings {
    pub template: ProjectTemplate,
    /// Channels the project starts with (starts as the template's; the user may add or remove).
    pub channels: Vec<ChannelKind>,
    pub resolution: u32,
    /// Material (texture-set base name, without a `.1001` tile suffix) -> resolution, for materials
    /// that differ from `resolution`. All tiles of a material share one resolution.
    pub resolution_overrides: BTreeMap<String, u32>,
    pub normal_format: NormalFormat,
    pub split: SplitRule,
    /// Give each UDIM tile its own texture set (only has an effect when the model uses 2+ tiles).
    pub udim: bool,
    /// Optional high-poly model to bake from after creating the project.
    pub high_poly: Option<PathBuf>,
}

impl Default for NewProjectSettings {
    fn default() -> Self {
        let template = ProjectTemplate::default();
        Self {
            template,
            channels: template.channels(),
            resolution: 2048,
            resolution_overrides: BTreeMap::new(),
            normal_format: template.normal_format(),
            split: SplitRule::default(),
            udim: true,
            high_poly: None,
        }
    }
}

impl NewProjectSettings {
    /// Switch template: channels and normal format follow it.
    pub fn apply_template(&mut self, template: ProjectTemplate) {
        self.template = template;
        self.channels = template.channels();
        self.normal_format = template.normal_format();
    }

    pub fn has_channel(&self, kind: ChannelKind) -> bool {
        self.channels.contains(&kind)
    }

    /// Turn a channel on or off. Base Color stays: every project needs something to paint.
    pub fn set_channel(&mut self, kind: ChannelKind, on: bool) {
        if on && !self.channels.contains(&kind) {
            self.channels.push(kind);
            self.channels.sort_by_key(|k| ChannelKind::ALL.iter().position(|a| a == k));
        } else if !on && kind != ChannelKind::BaseColor {
            self.channels.retain(|k| *k != kind);
        }
    }

    /// Resolution of a texture set (or of the material `set_name` belongs to).
    pub fn resolution_for(&self, set_name: &str) -> u32 {
        self.resolution_overrides.get(set_name).copied().unwrap_or(self.resolution)
    }

    fn valid_resolution(r: u32) -> u32 {
        subcraft_doc::RESOLUTIONS.iter().copied().min_by_key(|v| v.abs_diff(r)).unwrap_or(2048)
    }
}

/// One UDIM tile of a model.
#[derive(Clone, Debug, PartialEq)]
pub struct TileInfo {
    pub tile: Tile,
    pub number: u32,
    pub triangles: usize,
}

/// What the dialog shows after a model is loaded.
#[derive(Clone, Debug, Default)]
pub struct MeshSummary {
    pub name: String,
    pub triangles: usize,
    pub vertices: usize,
    pub objects: Vec<(String, usize)>,
    pub materials: Vec<(String, usize)>,
    pub uv_bounds: Option<(Vec2, Vec2)>,
    pub tiles: Vec<TileInfo>,
    pub has_uvs: bool,
    /// Share (0..1) of covered texels that more than one triangle lands on, per tile summed.
    pub overlap: f32,
    pub warnings: Vec<String>,
}

impl MeshSummary {
    pub fn uses_udim(&self) -> bool {
        self.tiles.len() > 1
    }

    /// Whether the model can be painted at all.
    pub fn paintable(&self) -> bool {
        self.has_uvs && self.triangles > 0
    }
}

/// Read a model's layout and collect warnings (`import_warnings` come from the importer).
pub fn summarize(mesh: &Mesh, import_warnings: &[String]) -> MeshSummary {
    let mut s = MeshSummary { name: mesh.name.clone(), triangles: mesh.triangle_count(), vertices: mesh.vertex_count(), ..Default::default() };
    for (i, m) in mesh.materials.iter().enumerate() {
        s.materials.push((m.name.clone(), mesh.material_triangles(i as u32).len()));
    }
    if mesh.objects.is_empty() {
        s.objects.push((mesh.name.clone(), mesh.triangle_count()));
    } else {
        for (i, o) in mesh.objects.iter().enumerate() {
            s.objects.push((o.name.clone(), mesh.tri_object.iter().filter(|t| **t == Some(i as u32)).count()));
        }
        let loose = mesh.tri_object.iter().filter(|t| t.is_none()).count();
        if loose > 0 {
            s.objects.push(("(no object)".into(), loose));
        }
    }
    s.uv_bounds = mesh.uv_bounds();
    let spread = s.uv_bounds.is_some_and(|(lo, hi)| (hi - lo).length() > 1e-6);
    s.has_uvs = spread;
    s.tiles = mesh.udim_tiles(None).into_iter().map(|(tile, triangles)| TileInfo { tile, number: udim_number(tile), triangles }).collect();
    s.tiles.sort_by_key(|t| t.number);
    s.warnings.extend(import_warnings.iter().cloned());
    if !spread && s.triangles > 0 {
        s.warnings.push("The model has no UV coordinates, so it cannot be painted. Use it as a high-poly bake source, or unwrap it in a modeling tool first (automatic unwrapping is not available yet).".into());
    }
    if s.uses_udim() {
        let list: Vec<String> = s.tiles.iter().map(|t| t.number.to_string()).collect();
        s.warnings.push(format!("The UVs use {} UDIM tiles ({}). Without the UDIM option they would be folded onto one tile and overlap.", s.tiles.len(), list.join(", ")));
        if s.tiles.iter().any(|t| !is_valid_udim(t.tile)) {
            s.warnings.push("Some UVs lie left of u = 0 or below v = 0, outside the UDIM range (tiles start at 1001, going right and up). They still get their own texture sets, numbered below 1001; check the unwrap.".into());
        }
        let crossing = mesh.triangles_crossing_tiles();
        if crossing > 0 {
            s.warnings.push(format!("{crossing} triangles cross a UDIM tile border; they are painted in the tile holding their center."));
        }
    }
    if spread {
        s.overlap = overlap_share(mesh);
        if s.overlap > 0.02 {
            s.warnings.push(format!("About {:.0}% of the UV space is covered by more than one triangle: overlapping islands share paint (fine for mirrored parts, a problem for unique detail).", s.overlap * 100.0));
        }
    }
    s
}

/// Rasterize all triangles into a coarse grid per tile and count texels hit more than once.
fn overlap_share(mesh: &Mesh) -> f32 {
    use subcraft_core::IRect;
    const N: u32 = 256;
    let mut grids: BTreeMap<Tile, Vec<u8>> = BTreeMap::new();
    for t in 0..mesh.triangle_count() {
        let tile = mesh.triangle_tile(t);
        let grid = grids.entry(tile).or_insert_with(|| vec![0u8; (N * N) as usize]);
        subcraft_mesh::uv_raster::raster_triangle(mesh.tri_uvs(t), N, N, IRect::from_size(N, N), 0.0, |h| {
            let cell = &mut grid[(h.y as u32 * N + h.x as u32) as usize];
            *cell = cell.saturating_add(1);
        });
    }
    let (mut covered, mut multiple) = (0usize, 0usize);
    for grid in grids.values() {
        covered += grid.iter().filter(|&&c| c > 0).count();
        multiple += grid.iter().filter(|&&c| c > 1).count();
    }
    if covered == 0 { 0.0 } else { multiple as f32 / covered as f32 }
}

/// A texture set the new project will have.
#[derive(Clone, Debug, PartialEq)]
pub struct SetPlan {
    pub name: String,
    /// The material / group the set belongs to (`name` without the tile suffix).
    pub base: String,
    pub tile: Option<Tile>,
    pub triangles: usize,
}

/// The texture sets `settings` would create for `mesh` (also what the dialog lists).
pub fn plan_sets(mesh: &Mesh, settings: &NewProjectSettings) -> Vec<SetPlan> {
    grouping(mesh, settings).2
}

/// (slot of each triangle, uv shift per slot, plans per slot, names per slot).
type Grouping = (Vec<u32>, Vec<Vec2>, Vec<SetPlan>, Vec<String>);

fn grouping(mesh: &Mesh, settings: &NewProjectSettings) -> Grouping {
    let n = mesh.triangle_count();
    // First the base group of every triangle.
    let (base_of, base_names): (Vec<u32>, Vec<String>) = match settings.split {
        SplitRule::ByMaterial => (mesh.tri_material.clone(), mesh.materials.iter().map(|m| m.name.clone()).collect()),
        SplitRule::Single => (vec![0; n], vec![if mesh.name.is_empty() { "Model".to_string() } else { mesh.name.clone() }]),
        SplitRule::ByObject if mesh.objects.is_empty() || mesh.tri_object.is_empty() => {
            (vec![0; n], vec![if mesh.name.is_empty() { "Model".to_string() } else { mesh.name.clone() }])
        }
        SplitRule::ByObject => {
            let loose = mesh.objects.len() as u32;
            let mut names: Vec<String> = mesh.objects.iter().map(|o| o.name.clone()).collect();
            names.push("Unassigned".into());
            (mesh.tri_object.iter().map(|o| o.unwrap_or(loose)).collect(), unique_names(names))
        }
    };
    let udim = settings.udim && mesh.udim_tiles(None).len() > 1;
    // Slots are ordered by group, then by UDIM number (1001, 1002, ... 1011).
    let tile_key = |t: usize| -> Option<(u32, Tile)> {
        udim.then(|| {
            let tile = mesh.triangle_tile(t);
            (udim_number(tile), tile)
        })
    };
    let mut keys: BTreeMap<(u32, Option<(u32, Tile)>), usize> = BTreeMap::new();
    let tri_key: Vec<(u32, Option<(u32, Tile)>)> = (0..n).map(|t| (base_of[t], tile_key(t))).collect();
    for key in &tri_key {
        *keys.entry(*key).or_insert(0) += 1;
    }
    let mut slot_of_key = BTreeMap::new();
    let (mut plans, mut names, mut shifts) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (key, triangles)) in keys.iter().enumerate() {
        slot_of_key.insert(*key, i as u32);
        let base = &base_names[key.0 as usize];
        let tile = key.1.map(|(_, tile)| tile);
        let name = match key.1 {
            Some((number, _)) => format!("{base}.{number}"),
            None => base.clone(),
        };
        shifts.push(tile.map_or(Vec2::ZERO, |t| Vec2::new(t[0] as f32, t[1] as f32)));
        names.push(name.clone());
        plans.push(SetPlan { name, base: base.clone(), tile, triangles: *triangles });
    }
    let slot_of_tri = tri_key.iter().map(|k| slot_of_key[k]).collect();
    (slot_of_tri, shifts, plans, names)
}

fn unique_names(mut names: Vec<String>) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for name in &mut names {
        let count = seen.entry(name.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            *name = format!("{name} {count}");
        }
    }
    names
}

impl Session {
    /// A new project from `mesh` with `settings`: texture sets per the split rule (and per UDIM
    /// tile), the chosen channels and resolutions, and the normal-map format. Returns the session
    /// and any warnings. The mesh is only rebuilt when the grouping differs from its own.
    pub fn from_mesh_with(mesh: Mesh, settings: &NewProjectSettings) -> Result<Self, String> {
        if mesh.triangle_count() == 0 {
            return Err("the model has no triangles".into());
        }
        if mesh.uv_bounds().is_none_or(|(lo, hi)| (hi - lo).length() <= 1e-6) {
            return Err("the model has no UV coordinates, so it cannot be painted".into());
        }
        if settings.channels.is_empty() {
            return Err("choose at least one channel".into());
        }
        let (slot_of_tri, shifts, plans, names) = grouping(&mesh, settings);
        let identity = !plans.iter().any(|p| p.tile.is_some())
            && plans.len() == mesh.materials.len()
            && slot_of_tri.iter().zip(&mesh.tri_material).all(|(a, b)| a == b)
            && plans.iter().zip(&mesh.materials).all(|(p, m)| p.name == m.name);
        let (mesh, kept) = if identity { (mesh, (0..plans.len() as u32).collect()) } else { mesh.regroup(&names, &slot_of_tri, &shifts) };
        let mut sets = Vec::new();
        for (slot, &plan_index) in kept.iter().enumerate() {
            let plan = &plans[plan_index as usize];
            let resolution = NewProjectSettings::valid_resolution(settings.resolution_for(&plan.base));
            let mut ts = TextureSet::new(plan.name.clone(), slot as u32, resolution);
            ts.channels = settings.channels.iter().map(|&k| ChannelDef::default_for(k)).collect();
            ts.udim_tile = plan.tile;
            let tc = ts.clone();
            ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
            sets.push(ts);
        }
        let mut session = Session::from_parts(mesh, Document { texture_sets: sets }, ResourceStore::default(), "New Project");
        session.set_settings(ProjectSettings {
            normal_format: Some(settings.normal_format),
            export_template: settings.template.export_template().to_string(),
            ..Default::default()
        });
        Ok(session)
    }
}
