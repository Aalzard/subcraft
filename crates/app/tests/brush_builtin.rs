//! Built-in brush presets: catalog, serialization and a real stroke with each one.
use glam::{Vec2, Vec3};
use subcraft_app::assets::alphas;
use subcraft_app::brush_builtin::{SMUDGE_PRESETS, builtin_brush_presets, describe};
use subcraft_app::brush_preset::BrushPreset;
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;

const RES: u32 = 256;

#[test]
fn catalog_is_complete_unique_and_valid() {
    let presets = builtin_brush_presets();
    assert!(presets.len() >= 22);
    let tips = alphas::builtin();
    let mut names: Vec<_> = presets.iter().map(|p| p.name.clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), presets.len(), "names are unique");
    for wanted in ["Smooth Ink", "Steady Liner", "Dust Scatter", "Drips", "Rust Flakes", "Sparkle Spray", "Soft Blend", "Wet Smear", "Paint Smear", "Hard Round", "Soft Airbrush", "Chalk", "Ink Pen", "Charcoal", "Scatter Dots", "Hatch Lines", "Splatter", "Dry Brush", "Texture Stamp"] {
        assert!(names.iter().any(|n| n == wanted), "missing {wanted}");
    }
    for p in &presets {
        assert!(describe(&p.name).is_some_and(|d| d.len() > 10), "{} has a description", p.name);
        if let Some(t) = &p.tip {
            assert!(tips.iter().any(|a| &a.name == t), "{}: tip {t} exists", p.name);
        }
        let s = p.settings;
        assert!((0.1..=100.0).contains(&s.size) && (0.0..=1.0).contains(&s.flow) && (0.0..=1.0).contains(&s.hardness), "{}", p.name);
        assert!((0.01..=5.0).contains(&s.spacing), "{}", p.name);
        assert!(s.size_jitter <= 1.0 && s.flow_jitter <= 1.0 && s.position_jitter <= 3.0, "{}", p.name);
        assert!(p.material.channels.is_empty(), "{} keeps the user's paint colour", p.name);
        let json = serde_json::to_string(p).unwrap();
        let back: BrushPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, p, "{} round-trips", p.name);
    }
    for (i, a) in presets.iter().enumerate() {
        for b in &presets[i + 1..] {
            assert!(a.settings != b.settings || a.tip != b.tip, "{} and {} are the same brush", a.name, b.name);
        }
    }
}

/// Paint one horizontal stroke on a flat plane with a preset; returns (coverage, mean alpha) of the layer.
fn paint(p: &BrushPreset) -> (f32, f32) {
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    assert!(s.apply_brush_preset(p, &tips).is_none(), "{}: tip found", p.name);
    assert_eq!(s.brush.settings, p.settings);
    assert_eq!(s.brush.material.channels.len(), 1, "{} leaves the paint colour alone", p.name);
    s.brush.settings.size = s.brush.settings.size.max(8.0);
    let cam = Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    let screen = |x: f32, z: f32| -> Vec2 { cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate() };
    s.begin_stroke().unwrap();
    for i in 0..=24 {
        s.stroke_to(&cam, screen(-0.8 + 1.6 * i as f32 / 24.0, 0.0), false).unwrap();
    }
    s.end_stroke();
    let LayerContent::Paint(l) = &s.selected_layer().unwrap().content else { panic!() };
    let img = &l.channels[&K::BaseColor];
    let (mut covered, mut sum) = (0u32, 0.0f32);
    for y in 0..RES {
        for x in 0..RES {
            let t = img.get(x, y);
            assert!(t.iter().all(|v| v.is_finite()));
            if t[3] > 0.01 { covered += 1; }
            sum += t[3];
        }
    }
    (covered as f32 / (RES * RES) as f32, sum / (RES * RES) as f32)
}

#[test]
fn every_builtin_brush_leaves_a_sensible_mark_that_differs_from_the_others() {
    let presets = builtin_brush_presets();
    let marks: Vec<_> = presets.iter().filter(|p| !SMUDGE_PRESETS.contains(&p.name.as_str())).map(|p| (p.name.clone(), paint(p))).collect();
    for (name, (cov, mean)) in &marks {
        assert!(*cov > 0.0005, "{name} paints something ({cov})");
        assert!(*cov < 0.6, "{name} does not flood the surface ({cov})");
        assert!(*mean > 0.0 && *mean <= 1.0, "{name}");
    }
    let get = |n: &str| marks.iter().find(|m| m.0 == n).unwrap().1;
    // Soft airbrush deposits much less per covered texel than the hard round.
    let (hc, hm) = get("Hard Round");
    let (ac, am) = get("Soft Airbrush");
    assert!(am / ac < 0.6 * (hm / hc), "airbrush is translucent: {} vs {}", am / ac, hm / hc);
    // The ink pen line is thinner than the airbrush.
    assert!(get("Ink Pen").0 < get("Soft Airbrush").0);
}

#[test]
fn smudge_presets_move_paint_and_scatter_presets_scatter() {
    let presets = builtin_brush_presets();
    let tips = alphas::builtin();
    let cam = Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    let screen = |x: f32, z: f32| -> Vec2 { cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate() };
    for name in SMUDGE_PRESETS {
        let p = presets.iter().find(|p| p.name == name).unwrap();
        let mut s = Session::new(primitives::plane(2.0, 1), RES);
        s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
        s.brush.settings.size = 18.0;
        s.brush.settings.hardness = 1.0;
        s.begin_stroke().unwrap();
        for x in [-0.8, -0.5, -0.2] { s.stroke_to(&cam, screen(x, 0.0), false).unwrap(); }
        s.end_stroke();
        s.apply_brush_preset(p, &tips);
        s.brush.mode = subcraft_paint::StrokeMode::Smudge;
        s.brush.settings.size = 18.0;
        s.begin_stroke().unwrap();
        for i in 0..=12 { s.stroke_to(&cam, screen(-0.4 + 1.0 * i as f32 / 12.0, 0.0), false).unwrap(); }
        s.end_stroke();
        let LayerContent::Paint(l) = &s.selected_layer().unwrap().content else { panic!() };
        let t = l.channels[&K::BaseColor].get(((0.25 / 2.0 + 0.5) * RES as f32) as u32, RES / 2);
        assert!(t[3] > 0.005 && t.iter().all(|v| v.is_finite()), "{name} dragged paint to clean surface: {t:?}");
    }
    for name in ["Dust Scatter", "Drips", "Rust Flakes", "Sparkle Spray"] {
        assert!(presets.iter().find(|p| p.name == name).unwrap().settings.scatter_count > 1, "{name}");
    }
    assert!(presets.iter().filter(|p| p.options.smoothing > 0.0 && p.options.catch_up).count() >= 2);
}
