//! QA sweep: a seeded random walk over the command set (layers, masks, effects, filters,
//! generators, strokes, undo / redo, resolution changes) on a small project. After every step:
//!
//! * no command may panic, whatever ids or values it is given;
//! * the session document is the history's current entry;
//! * the selection names a layer that exists;
//! * undo restores exactly the document before the command, redo the one after;
//! * the incrementally updated composite equals a from-scratch composite of the same document;
//! * (every few steps) save + open gives back the same document and the same pixels.
//!
//! `QA_FUZZ_SEED` / `QA_FUZZ_STEPS` reproduce or lengthen a run.

use std::sync::Arc;

use glam::Vec2;
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId};
use subcraft_doc::{
    BlendMode, ContentEffectKind, FillChannel, FillSource, Filter, Generator, GeneratorKind, LayerContent, Levels, MaskEffectKind, ProceduralKind,
    ProceduralParams, Projection,
};
use subcraft_materials::MeshMaps;
use subcraft_mesh::primitives;
use subcraft_paint::{PaintTarget, StrokeMode};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n.max(1) as u64) as usize }
    fn unit(&mut self) -> f32 { (self.next() % 10_001) as f32 / 10_000.0 }
    fn chance(&mut self, p: f32) -> bool { self.unit() < p }
    fn pick<T: Clone>(&mut self, items: &[T]) -> Option<T> { if items.is_empty() { None } else { Some(items[self.below(items.len())].clone()) } }
    /// Mostly sane 0..1 values, sometimes out of range or not finite.
    fn wild(&mut self) -> f32 {
        match self.below(20) {
            0 => -1.0,
            1 => 2.5,
            2 => f32::NAN,
            3 => f32::INFINITY,
            4 => 0.0,
            5 => 1.0,
            _ => self.unit(),
        }
    }
}

fn all_layers(s: &Session) -> Vec<LayerId> {
    let mut out = Vec::new();
    fn walk(layers: &[Arc<subcraft_doc::Layer>], out: &mut Vec<LayerId>) {
        for l in layers {
            out.push(l.id);
            if let LayerContent::Group(f) = &l.content { walk(&f.children, out); }
        }
    }
    walk(&s.selected_texture_set().layers, &mut out);
    out
}

fn mask_effects(s: &Session, layer: LayerId) -> Vec<EffectId> {
    s.selected_texture_set().find(layer).and_then(|l| l.mask.as_ref()).map(|m| m.effects.iter().map(|e| e.id).collect()).unwrap_or_default()
}

fn content_effects(s: &Session, layer: LayerId) -> Vec<EffectId> {
    s.selected_texture_set().find(layer).map(|l| l.content_effects.iter().map(|e| e.id).collect()).unwrap_or_default()
}

