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

    /// Texts drawn this frame whose centre is inside their clip rect (the combo list is a
    /// scroll area: rows scrolled out of it are clipped or not drawn).
    fn texts(&mut self) -> Vec<(String, Pos2)> {
        let out = self.frame(vec![]);
        out.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => {
                let c = t.pos + t.galley.size() * 0.5;
                s.clip_rect.contains(c).then(|| (t.galley.text().to_string(), c))
            }
            _ => None,
        }).collect()
    }

    /// Centre of the last-drawn visible text `label` (popups and menus are drawn last).
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

    /// Open the template list; returns the picker button's centre.
    fn open_list(&mut self) -> Pos2 {
        let current = self.presets.find(&self.key).unwrap().name.clone();
        let button = self.at(&current);
        self.press(button, PointerButton::Primary);
        self.frame(vec![]);
        assert!(self.find("BUILT-IN (READ-ONLY)").is_some() || self.find("MY PRESETS").is_some(), "the list opens: {:?}", self.texts());
        button
    }

    /// Scroll the open list (mouse wheel over it) until the row `name` is visible; its centre.
    fn reveal(&mut self, button: Pos2, name: &str) -> Pos2 {
        let over = egui::pos2(button.x, button.y + 80.0);
        let wheel = |y: f32| Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, y), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE };
        // Down first, then back up (the list remembers where it was scrolled to).
        for step in 0..120 {
            let row = self.texts().into_iter().filter(|(t, p)| t == name && p.y > button.y + 10.0).map(|(_, p)| p).last();
            if let Some(p) = row {
                return p;
            }
            let dy = if step < 40 { -60.0 } else { 60.0 };
            self.frame(vec![Event::PointerMoved(over), wheel(dy)]);
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }
        panic!("{name:?} never shows in the list: {:?}", self.texts())
    }

    /// Open the picker and right-click the template `name` in its list; the menu opens.
    fn menu_on(&mut self, name: &str) {
        let button = self.open_list();
        let row = self.reveal(button, name);
        self.press(row, PointerButton::Secondary);
        self.frame(vec![]);
        let texts = self.texts();
        assert!(texts.iter().filter(|(t, _)| t == "Duplicate").count() >= 2, "the menu opens over the list: {texts:?}");
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
    let button = ui.open_list();
    let row = ui.reveal(button, "PBR Metallic Roughness");
    ui.press(row, PointerButton::Primary);
    ui.frame(vec![]);
    assert_eq!(ui.presets.find(&ui.key).unwrap().name, "PBR Metallic Roughness");
    assert!(ui.find("BUILT-IN (READ-ONLY)").is_none() && ui.find("MY PRESETS").is_none(), "picking closes the list");
    std::fs::remove_dir_all(&dir).ok();
}
