//! Match by name: neighbouring high-poly parts must not bleed into a low-poly part.

use glam::{Vec2, Vec3};
use subcraft_core::{BakeMatch, MeshMapKind};
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder};

use super::*;
use name_match::{base_name, high_base, low_base};

const RES: u32 = 128;

fn at(img: &TiledImage, t: &Texel) -> [f32; 4] {
    img.get(t.index % RES, t.index / RES)
}

/// Push an up-facing quad `x0..x1 Ã— z0..z1` at heights `y(x)`, UVs mapped into `uv` = (u0, u1)
/// across x and 0.02..0.98 across z.
fn quad(b: &mut MeshBuilder, mat: u32, x: (f32, f32), z: (f32, f32), y: impl Fn(f32) -> f32, uv: (f32, f32)) {
    let c = |px: f32, pz: f32| {
        let u = uv.0 + (uv.1 - uv.0) * (px - x.0) / (x.1 - x.0);
        let v = 0.02 + 0.96 * (pz - z.0) / (z.1 - z.0);
        Corner { position: Vec3::new(px, y(px), pz), normal: None, uv: Some(Vec2::new(u, v)) }
    };
    b.push_triangle(mat, [c(x.0, z.0), c(x.0, z.1), c(x.1, z.1)]);
    b.push_triangle(mat, [c(x.0, z.0), c(x.1, z.1), c(x.1, z.0)]);
}

/// Low poly: parts A (x < 0, named `a_name`) and B (x > 0, `PartB_low`), flat at y = 0.
/// High poly: `PartA_high` flat over A, and `PartB_high` = flat over B plus a sloped plate that
/// overhangs A (y `lift` â†’ `lift` + 0.2 over x âˆ’1.5..âˆ’0.5). With `lift` 0.1 a ray cast down from
/// A's cage meets B's plate first; with `lift` 0.6 the plate is above the cage but still within
/// occlusion distance.
fn overlapping_parts(a_name: &str, lift: f32) -> (Mesh, Mesh) {
    let mut low = MeshBuilder::new("low");
    let m = low.material("Parts");
    low.object(a_name);
    quad(&mut low, m, (-2., -0.1), (-1., 1.), |_| 0., (0.02, 0.48));
    low.object("PartB_low");
    quad(&mut low, m, (0.1, 2.), (-1., 1.), |_| 0., (0.52, 0.98));
    let mut high = MeshBuilder::new("high");
    let m = high.material("Parts");
    high.object("PartA_high");
    quad(&mut high, m, (-2., -0.1), (-1., 1.), |_| 0., (0., 1.));
    high.object("PartB_high");
    quad(&mut high, m, (0.1, 2.), (-1., 1.), |_| 0., (0., 1.));
    quad(&mut high, m, (-1.5, -0.5), (-1.2, 1.2), move |x| lift + 0.2 * (x + 1.5), (0., 1.));
    (low.build().unwrap(), high.build_for_baking().unwrap())
}

fn bake_parts(low: &Mesh, high: &Mesh, name_match: BakeMatch) -> BTreeMap<MeshMapKind, TiledImage> {
    let (lb, hb) = (Bvh::build(low), Bvh::build(high));
    let input = BakeInput { low: Geometry { mesh: low, bvh: &lb }, high: Some(Geometry { mesh: high, bvh: &hb }), material_slot: 0 };
    let maps = vec![MeshMapKind::Normal, MeshMapKind::WorldSpaceNormal, MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness, MeshMapKind::Height];
    let s = BakeSettings { size: RES, maps, cage_front: 0.1, cage_rear: 0.05, ao_samples: 32, name_match, ..BakeSettings::default() };
    bake(&input, &s, &Progress::new()).unwrap()
}

/// Texels of part A that lie under B's overhanging plate.
fn under_plate(low: &Mesh) -> Vec<Texel> {
    texels(low, 0, RES).into_iter().filter(|t| (-1.4..-0.6).contains(&t.position.x) && t.position.z.abs() < 0.9).collect()
}

#[test]
fn always_lets_a_neighbouring_high_part_bleed_into_a_low_part() {
    let (low, high) = overlapping_parts("PartA_low", 0.1);
    let maps = bake_parts(&low, &high, BakeMatch::default());
    let texels = under_plate(&low);
    assert!(texels.len() > 200, "{} texels", texels.len());
    for t in &texels {
        let n = at(&maps[&MeshMapKind::Normal], t);
        assert!((n[0] - 0.5).abs() > 0.05 || (n[1] - 0.5).abs() > 0.05, "plate slope bleeds into A's normal: {n:?}");
        let w = at(&maps[&MeshMapKind::WorldSpaceNormal], t);
        assert!(w[0] < 0.45, "world normal leans to -X with the plate: {w:?}");
        let h = at(&maps[&MeshMapKind::Height], t)[0];
        assert!(h > 0.6, "plate stands above A: height {h}");
        // Sampled on the plate's top, thickness rays now reach A's high part below it.
        assert!(at(&maps[&MeshMapKind::Thickness], t)[0] > 0.2, "plate thickness bleeds into A");
    }
}

