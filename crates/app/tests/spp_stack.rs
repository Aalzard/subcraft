//! Rules of the Painter stack rebuild on hand-made layer descriptions (no Painter file needed
//! beyond the synthetic container the rebuilder reads embedded images from).

#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod fixture;

use std::io::Cursor;

use subcraft_app::spp_stack::Rebuilder;
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ContentEffectKind, Filter, LayerContent, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_mesh::spp::document::{ActionInfo, ActionKind, GraphRef, LayerInfo, LayerKind, SourceInfo};
use subcraft_mesh::spp::{SppFile, channel_bits};

fn action(kind: ActionKind, own: Vec<(u64, i32, f32)>) -> ActionInfo {
    ActionInfo { kind, enabled: true, label: String::new(), channels: u64::MAX, blends: vec![(u64::MAX, 25, 1.0)], group: 1, own_blends: own, uv: None }
}

fn layer(name: &str, content: Vec<ActionInfo>, mask: Option<(Vec<ActionInfo>, bool)>) -> LayerInfo {
    LayerInfo {
        name: name.into(),
        kind: LayerKind::Fill,
        visible: true,
        blends: Vec::new(),
        has_mask: mask.is_some(),
        sources: Vec::new(),
        strokes: 0,
        content,
        mask: mask.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
        mask_enabled: true,
        mask_initial_white: mask.is_some_and(|m| m.1),
        mesh_filter: None,
        children: Vec::new(),
    }
}

fn uniform(bits: u64, v: f32) -> ActionInfo {
    action(ActionKind::Fill { sources: vec![SourceInfo::Uniform { channels: bits, color: [v; 3] }], projection: 0 }, Vec::new())
}

fn graph(name: &str, tweaks: &[(&str, f32)]) -> GraphRef {
    GraphRef { url: format!("resource://starter_assets/{name}/{name}?version=0.sbsar"), tweaks: tweaks.iter().map(|(k, v)| (k.to_string(), vec![*v])).collect() }
}

#[test]
fn rebuild_rules() {
    let mut spp = SppFile::from_reader(Cursor::new(fixture::project_file())).unwrap();
    let ts = TextureSet::new("T", 0, 64);
    let mut resources = ResourceStore::default();
    let mut r = Rebuilder::new(&mut spp, &ts, &mut resources);
    let infos = vec![
        // An empty mask that starts black hides the layer (it is not "left off").
        layer("Hidden", vec![uniform(channel_bits::BASE_COLOR, 0.9)], Some((Vec::new(), false))),
        // A mask fill keeps its own blending (id 4 is Multiply).
        layer("Masked", vec![uniform(channel_bits::ROUGHNESS, 0.2)], Some((vec![uniform(0, 1.0), action(ActionKind::Fill { sources: vec![SourceInfo::Uniform { channels: 0, color: [0.5; 3] }], projection: 0 }, vec![(u64::MAX, 4, 0.8)])], false))),
        // A layer that only filters is an empty pass-through folder carrying the filter.
        layer("Sharpen", vec![action(ActionKind::Filter { graph: graph("sharpen", &[("intensity", 0.3)]) }, vec![(u64::MAX, 1, 1.0)])], None),
        // A procedural material fill becomes a uniform fill whose values are fitted later.
        layer("Plastic", vec![action(ActionKind::Fill { sources: vec![SourceInfo::Procedural { channels: channel_bits::BASE_COLOR | channel_bits::ROUGHNESS | channel_bits::NORMAL, graph: graph("Plastic", &[]) }], projection: 2 }, Vec::new())], None),
    ];
    let layers = r.rebuild(&infos);
    let fitted = r.fitted.clone();

    let hidden = layers[0].mask.as_ref().expect("the empty mask is kept");
    assert!(hidden.effects.is_empty() && hidden.base.get(3, 3)[0] == 0.0);

    let masked = layers[1].mask.as_ref().unwrap();
    assert_eq!(masked.effects.len(), 2);
    assert_eq!((masked.effects[0].blend, masked.effects[1].blend, masked.effects[1].opacity), (BlendMode::Normal, BlendMode::Multiply, 0.8));
    assert!(matches!(masked.effects[1].kind, MaskEffectKind::Fill { value, .. } if value == 0.5));

    let sharpen = &layers[2];
    assert!(matches!(sharpen.content, LayerContent::Group(ref g) if g.children.is_empty()));
    assert!(sharpen.blend.values().all(|b| b.mode == BlendMode::Passthrough));
    assert!(!sharpen.content_effects.is_empty());
    assert!(sharpen.content_effects.iter().all(|e| matches!(e.kind, ContentEffectKind::Filter { filter: Filter::Sharpen { .. }, channel } if channel != ChannelKind::Normal)));

    let plastic = &layers[3];
    let LayerContent::Fill(fill) = &plastic.content else { panic!("a fill layer") };
    assert!(fill.channels.contains_key(&ChannelKind::BaseColor) && fill.channels.contains_key(&ChannelKind::Roughness));
    // The material's normal starts flat (solved from Painter's result at import) and is not fitted.
    assert!(matches!(fill.channels[&ChannelKind::Normal].source, subcraft_doc::FillSource::Uniform { .. }));
    assert_eq!(fitted, vec![(plastic.id, vec![ChannelKind::BaseColor, ChannelKind::Roughness])]);
}

