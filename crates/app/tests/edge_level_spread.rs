//! Natural edge wear: Level is the amount, Spread is the reach, and they work together.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake, surface::texels};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

const SIZE: u32 = 128;

struct Crate { session: Session, mesh: subcraft_mesh::Mesh, ts: subcraft_core::TextureSetId, layer: subcraft_core::LayerId, slot: u32 }

fn setup() -> Crate {
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut session = Session::new(mesh.clone(), SIZE);
    let ts = session.document().texture_sets[0].id; let slot = session.document().texture_sets[0].material_slot;
    session.execute(Command::SelectTextureSet(ts)).unwrap();
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
        &BakeSettings { size: SIZE, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).unwrap();
    session.set_mesh_maps(ts, maps);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer = session.selection().layer.unwrap();
    session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 1.0, 1.0))) }).unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).unwrap();
    Crate { session, mesh, ts, layer, slot }
}

impl Crate {
    /// Wear (0..1) per texel of the Body set for these settings.
    fn wear(&mut self, level: f32, spread: f32, variation: f32, seed: u32, level_reach: bool) -> Vec<f32> {
        let mut g = Generator::new(GeneratorKind::Edges);
        g.level = level; g.noise_amount = 0.0; g.noise.seed = seed;
        let e = g.edge_spread.as_mut().unwrap(); e.distance = spread; e.variation = variation; e.level_reach = level_reach;
        let mut effect = self.session.selected_texture_set().find(self.layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
        effect.kind = MaskEffectKind::Generator(g);
        self.session.execute(Command::SetMaskEffect { layer: self.layer, effect }).unwrap();
        self.session.update_composites();
        let albedo = self.session.composite(self.ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
        texels(&self.mesh, self.slot, SIZE).iter().map(|t| ((albedo.get(t.index % SIZE, t.index / SIZE)[0] - 0.5) * 2.0).clamp(0.0, 1.0)).collect()
    }
}
fn total(v: &[f32]) -> f32 { v.iter().sum() }

#[test]
fn coverage_grows_smoothly_with_level_and_reach_grows_with_spread_and_level_zero_is_clean() {
    let mut c = setup();
    let levels = [0.0, 0.065, 0.15, 0.25, 0.4, 0.5, 0.75, 1.0];
    for spread in [3.0f32, 6.5, 12.5] {
        let sums: Vec<f32> = levels.iter().map(|&l| total(&c.wear(l, spread, 0.85, 1, true))).collect();
        assert!(sums[0] < 1e-3, "Level 0 must give no wear at Spread {spread}: {}", sums[0]);
        for w in sums.windows(2) { assert!(w[1] > w[0], "Level must always add wear at Spread {spread}: {sums:?}"); }
    }
    for level in [0.065f32, 0.25, 0.5, 1.0] {
        let spreads = [0.0f32, 1.5, 3.0, 6.5, 9.0, 12.5];
        let sums: Vec<f32> = spreads.iter().map(|&s| total(&c.wear(level, s, 0.85, 1, true))).collect();
        for w in sums.windows(2) { assert!(w[1] > w[0], "Spread must always reach further at Level {level}: {sums:?}"); }
    }
    // Both together: a wide Spread at a low Level is still more wear than a narrow one at the same Level.
    let (wide, narrow) = (total(&c.wear(0.065, 12.5, 0.85, 1, true)), total(&c.wear(0.065, 1.5, 0.85, 1, true)));
    assert!(wide > 1.5 * narrow, "wide {wide} vs narrow {narrow}");
}

#[test]
fn natural_wear_is_deterministic_per_seed_and_the_seed_changes_the_boundary() {
    let mut c = setup();
    let a = c.wear(0.5, 6.5, 0.85, 7, true);
    assert_eq!(a, c.wear(0.5, 6.5, 0.85, 7, true));
    assert_ne!(a, c.wear(0.5, 6.5, 0.85, 8, true));
    // Variation zero is a clean band: the seed no longer matters.
    assert_eq!(c.wear(0.5, 6.5, 0.0, 7, true), c.wear(0.5, 6.5, 0.0, 8, true));
}

#[test]
fn old_projects_keep_independent_reach_and_new_wear_defaults_to_level_as_amount() {
    let mut c = setup();
    // Original behavior: Level does not shrink the reach (wear exists even at Level 0).
    let old_zero = total(&c.wear(0.0, 6.5, 0.85, 1, false));
    let old_one = total(&c.wear(1.0, 6.5, 0.85, 1, false));
    assert!(old_zero > 100.0, "old projects keep the Spread band at Level 0: {old_zero}");
    assert!(old_one >= old_zero);
    // New default: Level 0 is none.
    assert!(total(&c.wear(0.0, 6.5, 0.85, 1, true)) < 1e-3);
    assert!(subcraft_doc::EdgeSpread::default().level_reach);
    // A project written before the flag existed reads back as the old behavior; the old JSON is unchanged.
    let old: subcraft_doc::EdgeSpread = serde_json::from_str(r#"{"distance":4.25,"variation":0.86}"#).unwrap();
    assert!(!old.level_reach);
    assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"distance":4.25,"variation":0.86}"#);
    assert!(serde_json::to_string(&subcraft_doc::EdgeSpread::default()).unwrap().contains("level_reach"));
    // The extent curve itself: zero at 0, strictly increasing, one at 1, and visible at low Levels.
    assert_eq!(subcraft_doc::EdgeSpread::level_extent(0.0), 0.0);
    assert_eq!(subcraft_doc::EdgeSpread::level_extent(1.0), 1.0);
    assert!(subcraft_doc::EdgeSpread::level_extent(0.065) > 0.3);
}
