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
