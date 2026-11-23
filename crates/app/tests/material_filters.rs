//! Material filters (MatFinish / MatFX / PBR Validate) through the real session: which channels
//! they rewrite, that hiding one restores the composite exactly, PBR Validate's read of the
//! Metallic below (also after editing only that channel), Passthrough use and the project file.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId, TextureSetId};
use subcraft_doc::{BlendMode, ContentEffectKind, FillChannel, FillSource, MaterialFilter};
use subcraft_mesh::primitives;

const RES: u32 = 128;
const KINDS: [ChannelKind; 5] = [ChannelKind::BaseColor, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::Normal];

fn set_fill(s: &mut Session, layer: LayerId, base: [f32; 3], rough: f32, metal: f32) {
    let fill = |v: ChannelValue| FillChannel::new(FillSource::uniform(v));
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: fill(ChannelValue::rgb(base[0], base[1], base[2])) }).unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Roughness, fill: fill(ChannelValue::scalar(rough)) }).unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Metallic, fill: fill(ChannelValue::scalar(metal)) }).unwrap();
}

fn session(base: [f32; 3], rough: f32, metal: f32) -> (Session, TextureSetId, LayerId) {
    let mut s = Session::new(primitives::cube(2.0), RES);
    let ts = s.selected_texture_set().id;
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    set_fill(&mut s, layer, base, rough, metal);
    (s, ts, layer)
}

/// Every texel of a channel of the up-to-date composite (first component, or RGB).
fn channel(s: &mut Session, ts: TextureSetId, kind: ChannelKind) -> Vec<[f32; 3]> {
    s.update_composites();
    let img = s.composite(ts).unwrap().channel(kind).unwrap_or_else(|| panic!("{kind:?} is composited")).clone();
    let mut out = Vec::with_capacity((RES * RES) as usize);
    for y in 0..RES {
        for x in 0..RES {
            let p = img.get(x, y);
            out.push(if kind.components() >= 3 { [p[0], p[1], p[2]] } else { [p[0]; 3] });
        }
    }
    out
}

fn snapshot(s: &mut Session, ts: TextureSetId) -> Vec<Vec<[f32; 3]>> {
    KINDS.iter().map(|k| channel(s, ts, *k)).collect()
}

fn add(s: &mut Session, layer: LayerId, filter: MaterialFilter) -> EffectId {
    s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Material(filter) }).unwrap();
    s.selected_texture_set().find(layer).unwrap().content_effects.last().unwrap().id
}

fn changed(a: &[[f32; 3]], b: &[[f32; 3]]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| (p - q).abs() > 1e-4)).count()
}

#[test]
fn every_filter_rewrites_its_channels_never_normal_and_hiding_it_restores_the_composite() {
    for filter in MaterialFilter::ALL {
        // A dark half-metal: every filter has something to change in each channel it writes.
        let (mut s, ts, layer) = session([0.01, 0.012, 0.02], 0.6, 0.5);
        let before = snapshot(&mut s, ts);
        let effect = add(&mut s, layer, filter);
        let after = snapshot(&mut s, ts);
        for (i, kind) in KINDS.iter().enumerate() {
            let n = changed(&before[i], &after[i]);
            if filter.writes(*kind) {
                assert!(n > 0, "{} changes {kind:?}", filter.label());
            } else {
                assert_eq!(n, 0, "{} leaves {kind:?} alone", filter.label());
            }
            assert!(after[i].iter().flatten().all(|v| v.is_finite()), "{} {kind:?} is finite", filter.label());
        }
        assert_eq!(changed(&before[4], &after[4]), 0, "{} never writes Normal", filter.label());
        // Deterministic, and one undo step.
        assert_eq!(snapshot(&mut s, ts), after);
        let mut hidden = s.selected_texture_set().find(layer).unwrap().content_effects.iter().find(|e| e.id == effect).unwrap().as_ref().clone();
        hidden.visible = false;
        s.execute(Command::SetContentEffect { layer, effect: hidden }).unwrap();
        assert_eq!(snapshot(&mut s, ts), before, "{} hidden: the composite is exactly as before", filter.label());
        s.execute(Command::Undo).unwrap();
        assert_eq!(snapshot(&mut s, ts), after);
        s.execute(Command::Undo).unwrap();
        assert_eq!(snapshot(&mut s, ts), before);
    }
}

