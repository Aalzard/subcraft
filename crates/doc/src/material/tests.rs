use std::sync::Arc;

use subcraft_core::{ChannelKind, ChannelValue, EffectId};

use super::*;
use crate::{FillChannel, Generator, GeneratorKind, Mask, MaskEffect, ProceduralKind, TextureSet};

fn uniform(v: ChannelValue) -> FillChannel {
    FillChannel::new(FillSource::uniform(v))
}

/// A folder with a metal base and a painted layer masked by an edge-wear generator, exposing
/// Paint colour, Roughness, Variation, Wear, Scale, Seed and a Dirt toggle.
fn painted_metal() -> (Layer, LayerId, LayerId, EffectId) {
    let ts = TextureSet::new("set", 0, 64);
    let mut base = Layer::new_fill("Metal", &ts);
    let mut paint = Layer::new_fill("Paint", &ts);
    let (base_id, paint_id) = (base.id, paint.id);
    if let LayerContent::Fill(f) = &mut base.content {
        f.channels.insert(
            ChannelKind::Roughness,
            FillChannel::new(FillSource::Procedural { params: ProceduralParams::new(ProceduralKind::Grunge), low: ChannelValue::scalar(0.3), high: ChannelValue::scalar(0.5) }),
        );
    }
    if let LayerContent::Fill(f) = &mut paint.content {
        f.channels.insert(ChannelKind::BaseColor, uniform(ChannelValue::rgb(0.1, 0.2, 0.3)));
        f.channels.insert(ChannelKind::Roughness, uniform(ChannelValue::scalar(0.5)));
    }
    let wear = MaskEffect::new(MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)));
    let wear_id = wear.id;
    let mut mask = Mask::new(64, true);
    mask.effects = vec![Arc::new(wear)];
    paint.mask = Some(mask);
    let mut folder = Layer::new_folder("Painted Metal", &ts);
    folder.children_mut().unwrap().extend([Arc::new(base), Arc::new(paint)]);
    let rough = ChannelKind::Roughness;
    folder.material = Some(Arc::new(MaterialControls {
        params: vec![
            Param::color("paint_color", "Paint color", [0.1, 0.2, 0.3]).group("Paint"),
            Param::float("roughness", "Roughness", 0.0, 1.0, 0.4),
            Param::float("variation", "Roughness variation", 0.0, 1.0, 0.2),
            Param::float("wear", "Wear", 0.0, 1.0, 0.5).tip("How much paint is worn off the edges"),
            Param::float("scale", "Scale", 0.25, 4.0, 1.0),
            Param::seed("seed", "Seed", 1),
            Param::toggle("painted", "Painted", true),
        ],
        bindings: vec![
            Binding::layer(paint_id, Prop::Value { channel: ChannelKind::BaseColor }, "paint_color"),
            Binding::layer(base_id, Prop::Low { channel: rough }, "clamp(roughness - variation * 0.5, 0, 1)"),
            Binding::layer(base_id, Prop::High { channel: rough }, "clamp(roughness + variation * 0.5, 0, 1)"),
            Binding::layer(base_id, Prop::PatternScale { channel: None }, "4 * scale"),
            Binding::layer(base_id, Prop::PatternSeed { channel: Some(rough) }, "seed + 10"),
            Binding::effect(paint_id, wear_id, Prop::Level, "wear"),
            Binding::effect(paint_id, wear_id, Prop::BreakupSeed, "seed"),
            Binding::layer(paint_id, Prop::Visible, "painted"),
        ],
    }));
    (folder, base_id, paint_id, wear_id)
}

fn fill_of(root: &Layer, id: LayerId) -> &crate::FillContent {
    let l = root.children().iter().find(|c| c.id == id).expect("layer");
    match &l.content {
        LayerContent::Fill(f) => f,
        _ => panic!("fill"),
    }
}

fn generator_of(root: &Layer, layer: LayerId, effect: EffectId) -> Generator {
    let l = root.children().iter().find(|c| c.id == layer).unwrap();
    match &l.mask.as_ref().unwrap().effects.iter().find(|e| e.id == effect).unwrap().kind {
        MaskEffectKind::Generator(g) => *g,
        _ => panic!("generator"),
    }
}

fn set(root: &mut Layer, id: &str, v: ParamValue) -> bool {
    let changed = Arc::make_mut(root.material.as_mut().unwrap()).set(id, v);
    root.apply_material();
    changed
}

