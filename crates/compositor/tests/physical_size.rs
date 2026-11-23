//! Scale "Physical size" for fills: one repeat has the same size in centimeters on every model,
//! whatever its UV density, and evaluates exactly like the matching hand-set tiling.

use std::sync::Arc;

use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, ProceduralKind, ProceduralParams, Projection, ProjectionMode, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

fn checker_fill(ts: &TextureSet, projection: Projection) -> Layer {
    let mut l = Layer::new_fill("Checker", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if *k == ChannelKind::Roughness {
            *ch = FillChannel::new(FillSource::Procedural {
                params: ProceduralParams::new(ProceduralKind::Checker),
                low: ChannelValue::scalar(0.0),
                high: ChannelValue::scalar(1.0),
            });
        }
    }
    f.projection = projection;
    l
}

fn composite(geo: &TextureSetGeometry, projection: Projection) -> Composite {
    let mut ts = TextureSet::new("T", 0, RES);
    let layer = checker_fill(&ts, projection);
    ts.insert_above(None, layer);
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    composite_full(&ts, &EvalContext { geometry: geo, resources: &res, mesh_maps: &maps })
}

fn roughness(c: &Composite) -> Vec<f32> {
    let img = c.channel(ChannelKind::Roughness).unwrap();
    (0..RES * RES).map(|i| img.get(i % RES, i / RES)[0]).collect()
}

/// Number of light/dark switches along the middle row of the roughness channel.
fn switches(c: &Composite) -> usize {
    let r = roughness(c);
    let row = &r[(RES / 2 * RES) as usize..((RES / 2 + 1) * RES) as usize];
    row.windows(2).filter(|w| (w[0] > 0.5) != (w[1] > 0.5)).count()
}

#[test]
fn surface_scale_is_meters_per_uv_unit_and_model_extent() {
    for size in [0.5f32, 2.0, 3.0] {
        let geo = TextureSetGeometry::new(Arc::new(primitives::plane(size, 2)), 0, RES);
        let s = geo.surface_scale();
        assert!((s.uv_meters - size).abs() < 1e-5, "plane {size} m: {s:?}");
        assert!((s.extent_meters - size).abs() < 1e-5, "plane {size} m: {s:?}");
    }
}

#[test]
fn a_model_in_centimeters_is_measured_in_meters() {
    // A 200-unit plane from a centimeter FBX is 2 m: same scale as a 2-unit plane in meters.
    let mut cm = primitives::plane(200.0, 2);
    cm.unit_meters = 0.01;
    let s = TextureSetGeometry::new(Arc::new(cm), 0, RES).surface_scale();
    assert!((s.uv_meters - 2.0).abs() < 1e-4 && (s.extent_meters - 2.0).abs() < 1e-4, "{s:?}");
}

#[test]
fn physical_size_matches_the_equivalent_tiling_exactly() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    // 2 m per UV unit, 25 cm repeat: tiling 8.
    let physical = Projection { physical_size: Some([25.0, 25.0]), ..Projection::default() };
    let tiling = Projection { scale: [8.0, 8.0], ..Projection::default() };
    assert_eq!(roughness(&composite(&geo, physical)), roughness(&composite(&geo, tiling)));
    // Tri-planar reads positions divided by the 2 m extent: same repeat count.
    let tri = |p: Projection| Projection { mode: ProjectionMode::Triplanar, ..p };
    assert_eq!(roughness(&composite(&geo, tri(physical))), roughness(&composite(&geo, tri(tiling))));
}

#[test]
fn the_same_physical_size_gives_the_same_real_repeat_on_models_of_different_size() {
    let small = TextureSetGeometry::new(Arc::new(primitives::plane(1.0, 2)), 0, RES);
    let large = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    // A repeat every 25 cm: 4 repeats across 1 m, 8 across 2 m. The checker pattern has the
    // same number of cells per repeat on both, so the 2 m plane shows twice as many cells
    // (cells = switches + 1 along a row that starts on a cell edge).
    let p = Projection { physical_size: Some([25.0, 25.0]), ..Projection::default() };
    let (a, b) = (switches(&composite(&small, p)) + 1, switches(&composite(&large, p)) + 1);
    assert_eq!(b, 2 * a, "1 m plane {a} cells, 2 m plane {b} cells");
    // And the 1 m plane equals Tiling 4 (1 m / 25 cm).
    assert_eq!(a, switches(&composite(&small, Projection { scale: [4.0, 4.0], ..Projection::default() })) + 1);
    // With Tiling both planes show the same number of repeats instead.
    let t = Projection { scale: [4.0, 4.0], ..Projection::default() };
    assert_eq!(switches(&composite(&small, t)), switches(&composite(&large, t)));
}
