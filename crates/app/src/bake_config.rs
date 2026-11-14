//! The Bake window's settings as saved with the project (issue #320): converting between the
//! baker's [`BakeSettings`] and the project file's [`BakeConfig`].
//!
//! The project file names the baker's options in words (`"vertex_color"`, `"cosine"`), so files
//! stay readable and an option the program no longer knows falls back to its default instead of
//! failing the load.

use subcraft_bake::{AoAttenuation, Antialiasing, BakeSettings, HeightNormalization, IdGenerator, IdSource, PositionNormalization, RayDistribution};
use subcraft_doc::BakeConfig;

/// The files and choices of the Bake window that live beside [`BakeSettings`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakeFilesConfig {
    pub high_poly: String,
    pub extra_high: Vec<String>,
    pub cage_file: String,
    /// `None`: the output size follows the texture sets' resolution.
    pub size: Option<u32>,
    /// Texture sets unticked for baking.
    pub skip_sets: Vec<subcraft_core::TextureSetId>,
}

fn id_source_name(s: IdSource) -> &'static str {
    match s {
        IdSource::VertexColor => "vertex_color",
        IdSource::MaterialColor => "material_color",
        IdSource::MeshId => "mesh_id",
        IdSource::Submesh => "submesh",
        IdSource::FileId => "file_id",
    }
}

fn distribution_name(d: RayDistribution) -> &'static str {
    match d {
        RayDistribution::Cosine => "cosine",
        RayDistribution::Uniform => "uniform",
    }
}

fn attenuation_name(a: AoAttenuation) -> &'static str {
    match a {
        AoAttenuation::None => "none",
        AoAttenuation::Smooth => "smooth",
        AoAttenuation::Linear => "linear",
    }
}

fn normalization_name(n: PositionNormalization) -> &'static str {
    match n {
        PositionNormalization::BoundingBox => "bounding_box",
        PositionNormalization::BoundingSphere => "bounding_sphere",
    }
}

fn generator_name(g: IdGenerator) -> &'static str {
    match g {
        IdGenerator::Random => "random",
        IdGenerator::HueShift => "hue_shift",
        IdGenerator::Grayscale => "grayscale",
    }
}

/// The option of `all` whose name is `name`, or `fallback`.
fn by_name<T: Copy>(all: &[T], name: &str, to_name: fn(T) -> &'static str, fallback: T) -> T {
    all.iter().copied().find(|v| to_name(*v) == name).unwrap_or(fallback)
}

/// What the project file stores for these settings and files.
pub fn to_config(s: &BakeSettings, files: &BakeFilesConfig) -> BakeConfig {
    let mut maps = s.maps.clone();
    maps.sort();
    maps.dedup();
    let mut skip_sets = files.skip_sets.clone();
    skip_sets.sort();
    BakeConfig {
        high_poly: files.high_poly.clone(),
        extra_high: files.extra_high.clone(),
        cage_file: files.cage_file.clone(),
        size: files.size,
        maps,
        skip_sets,
        ao_samples: s.ao_samples,
        ao_distance: s.ao_distance,
        ao_min_distance: s.ao.min_distance,
        ao_spread_angle: s.ao.spread_angle,
        ao_distribution: distribution_name(s.ao.distribution).into(),
        ao_attenuation: attenuation_name(s.ao.attenuation).into(),
        ao_ground_plane: s.ao.ground_plane,
        ao_ground_offset: s.ao.ground_offset,
        thickness_samples: s.thickness_samples,
        thickness_distance: s.thickness_distance,
        thickness_min_distance: s.thickness.min_distance,
        thickness_spread_angle: s.thickness.spread_angle,
        thickness_distribution: distribution_name(s.thickness.distribution).into(),
        curvature_radius: s.curvature_radius,
        curvature_from_normal: s.curvature_from_normal,
        normal_curvature_width: s.normal_curvature.radius_texels,
        normal_curvature_intensity: s.normal_curvature.intensity,
        cage_front: s.cage_front,
        cage_rear: s.cage_rear,
        id_source: id_source_name(s.id_source).into(),
        id_generator: generator_name(s.id_generator).into(),
        vertex_color_set: s.vertex_color_set,
        dilation: s.dilation,
        diffusion: s.diffusion,
        antialiasing: s.antialiasing.factor(),
        average_normals: s.average_normals,
        ignore_backface: s.ignore_backface,
        position_axis: s.position.single_axis,
        position_normalization: normalization_name(s.position.normalization).into(),
        height_range: match s.height {
            HeightNormalization::Cage => None,
            HeightNormalization::Manual { distance } => Some(distance),
        },
    }
}