#[test]
fn finishes_give_the_material_they_describe() {
    // Galvanized: a full metal whatever was below. Powder coat: a dielectric that keeps the colour.
    let (mut s, ts, layer) = session([0.5, 0.04, 0.03], 0.2, 1.0);
    add(&mut s, layer, MaterialFilter::ALL[2]);
    assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 0.0), "powder coat is not a metal");
    let color = channel(&mut s, ts, ChannelKind::BaseColor);
    assert!(color.iter().all(|c| (c[0] - 0.5).abs() < 0.03 && c[1] < 0.06), "powder coat keeps the colour below");
    let rough = channel(&mut s, ts, ChannelKind::Roughness);
    assert!(rough.iter().all(|r| (0.3..0.66).contains(&r[0])), "its own roughness, not the 0.2 below");

    let (mut s, ts, layer) = session([0.2, 0.2, 0.2], 0.9, 0.0);
    add(&mut s, layer, MaterialFilter::ALL[0]);
    assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 1.0), "galvanized is a metal");
    let rough = channel(&mut s, ts, ChannelKind::Roughness);
    let (lo, hi) = rough.iter().fold((1.0f32, 0.0f32), |(a, b), r| (a.min(r[0]), b.max(r[0])));
    assert!(lo < 0.3 && hi > 0.34 && hi < 0.6, "spangles vary the roughness around 0.32: {lo}..{hi}");

    // Hammered: relief on both sides of the surface, material otherwise kept.
    let (mut s, ts, layer) = session([0.93, 0.62, 0.52], 0.28, 1.0);
    add(&mut s, layer, MaterialFilter::ALL[1]);
    let h = channel(&mut s, ts, ChannelKind::Height);
    assert!(h.iter().any(|v| v[0] < -0.03) && h.iter().any(|v| v[0] > 0.02), "dimples and ridges");
    assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 1.0));
}

#[test]
fn weathering_exposes_another_material_in_patches_and_carves_or_raises_the_height() {
    for (filter, exposed_metal) in [(MaterialFilter::ALL[3], true), (MaterialFilter::ALL[5], true), (MaterialFilter::ALL[4], false)] {
        let start_metal = if exposed_metal { 0.0 } else { 1.0 };
        let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, start_metal);
        add(&mut s, layer, filter);
        let metal = channel(&mut s, ts, ChannelKind::Metallic);
        let total = metal.len() as f32;
        let flipped = metal.iter().filter(|m| (m[0] - start_metal).abs() > 0.9).count() as f32 / total;
        let kept = metal.iter().filter(|m| (m[0] - start_metal).abs() < 0.05).count() as f32 / total;
        assert!(flipped > 0.01 && kept > 0.3, "{}: {flipped} exposed, {kept} untouched", filter.label());
        let height = channel(&mut s, ts, ChannelKind::Height);
        let (lo, hi) = height.iter().fold((0.0f32, 0.0f32), |(a, b), h| (a.min(h[0]), b.max(h[0])));
        if exposed_metal {
            assert!(lo < -0.005, "{} carves: {lo}", filter.label());
        } else {
            assert!(hi > 0.003, "{} raises scale: {hi}", filter.label());
        }
    }
}

#[test]
fn detail_edge_wear_is_a_thinner_line_than_edge_damages_and_the_new_finishes_keep_the_metal() {
    let exposed = |filter: MaterialFilter| {
        let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, 0.0);
        add(&mut s, layer, filter);
        let metal = channel(&mut s, ts, ChannelKind::Metallic);
        let height = channel(&mut s, ts, ChannelKind::Height);
        (metal.iter().filter(|m| m[0] > 0.9).count(), height.iter().fold(0.0f32, |a, h| a.min(h[0])))
    };
    let (damages, chip_depth) = exposed(MaterialFilter::ALL[3]);
    let (wear, wear_depth) = exposed(MaterialFilter::ALL[10]);
    assert!(wear > 20 && wear * 5 < damages * 4, "{wear} worn texels against {damages} chipped");
    assert!(wear_depth < 0.0 && wear_depth > chip_depth * 0.6, "wear {wear_depth} is shallower than chips {chip_depth}");

    for filter in [MaterialFilter::ALL[7], MaterialFilter::ALL[8], MaterialFilter::ALL[9]] {
        let (mut s, ts, layer) = session([0.56, 0.57, 0.58], 0.3, 1.0);
        add(&mut s, layer, filter);
        assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 1.0), "{} keeps the metal", filter.label());
        let rough = channel(&mut s, ts, ChannelKind::Roughness);
        let (lo, hi) = rough.iter().fold((1.0f32, 0.0f32), |(a, b), r| (a.min(r[0]), b.max(r[0])));
        assert!(hi - lo > 0.05 && lo > 0.1 && hi < 0.9, "{} varies the roughness around 0.3: {lo}..{hi}", filter.label());
    }
}

