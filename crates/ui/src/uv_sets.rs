//! Texture Set Settings > UV set: paint a texture set with another UV layout of the model.
//! Shown only for models with more than one UV set. The work is `subcraft_app::uv_sets`.

use egui::{RichText, Ui};
use subcraft_app::{MeshReloadJob, UvSetOutput};
use subcraft_core::TextureSetId;

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};

#[derive(Default)]
pub struct UvSetUi {
    pub job: Option<MeshReloadJob<UvSetOutput>>,
    /// A set picked in the list, waiting for the user to confirm (the switch clears undo).
    pub pending: Option<(TextureSetId, usize)>,
    pub error: Option<String>,
}

/// Start switching texture set `ts` to UV set `set`.
pub fn start(state: &mut UiState, ts: TextureSetId, set: usize) {
    state.uv_sets.pending = None;
    if state.session.is_stroking() {
        state.uv_sets.error = Some("finish the stroke first".into());
        return;
    }
    match state.session.start_uv_set_switch(ts, set) {
        Ok(job) => {
            state.uv_sets.error = None;
            state.uv_sets.job = Some(job);
        }
        Err(e) => state.uv_sets.error = Some(e),
    }
}

/// Poll the background switch (call every frame).
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(job) = &mut state.uv_sets.job else { return };
    ctx.request_repaint();
    let Some(result) = job.poll() else { return };
    state.uv_sets.job = None;
    match result.and_then(|out| state.session.finish_uv_set_switch(out)) {
        Ok(summary) => {
            state.mesh_reloaded();
            state.set_status(summary, now);
        }
        Err(e) => state.uv_sets.error = Some(e),
    }
}

/// The "UV set" row of Texture Set Settings; nothing for a model with one UV set.
pub fn row(state: &mut UiState, ui: &mut Ui, label: impl FnOnce(&mut Ui)) {
    let id = state.session.selected_texture_set().id;
    let Some(choice) = state.session.uv_set_choice(id) else { return };
    let busy = state.uv_sets.job.is_some();
    let mut picked = state.uv_sets.pending.filter(|p| p.0 == id).map_or(choice.current, |p| p.1);
    ui.horizontal(|ui| {
        label(ui);
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt(("tss_uv_set", id)).width(ui.available_width() - 28.0).selected_text(&choice.names[picked]).show_ui(ui, |ui| {
                for (i, name) in choice.names.iter().enumerate() {
                    ui.selectable_value(&mut picked, i, name);
                }
            }).response.on_hover_text("The UV layout this texture set is painted and exported with. The model file has several.");
        });
        if busy {
            ui.spinner();
        }
    });
    if picked != choice.current {
        state.uv_sets.pending = Some((id, picked));
    } else if state.uv_sets.pending.is_some_and(|p| p.0 == id) {
        state.uv_sets.pending = None;
    }
    if let Some((_, set)) = state.uv_sets.pending.filter(|p| p.0 == id) {
        ui.label(RichText::new(format!("Paint with {}? Painted layers are moved onto it so they stay in place. This clears the undo history.", choice.names[set])).color(TEXT_DIM()));
        ui.horizontal(|ui| {
            if ui.button("Switch").clicked() {
                start(state, id, set);
            }
            if ui.button("Cancel").clicked() {
                state.uv_sets.pending = None;
            }
        });
    }
    if let Some(e) = &state.uv_sets.error {
        ui.label(RichText::new(e).color(theme::WARNING()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec2, Vec3};
    use subcraft_app::Session;
    use subcraft_mesh::{Corner, MeshBuilder};

    fn two_uv_sets() -> subcraft_mesh::Mesh {
        let mut b = MeshBuilder::new("quad");
        let m = b.material("body");
        let p = [Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.), Vec3::new(1., 1., 0.), Vec3::new(0., 1., 0.)];
        let uv0 = [Vec2::new(0., 1.), Vec2::new(1., 1.), Vec2::new(1., 0.), Vec2::new(0., 0.)];
        for tri in [[0, 1, 2], [0, 2, 3]] {
            let c = tri.map(|i| Corner { position: p[i], normal: Some(Vec3::Z), uv: Some(uv0[i]) });
            b.push_triangle_uv_sets(m, c, &[], &[tri.map(|i| Some(uv0[i] * 0.5))]);
        }
        b.build().unwrap()
    }

    /// One frame of Texture Set Settings, optionally with a click; returns the drawn texts.
    fn frame(ctx: &egui::Context, state: &mut UiState, click: Option<egui::Pos2>) -> Vec<(String, egui::Rect)> {
        let mut events = Vec::new();
        if let Some(at) = click {
            events.push(egui::Event::PointerMoved(at));
            events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
            events.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        }
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 3000.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| { egui::CentralPanel::default().show(ui, |ui| crate::panels::texture_sets::settings(state, ui)); });
        out.textures_delta.clear();
        out.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.job.text.clone(), t.visual_bounding_rect())),
            _ => None,
        }).collect()
    }

    #[test]
    fn the_uv_set_row_appears_only_with_several_sets_and_switches_after_confirming() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut one = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        frame(&ctx, &mut one, None);
        assert!(!frame(&ctx, &mut one, None).iter().any(|(t, _)| t == "UV set"), "no row for a single UV set");

        let mut state = UiState::new(Session::new(two_uv_sets(), 64), None);
        frame(&ctx, &mut state, None);
        let texts = frame(&ctx, &mut state, None);
        assert!(texts.iter().any(|(t, _)| t == "UV set"));
        assert!(texts.iter().any(|(t, _)| t == "UV0"), "the current set is shown");
        // Picking a set asks first; nothing changes until Switch.
        let ts = state.session.selected_texture_set().id;
        state.uv_sets.pending = Some((ts, 1));
        let texts = frame(&ctx, &mut state, None);
        assert_eq!(state.session.uv_set_choice(ts).unwrap().current, 0);
        let switch = texts.iter().find(|(t, _)| t == "Switch").expect("a Switch button").1.center();
        frame(&ctx, &mut state, Some(switch));
        let job = state.uv_sets.job.take().expect("the switch started");
        let out = job.wait().unwrap();
        state.session.finish_uv_set_switch(out).unwrap();
        assert_eq!(state.session.uv_set_choice(ts).unwrap().current, 1);
        assert!(state.uv_sets.pending.is_none());
        let texts = frame(&ctx, &mut state, None);
        assert!(texts.iter().any(|(t, _)| t == "UV1"), "the row shows the new set");
    }
}