/// Put a stored configuration into `settings` (options the file does not name keep their
/// defaults) and return the files part.
pub fn apply(c: &BakeConfig, s: &mut BakeSettings) -> BakeFilesConfig {
    s.maps = c.maps.clone();
    s.ao_samples = c.ao_samples.clamp(1, 4096);
    s.ao_distance = c.ao_distance;
    s.ao.min_distance = c.ao_min_distance;
    s.ao.spread_angle = c.ao_spread_angle.clamp(1.0, 180.0);
    s.ao.distribution = by_name(&RayDistribution::ALL, &c.ao_distribution, distribution_name, RayDistribution::default());
    s.ao.attenuation = by_name(&AoAttenuation::ALL, &c.ao_attenuation, attenuation_name, AoAttenuation::default());
    s.ao.ground_plane = c.ao_ground_plane;
    s.ao.ground_offset = c.ao_ground_offset;
    s.thickness_samples = c.thickness_samples.clamp(1, 4096);
    s.thickness_distance = c.thickness_distance;
    s.thickness.min_distance = c.thickness_min_distance;
    s.thickness.spread_angle = c.thickness_spread_angle.clamp(1.0, 180.0);
    s.thickness.distribution = by_name(&RayDistribution::ALL, &c.thickness_distribution, distribution_name, RayDistribution::default());
    s.curvature_radius = c.curvature_radius;
    s.curvature_from_normal = c.curvature_from_normal;
    s.normal_curvature.radius_texels = c.normal_curvature_width;
    s.normal_curvature.intensity = c.normal_curvature_intensity;
    s.cage_front = c.cage_front;
    s.cage_rear = c.cage_rear;
    s.id_source = by_name(&IdSource::ALL, &c.id_source, id_source_name, IdSource::default());
    s.id_generator = by_name(&IdGenerator::ALL, &c.id_generator, generator_name, IdGenerator::default());
    s.vertex_color_set = c.vertex_color_set.min(7);
    s.dilation = c.dilation;
    s.diffusion = c.diffusion;
    s.antialiasing = Antialiasing::ALL.into_iter().find(|a| a.factor() == c.antialiasing).unwrap_or_default();
    s.average_normals = c.average_normals;
    s.ignore_backface = c.ignore_backface;
    s.position.single_axis = c.position_axis.filter(|a| *a < 3);
    s.position.normalization = by_name(&PositionNormalization::ALL, &c.position_normalization, normalization_name, PositionNormalization::default());
    s.height = match c.height_range {
        Some(distance) => HeightNormalization::Manual { distance },
        None => HeightNormalization::Cage,
    };
    if let Some(size) = c.size {
        s.size = size.clamp(16, 8192);
    }
    BakeFilesConfig { high_poly: c.high_poly.clone(), extra_high: c.extra_high.clone(), cage_file: c.cage_file.clone(), size: c.size, skip_sets: c.skip_sets.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::MeshMapKind;

    #[test]
    fn every_bake_setting_survives_the_project_file() {
        let mut s = BakeSettings {
            maps: vec![MeshMapKind::Normal, MeshMapKind::Id, MeshMapKind::AmbientOcclusion],
            ao_samples: 96, ao_distance: 0.4, thickness_samples: 24, thickness_distance: 0.3, curvature_radius: 0.05,
            cage_front: 0.03, cage_rear: 0.02, curvature_from_normal: true, id_source: IdSource::VertexColor,
            dilation: 8, diffusion: false, antialiasing: Antialiasing::X4, average_normals: true, ignore_backface: true,
            id_generator: IdGenerator::Grayscale, vertex_color_set: 2, height: HeightNormalization::Manual { distance: 0.07 },
            ..Default::default()
        };
        s.normal_curvature.radius_texels = 3.5;
        s.normal_curvature.intensity = 2.0;
        s.ao.min_distance = 0.01;
        s.ao.spread_angle = 90.0;
        s.ao.distribution = RayDistribution::Uniform;
        s.ao.attenuation = AoAttenuation::Smooth;
        s.ao.ground_plane = true;
        s.ao.ground_offset = 0.2;
        s.position.single_axis = Some(2);
        s.position.normalization = PositionNormalization::BoundingSphere;
        s.thickness.min_distance = 0.02;
        s.thickness.spread_angle = 120.0;
        s.thickness.distribution = RayDistribution::Uniform;
        let set = subcraft_core::TextureSetId::new();
        let files = BakeFilesConfig { high_poly: "/models/crate_high.glb".into(), extra_high: vec!["/models/bolts_high.glb".into()], cage_file: "/models/cage.obj".into(), size: Some(2048), skip_sets: vec![set] };
        let config = to_config(&s, &files);
        // Through the project file's JSON and back.
        let json = serde_json::to_string(&subcraft_doc::ProjectSettings { bake: Some(config.clone()), ..Default::default() }).unwrap();
        assert!(json.contains("\"vertex_color\"") && json.contains("crate_high.glb"), "{json}");
        let back: subcraft_doc::ProjectSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bake.as_ref(), Some(&config));
        let mut restored = BakeSettings::default();
        let restored_files = apply(back.bake.as_ref().unwrap(), &mut restored);
        assert_eq!(restored_files, files);
        assert_eq!(restored.size, 2048);
        let mut expected = s.clone();
        expected.size = 2048;
        expected.maps.sort();
        assert_eq!(restored, expected, "every option comes back");
    }

    #[test]
    fn unknown_or_missing_options_fall_back_to_their_defaults() {
        let config: BakeConfig = serde_json::from_str(r#"{"id_source":"something_new","antialiasing":3,"ao_distribution":"?"}"#).unwrap();
        let mut s = BakeSettings::default();
        let files = apply(&config, &mut s);
        assert_eq!((s.id_source, s.antialiasing, s.ao.distribution), (IdSource::default(), Antialiasing::None, RayDistribution::Cosine));
        assert_eq!(files.size, None, "no size: it follows the texture sets");
        // A configuration of the defaults round-trips to the default settings.
        let mut d = BakeSettings::default();
        apply(&to_config(&BakeSettings::default(), &BakeFilesConfig::default()), &mut d);
        let mut want = BakeSettings::default();
        want.maps.sort();
        assert_eq!(d, want);
    }
}