#[test]
fn a_built_material_has_no_problems_and_defaults_change_nothing() {
    let (mut folder, ..) = painted_metal();
    let m = folder.material.clone().unwrap();
    assert_eq!(m.validate(), Vec::<String>::new());
    assert_eq!(m.check(&folder), vec![]);
    assert!(m.is_default());
    // The layers were built at the defaults except the bound pattern values; one apply settles
    // them, a second changes nothing and leaves every `Arc` alone.
    folder.apply_material();
    let before = folder.clone();
    assert!(!folder.apply_material());
    assert_eq!(folder, before);
    assert!(folder.children().iter().zip(before.children()).all(|(a, b)| Arc::ptr_eq(a, b)), "unchanged layers are not copied");
}

#[test]
fn setting_a_parameter_writes_every_bound_property() {
    let (mut folder, base, paint, wear) = painted_metal();
    folder.apply_material();
    let rough = ChannelKind::Roughness;

    assert!(set(&mut folder, "paint_color", ParamValue::Color([0.8, 0.1, 0.05])));
    assert_eq!(fill_of(&folder, paint).channels[&ChannelKind::BaseColor].source, FillSource::uniform(ChannelValue::rgb(0.8, 0.1, 0.05)));

    assert!(set(&mut folder, "roughness", ParamValue::Number(0.7)));
    assert!(set(&mut folder, "variation", ParamValue::Number(0.4)));
    let FillSource::Procedural { params, low, high } = fill_of(&folder, base).channels[&rough].source else { panic!() };
    assert!((low.x() - 0.5).abs() < 1e-6 && (high.x() - 0.9).abs() < 1e-6, "{low:?} {high:?}");
    assert_eq!((params.scale, params.seed), (4.0, 11));

    // The range is clamped by the formula, the slider by its own range.
    set(&mut folder, "roughness", ParamValue::Number(5.0));
    assert_eq!(folder.material.as_ref().unwrap().param("roughness").unwrap().value, ParamValue::Number(1.0));
    let FillSource::Procedural { high, .. } = fill_of(&folder, base).channels[&rough].source else { panic!() };
    assert_eq!(high.x(), 1.0);

    set(&mut folder, "scale", ParamValue::Number(2.0));
    set(&mut folder, "seed", ParamValue::Number(41.6));
    let FillSource::Procedural { params, .. } = fill_of(&folder, base).channels[&rough].source else { panic!() };
    assert_eq!((params.scale, params.seed), (8.0, 52), "a seed is a whole number");

    set(&mut folder, "wear", ParamValue::Number(0.9));
    let g = generator_of(&folder, paint, wear);
    assert_eq!((g.level, g.noise.seed), (0.9, 42));

    assert!(folder.children()[1].visible);
    set(&mut folder, "painted", ParamValue::Toggle(false));
    assert!(!folder.children()[1].visible);

    // Wrong types and unknown ids change nothing.
    assert!(!set(&mut folder, "wear", ParamValue::Color([1.0; 3])));
    assert!(!set(&mut folder, "painted", ParamValue::Number(1.0)));
    assert!(!set(&mut folder, "nope", ParamValue::Number(1.0)));
    assert!(!set(&mut folder, "wear", ParamValue::Number(0.9)), "the same value is not a change");
    set(&mut folder, "wear", ParamValue::Number(f32::NAN));
    assert_eq!(generator_of(&folder, paint, wear).level, 0.5, "not a number falls back to the default");
}

#[test]
fn reset_restores_the_defaults() {
    let (mut folder, _, paint, wear) = painted_metal();
    folder.apply_material();
    let fresh = folder.clone();
    set(&mut folder, "wear", ParamValue::Number(0.1));
    set(&mut folder, "paint_color", ParamValue::Color([1.0, 0.0, 0.0]));
    assert!(!folder.material.as_ref().unwrap().is_default());
    assert!(Arc::make_mut(folder.material.as_mut().unwrap()).reset());
    folder.apply_material();
    assert_eq!(folder, fresh);
    assert_eq!(generator_of(&folder, paint, wear).level, 0.5);
    assert!(!Arc::make_mut(folder.material.as_mut().unwrap()).reset());
}