#[test]
fn a_material_normal_is_solved_from_painters_result_into_its_layer() {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use subcraft_image::TiledImage;
    let mut spp = SppFile::from_reader(Cursor::new(fixture::project_file())).unwrap();
    let mut ts = TextureSet::new("T", 0, 32);
    let mut resources = ResourceStore::default();
    // A layer that puts a procedural material into Normal only (as Painter's Rubber / Plastic).
    let rubber = layer("Rubber", vec![action(ActionKind::Fill { sources: vec![SourceInfo::Procedural { channels: channel_bits::NORMAL, graph: graph("Rubber - Synthetic", &[]) }], projection: 0 }, Vec::new())], None);
    let (layers, fitted) = {
        let mut r = Rebuilder::new(&mut spp, &ts, &mut resources);
        let layers = r.rebuild(&[rubber]);
        (layers, r.fitted.clone())
    };
    assert_eq!(fitted, vec![(layers[0].id, Vec::new())], "listed for the solve, with no fitted channel");
    let mut folder = subcraft_doc::Layer::new_folder("Painter stack (reconstructed)", &ts);
    if let LayerContent::Group(g) = &mut folder.content {
        g.children = layers.into_iter().map(Arc::new).collect();
    }
    ts.layers = vec![Arc::new(folder)];
    // Painter's normal: tilted along x everywhere.
    let n = glam::Vec3::new(0.3, 0.0, 1.0).normalize() * 0.5 + glam::Vec3::splat(0.5);
    let mut target = TiledImage::new(32, 32, 3, [0.5, 0.5, 1.0, 0.0]);
    for y in 0..32 {
        for x in 0..32 {
            target.set(x, y, [n.x, n.y, n.z, 0.0]);
        }
    }
    let flat = BTreeMap::from([(ChannelKind::Normal, target)]);
    let mesh = subcraft_mesh::primitives::cube(2.0);
    let (_, solved, composite) = subcraft_app::spp_import::fit_solve_composite(&mut ts, &fitted, &mesh, &mut resources, &BTreeMap::new(), &flat, None, true);
    assert_eq!(solved.len(), 1);
    assert_eq!(solved[0].1, ChannelKind::Normal);
    assert!(resources.get(solved[0].2).unwrap().name.contains("solved from Painter's result"));
    let got = composite.channel(ChannelKind::Normal).unwrap().get(16, 16);
    assert!((got[0] - n.x).abs() < 0.02 && (got[2] - n.z).abs() < 0.02, "{got:?} vs {n}");
}