fn wear(detail: f32) -> MaterialFilter {
    MaterialFilter::DetailEdgeWear { level: 0.5, detail, contrast: 0.6, grunge: 0.0, grunge_scale: 24.0, color: [0.56, 0.57, 0.58], roughness: 0.3, metallic: 1.0, seed: 1 }
}

fn worn(s: &mut Session, ts: TextureSetId) -> usize {
    channel(s, ts, ChannelKind::Metallic).iter().filter(|m| m[0] > 0.5).count()
}

#[test]
fn detail_edge_wear_wears_the_height_detail_below_it() {
    let dimples = MaterialFilter::Hammered { scale: 6.0, depth: 1.0, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
    // On a flat material the painted-detail slider changes nothing: only the mesh edges wear.
    let flat = |detail: f32| {
        let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, 0.0);
        add(&mut s, layer, wear(detail));
        worn(&mut s, ts)
    };
    let edges_only = flat(0.0);
    assert_eq!(flat(1.0), edges_only);

    // Same layer: hammered relief first, then the wear. The ridges between the dimples wear too.
    let same_layer = |detail: f32| {
        let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, 0.0);
        add(&mut s, layer, dimples);
        add(&mut s, layer, wear(detail));
        worn(&mut s, ts)
    };
    assert_eq!(same_layer(0.0), edges_only, "detail 0 ignores the relief");
    let (half, full) = (same_layer(0.5), same_layer(1.0));
    assert!(half > edges_only && full > half + 500, "{edges_only} on edges, {half} at half detail, {full} at full");

    // Relief on a layer below, the wear on a paint coat above it.
    let (mut s, ts, layer) = session([0.56, 0.57, 0.58], 0.3, 1.0);
    add(&mut s, layer, dimples);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let coat = s.selection().layer.unwrap();
    set_fill(&mut s, coat, [0.1, 0.17, 0.26], 0.45, 0.0);
    let effect = add(&mut s, coat, wear(1.0));
    let stacked = worn(&mut s, ts);
    assert!(stacked > edges_only + 50, "{stacked} worn through the coat against {edges_only} on edges");

    // Editing the relief below moves the wear: the result matches a session built that way.
    let before = channel(&mut s, ts, ChannelKind::BaseColor);
    let below = s.selected_texture_set().find(layer).unwrap().content_effects[0].as_ref().clone();
    let mut edited = below.clone();
    edited.kind = ContentEffectKind::Material(MaterialFilter::Hammered { scale: 9.0, depth: 1.0, irregularity: 0.6, roughness_variation: 0.4, seed: 3 });
    s.execute(Command::SetContentEffect { layer, effect: edited }).unwrap();
    let after = channel(&mut s, ts, ChannelKind::BaseColor);
    assert!(changed(&before, &after) > 50, "the wear follows the new relief");
    s.execute(Command::Undo).unwrap();
    assert_eq!(channel(&mut s, ts, ChannelKind::BaseColor), before);
    // Hidden: exactly the coat.
    let mut hidden = s.selected_texture_set().find(coat).unwrap().content_effects.iter().find(|e| e.id == effect).unwrap().as_ref().clone();
    hidden.visible = false;
    s.execute(Command::SetContentEffect { layer: coat, effect: hidden }).unwrap();
    assert_eq!(worn(&mut s, ts), 0);
}

#[test]
fn amount_zero_changes_nothing() {
    let quiet = [
        MaterialFilter::EdgeDamages { amount: 0.0, spread: 2.5, scale: 30.0, depth: 0.5, color: [0.56, 0.57, 0.58], roughness: 0.38, metallic: 1.0, seed: 1 },
        MaterialFilter::RustWeathering { amount: 0.0, edges: 0.5, cavities: 0.6, drips: 0.4, scale: 6.0, relief: 0.4, roughness: 0.88, seed: 1 },
        MaterialFilter::PeelingPaint { amount: 0.0, scale: 6.0, edge_bias: 0.5, thickness: 0.5, curl: 0.5, color: [0.52, 0.53, 0.54], roughness: 0.45, metallic: 1.0, rust: 0.4, seed: 1 },
    ];
    for filter in quiet {
        let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, 0.0);
        let before = snapshot(&mut s, ts);
        add(&mut s, layer, filter);
        assert_eq!(snapshot(&mut s, ts), before, "{}", filter.label());
    }
}

