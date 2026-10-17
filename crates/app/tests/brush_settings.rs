use subcraft_app::{Command, CommandError, Session};
use subcraft_paint::BrushSettings;

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