#[test]
fn a_copy_drives_its_own_layers() {
    let (mut folder, _, paint, wear) = painted_metal();
    folder.apply_material();
    let mut copy = folder.duplicate_with_new_ids();
    let m = copy.material.clone().unwrap();
    assert_eq!(m.check(&copy), vec![], "every binding found its copied target");
    assert!(m.bindings.iter().all(|b| b.layer != paint && b.effect != Some(wear)));
    set(&mut copy, "wear", ParamValue::Number(0.05));
    let (copied_paint, copied_wear) = (copy.children()[1].id, copy.children()[1].mask.as_ref().unwrap().effects[0].id);
    assert_eq!(generator_of(&copy, copied_paint, copied_wear).level, 0.05);
    assert_eq!(generator_of(&folder, paint, wear).level, 0.5, "the original is untouched");
    // Several layers copied together (paste) are remapped too.
    let copies = crate::duplicate_layers_with_new_ids([&folder]);
    assert_eq!(copies[0].material.as_ref().unwrap().check(&copies[0]), vec![]);
}

#[test]
fn bindings_whose_target_is_gone_or_changed_are_idle() {
    let (mut folder, base, paint, _) = painted_metal();
    folder.apply_material();
    // The user deletes the paint layer and turns the base roughness into a plain value.
    folder.children_mut().unwrap().retain(|c| c.id != paint);
    let base_layer = Arc::make_mut(&mut folder.children_mut().unwrap()[0]);
    if let LayerContent::Fill(f) = &mut base_layer.content {
        f.channels.insert(ChannelKind::Roughness, uniform(ChannelValue::scalar(0.33)));
    }
    let m = folder.material.clone().unwrap();
    let problems = m.check(&folder);
    assert_eq!(problems.iter().filter(|(_, p)| *p == BindingProblem::TargetMissing).count(), 4);
    assert_eq!(problems.iter().filter(|(_, p)| *p == BindingProblem::NotApplicable).count(), 4);
    let before = folder.clone();
    set(&mut folder, "wear", ParamValue::Number(1.0));
    set(&mut folder, "roughness", ParamValue::Number(0.9));
    assert_eq!(fill_of(&folder, base), fill_of(&before, base), "the user's own edit is kept");
}

#[test]
fn broken_formulas_are_reported_and_skipped() {
    let (mut folder, base, ..) = painted_metal();
    let m = Arc::make_mut(folder.material.as_mut().unwrap());
    m.bindings = vec![
        Binding::layer(base, Prop::Tiling, "scale *"),
        Binding::layer(base, Prop::Rotation, "angle"),
        Binding::layer(base, Prop::Tiling, "scale * 3"),
    ];
    let m = folder.material.clone().unwrap();
    let problems = m.check(&folder);
    assert_eq!(problems.len(), 2);
    assert!(matches!(&problems[0], (0, BindingProblem::Formula(_))));
    assert_eq!(problems[1], (1, BindingProblem::Formula(crate::material_expr::ExprError::UnknownParam("angle".into()))));
    assert!(folder.apply_material());
    assert_eq!(fill_of(&folder, base).projection.scale, [3.0, 3.0]);
    assert_eq!(fill_of(&folder, base).projection.rotation_deg, 0.0);
}