#[test]
fn by_name_keeps_each_low_part_on_its_own_high_part() {
    let (low, high) = overlapping_parts("PartA_low", 0.1);
    let maps = bake_parts(&low, &high, BakeMatch::by_name());
    let always = bake_parts(&low, &high, BakeMatch::default());
    for t in &under_plate(&low) {
        let n = at(&maps[&MeshMapKind::Normal], t);
        assert!((n[0] - 0.5).abs() < 1e-3 && (n[1] - 0.5).abs() < 1e-3 && (n[2] - 1.).abs() < 1e-3, "A sees only its flat high part: {n:?}");
        let w = at(&maps[&MeshMapKind::WorldSpaceNormal], t);
        assert!((w[0] - 0.5).abs() < 1e-3 && (w[1] - 1.).abs() < 1e-3, "world normal is +Y: {w:?}");
        let h = at(&maps[&MeshMapKind::Height], t)[0];
        assert!((h - 0.5).abs() < 1e-3, "flat height {h}");
        assert!(at(&maps[&MeshMapKind::Thickness], t)[0].abs() < 1e-3, "nothing of A's own part lies below it");
    }
    // B's own texels see the same high poly in both modes (its plate is not above B).
    for t in texels(&low, 0, RES).iter().filter(|t| t.position.x > 0.3) {
        assert_eq!(at(&maps[&MeshMapKind::Normal], t), at(&always[&MeshMapKind::Normal], t));
    }
    let m = PartMatch::new(&low, &high, &BakeMatch::by_name()).unwrap();
    assert_eq!(m.groups, ["parta", "partb"]);
    assert!(m.unmatched_low.is_empty() && m.unmatched_high.is_empty() && m.report().is_none());
}

#[test]
fn by_name_occlusion_ignores_other_parts_above_the_cage() {
    let (low, high) = overlapping_parts("PartA_low", 0.6);
    let always = bake_parts(&low, &high, BakeMatch::default());
    let by_name = bake_parts(&low, &high, BakeMatch::by_name());
    let (mut occluded, mut n) = (0.0, 0);
    for t in &under_plate(&low) {
        // Both modes project onto A's flat high part (the plate is beyond the front cage) ...
        assert_eq!(at(&always[&MeshMapKind::Normal], t), at(&by_name[&MeshMapKind::Normal], t));
        // ... but only Always lets B's plate shadow it.
        assert_eq!(at(&by_name[&MeshMapKind::AmbientOcclusion], t)[0], 1.0, "B's plate does not occlude A");
        occluded += at(&always[&MeshMapKind::AmbientOcclusion], t)[0];
        n += 1;
    }
    assert!(occluded / (n as f32) < 0.85, "Always: the plate occludes A (mean AO {})", occluded / n as f32);
}

#[test]
fn an_unmatched_low_part_falls_back_to_always_and_is_reported() {
    let (low, high) = overlapping_parts("Bracket_low", 0.1);
    let by_name = bake_parts(&low, &high, BakeMatch::by_name());
    let always = bake_parts(&low, &high, BakeMatch::default());
    for kind in [MeshMapKind::Normal, MeshMapKind::Height, MeshMapKind::AmbientOcclusion] {
        for t in &under_plate(&low) {
            assert_eq!(at(&by_name[&kind], t), at(&always[&kind], t), "fallback = Always for {kind:?}");
        }
    }
    let m = PartMatch::new(&low, &high, &BakeMatch::by_name()).unwrap();
    assert_eq!(m.unmatched_low, ["Bracket_low"]);
    assert_eq!(m.unmatched_high, ["PartA_high"]);
    let report = m.report().unwrap();
    assert!(report.contains("1 low-poly part") && report.contains("Bracket_low"), "{report}");
    assert!(PartMatch::new(&low, &high, &BakeMatch::default()).is_none(), "Always needs no groups");
}

#[test]
fn suffixes_are_case_insensitive_with_short_forms_and_duplicate_counters() {
    let m = BakeMatch::by_name();
    assert_eq!(low_base("Bolt_LOW", &m), "bolt");
    assert_eq!(low_base("bolt_lo", &m), "bolt");
    assert_eq!(high_base("Bolt_High", &m), "bolt");
    assert_eq!(high_base("BOLT_HI", &m), "bolt");
    assert_eq!(high_base("bolt_high.001", &m), "bolt", "duplicate counters are ignored");
    assert_eq!(low_base("bolt", &m), "bolt", "a name without a suffix is its own base");
    assert_eq!(low_base("bolt_high", &m), "bolt_high", "low names only drop low suffixes");
    assert_eq!(low_base("_low", &m), "_low", "never an empty base");
    assert_eq!(low_base("v1.2a", &m), "v1.2a");
    let custom = BakeMatch { low_suffix: "-LP".into(), high_suffix: "-HP".into(), ..m };
    assert_eq!(low_base("gear-lp", &custom), "gear");
    assert_eq!(high_base("Gear-HP", &custom), "gear");
    assert_eq!(high_base("gear_hi", &custom), "gear", "short forms stay accepted");
    assert_eq!(base_name(" Arm_Low ", &["_low", "", "_lo"]), "arm", "blank suffixes are ignored");
}

#[test]
fn meshes_without_objects_match_by_mesh_name() {
    let mut low = MeshBuilder::new("Crate_low");
    let m = low.material("m");
    quad(&mut low, m, (-1., 1.), (-1., 1.), |_| 0., (0.02, 0.98));
    let low = low.build().unwrap();
    let mut high = MeshBuilder::new("crate_high");
    let m = high.material("m");
    quad(&mut high, m, (-1., 1.), (-1., 1.), |_| 0., (0., 1.));
    let high = high.build_for_baking().unwrap();
    let pm = PartMatch::new(&low, &high, &BakeMatch::by_name()).unwrap();
    assert!(pm.unmatched_low.is_empty());
    assert_eq!(pm.low_group(0), 0);
    assert!(pm.accepts(0, 1) && pm.accepts(NO_GROUP, 1) && !pm.accepts(1, 1));
}