fn random_filter(r: &mut Rng) -> Filter {
    let f = Filter::ALL[r.below(Filter::ALL.len())];
    // Keep spatial supports small: the walk composites after every step.
    match f {
        Filter::Blur { .. } => Filter::Blur { radius: r.unit() * 6.0 },
        Filter::Sharpen { .. } => Filter::Sharpen { radius: r.unit() * 4.0, amount: r.unit() * 2.0 },
        Filter::DirectionalBlur { .. } => Filter::DirectionalBlur { radius: r.unit() * 6.0, angle_deg: r.unit() * 360.0 - 180.0 },
        Filter::Median { .. } => Filter::Median { radius: r.unit() * 2.0 },
        Filter::Warp { .. } => Filter::Warp { intensity: r.unit() * 6.0, scale: 1 + r.below(6) as u32, seed: r.below(9) as u32 },
        Filter::Mosaic { .. } => Filter::Mosaic { size: 1 + r.below(12) as u32 },
        Filter::Posterize { .. } => Filter::Posterize { levels: 2 + r.below(6) as u32 },
        Filter::Glow { .. } => Filter::Glow { radius: r.unit() * 6.0, amount: r.unit(), threshold: r.unit() * 0.9 },
        Filter::Threshold { .. } => Filter::Threshold { cutoff: r.unit() },
        Filter::HistogramScan { .. } => Filter::HistogramScan { position: r.unit(), contrast: r.unit(), invert: r.chance(0.5) },
        Filter::HistogramRange { .. } => Filter::HistogramRange { position: r.unit(), range: r.unit(), softness: r.unit(), invert: r.chance(0.5) },
        Filter::Bevel { .. } => Filter::Bevel { radius: r.unit() * 6.0, amount: r.unit(), threshold: r.unit() },
        Filter::EdgeDetect { .. } => Filter::EdgeDetect { radius: 1.0 + r.unit() * 2.0, strength: r.unit() * 2.0 },
        Filter::DirectionalWarp { .. } => Filter::DirectionalWarp { intensity: r.unit() * 6.0, angle_deg: r.unit() * 360.0 - 180.0 },
        Filter::Clamp { .. } => Filter::Clamp { low: r.unit() * 0.5, high: 0.5 + r.unit() * 0.5 },
        Filter::HistogramShift { .. } => Filter::HistogramShift { shift: r.unit() - 0.5 },
        Filter::AnisotropicKuwahara { .. } => Filter::AnisotropicKuwahara { radius: r.unit() * 3.0, smoothness: r.unit(), sharpness: r.unit() * 12.0, tensor_smoothness: r.unit() * 2.0, anisotropy: r.unit() * 2.0, angle: r.unit(), extract_direction: r.chance(0.5) },
        other => other,
    }
}

fn random_mask_effect(r: &mut Rng) -> MaskEffectKind {
    match r.below(9) {
        0 => MaskEffectKind::Fill { value: r.unit(), procedural: None, projection: Projection::default() },
        1 => {
            let kinds = [ProceduralKind::Clouds, ProceduralKind::Grunge, ProceduralKind::WoodRings];
            let mut p = ProceduralParams::new(kinds[r.below(kinds.len())]);
            p.scale = 1.0 + r.unit() * 8.0;
            MaskEffectKind::Fill { value: 1.0, procedural: Some(p), projection: Projection::default() }
        }
        2 | 3 => {
            let kinds = [GeneratorKind::Edges, GeneratorKind::Cavity, GeneratorKind::Directional, GeneratorKind::PositionGradient, GeneratorKind::Concave, GeneratorKind::AmbientOcclusion];
            let mut g = Generator::new(kinds[r.below(kinds.len())]);
            g.level = r.unit();
            g.noise_amount = r.unit();
            MaskEffectKind::Generator(g)
        }
        4 => MaskEffectKind::Levels(Levels::default()),
        5 | 6 => {
            let f = random_filter(r);
            if f.supports_mask() { MaskEffectKind::Filter(f) } else { MaskEffectKind::Invert }
        }
        7 => MaskEffectKind::Anchor,
        _ => MaskEffectKind::Invert,
    }
}

fn random_content_effect(r: &mut Rng, s: &Session) -> ContentEffectKind {
    let channels: Vec<ChannelKind> = s.selected_texture_set().channels.iter().map(|c| c.kind).collect();
    let channel = r.pick(&channels).unwrap_or(ChannelKind::BaseColor);
    match r.below(5) {
        0 => ContentEffectKind::Levels { channel, levels: Levels::default() },
        1 | 2 => {
            let f = random_filter(r);
            if f.supports_channel(channel) { ContentEffectKind::Filter { channel, filter: f } } else { ContentEffectKind::Anchor }
        }
        3 => ContentEffectKind::Paint { channels: Default::default() },
        _ => ContentEffectKind::Anchor,
    }
}

