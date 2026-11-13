//! Small building blocks of the Bake window: map toggles with icons, per-map progress rows and
//! the card title. Pure presentation; the window logic stays in `dialogs`.

use egui::{Align2, Color32, RichText, Sense, Ui, vec2};
use subcraft_app::{MapProgress, MapState};
use subcraft_core::MeshMapKind;

use crate::i18n::tr;
use crate::icons;
use crate::theme::{self, ACCENT, ACCENT_SOFT, BG_3, BG_4, TEXT, TEXT_DIM, TEXT_FAINT};

/// "Done" marks (the theme's success colour).
#[allow(non_snake_case)]
pub fn OK_GREEN() -> Color32 {
    theme::SUCCESS()
}

/// Icon and one-line explanation of a baked map (shown as the tooltip of its toggle).
pub fn map_info(kind: MeshMapKind) -> (&'static str, &'static str) {
    match kind {
        MeshMapKind::Normal => (icons::IMAGE, "Surface detail of the high poly as a tangent-space normal map"),
        MeshMapKind::WorldSpaceNormal => (icons::GRID, "Surface direction in world space (used for edge and dirt generators)"),
        MeshMapKind::Id => (icons::SQUARES, "A flat colour for every mesh part, for masks"),
        MeshMapKind::AmbientOcclusion => (icons::SUN, "Shadowing in crevices, from how much sky each point sees"),
        MeshMapKind::Curvature => (icons::CURVE, "Convex edges bright, concave edges dark (edge wear, dirt)"),
        MeshMapKind::Position => (icons::CROSSHAIR, "Where each point is on the model (gradients and masks)"),
        MeshMapKind::Thickness => (icons::DROP, "How thick the model is behind each point (scattering, wear)"),
        MeshMapKind::Height => (icons::FIT, "How far the high poly stands above or below the low poly"),
        MeshMapKind::BentNormal => (icons::CURSOR, "Average open direction, for better ambient light"),
    }
}

/// An icon + name toggle for one map. Returns true when it was clicked.
pub fn map_chip(ui: &mut Ui, kind: MeshMapKind, on: bool, width: f32) -> bool {
    let (icon, tip) = map_info(kind);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    let fill = if on { ACCENT_SOFT() } else if response.hovered() { BG_4() } else { BG_3() };
    let painter = ui.painter();
    painter.rect(rect, 6, fill, egui::Stroke::new(1.0, if on { theme::ACCENT_DIM() } else { Color32::TRANSPARENT }), egui::StrokeKind::Inside);
    let color = if on { ACCENT() } else { TEXT_FAINT() };
    painter.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, icon, theme::font(16.0), color);
    painter.text(rect.left_center() + vec2(38.0, 0.0), Align2::LEFT_CENTER, tr(kind.display_name()), theme::font(13.0), if on { TEXT() } else { TEXT_DIM() });
    if on {
        painter.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, icons::CHECK, theme::font(13.0), ACCENT());
    }
    response.on_hover_text(tr(tip)).clicked()
}

/// A small square icon toggle with a tooltip.
pub fn icon_toggle(ui: &mut Ui, icon: &str, on: &mut bool, tip: &'static str) {
    let (rect, response) = ui.allocate_exact_size(vec2(34.0, 30.0), Sense::click());
    if response.clicked() { *on = !*on; }
    let fill = if *on { ACCENT_SOFT() } else if response.hovered() { BG_4() } else { BG_3() };
    ui.painter().rect(rect, 6, fill, egui::Stroke::new(1.0, if *on { theme::ACCENT_DIM() } else { Color32::TRANSPARENT }), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::font(16.0), if *on { ACCENT() } else { TEXT_DIM() });
    response.on_hover_text(tr(tip));
}

/// Card title: an icon, a bold name, and whatever `right` adds at the right edge.
pub fn card_title(ui: &mut Ui, icon: &str, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).font(theme::font(16.0)).color(ACCENT()));
        ui.label(RichText::new(title).font(theme::font_strong(14.0)).color(TEXT()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(theme::SP_S);
}

/// One line per map while baking: status icon, name, time spent.
pub fn status_row(ui: &mut Ui, p: &MapProgress) {
    ui.horizontal(|ui| {
        ui.set_min_height(20.0);
        let (icon, color): (&str, Color32) = match p.state {
            MapState::Waiting => ("\u{b7}", TEXT_FAINT()),
            MapState::Running => ("", ACCENT()),
            MapState::Done => (icons::CHECK, OK_GREEN()),
            MapState::Warning => (icons::WARNING, theme::WARNING()),
        };
        let (slot, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        if p.state == MapState::Running {
            ui.put(egui::Rect::from_center_size(slot.center(), vec2(14.0, 14.0)), egui::Spinner::new().size(14.0).color(ACCENT()));
        } else {
            ui.painter().text(slot.center(), Align2::CENTER_CENTER, icon, theme::font(15.0), color);
        }
        let (map_icon, _) = map_info(p.kind);
        ui.label(RichText::new(map_icon).font(theme::font(14.0)).color(TEXT_FAINT()));
        let name = RichText::new(tr(p.kind.display_name())).color(if p.state == MapState::Waiting { TEXT_FAINT() } else { TEXT() });
        ui.label(name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = match p.state {
                MapState::Waiting => String::new(),
                MapState::Running => format!("{:.1} s", p.seconds),
                _ => format!("{:.1} s", p.seconds),
            };
            ui.label(RichText::new(text).color(TEXT_DIM()).monospace());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_map_has_an_icon_that_exists_in_the_embedded_font_and_a_tooltip() {
        for kind in MeshMapKind::ALL {
            let (icon, tip) = map_info(kind);
            assert!(icons::ALL.contains(&icon), "{kind:?}");
            assert!(tip.len() > 10);
        }
    }
}
