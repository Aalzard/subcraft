use subcraft_app::{Command, CommandError, Session};
use subcraft_paint::{BrushSettings, StrokeOptions};

#[test]
fn brush_settings_command_updates_tool_and_presets_without_document_history() {
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let document = session.document().clone();
    let count = session.history().entries().len();
    let settings = BrushSettings {
        size: 12.0,
        flow: 0.4,
        stroke_opacity: 0.73,
        hardness: 0.2,
        ..Default::default()
    };
    session
        .execute(Command::SetBrushSettings(settings))
        .unwrap();
    assert_eq!(session.brush.settings, settings);
    assert_eq!(session.brush_preset("Toolbar").settings, settings);
    assert_eq!(session.document(), &document);
    assert_eq!(session.history().entries().len(), count);
    let invalid = BrushSettings {
        flow: f32::NAN,
        ..settings
    };
    assert!(session.execute(Command::SetBrushSettings(invalid)).is_err());
    assert_eq!(session.brush.settings, settings);
    session
        .execute(Command::SetBrushSettings(BrushSettings {
            size: 1000.0,
            flow: -1.0,
            stroke_opacity: 2.0,
            hardness: -5.0,
            ..settings
        }))
        .unwrap();
    assert_eq!(
        (
            session.brush.settings.size,
            session.brush.settings.flow,
            session.brush.settings.stroke_opacity,
            session.brush.settings.hardness
        ),
        (100.0, 0.0, 1.0, 0.0)
    );
    session.begin_stroke().unwrap();
    assert_eq!(
        session.execute(Command::SetBrushSettings(settings)),
        Err(CommandError::StrokeInProgress)
    );
    session.cancel_stroke();
}

#[test]
fn stroke_options_command_validates_shared_preset_state_without_history_or_mid_stroke_edits() {
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let before = session.document().clone();
    let history = session.history().entries().len();
    let options = StrokeOptions {
        symmetry: [true, false, true],
        mirror_offset: [0.25, 0.0, -0.5],
        lazy_distance_px: 25.0,
        smoothing: 0.5,
        catch_up: true,
        adaptive_smoothing: true,
        radial: subcraft_paint::RadialSymmetry { enabled: true, axis: 2, count: 12, center_offset: [0.1, 0.0, -0.2], flip: true },
    };
    session.execute(Command::SetStrokeOptions(options)).unwrap();
    assert_eq!(session.brush.options, options);
    assert_eq!(session.brush_preset("Toolbar mirror").options, options);
    assert_eq!(session.document(), &before);
    assert_eq!(session.history().entries().len(), history);
    assert!(
        session
            .execute(Command::SetStrokeOptions(StrokeOptions {
                lazy_distance_px: f32::NAN,
                ..options
            }))
            .is_err()
    );
    assert_eq!(session.brush.options, options);
    session
        .execute(Command::SetStrokeOptions(StrokeOptions {
            lazy_distance_px: 1000.0,
            ..options
        }))
        .unwrap();
    assert_eq!(session.brush.options.lazy_distance_px, 100.0);
    // Radial: count and axis are clamped to what painting supports; a non-finite center is refused.
    let wild = subcraft_paint::RadialSymmetry { enabled: true, axis: 7, count: 500, center_offset: [0.0; 3], flip: false };
    session.execute(Command::SetStrokeOptions(StrokeOptions { radial: wild, ..options })).unwrap();
    assert_eq!((session.brush.options.radial.axis, session.brush.options.radial.count), (2, 64));
    let bad = subcraft_paint::RadialSymmetry { center_offset: [f32::INFINITY, 0.0, 0.0], ..wild };
    assert!(session.execute(Command::SetStrokeOptions(StrokeOptions { radial: bad, ..options })).is_err());
    assert!(session.execute(Command::SetStrokeOptions(StrokeOptions { mirror_offset: [0.0, f32::NAN, 0.0], ..options })).is_err(), "a non-finite mirror position is refused");
    assert_eq!(session.brush.options.radial.count, 64);
    session.begin_stroke().unwrap();
    assert_eq!(
        session.execute(Command::SetStrokeOptions(StrokeOptions::default())),
        Err(CommandError::StrokeInProgress)
    );
    session.cancel_stroke();
}

/// Ten settings (spacing, angle, roundness, the jitters, pen pressure, the backface angle) were
/// never checked: a preset file reads `1e39` as infinity, which made every stamp NaN, and a
/// roundness of 0 flattened the footprint to nothing.
#[test]
fn every_numeric_brush_setting_is_finite_and_clamped() {
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let ok = BrushSettings::default();
    let fields: [(&str, fn(&mut BrushSettings, f32)); 10] = [
        ("spacing", |s, v| s.spacing = v),
        ("angle_deg", |s, v| s.angle_deg = v),
        ("roundness", |s, v| s.roundness = v),
        ("size_jitter", |s, v| s.size_jitter = v),
        ("flow_jitter", |s, v| s.flow_jitter = v),
        ("angle_jitter_deg", |s, v| s.angle_jitter_deg = v),
        ("position_jitter", |s, v| s.position_jitter = v),
        ("pressure_size", |s, v| s.pressure_size = v),
        ("pressure_flow", |s, v| s.pressure_flow = v),
        ("backface_angle_deg", |s, v| s.backface_angle_deg = v),
    ];
    for (name, set) in fields {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut s = ok;
            set(&mut s, bad);
            assert!(session.execute(Command::SetBrushSettings(s)).is_err(), "{name} = {bad} is refused");
        }
    }
    // Finite but silly values are clamped, and the built-in extremes (spacing 4, jitter 1.5) pass.
    let wild = BrushSettings { spacing: 0.0, roundness: 0.0, size_jitter: 9.0, position_jitter: 99.0, pressure_size: -3.0, backface_angle_deg: 0.0, ..ok };
    session.execute(Command::SetBrushSettings(wild)).unwrap();
    let s = session.brush.settings;
    assert_eq!((s.spacing, s.roundness, s.size_jitter, s.position_jitter, s.pressure_size, s.backface_angle_deg), (0.01, 0.05, 1.0, 4.0, 0.0, 10.0));
    let builtin = BrushSettings { spacing: 4.0, position_jitter: 1.5, ..ok };
    session.execute(Command::SetBrushSettings(builtin)).unwrap();
    assert_eq!((session.brush.settings.spacing, session.brush.settings.position_jitter), (4.0, 1.5));
}