/// One random command. Ids are usually live, sometimes stale (deleted or made up).
fn random_command(r: &mut Rng, s: &Session, graveyard: &[LayerId]) -> Command {
    let layers = all_layers(s);
    let layer = if r.chance(0.04) { r.pick(graveyard).unwrap_or_else(LayerId::new) } else { r.pick(&layers).unwrap_or_else(LayerId::new) };
    let ts = s.selected_texture_set();
    let channels: Vec<ChannelKind> = ts.channels.iter().map(|c| c.kind).collect();
    let channel = if r.chance(0.1) { ChannelKind::ALL[r.below(ChannelKind::ALL.len())] } else { r.pick(&channels).unwrap_or(ChannelKind::BaseColor) };
    let ts_id = ts.id;
    match r.below(46) {
        0..=2 => Command::AddLayer([NewLayer::Paint, NewLayer::Fill, NewLayer::Folder][r.below(3)]),
        3 => Command::DeleteLayer(layer),
        4 => Command::DuplicateLayer(layer),
        5 => Command::GroupLayer(layer),
        6 | 7 => Command::MoveLayer { layer, parent: if r.chance(0.5) { r.pick(&layers) } else { None }, index: r.below(6) },
        8 => Command::MoveUp(layer),
        9 => Command::MoveDown(layer),
        10 => Command::SetVisible { layer, visible: r.chance(0.6) },
        11 => Command::Rename { layer, name: ["", " ", "Layer", "名前", "a/b\\c:d"][r.below(5)].into() },
        12 | 13 => Command::SetBlendMode { layer, channel, mode: BlendMode::ALL[r.below(BlendMode::ALL.len())] },
        14 | 15 => Command::SetBlendOpacity { layer, channel, opacity: r.wild() },
        16 | 17 => Command::AddMask { layer, white: r.chance(0.5) },
        18 => Command::RemoveMask(layer),
        19 => Command::InvertMask(layer),
        20 => Command::SetMaskEnabled { layer, enabled: r.chance(0.7) },
        21..=24 => Command::AddMaskEffect { layer, kind: random_mask_effect(r) },
        25 => Command::RemoveMaskEffect { layer, effect: r.pick(&mask_effects(s, layer)).unwrap_or_else(EffectId::new) },
        26 => Command::MoveMaskEffect { layer, effect: r.pick(&mask_effects(s, layer)).unwrap_or_else(EffectId::new), index: r.below(5) },
        27 => Command::DuplicateMaskEffect { layer, effect: r.pick(&mask_effects(s, layer)).unwrap_or_else(EffectId::new) },
        28..=30 => Command::AddContentEffect { layer, kind: random_content_effect(r, s) },
        31 => Command::RemoveContentEffect { layer, effect: r.pick(&content_effects(s, layer)).unwrap_or_else(EffectId::new) },
        32 => Command::MoveContentEffect { layer, effect: r.pick(&content_effects(s, layer)).unwrap_or_else(EffectId::new), index: r.below(5) },
        33 => Command::DuplicateContentEffect { layer, effect: r.pick(&content_effects(s, layer)).unwrap_or_else(EffectId::new) },
        34 => Command::CopyMask(layer),
        35 => Command::PasteMask(layer),
        36 => Command::SetResolution { texture_set: if r.chance(0.5) { Some(ts_id) } else { None }, resolution: [128, 256, 128, 100, 0][r.below(5)] },
        37 => Command::SelectLayer(if r.chance(0.2) { None } else { Some(layer) }),
        38 => Command::AddChannel { texture_set: ts_id, kind: channel },
        39 => Command::RemoveChannel { texture_set: ts_id, kind: channel },
        40 => {
            let n = r.below(4);
            Command::DeleteLayers((0..n).filter_map(|_| r.pick(&layers)).collect())
        }
        41 => {
            let n = r.below(4);
            Command::DuplicateLayers((0..n).filter_map(|_| r.pick(&layers)).collect())
        }
        42 => {
            let n = r.below(4);
            Command::SetLayersVisible { layers: (0..n).filter_map(|_| r.pick(&layers)).collect(), visible: r.chance(0.5) }
        }
        43 => {
            let v = r.unit();
            let mut fill = FillChannel::new(if channel == ChannelKind::BaseColor { FillSource::uniform(ChannelValue::rgb(v, r.unit(), r.unit())) } else { FillSource::uniform(ChannelValue::scalar(v)) });
            fill.strength = r.wild();
            Command::SetFillChannel { layer, channel, fill }
        }
        44 => Command::ToggleFillChannel { layer, effect: None, channel, solo: r.chance(0.3) },
        _ => Command::SelectLayerTarget { layer, target: if r.chance(0.5) { LayerTarget::Mask } else { LayerTarget::Content } },
    }
}