#[test]
fn layer_and_effect_properties_are_written_in_range() {
    let (mut folder, base, paint, wear) = painted_metal();
    let rough = ChannelKind::Roughness;
    let m = Arc::make_mut(folder.material.as_mut().unwrap());
    m.params = vec![Param::float("x", "X", -10.0, 500.0, 0.25)];
    m.bindings = vec![
        Binding::layer(paint, Prop::Opacity { channel: None }, "x"),
        Binding::layer(base, Prop::Opacity { channel: Some(rough) }, "x * 2"),
        Binding::layer(base, Prop::Strength { channel: rough }, "x * 4"),
        Binding::layer(base, Prop::Rotation, "x * 100"),
        Binding::layer(base, Prop::PatternBalance { channel: None }, "x"),
        Binding::layer(base, Prop::PatternContrast { channel: Some(rough) }, "x + 0.5"),
        Binding::effect(paint, wear, Prop::Opacity { channel: None }, "x"),
        Binding::effect(paint, wear, Prop::Softness, "x"),
        Binding::effect(paint, wear, Prop::Breakup, "x"),
        Binding::effect(paint, wear, Prop::BreakupScale, "x * 40"),
        Binding::effect(paint, wear, Prop::Grain, "x"),
        Binding::effect(paint, wear, Prop::AoMasking, "x"),
        Binding::effect(paint, wear, Prop::Smoothness, "x"),
        Binding::effect(paint, wear, Prop::Spread, "x * 20"),
    ];
    let m = folder.material.clone().unwrap();
    assert_eq!(m.check(&folder), vec![]);
    folder.apply_material();
    let paint_layer = &folder.children()[1];
    assert!(paint_layer.blend.values().all(|b| b.opacity == 0.25));
    let base_layer = &folder.children()[0];
    assert_eq!(base_layer.blend[&rough].opacity, 0.5);
    assert_eq!(base_layer.blend[&ChannelKind::BaseColor].opacity, 1.0);
    let f = fill_of(&folder, base);
    assert_eq!((f.channels[&rough].strength, f.projection.rotation_deg), (1.0, 25.0));
    let FillSource::Procedural { params, .. } = f.channels[&rough].source else { panic!() };
    assert_eq!((params.balance, params.contrast), (0.25, 0.75));
    let effect = &paint_layer.mask.as_ref().unwrap().effects[0];
    assert_eq!(effect.opacity, 0.25);
    let g = generator_of(&folder, paint, wear);
    let pc = g.painter.unwrap();
    assert_eq!((g.softness, g.noise_amount, g.noise.scale, pc.grain, pc.ao_masking, pc.smoothness), (0.25, 0.25, 10.0, 0.25, 0.25, 0.25));
    assert_eq!(g.edge_spread.unwrap().distance, 5.0);

    // Far out of range: every property stays inside what the app accepts.
    set(&mut folder, "x", ParamValue::Number(500.0));
    let g = generator_of(&folder, paint, wear);
    assert_eq!((g.softness, g.noise_amount, g.noise.scale, g.edge_spread.unwrap().distance), (1.0, 1.0, 100.0, 100.0));
    assert!(folder.children()[1].blend.values().all(|b| b.opacity == 1.0));
    set(&mut folder, "x", ParamValue::Number(-10.0));
    let g = generator_of(&folder, paint, wear);
    assert_eq!((g.softness, g.noise_amount, g.noise.scale), (0.001, 0.0, 1.0));
    let FillSource::Procedural { params, .. } = fill_of(&folder, base).channels[&rough].source else { panic!() };
    assert_eq!((params.balance, params.contrast), (0.0, 0.0));

    // Properties a target does not have are reported, not written.
    let m = Arc::make_mut(folder.material.as_mut().unwrap());
    m.bindings = vec![
        Binding::layer(folder.id, Prop::Tiling, "x"),
        Binding::layer(base, Prop::Level, "x"),
        Binding::layer(base, Prop::Value { channel: rough }, "x"),
        Binding::effect(paint, wear, Prop::FillValue, "x"),
        Binding::effect(paint, EffectId::new(), Prop::Level, "x"),
    ];
    let m = folder.material.clone().unwrap();
    let problems: Vec<_> = m.check(&folder).into_iter().map(|(_, p)| p).collect();
    assert_eq!(problems, vec![BindingProblem::NotApplicable, BindingProblem::NotApplicable, BindingProblem::NotApplicable, BindingProblem::NotApplicable, BindingProblem::TargetMissing]);
    assert!(!folder.apply_material());
}

#[test]
fn mask_fills_take_pattern_and_value_bindings() {
    let ts = TextureSet::new("set", 0, 64);
    let mut layer = Layer::new_fill("Rust", &ts);
    let fill = MaskEffect::new(MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Grunge)), projection: Default::default() });
    let fill_id = fill.id;
    let mut mask = Mask::new(64, false);
    mask.effects = vec![Arc::new(fill)];
    layer.mask = Some(mask);
    let id = layer.id;
    layer.material = Some(Arc::new(MaterialControls {
        params: vec![Param::float("amount", "Amount", 0.0, 1.0, 0.5), Param::seed("seed", "Seed", 3)],
        bindings: vec![
            Binding::effect(id, fill_id, Prop::PatternBalance { channel: None }, "1 - amount"),
            Binding::effect(id, fill_id, Prop::PatternScale { channel: None }, "6"),
            Binding::effect(id, fill_id, Prop::PatternSeed { channel: None }, "seed"),
            Binding::effect(id, fill_id, Prop::FillValue, "amount + 0.25"),
            Binding::effect(id, fill_id, Prop::Visible, "step(0.01, amount)"),
        ],
    }));
    let m = layer.material.clone().unwrap();
    assert_eq!(m.check(&layer), vec![]);
    assert!(layer.apply_material());
    let read = |l: &Layer| match &l.mask.as_ref().unwrap().effects[0].kind {
        MaskEffectKind::Fill { value, procedural: Some(p), .. } => (*value, p.balance, p.scale, p.seed),
        _ => panic!(),
    };
    assert_eq!(read(&layer), (0.75, 0.5, 6.0, 3));
    set(&mut layer, "amount", ParamValue::Number(0.0));
    assert_eq!(read(&layer).1, 1.0);
    assert!(!layer.mask.as_ref().unwrap().effects[0].visible, "a single layer can carry its own material");
}