#[test]
fn fractal_sums_stay_patterns_and_generators_take_painters_breakup() {
    let mut spp = SppFile::from_reader(Cursor::new(fixture::project_file())).unwrap();
    let ts = TextureSet::new("T", 0, 64);
    let mut resources = ResourceStore::default();
    let mut r = Rebuilder::new(&mut spp, &ts, &mut resources);
    let grain = graph("fractal_sum_base_colored", &[("Min_Level", 9.0), ("Max_Level", 9.0), ("Position", 0.5)]);
    let dirt = graph("mg_dirt", &[("dirt_level", 0.3), ("dirt_contrast", 0.5), ("grunge_amount", 0.12)]);
    let infos = vec![
        layer("Grain", vec![action(ActionKind::Fill { sources: vec![SourceInfo::Procedural { channels: channel_bits::BASE_COLOR, graph: grain }], projection: 2 }, Vec::new())], None),
        layer("Dirt", vec![uniform(channel_bits::BASE_COLOR, 0.4)], Some((vec![action(ActionKind::Fill { sources: vec![SourceInfo::Procedural { channels: 0, graph: dirt }], projection: 0 }, Vec::new())], false))),
    ];
    let layers = r.rebuild(&infos);
    assert!(r.fitted.is_empty(), "the fractal sum is drawn, not fitted: {:?}", r.fitted);
    let LayerContent::Fill(fill) = &layers[0].content else { panic!("a fill") };
    let subcraft_doc::FillSource::Procedural { params, .. } = &fill.channels[&ChannelKind::BaseColor].source else { panic!("a procedural source") };
    assert!(params.colored && params.scale == 512.0, "{params:?}");
    let mask = layers[1].mask.as_ref().unwrap();
    let MaskEffectKind::Generator(g) = &mask.effects.last().unwrap().kind else { panic!("a generator") };
    assert_eq!(g.noise_amount, 0.12, "Painter's grunge amount, not SubCraft's default breakup");
}

#[test]
fn painter_blending_ids() {
    use subcraft_app::spp_stack::painter_blend;
    let table: Vec<(i32, Option<BlendMode>)> = [2, 4, 9, 10, 11, 15, 25, 29, 3].iter().map(|&id| (id, painter_blend(id))).collect();
    assert_eq!(table, vec![
        (2, Some(BlendMode::HardLight)),
        (4, Some(BlendMode::Multiply)),
        (9, Some(BlendMode::Color)),
        (10, Some(BlendMode::LinearBurn)),
        (11, Some(BlendMode::SoftLight)),
        (15, Some(BlendMode::SoftLight)),
        (25, Some(BlendMode::Normal)),
        (29, Some(BlendMode::Subtract)),
        (3, None),
    ]);
}

#[test]
fn geometry_mask_restricts_a_layer_to_its_mesh_objects() {
    let mut spp = SppFile::from_reader(Cursor::new(fixture::project_file())).unwrap();
    let mut mesh = subcraft_mesh::primitives::cube(2.0);
    mesh.objects = vec![
        subcraft_mesh::MeshObject { id: subcraft_core::ObjectId::new(), name: "Body".into() },
        subcraft_mesh::MeshObject { id: subcraft_core::ObjectId::new(), name: "Lens".into() },
        subcraft_mesh::MeshObject { id: subcraft_core::ObjectId::new(), name: "Strap".into() },
    ];
    let ts = TextureSet::new("T", 0, 64);
    let mut resources = ResourceStore::default();
    let mut r = Rebuilder::new(&mut spp, &ts, &mut resources);
    r.geometry = Some(std::sync::Arc::new(subcraft_materials::TextureSetGeometry::new(std::sync::Arc::new(mesh), 0, 64)));
    let mut only = layer("Only lens", vec![uniform(channel_bits::BASE_COLOR, 0.9)], None);
    only.mesh_filter = Some((false, vec!["Lens".into()]));
    let mut except = layer("All but strap", vec![uniform(channel_bits::BASE_COLOR, 0.9)], None);
    except.mesh_filter = Some((true, vec!["Strap".into()]));
    let layers = r.rebuild(&[only, except, layer("Everywhere", vec![uniform(channel_bits::BASE_COLOR, 0.9)], None)]);
    let parts = |l: &subcraft_doc::Layer| l.parts.as_ref().map(|p| (p.kind, p.parts.iter().copied().collect::<Vec<_>>()));
    assert_eq!(parts(&layers[0]), Some((subcraft_core::PartKind::Object, vec![1])));
    assert_eq!(parts(&layers[1]), Some((subcraft_core::PartKind::Object, vec![0, 1])));
    assert_eq!(parts(&layers[2]), None);
}