/// A short stroke in the 2D view on whatever is selected, with a random mode and brush.
fn random_stroke(r: &mut Rng, s: &mut Session) {
    let Some(layer) = s.selection().layer else { return };
    let mut settings = s.brush.settings;
    settings.size = [0.0, 0.5, 4.0, 12.0, 60.0][r.below(5)];
    settings.spacing = [0.0, 0.05, 0.25, 1.0][r.below(4)];
    settings.hardness = r.unit();
    settings.flow = r.wild().clamp(0.0, 1.0);
    let _ = s.execute(Command::SetBrushSettings(settings));
    s.brush.mode = [StrokeMode::Paint, StrokeMode::Paint, StrokeMode::Erase, StrokeMode::Smudge][r.below(4)];
    s.brush.target = match r.below(4) {
        0 if !mask_effects(s, layer).is_empty() => PaintTarget::MaskEffect(mask_effects(s, layer)[0]),
        1 => PaintTarget::MaskBase,
        2 if !content_effects(s, layer).is_empty() => PaintTarget::ContentEffect(content_effects(s, layer)[0]),
        _ => PaintTarget::Content,
    };
    if s.begin_stroke().is_err() {
        s.brush.target = PaintTarget::Content;
        s.brush.mode = StrokeMode::Paint;
        return;
    }
    let points = 1 + r.below(4);
    for _ in 0..points {
        // Sometimes outside the texture.
        let uv = Vec2::new(r.unit() * 1.2 - 0.1, r.unit() * 1.2 - 0.1);
        let _ = s.stroke_to_uv(uv, r.chance(0.3), 1.0);
    }
    if r.chance(0.1) { s.cancel_stroke() } else { s.end_stroke() }
    s.brush.target = PaintTarget::Content;
    s.brush.mode = StrokeMode::Paint;
}

fn check_composite(s: &mut Session, what: &str) {
    s.update_composites();
    let ts = s.selected_texture_set().clone();
    let geo = s.scene().geometry.get(ts.material_slot, ts.resolution);
    let none = MeshMaps::new();
    let maps = s.mesh_maps(ts.id).unwrap_or(&none);
    let full = composite_full(&ts, &EvalContext { geometry: &geo, resources: s.resources(), mesh_maps: maps });
    let live = s.composite(ts.id).unwrap_or_else(|| panic!("{what}: no composite"));
    for def in &ts.channels {
        let (a, b) = (live.channel(def.kind), full.channel(def.kind));
        let (Some(a), Some(b)) = (a, b) else {
            assert_eq!(a.is_some(), b.is_some(), "{what}: channel {:?} present in only one composite", def.kind);
            continue;
        };
        let (da, db) = (a.to_dense(), b.to_dense());
        assert_eq!(da.len(), db.len(), "{what}: channel {:?} size", def.kind);
        let mut worst = 0.0f32;
        let mut at = 0;
        for (i, (x, y)) in da.iter().zip(&db).enumerate() {
            // NaN in one and not the other is a difference; NaN in both is reported separately.
            let d = if x.is_nan() && y.is_nan() { 0.0 } else if x.is_nan() || y.is_nan() { f32::INFINITY } else { (x - y).abs() };
            if d > worst { worst = d; at = i; }
        }
        assert!(worst <= 1e-4, "{what}: incremental composite of {:?} differs from a full composite by {worst} at value index {at} ({} vs {})", def.kind, da[at], db[at]);
        assert!(!da.iter().any(|v| v.is_nan()), "{what}: NaN in the composite of {:?}", def.kind);
    }
}

