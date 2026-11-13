//! Painter's output template list actions: right-click a template for Rename, Duplicate and
//! Delete (built-ins can only be duplicated). Real egui pointer and keyboard events on the
//! template picker; no desktop mouse.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use subcraft_app::export_presets::ExportPresets;
use subcraft_assets::UserLibrary;
use subcraft_ui::export_presets::{PresetEditor, preset_bar};

struct Ui {
    ctx: egui::Context,
    presets: ExportPresets,
    key: String,
    ed: PresetEditor,
    status: Option<String>,
}

impl Ui {
    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        let (presets, key, ed, status) = (&mut self.presets, &mut self.key, &mut self.ed, &mut self.status);
        let mut out = self.ctx.run_ui(
            egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 900.0))), ..Default::default() },
            |ui| {
                if let Some(s) = preset_bar(ui, presets, key, ed) {
                    *status = Some(s);
                }
            },
        );
        out.textures_delta.clear();
        out
    }

    fn texts(&mut self) -> Vec<(String, Pos2)> {
        let out = self.frame(vec![]);
        out.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5)),
            _ => None,
        }).collect()
    }

    /// Centre of the last-drawn text `label` (popups and menus are drawn last).
    fn find(&mut self, label: &str) -> Option<Pos2> {
        self.texts().into_iter().filter(|(t, _)| t == label).map(|(_, p)| p).last()
    }

    fn at(&mut self, label: &str) -> Pos2 {
        let texts = self.texts();
        texts.iter().filter(|(t, _)| t == label).map(|(_, p)| *p).last().unwrap_or_else(|| {
            let all: Vec<&str> = texts.iter().map(|(t, _)| t.as_str()).collect();
            panic!("no {label:?} on screen: {all:?}")
        })
    }

    fn press(&mut self, pos: Pos2, which: PointerButton) {
        let b = |pressed| Event::PointerButton { pos, button: which, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(pos), b(true)]);
        self.frame(vec![b(false)]);
    }

    fn click(&mut self, label: &str, which: PointerButton) {
        let p = self.at(label);
        self.press(p, which);
        self.frame(vec![]);
    }

    /// Open the picker and right-click the template `name` in its list.
    fn menu_on(&mut self, name: &str) {
        let current = self.presets.find(&self.key).unwrap().name.clone();
        self.click(&current, PointerButton::Primary);
        self.click(name, PointerButton::Secondary);
    }

    fn user_names(&self) -> Vec<String> {
        self.presets.user().iter().map(|p| p.name.clone()).collect()
    }
}

fn setup(name: &str) -> (std::path::PathBuf, Ui) {
    let dir = std::env::temp_dir().join(format!("subcraft-template-menu-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = UserLibrary::new(dir.clone());
    let mut presets = ExportPresets::load(Some(&lib));
    let p = presets.duplicate("Unreal Engine (ORM packed)").unwrap();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    let mut ui = Ui { ctx, presets, key: p.key().to_string(), ed: PresetEditor::default(), status: None };
    for _ in 0..3 {
        ui.frame(vec![]);
    }
    (dir, ui)
}

#[test]
fn right_click_renames_duplicates_and_deletes_templates() {
    let (dir, mut ui) = setup("menu");
    let copy = "Unreal Engine (ORM packed) copy";
    assert_eq!(ui.user_names(), [copy]);

    // Right-click > Rename… on the user preset, type a new name, Enter.
    ui.menu_on(copy);
    ui.click("Rename…", PointerButton::Primary);
    assert!(ui.find("Rename preset").is_some(), "the rename field opens");
    let field = ui.at(copy);
    ui.press(field, PointerButton::Primary);
    ui.frame(vec![Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    ui.frame(vec![Event::Text("Hero maps".into())]);
    ui.frame(vec![Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    ui.frame(vec![]);
    assert_eq!(ui.user_names(), ["Hero maps"], "status: {:?}", ui.status);
    assert!(ui.find("Rename preset").is_none(), "the field closes");

    // Right-click > Duplicate on a built-in: a user copy, selected.
    ui.menu_on("PBR Metallic Roughness");
    ui.click("Duplicate", PointerButton::Primary);
    assert_eq!(ui.user_names().len(), 2, "status: {:?}", ui.status);
    assert_eq!(ui.presets.find(&ui.key).unwrap().name, "PBR Metallic Roughness copy");

    // Built-ins can't be renamed: the entry is disabled.
    ui.menu_on("PBR Metallic Roughness");
    ui.click("Rename…", PointerButton::Primary);
    assert!(ui.find("Rename preset").is_none(), "no rename field for a built-in");
    ui.frame(vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    ui.frame(vec![]);

    // Right-click > Delete asks first; confirming removes the file.
    ui.menu_on("Hero maps");
    ui.click("Delete", PointerButton::Primary);
    let ask = ui.texts().into_iter().any(|(t, _)| t.starts_with("Delete preset \"Hero maps\""));
    assert!(ask, "asks before deleting");
    ui.click("Delete", PointerButton::Primary);
    assert_eq!(ui.user_names(), ["PBR Metallic Roughness copy"], "status: {:?}", ui.status);
    assert_eq!(ExportPresets::load(Some(&UserLibrary::new(dir.clone()))).user().len(), 1, "the file is gone");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_left_click_picks_a_template_and_closes_the_list() {
    let (dir, mut ui) = setup("pick");
    let copy = "Unreal Engine (ORM packed) copy";
    ui.click(copy, PointerButton::Primary);
    assert!(ui.find("MY PRESETS").is_some(), "the list is open");
    ui.click("PBR Metallic Roughness", PointerButton::Primary);
    assert_eq!(ui.presets.find(&ui.key).unwrap().name, "PBR Metallic Roughness");
    assert!(ui.find("MY PRESETS").is_none(), "picking closes the list");
    std::fs::remove_dir_all(&dir).ok();
}