#[test]
fn gutter_fill_copies_the_nearest_island_texel_outside_the_islands_only() {
    use std::sync::Arc;
    use subcraft_image::TiledImage;
    use subcraft_materials::TextureSetGeometry;
    let geometry = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::cube(2.0)), 0, 64);
    let mut inside = vec![false; 64 * 64];
    for t in 0..geometry.tile_count() {
        let tile = geometry.tile(t);
        for i in 0..subcraft_image::TILE_TEXELS {
            let (x, y) = tile.texel_xy(i);
            if x < 64 && y < 64 && tile.triangle(i).is_some() {
                inside[(y * 64 + x) as usize] = true;
            }
        }
    }
    assert!(inside.iter().any(|&b| !b), "the cube's layout leaves a gutter");
    // A mask painted on the islands only: value varies with x, alpha 1.
    let mut img = TiledImage::new(64, 64, 2, [0.0; 4]);
    for y in 0..64u32 {
        for x in 0..64u32 {
            if inside[(y * 64 + x) as usize] {
                img.set(x, y, [x as f32 / 64.0, 1.0, 0.0, 0.0]);
            }
        }
    }
    let filled = subcraft_app::spp_stack::GutterFill::new(&geometry).fill(&img);
    for y in 0..64u32 {
        for x in 0..64u32 {
            let (a, b) = (img.get(x, y), filled.get(x, y));
            if inside[(y * 64 + x) as usize] {
                assert_eq!((a[0], a[1]), (b[0], b[1]), "inside unchanged at {x},{y}");
            } else {
                assert_eq!(b[1], 1.0, "outside filled at {x},{y}");
                // The copied value comes from an island texel close by (its column is near).
                let near = (0..64u32).flat_map(|yy| (0..64u32).map(move |xx| (xx, yy))).filter(|&(xx, yy)| inside[(yy * 64 + xx) as usize]).map(|(xx, yy)| (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2)).fold(f32::MAX, f32::min).sqrt();
                assert!((b[0] * 64.0 - x as f32).abs() <= near + 1.0, "value from a near texel at {x},{y}");
            }
        }
    }
}

#[test]
fn the_sets_normal_map_is_a_top_layer() {
    let ts = TextureSet::new("T", 0, 64);
    let layer = subcraft_app::spp_import::baked_normal_layer(&ts);
    let LayerContent::Fill(fill) = &layer.content else { panic!("a fill") };
    assert_eq!(fill.channels.len(), 1);
    assert!(matches!(fill.channels[&ChannelKind::Normal].source, subcraft_doc::FillSource::MeshMap { map: subcraft_core::MeshMapKind::Normal }));
    assert_eq!(layer.blend[&ChannelKind::Normal].mode, BlendMode::NormalMapInverseDetail);
}


#[test]
fn roughness_grain_has_painters_texel_scale_spread_at_every_size() {
    use subcraft_doc::FillSource;
    let ts = TextureSet::new("T", 0, 64);
    let layer = subcraft_app::spp_import::roughness_grain_layer(&ts).expect("grain layer");
    let LayerContent::Fill(fill) = &layer.content else { panic!("a fill") };
    assert_eq!(fill.channels.len(), 1, "roughness only");
    assert_eq!(layer.blend[&ChannelKind::Roughness].mode, BlendMode::SignedAddition);
    let FillSource::Procedural { params, low, high } = &fill.channels[&ChannelKind::Roughness].source else { panic!("procedural") };
    // Painter's robust texel-to-texel sigma on DBAL-A2 (0.0073 at 512, 0.0138 at 2048) minus what the
    // rebuilt layers already carry (about 0.005) asks for 0.005..0.012; the same field must give it on
    // every grid (the first version sat on the noise lattice at power-of-two sizes and was flat).
    for size in [256u32, 512, 1024, 2048] {
        let mut values = Vec::new();
        for y in (0..size).step_by((size / 128) as usize) {
            for x in (0..size).step_by((size / 128) as usize) {
                let p = glam::Vec2::new((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32);
                let t = subcraft_materials::pattern::eval(params, p);
                values.push(2.0 * (low.0[0] + (high.0[0] - low.0[0]) * t) - 1.0);
            }
        }
        let n = values.len() as f32;
        let mean = values.iter().sum::<f32>() / n;
        let sigma = (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt();
        assert!(mean.abs() < 0.004, "{size}: mean shift {mean}");
        assert!((0.003..0.012).contains(&sigma), "{size}: sigma {sigma}");
    }
}