#[test]
fn the_file_form_is_stable_and_reads_back() {
    let layer = LayerId(uuid::Uuid::from_u128(1));
    let effect = EffectId(uuid::Uuid::from_u128(2));
    let m = MaterialControls {
        params: vec![
            Param::float("wear", "Wear", 0.0, 1.0, 0.5).group("Wear").tip("Paint worn off the edges"),
            Param::color("paint_color", "Paint color", [0.5, 0.25, 0.125]),
            Param::toggle("dirt", "Dirt", true),
            Param::seed("seed", "Seed", 7),
        ],
        bindings: vec![
            Binding::effect(layer, effect, Prop::Level, "wear"),
            Binding::layer(layer, Prop::Value { channel: ChannelKind::BaseColor }, "paint_color"),
            Binding::layer(layer, Prop::PatternScale { channel: None }, "4"),
            Binding::layer(layer, Prop::Opacity { channel: Some(ChannelKind::Height) }, "dirt"),
        ],
    };
    let json = serde_json::to_string(&m).unwrap();
    // Pinned: this text is part of the project file format (ADR-0047).
    assert_eq!(
        json,
        concat!(
            r#"{"params":[{"id":"wear","label":"Wear","group":"Wear","tip":"Paint worn off the edges","type":"float","min":0.0,"max":1.0,"default":0.5,"value":0.5},"#,
            r#"{"id":"paint_color","label":"Paint color","type":"color","default":[0.5,0.25,0.125],"value":[0.5,0.25,0.125]},"#,
            r#"{"id":"dirt","label":"Dirt","type":"toggle","default":true,"value":true},"#,
            r#"{"id":"seed","label":"Seed","type":"seed","default":7,"value":7.0}],"#,
            r#""bindings":[{"layer":"00000000-0000-0000-0000-000000000001","effect":"00000000-0000-0000-0000-000000000002","prop":"level","formula":"wear"},"#,
            r#"{"layer":"00000000-0000-0000-0000-000000000001","prop":"value","channel":"base_color","formula":"paint_color"},"#,
            r#"{"layer":"00000000-0000-0000-0000-000000000001","prop":"pattern_scale","formula":"4"},"#,
            r#"{"layer":"00000000-0000-0000-0000-000000000001","prop":"opacity","channel":"height","formula":"dirt"}]}"#
        )
    );
    assert_eq!(serde_json::from_str::<MaterialControls>(&json).unwrap(), m);
    // A whole-number value written by hand still reads as a number.
    let by_hand: MaterialControls = serde_json::from_str(r#"{"params":[{"id":"a","label":"A","type":"float","min":0,"max":2,"default":1,"value":2}]}"#).unwrap();
    assert_eq!(by_hand.params[0].value, ParamValue::Number(2.0));
    assert!(by_hand.bindings.is_empty());
}

#[test]
fn a_definition_from_a_file_is_made_safe() {
    let mut m = MaterialControls {
        params: vec![
            Param::float("ok", "Ok", 0.0, 1.0, 0.5),
            Param::float("ok", "Twice", 0.0, 1.0, 0.5),
            Param::float("1bad", "Bad id", 0.0, 1.0, 0.5),
            Param::float("mix", "A function name", 0.0, 1.0, 0.5),
            Param::float("has space", "Bad id", 0.0, 1.0, 0.5),
            Param::seed("seed", "Seed", 3),
            Param::color("tint", "Tint", [0.1, 0.2, 0.3]),
        ],
        bindings: Vec::new(),
    };
    m.params[0].value = ParamValue::Number(9.0);
    m.params[5].value = ParamValue::Color([1.0; 3]);
    m.params[6].value = ParamValue::Color([f32::NAN, 0.0, 0.0]);
    assert_eq!(m.validate().len(), 7, "{:?}", m.validate());
    let safe = m.sanitized();
    assert_eq!(safe.params.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["ok", "seed", "tint"]);
    assert_eq!(safe.params[0].value, ParamValue::Number(1.0));
    assert_eq!(safe.params[1].value, ParamValue::Number(3.0), "a wrong type falls back to the default");
    assert_eq!(safe.params[2].value, ParamValue::Color([0.1, 0.2, 0.3]));
    assert_eq!(safe.validate(), Vec::<String>::new());
    let many = MaterialControls { params: (0..100).map(|i| Param::float(&format!("p{i}"), "P", 0.0, 1.0, 0.0)).collect(), bindings: Vec::new() };
    assert_eq!(many.sanitized().params.len(), MAX_PARAMS);
    assert!(valid_param_id("_wear2") && !valid_param_id("") && !valid_param_id(&"a".repeat(49)));
}

/// A material filter's settings can be exposed and driven like any other property: the value
/// is clamped into the setting's range, and a setting the filter lacks is reported.
#[test]
fn filter_settings_are_exposed_and_driven() {
    use crate::{ContentEffect, ContentEffectKind, FilterSetting, MaterialFilter};
    let ts = TextureSet::new("set", 0, 64);
    let mut layer = Layer::new_fill("Steel", &ts);
    let rust = ContentEffect::new(ContentEffectKind::Material(MaterialFilter::ALL[4]), &ts);
    let hammered = ContentEffect::new(ContentEffectKind::Material(MaterialFilter::ALL[1]), &ts);
    let (rust_id, hammered_id) = (rust.id, hammered.id);
    layer.content_effects = vec![Arc::new(hammered), Arc::new(rust)];

    let all = exposable(&layer);
    let amount = all.iter().find(|e| e.effect == Some(rust_id) && e.prop == Prop::Filter { setting: FilterSetting::Amount }).expect("Rust amount is offered");
    assert_eq!(amount.label, "Amount");
    assert_eq!(amount.effect_name.as_deref(), Some("MatFx Rust Weathering"));
    assert_eq!(amount.kind, ParamKind::Float { min: 0.0, max: 1.0, default: 0.45 });
    assert_eq!(all.iter().filter(|e| e.effect == Some(rust_id)).count(), 8, "every Rust Weathering setting");
    assert!(all.iter().any(|e| e.effect == Some(hammered_id) && e.prop == Prop::Filter { setting: FilterSetting::Depth }));

    let mut controls = MaterialControls::default();
    let id = controls.expose(amount).unwrap();
    controls.bindings.push(Binding::effect(layer.id, hammered_id, Prop::Filter { setting: FilterSetting::Depth }, &format!("{id} * 4")));
    assert_eq!(controls.check(&layer), vec![]);
    assert!(controls.set(&id, ParamValue::Number(0.9)));
    assert!(controls.apply(&mut layer));
    let kind = |l: &Layer, id| l.content_effects.iter().find(|e| e.id == id).unwrap().kind.clone();
    let ContentEffectKind::Material(MaterialFilter::RustWeathering { amount, edges, .. }) = kind(&layer, rust_id) else { panic!() };
    assert_eq!((amount, edges), (0.9, 0.5), "only the bound setting moved");
    let ContentEffectKind::Material(m @ MaterialFilter::Hammered { depth, .. }) = kind(&layer, hammered_id) else { panic!() };
    assert_eq!(depth, 1.0, "0.9 * 4 is clamped");
    assert!(m.valid());
    assert!(!controls.apply(&mut layer), "applying again changes nothing");

    // A setting this filter does not have.
    controls.bindings.push(Binding::effect(layer.id, hammered_id, Prop::Filter { setting: FilterSetting::Rust }, &id));
    assert_eq!(controls.check(&layer), vec![(2, BindingProblem::NotApplicable)]);

    let json = serde_json::to_string(&controls.bindings[1]).unwrap();
    assert!(json.contains(r#""prop":"filter","setting":"depth""#), "{json}");
    assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), controls.bindings[1]);
}
