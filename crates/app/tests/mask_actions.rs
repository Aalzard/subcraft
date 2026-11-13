//! Painter's mask menu: invert the background, clear, the evaluated mask (shown in the viewport)
//! and export to a file.
use subcraft_app::{Command, Session};
use subcraft_core::LayerId;
use subcraft_doc::MaskEffectKind;

fn session() -> (Session, LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), 128);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::AddMask { layer, white: false }).unwrap();
    (s, layer)
}

#[test]
fn replacing_the_mask_effects_drops_the_old_ones_keeps_the_painting_and_is_one_undo_step() {
    let (mut s, layer) = session();
    paint_square(&mut s, layer);
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Levels(Default::default()) }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
    let effects = |s: &Session| s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects.iter().map(|e| e.kind.clone()).collect::<Vec<_>>();
    let steps = s.history().entries().len();
    s.execute(Command::ReplaceMaskEffects { layer, effects: vec![MaskEffectKind::Invert], label: "Smart Mask: test".into() }).unwrap();
    assert!(matches!(effects(&s)[..], [MaskEffectKind::Invert]), "only the new effect is left");
    assert!((base_at(&s, layer, 15, 15) - 1.0).abs() < 1e-3, "the painting stays");
    assert_eq!(s.history().entries().len(), steps + 1);
    s.execute(Command::Undo).unwrap();
    assert!(matches!(effects(&s)[..], [MaskEffectKind::Levels(_), MaskEffectKind::Invert]));
    // A layer without a mask gets a black one carrying the effects.
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    let bare = s.selection().layer.unwrap();
    s.execute(Command::ReplaceMaskEffects { layer: bare, effects: vec![MaskEffectKind::Invert], label: "x".into() }).unwrap();
    assert_eq!(s.selected_texture_set().find(bare).unwrap().mask.as_ref().unwrap().effects.len(), 1);
}

/// Paint a white square into the mask base, like a stroke would.
fn paint_square(s: &mut Session, layer: LayerId) {
    let mut l = s.selected_texture_set().find(layer).unwrap().clone();
    let base = &mut l.mask.as_mut().unwrap().base;
    for y in 10..20 {
        for x in 10..20 {
            base.set(x, y, [1.0, 0.0, 0.0, 0.0]);
        }
    }
    s.execute(Command::ReplaceLayer { layer, replacement: Box::new(l), label: "paint".into() }).unwrap();
}

fn base_at(s: &Session, layer: LayerId, x: u32, y: u32) -> f32 {
    s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().base.get(x, y)[0]
}

#[test]
fn invert_background_swaps_black_and_white_including_strokes_and_keeps_effects() {
    let (mut s, layer) = session();
    paint_square(&mut s, layer);
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Levels(Default::default()) }).unwrap();
    let steps = s.history().entries().len();
    s.execute(Command::InvertMaskBase(layer)).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1);
    assert!((base_at(&s, layer, 100, 100) - 1.0).abs() < 1e-3, "the black background turned white");
    assert!(base_at(&s, layer, 15, 15).abs() < 1e-3, "the white stroke turned black");
    assert_eq!(s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects.len(), 1, "effects stay");
    // Twice is the original again; undo too.
    s.execute(Command::InvertMaskBase(layer)).unwrap();
    assert!(base_at(&s, layer, 100, 100).abs() < 1e-3 && (base_at(&s, layer, 15, 15) - 1.0).abs() < 1e-3);
    s.execute(Command::Undo).unwrap();
    assert!((base_at(&s, layer, 100, 100) - 1.0).abs() < 1e-3);
}

#[test]
fn clear_removes_the_painting_back_to_the_background_and_keeps_effects() {
    let (mut s, layer) = session();
    paint_square(&mut s, layer);
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
    s.execute(Command::ClearMask(layer)).unwrap();
    assert!(base_at(&s, layer, 15, 15).abs() < 1e-3, "the stroke is gone");
    assert!(base_at(&s, layer, 100, 100).abs() < 1e-3, "the background stays black");
    assert_eq!(s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects.len(), 1);
    s.execute(Command::Undo).unwrap();
    assert!((base_at(&s, layer, 15, 15) - 1.0).abs() < 1e-3);
    // QA 2026-11: clearing a mask that has nothing painted added an empty undo step.
    s.execute(Command::Redo).unwrap();
    s.execute(Command::Seal).unwrap();
    let entries = s.history().entries().len();
    s.execute(Command::ClearMask(layer)).unwrap();
    assert_eq!(s.history().entries().len(), entries, "nothing to clear: no undo step");

    let plain = s.selected_texture_set().layers.len();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    let no_mask = s.selection().layer.unwrap();
    assert!(s.execute(Command::ClearMask(no_mask)).is_err());
    assert!(s.execute(Command::InvertMaskBase(no_mask)).is_err());
    assert_eq!(s.selected_texture_set().layers.len(), plain + 1);
}

#[test]
fn the_evaluated_mask_includes_effects_and_exports_as_a_grayscale_file() {
    let (mut s, layer) = session();
    paint_square(&mut s, layer);
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
    // A disabled mask is still shown as authored.
    s.execute(Command::SetMaskEnabled { layer, enabled: false }).unwrap();
    let m = s.mask_image(layer).unwrap();
    assert_eq!(m.texture_set, s.selection().texture_set);
    assert_eq!(m.image.width(), 128);
    assert!((m.image.get(100, 100)[0] - 1.0).abs() < 1e-2, "black background, inverted by the effect");
    assert!(m.image.get(15, 15)[0] < 1e-2, "the white stroke, inverted");

    let dir = std::env::temp_dir().join(format!("subcraft-mask-export-{}", layer.0));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mask.png");
    s.export_mask(layer, &path, subcraft_image::FileFormat::Png8).unwrap();
    let bitmap = subcraft_image::load_bitmap(&path).unwrap();
    assert_eq!((bitmap.width, bitmap.height), (128, 128));
    std::fs::remove_dir_all(&dir).ok();

    s.execute(Command::RemoveMask(layer)).unwrap();
    assert!(s.mask_image(layer).is_err());
    assert!(s.export_mask(layer, &path, subcraft_image::FileFormat::Png8).is_err());
}