fn walk(seed: u64, steps: usize) {
    let dir = std::env::temp_dir().join(format!("subcraft-qa-fuzz-{seed}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut s = Session::new(primitives::sample_crate(), 128);
    let mut graveyard: Vec<LayerId> = Vec::new();
    let mut log: Vec<String> = Vec::new();
    for step in 0..steps {
        let _ = s.execute(Command::Seal);
        let before = s.document().clone();
        let layers_before = all_layers(&s);
        let what;
        match r.below(12) {
            0 => {
                what = format!("seed {seed} step {step}: stroke");
                log.push(what.clone());
                random_stroke(&mut r, &mut s);
            }
            1 => {
                let cmd = if r.chance(0.6) { Command::Undo } else { Command::Redo };
                what = format!("seed {seed} step {step}: {cmd:?}");
                log.push(what.clone());
                let _ = s.execute(cmd);
            }
            2 if r.chance(0.2) => {
                let cmd = Command::JumpToHistory(r.below(s.history().entries().len() + 2));
                what = format!("seed {seed} step {step}: {cmd:?}");
                log.push(what.clone());
                let _ = s.execute(cmd);
            }
            _ => {
                let cmd = random_command(&mut r, &s, &graveyard);
                let text = format!("{cmd:?}");
                what = format!("seed {seed} step {step}: {}", text.chars().take(300).collect::<String>());
                log.push(what.clone());
                let entries = s.history().entries().len();
                let cursor = s.history().cursor();
                let result = s.execute(cmd);
                let after = s.document().clone();
                if result.is_err() {
                    assert_eq!(after, before, "{what}: a refused command changed the document");
                }
                // A new history entry: undo and redo must reproduce both sides exactly.
                if s.history().cursor() == cursor + 1 && s.history().entries().len() >= entries {
                    s.execute(Command::Undo).unwrap();
                    assert_eq!(s.document(), &before, "{what}: undo did not restore the document");
                    s.execute(Command::Redo).unwrap();
                    assert_eq!(s.document(), &after, "{what}: redo did not restore the document");
                }
            }
        }
        let trail = || log.iter().rev().take(12).rev().cloned().collect::<Vec<_>>().join("\n  ");
        assert_eq!(s.document(), s.history().current(), "{what}: document and history disagree\n  {}", trail());
        assert!(!s.is_stroking(), "{what}: a stroke was left open");
        let now = all_layers(&s);
        graveyard.extend(layers_before.iter().filter(|l| !now.contains(l)));
        if let Some(sel) = s.selection().layer {
            assert!(now.contains(&sel), "{what}: the selection names a layer that does not exist\n  {}", trail());
        }
        assert!(s.document().texture_sets.iter().any(|t| t.id == s.selection().texture_set), "{what}: selected texture set missing");
        check_composite(&mut s, &format!("{what}\n  {}", trail()));
        if step % 25 == 24 {
            let path = dir.join("walk.subcraft");
            s.save(Some(&path)).unwrap_or_else(|e| panic!("{what}: save failed: {e}"));
            let mut reopened = Session::open_project(&path).unwrap_or_else(|e| panic!("{what}: a saved project does not open: {e}\n  {}", trail()));
            assert_eq!(reopened.document(), s.document(), "{what}: save + open changed the document\n  {}", trail());
            reopened.update_composites();
            let ts = s.selected_texture_set().id;
            assert_eq!(reopened.composite(ts), s.composite(ts), "{what}: save + open changed the pixels\n  {}", trail());
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn random_command_walks_keep_every_invariant() {
    let steps = std::env::var("QA_FUZZ_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    if let Some(seed) = std::env::var("QA_FUZZ_SEED").ok().and_then(|v| v.parse().ok()) {
        walk(seed, steps);
        return;
    }
    for seed in 1..=6 {
        walk(seed, steps);
    }
}
