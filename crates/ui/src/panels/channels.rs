//! Channels: the selected texture set's material channels, as in other painters' texture set
//! settings: each channel with its storage format, a button to show it in the 3D view and one to
//! remove it, and "+ Add channel" for the ones the set lacks. Every change is one undo step.
use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_core::ChannelKind;
use subcraft_doc::ChannelFormat;
use subcraft_render::ViewMode;

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::icon_button;

const FORMATS: [ChannelFormat; 4] = [ChannelFormat::Bit8, ChannelFormat::Bit16, ChannelFormat::Float16, ChannelFormat::Float32];

/// The 3D view mode that shows `kind` on its own, if there is one.
fn view_for(kind: ChannelKind) -> Option<ViewMode> {
    Some(match kind {
        ChannelKind::BaseColor => ViewMode::BaseColor,
        ChannelKind::Normal => ViewMode::Normal,
        ChannelKind::Roughness => ViewMode::Roughness,
        ChannelKind::Metallic => ViewMode::Metallic,
        ChannelKind::Height => ViewMode::Height,
        ChannelKind::AmbientOcclusion => ViewMode::AmbientOcclusion,
        _ => return None,
    })
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let ts = state.session.selected_texture_set().clone();
    let mut cmds = Vec::new();
    ui.horizontal(|ui| {
        ui.label(RichText::new(&ts.name).font(theme::font_strong(13.0)).color(theme::TEXT));
        ui.label(RichText::new(format!("{0} × {0}", ts.resolution)).color(TEXT_DIM));
    });
    ui.label(RichText::new("Channels of the selected texture set. Layers paint and fill only the channels listed here; export writes them.").color(TEXT_DIM).font(theme::font(11.0)));
    ui.add_space(6.0);

    let current_view = {
        let prefs = &state.recovery.prefs.viewport;
        (prefs.mesh_map.is_none()).then(|| prefs.view_mode())
    };
    let mut show_view: Option<Option<ViewMode>> = None;
    egui::Grid::new("channels_grid").num_columns(4).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
        for def in &ts.channels {
            let kind = def.kind;
            ui.label(RichText::new(kind.display_name()).color(theme::channel_accent(kind)).font(theme::font_strong(12.5)));
            let mut format = def.format;
            egui::ComboBox::from_id_salt(("channel_format", kind as u32)).width(110.0).selected_text(format.label()).show_ui(ui, |ui| {
                for f in FORMATS {
                    ui.selectable_value(&mut format, f, f.label());
                }
            });
            if format != def.format {
                cmds.push(Command::SetChannelFormat { texture_set: ts.id, kind, format });
            }
            match view_for(kind) {
                Some(mode) => {
                    let on = current_view == Some(mode);
                    let tip = if on { "Back to the material view" } else { "Show this channel alone in the 3D view" };
                    if icon_button(ui, if on { icons::EYE } else { icons::EYE_SLASH }, tip).clicked() {
                        show_view = Some(if on { None } else { Some(mode) });
                    }
                }
                None => { ui.label(""); }
            }
            let last = ts.channels.len() == 1;
            let resp = ui.add_enabled(!last, egui::Button::new(icons::TRASH).frame(false))
                .on_hover_text(format!("Remove the {} channel (undo brings it back)", kind.display_name()))
                .on_disabled_hover_text("A texture set needs at least one channel");
            if resp.clicked() {
                cmds.push(Command::RemoveChannel { texture_set: ts.id, kind });
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    let missing: Vec<ChannelKind> = ChannelKind::ALL.into_iter().filter(|k| !ts.has_channel(*k)).collect();
    ui.add_enabled_ui(!missing.is_empty(), |ui| {
        ui.menu_button(format!("{}  Add channel", icons::PLUS), |ui| {
            for kind in &missing {
                if ui.button(kind.display_name()).clicked() {
                    cmds.push(Command::AddChannel { texture_set: ts.id, kind: *kind });
                    ui.close();
                }
            }
        });
    });
    if let Some(view) = show_view {
        let prefs = &mut state.recovery.prefs.viewport;
        prefs.mesh_map = None;
        prefs.solo = None;
        let mode = view.unwrap_or(ViewMode::Material);
        prefs.view_mode = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
    }
    if !cmds.is_empty() {
        state.run(cmds, now);
    }
}