fn dominant(c: [f32; 3]) -> usize {
    (0..3).max_by(|a, b| c[*a].total_cmp(&c[*b])).unwrap()
}

#[test]
fn pbr_validate_reads_the_metallic_below_and_follows_edits_of_that_channel_alone() {
    let validate = MaterialFilter::PbrValidate { strict: false, flag_partial_metal: true, flat: false };
    // Mid gray: fine as a dielectric, too dark as a metal.
    let (mut s, ts, layer) = session([0.2, 0.2, 0.2], 0.5, 0.0);
    add(&mut s, layer, validate);
    assert!(channel(&mut s, ts, ChannelKind::BaseColor).iter().all(|c| dominant(*c) == 1), "plausible dielectric: green");
    assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 0.0), "not flat: Metallic is left alone");
    // Only the Metallic fill changes; the Base Color verdict must follow.
    let fill = FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0)));
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Metallic, fill }).unwrap();
    assert!(channel(&mut s, ts, ChannelKind::BaseColor).iter().all(|c| dominant(*c) == 0), "a metal this dark: red");
    let fill = FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.5)));
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Metallic, fill }).unwrap();
    let c = channel(&mut s, ts, ChannelKind::BaseColor);
    assert!(c.iter().all(|c| c[0] > c[2] * 4.0 && c[1] > c[2] * 4.0), "half metal: yellow, {:?}", c[0]);
    // Too bright and too dark dielectrics.
    for (base, want) in [([0.98, 0.98, 0.98], 2usize), ([0.004, 0.004, 0.004], 0usize)] {
        let (mut s, ts, layer) = session(base, 0.5, 0.0);
        add(&mut s, layer, validate);
        assert!(channel(&mut s, ts, ChannelKind::BaseColor).iter().all(|c| dominant(*c) == want), "{base:?}");
    }
}

#[test]
fn a_filter_on_a_passthrough_layer_processes_the_stack_below() {
    let (mut s, ts, _base) = session([0.56, 0.57, 0.58], 0.35, 1.0);
    let plain = snapshot(&mut s, ts);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let top = s.selection().layer.unwrap();
    for kind in KINDS {
        s.execute(Command::SetBlendMode { layer: top, channel: kind, mode: BlendMode::Passthrough }).unwrap();
    }
    assert_eq!(snapshot(&mut s, ts), plain, "an empty passthrough layer changes nothing");
    add(&mut s, top, MaterialFilter::ALL[4]);
    let rusted = snapshot(&mut s, ts);
    let n = changed(&plain[2], &rusted[2]);
    assert!(n > 100, "rust on the steel below: {n} texels lost their metal");
    // And PBR Validate above it judges the stack: the steel is a plausible metal, rust is fine too.
    add(&mut s, top, MaterialFilter::PbrValidate { strict: false, flag_partial_metal: false, flat: true });
    let flags = channel(&mut s, ts, ChannelKind::BaseColor);
    assert!(flags.iter().filter(|c| dominant(**c) == 1).count() > flags.len() / 2, "mostly green");
    assert!(channel(&mut s, ts, ChannelKind::Metallic).iter().all(|m| m[0] == 0.0), "flat shading");
}

#[test]
fn filters_survive_a_project_round_trip_and_invalid_ones_are_refused() {
    let (mut s, ts, layer) = session([0.1, 0.17, 0.26], 0.45, 0.0);
    for filter in MaterialFilter::ALL {
        add(&mut s, layer, filter);
    }
    let want = snapshot(&mut s, ts);
    let dir = std::env::temp_dir().join(format!("subcraft-material-filters-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("filters.subcraft");
    s.save(Some(&path)).unwrap();
    let mut loaded = Session::open_project(&path).unwrap();
    let ts2 = loaded.selected_texture_set().id;
    let kinds: Vec<_> = loaded.selected_texture_set().layers.iter().flat_map(|l| l.content_effects.iter()).map(|e| e.kind.clone()).collect();
    assert_eq!(kinds, MaterialFilter::ALL.map(ContentEffectKind::Material).to_vec());
    assert_eq!(snapshot(&mut loaded, ts2), want, "the loaded project composites identically");
    std::fs::remove_dir_all(&dir).ok();

    let bad = MaterialFilter::Hammered { scale: f32::NAN, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 1 };
    assert!(s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Material(bad) }).is_err());
}
