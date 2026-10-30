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
        lazy_distance_px: 25.0,
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
    session.begin_stroke().unwrap();
    assert_eq!(
        session.execute(Command::SetStrokeOptions(StrokeOptions::default())),
        Err(CommandError::StrokeInProgress)
    );
    session.cancel_stroke();
}

#[test]
fn pressure_curves_are_validated_and_drive_session_strokes() {
    use glam::{Vec2, Vec3};
    use subcraft_doc::LayerContent;
    use subcraft_paint::PressureCurve;
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let mut settings = session.brush.settings;
    settings.pressure.size = PressureCurve { enabled: true, min: f32::NAN, gamma: 1.0 };
    assert!(session.execute(Command::SetBrushSettings(settings)).is_err());
    settings.pressure.size = PressureCurve { enabled: true, min: 3.0, gamma: 99.0 };
    session.execute(Command::SetBrushSettings(settings)).unwrap();
    assert_eq!(session.brush.settings.pressure.size, PressureCurve { enabled: true, min: 1.0, gamma: 5.0 });

    let cam = subcraft_app::Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    settings.size = 20.0;
    settings.pressure.size = PressureCurve { enabled: true, min: 0.0, gamma: 1.0 };
    session.execute(Command::SetBrushSettings(settings)).unwrap();
    let painted = |s: &Session| -> usize {
        let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
        let img = &p.channels[&subcraft_core::ChannelKind::BaseColor];
        let (w, h) = (img.width(), img.height());
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y)[img.comps() - 1] > 0.01).count()
    };
    let mut stroke = |pressure: Option<f32>| {
        session.begin_stroke().unwrap();
        for x in [236.0, 246.0, 256.0, 266.0, 276.0] {
            match pressure {
                Some(p) => session.stroke_to_pressure(&cam, Vec2::new(x, 256.0), false, p).unwrap(),
                None => session.stroke_to(&cam, Vec2::new(x, 256.0), false).unwrap(),
            };
        }
        session.end_stroke();
        let n = painted(&session);
        session.execute(Command::Undo).unwrap();
        n
    };
    let (light, mouse, full) = (stroke(Some(0.3)), stroke(None), stroke(Some(1.0)));
    assert!(light > 0 && light < mouse / 2, "light {light} vs mouse {mouse}");
    assert_eq!(mouse, full, "mouse strokes paint at full pressure");
}
