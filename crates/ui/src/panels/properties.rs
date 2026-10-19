//! Properties of the selected layer: name, fill channels + projection, mask and its effects.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, LayerContent, Levels, MaskEffect, MaskEffectKind, ProceduralKind,
    PartMask, ProceduralParams, Projection, ProjectionMode,
};

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, BG_2, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{channel_value, empty_state, fill_width, hint, icon_button, panel, primary_button, prop_row, secondary_button, section, section_with, segmented, slider_row};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    if super::effects::properties(state, ui) {
        return;
    }
    if state.session.layer_target() == subcraft_app::LayerTarget::Mask {
        super::mask::show(state, ui);
        return;
    }
    let now = ui.input(|i| i.time);
    let Some(layer) = state.session.selected_layer().cloned() else {
        panel(ui, |ui| empty_state(ui, icons::SLIDERS, "Select a layer to edit its properties."));
        return;
    };
    let mut cmds = Vec::new();
    let mut texture_drop: Option<(ChannelKind, std::path::PathBuf)> = None;
    let mut starter_drop = None;
    let id = layer.id;
    let channels = state.session.channels();
    let part_kind = layer.parts.as_ref().map_or(subcraft_core::PartKind::Connected, |p| p.kind);
    let mesh_parts = state.session.parts(part_kind).iter().filter(|p| p.materials.contains(&state.session.selected_texture_set().material_slot)).cloned().collect::<Vec<_>>();
    let mut picking = state.picking_parts;

    egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| panel(ui, |ui| {
        // Name, with the layer type as an icon.
        ui.horizontal(|ui| {
            let icon = match &layer.content {
                LayerContent::Paint(p) if p.path.is_some() => icons::CURVE,
                LayerContent::Paint(_) => icons::PAINT_BRUSH,
                LayerContent::Fill(_) => icons::PAINT_BUCKET,
                LayerContent::Group(_) => icons::FOLDER_OPEN,
            };
            ui.label(RichText::new(icon).font(theme::font(18.0)).color(ACCENT));
            fill_width(ui);
            let buf = match &mut state.rename {
                Some((lid, s)) if *lid == id => s,
                slot => {
                    *slot = Some((id, layer.name.clone()));
                    &mut slot.as_mut().expect("just set").1
                }
            };
            let r = ui.add(egui::TextEdit::singleline(buf).font(theme::font_strong(14.0)).margin(egui::vec2(8.0, 5.0)).desired_width(f32::INFINITY));
            if r.lost_focus() && *buf != layer.name && !buf.trim().is_empty() {
                cmds.push(Command::Rename { layer: id, name: buf.trim().to_string() });
            }
        });

        ui.add_space(8.0);

        let mut part_cmds = Vec::new();
        let mut part_head = Vec::new();
        section_with(
            ui,
            "parts",
            "Parts",
            layer.parts.is_some(),
            |ui| {
                if let Some(p) = &layer.parts {
                    if icon_button(ui, icons::TRASH, "Show on the whole model again").clicked() {
                        part_head.push(Command::SetParts { layer: id, parts: None });
                    }
                    let (icon, tip) = if p.enabled { (icons::EYE, "Ignore parts (show everywhere)") } else { (icons::EYE_SLASH, "Use parts again") };
                    if icon_button(ui, icon, tip).clicked() {
                        part_head.push(Command::SetParts { layer: id, parts: Some(PartMask { enabled: !p.enabled, ..p.clone() }) });
                    }
                }
            },
            |ui| {
                let label = if picking { format!("{}  Done picking", icons::CHECK) } else { format!("{}  Pick parts in the 3D view", icons::CURSOR) };
                let pick = if picking { primary_button(ui, label) } else { secondary_button(ui, label) };
                if pick.clicked() {
                    picking = !picking;
                }
                match &layer.parts {
                    None => hint(ui, "Show this layer only on some pieces of the model (a blade, a handle…): pick them, then paint or fill as usual."),
                    Some(p) => {
                        let shown = mesh_parts.iter().filter(|m| p.parts.contains(&m.id)).count();
                        let state_text = if p.enabled { format!("Visible on {shown} of {} parts.", mesh_parts.len()) } else { "Off: the layer shows everywhere.".into() };
                        hint(ui, &state_text);
                        ui.horizontal(|ui| {
                            let all: std::collections::BTreeSet<u32> = mesh_parts.iter().map(|m| m.id).collect();
                            if ui.small_button("All").clicked() {
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, all.clone())) });
                            }
                            if ui.small_button("None").clicked() {
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, [])) });
                            }
                            if ui.small_button("Invert").clicked() {
                                let inv = all.difference(&p.parts).copied();
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, inv)) });
                            }
                        });
                        egui::ScrollArea::vertical().id_salt("part-list").max_height(160.0).min_scrolled_height(0.0).show(ui, |ui| {
                            for m in &mesh_parts {
                                let mut on = p.parts.contains(&m.id);
                                if ui.checkbox(&mut on, format!("{}  ·  {} triangles", m.name, m.triangles)).changed() {
                                    let mut next = p.clone();
                                    if on {
                                        next.parts.insert(m.id);
                                    } else {
                                        next.parts.remove(&m.id);
                                    }
                                    next.enabled = true;
                                    part_cmds.push(Command::SetParts { layer: id, parts: Some(next) });
                                }
                            }
                        });
                    }
                }
                if mesh_parts.len() == 1 && part_kind == subcraft_core::PartKind::Connected {
                    hint(ui, "This texture set has one connected piece. Use UV islands in Parts for smaller regions.");
                }
            },
        );
        cmds.extend(part_head);
        cmds.extend(part_cmds);

        match &layer.content {
            LayerContent::Paint(p) if p.path.is_some() => {
                let path = state.pen.preview(&state.session).cloned().unwrap_or_else(|| p.path.as_ref().unwrap().clone());
                section(ui, "pen", "Editable pen path", true, |ui| crate::pen::properties(state, ui, &path, now));
            }
            LayerContent::Paint(p) => {
                section(ui, "paint", "Paint layer", true, |ui| {
                    if p.channels.is_empty() {
                        hint(ui, "Empty. Paint in the viewport with the Brush tool.");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for k in p.channels.keys() {
                                crate::widgets::badge(ui, k.display_name(), TEXT_DIM);
                            }
                        });
                    }
                });
            }
            LayerContent::Group(g) => {
                section(ui, "folder", "Folder", true, |ui| {
                    ui.label(RichText::new(format!("{} layers inside", g.children.len())).color(TEXT_DIM));
                });
            }
            LayerContent::Fill(f) => {
                section(ui, "projection", "Projection", true, |ui| {
                    let mut proj = f.projection;
                    let changed = projection_editor(ui, &mut proj);
                    if changed {
                        cmds.push(Command::SetFillProjection { layer: id, projection: proj });
                    }
                });

                section(ui, "channels", "Channels", true, |ui| {
                let states: Vec<_> = channels.iter().map(|k| (*k, f.channels.get(k).is_some_and(|c| c.enabled))).collect();
                if let Some((channel, solo)) = channel_toggles(ui, &states) {
                    cmds.push(Command::ToggleFillChannel { layer: id, effect: None, channel, solo });
                }
                hint(ui, "Tip: drag a texture from Shelf › Textures onto a channel below.");
                for kind in &channels {
                    let mut ch = f.channels.get(kind).cloned().unwrap_or(FillChannel::disabled(FillSource::uniform(kind.default_paint_value())));
                    if !ch.enabled { continue; }
                    let mut changed = false;
                    let mut pick_image = false;
                    let row = egui::Frame::new().fill(BG_2).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(kind.display_name()).color(theme::TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let current = match ch.source {
                                FillSource::Uniform { .. } => 0,
                                FillSource::Procedural { .. } => 1,
                                FillSource::MeshMap { .. } => 2,
                                FillSource::Bitmap { .. } => 3,
                            };
                            let mut sel = current;
                            egui::ComboBox::from_id_salt(("src", *kind))
                                .width(110.0)
                                .selected_text(["Uniform", "Procedural", "Mesh map", "Image"][current])
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut sel, 0, "Uniform");
                                    ui.selectable_value(&mut sel, 1, "Procedural");
                                    ui.selectable_value(&mut sel, 2, "Mesh map");
                                    ui.selectable_value(&mut sel, 3, "Image");
                                });
                            if sel != current {
                                match sel {
                                    0 => ch.source = FillSource::uniform(kind.default_paint_value()),
                                    1 => {
                                        ch.source = FillSource::Procedural {
                                            params: ProceduralParams::new(ProceduralKind::Clouds),
                                            low: ChannelValue::scalar(0.0),
                                            high: ChannelValue::scalar(1.0),
                                        }
                                    }
                                    2 => ch.source = FillSource::MeshMap { map: MeshMapKind::AmbientOcclusion },
                                    // Picked after the layout closure (needs the session).
                                    _ => pick_image = true,
                                }
                                changed |= sel != 3;
                            }
                        });
                    });
                    let (c, p) = source_controls(ui, state, *kind, &mut ch);
                    changed |= c;
                    pick_image |= p;
                    if pick_image {
                        let purpose = crate::files::Purpose::FillChannel { layer: id, channel: *kind };
                        crate::files::pick(ui.ctx(), purpose, crate::files::IMAGES, false, None);
                    }
                    });
                    // Drag a texture from Shelf → Textures onto this row to use it here.
                    let is_texture = |d: &crate::panels::shelf::ShelfDrag| matches!(d, crate::panels::shelf::ShelfDrag::Texture(_)|crate::panels::shelf::ShelfDrag::Starter(_));
                    if row.response.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().is_some_and(|d| is_texture(&d)) {
                        ui.painter().rect_stroke(row.response.rect, 0.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                    }
                    if let Some(d) = row.response.dnd_release_payload::<crate::panels::shelf::ShelfDrag>() {
                        match *d {
                            crate::panels::shelf::ShelfDrag::Texture(i)=>if let Some(path)=state.textures.items.get(i).map(|t|t.path.clone()) {texture_drop=Some((*kind,path));},
                            crate::panels::shelf::ShelfDrag::Starter(i)=>starter_drop=Some((i,*kind)),
                            _=>{},
                        }
                    }
                    ui.add_space(2.0);
                    if changed {
                        cmds.push(Command::SetFillChannel { layer: id, channel: *kind, fill: ch });
                    }
                }
                });
            }
        }


        let mut head_cmds = Vec::new();
        section_with(
            ui,
            "mask",
            "Mask",
            true,
            |ui| {
                if let Some(mask) = &layer.mask {
                    if icon_button(ui, icons::TRASH, "Remove mask").clicked() {
                        head_cmds.push(Command::RemoveMask(id));
                    }
                    let (icon, tip) = if mask.enabled { (icons::EYE, "Disable mask") } else { (icons::EYE_SLASH, "Enable mask") };
                    if icon_button(ui, icon, tip).clicked() {
                        head_cmds.push(Command::SetMaskEnabled { layer: id, enabled: !mask.enabled });
                    }
                }
            },
            |ui| match &layer.mask {
                None => {
                    hint(ui, "Masks hide parts of the layer. Start white (all visible) or black (all hidden).");
                    ui.horizontal(|ui| {
                        if secondary_button(ui, "Add white mask").clicked() {
                            cmds.push(Command::AddMask { layer: id, white: true });
                        }
                        if secondary_button(ui, "Add black mask").clicked() {
                            cmds.push(Command::AddMask { layer: id, white: false });
                        }
                    });
                }
                Some(mask) => {
                    hint(ui, "Paint the mask with the mask toggle in the tool strip, or stack generators below.");
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        let mut add = |ui: &mut Ui, label: &str, kind: MaskEffectKind| {
                            if ui.add(egui::Button::new(RichText::new(format!("{}  {label}", icons::PLUS)).font(theme::font(12.0)))).clicked() {
                                cmds.push(Command::AddMaskEffect { layer: id, kind });
                            }
                        };
                        add(ui, "Edges", MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)));
                        add(ui, "Cavity", MaskEffectKind::Generator(Generator::new(GeneratorKind::Cavity)));
                        add(ui, "Directional", MaskEffectKind::Generator(Generator::new(GeneratorKind::Directional)));
                        add(ui, "Gradient", MaskEffectKind::Generator(Generator::new(GeneratorKind::PositionGradient)));
                        add(
                            ui,
                            "Noise",
                            MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Grunge)), projection: Projection::default() },
                        );
                        add(ui, "Levels", MaskEffectKind::Levels(Levels::default()));
                        add(ui, "Invert", MaskEffectKind::Invert);
                        if ui.add(egui::Button::new(RichText::new(format!("{}  Image", icons::PLUS)).font(theme::font(12.0))))
                            .on_hover_text("A black & white image or a logo with transparency")
                            .clicked()
                        {
                            crate::files::pick(ui.ctx(), crate::files::Purpose::AddMaskImage { layer: id }, crate::files::IMAGES, false, None);
                        }
                    });
                    ui.add_space(6.0);
                    for e in mask.effects.iter().rev() {
                        ui.push_id(e.id, |ui| effect_editor(ui, state, id, e, &mut cmds));
                    }
                }
            },
        );
        cmds.extend(head_cmds);
    }));
    if picking != state.picking_parts {
        state.picking_parts = picking;
        if picking {
            state.set_status("Click parts in the 3D view to show or hide this layer there. Esc when done.", now);
        }
    }
    state.run(cmds, now);
    if let Some((i,channel))=starter_drop {state.asset_preview.queue(&state.session,i,Some((id,channel)));}
    if let Some((channel, path)) = texture_drop {
        match state.session.set_fill_channel_image(id, channel, &path) {
            Ok(()) => state.set_status(
                format!("{} now uses {}", channel.display_name(), path.file_name().and_then(|n| n.to_str()).unwrap_or("the texture")),
                now,
            ),
            Err(e) => state.set_status(format!("Texture: {e}"), now),
        }
    }
}

pub(crate) fn effect_editor(ui: &mut Ui, state: &mut UiState, layer: subcraft_core::LayerId, e: &Arc<MaskEffect>, cmds: &mut Vec<Command>) {
    let mut eff = (**e).clone();
    let default_name = eff.name == eff.kind.label();
    let mut changed = false;
    egui::Frame::new().fill(BG_2).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (icon, tip) = if eff.visible { (icons::EYE, "Hide effect") } else { (icons::EYE_SLASH, "Show effect") };
            if icon_button(ui, icon, tip).clicked() {
                eff.visible = !eff.visible;
                changed = true;
            }
            ui.label(RichText::new(&eff.name).font(theme::font_strong(12.5)).color(if eff.visible { theme::TEXT } else { TEXT_FAINT }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, icons::X, "Remove effect").clicked() {
                    cmds.push(Command::RemoveMaskEffect { layer, effect: eff.id });
                }
            });
        });
        if eff.visible {
            prop_row(ui, "Blend", |ui| {
                egui::ComboBox::from_id_salt(("effect_blend", eff.id)).selected_text(eff.blend.label()).show_ui(ui, |ui| {
                    for mode in subcraft_doc::BlendMode::available(false, false) {
                        changed |= ui.selectable_value(&mut eff.blend, mode, mode.label()).changed();
                    }
                });
            });
            changed |= slider_row(ui, "Opacity", egui::Slider::new(&mut eff.opacity, 0.0..=1.0)).changed();
            match &mut eff.kind {
                MaskEffectKind::Generator(g) => {
                    prop_row(ui, "Generator", |ui| egui::ComboBox::from_id_salt("generator_kind").selected_text(g.kind.label()).show_ui(ui, |ui| {
                        for kind in [GeneratorKind::Edges, GeneratorKind::Cavity, GeneratorKind::Directional, GeneratorKind::PositionGradient]
                            .into_iter().chain(MeshMapKind::ALL.into_iter().map(GeneratorKind::MeshMap)) {
                            changed |= ui.selectable_value(&mut g.kind, kind, kind.label()).changed();
                        }
                    }));
                    if g.kind == GeneratorKind::Edges {
                        let mut natural = g.edge_spread.is_some();
                        if ui.checkbox(&mut natural, "Natural edge wear").on_hover_text("Surface-constrained spread with an irregular boundary. Older projects retain their original wear until enabled.").changed() {
                            g.edge_spread = natural.then(subcraft_doc::EdgeSpread::default); changed = true;
                        }
                        if let Some(spread) = &mut g.edge_spread {
                            changed |= slider_row(ui, "Spread", egui::Slider::new(&mut spread.distance, 0.0..=12.5).suffix("%")).on_hover_text("Reach as a percentage of the model's longest dimension, independent of Level. Follows connected edges, not UV islands.").changed();
                            changed |= slider_row(ui, "Variation", egui::Slider::new(&mut spread.variation, 0.0..=1.0)).on_hover_text("Irregular edge boundary, independent of Breakup. Zero gives a clean geometric band.").changed();
                        }
                    }
                    changed |= slider_row(ui, "Level", egui::Slider::new(&mut g.level, 0.0..=1.0).max_decimals(3)).on_hover_text(
                        if g.kind == GeneratorKind::Edges && g.edge_spread.is_some() {
                            "Select baked convex detail. Spread controls geometric reach independently."
                        } else { "Threshold for the generator's source values." }).changed();
                    changed |= slider_row(ui, "Softness", egui::Slider::new(&mut g.softness, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Breakup", egui::Slider::new(&mut g.noise_amount, 0.0..=1.0)).changed();
                    let mut size = g.pattern_size();
                    if slider_row(ui, "Pattern size", egui::Slider::new(&mut size, 1.0..=100.0).logarithmic(true).suffix("%")).changed() {
                        g.set_pattern_size(size); changed = true;
                    }
                    prop_row(ui, "Seed", |ui| {
                        changed |= ui.add(egui::DragValue::new(&mut g.noise.seed).speed(1.)).changed();
                        if ui.small_button("Next").on_hover_text("Try the next deterministic pattern seed").clicked() {
                            g.noise.seed = g.noise.seed.wrapping_add(1); changed = true;
                        }
                    });
                    if g.kind != GeneratorKind::Edges || g.edge_spread.is_none() {
                        changed |= ui.checkbox(&mut g.organic_breakup, "Reduce repetition").changed();
                    }
                    prop_row(ui, "", |ui| changed |= ui.checkbox(&mut g.invert, "Invert").changed());
                    if g.kind == GeneratorKind::Directional {
                        prop_row(ui, "Direction", |ui| { for value in &mut g.direction {
                            changed |= ui.add(egui::DragValue::new(value).range(-1.0..=1.0).speed(0.01)).changed();
                        } });
                    }
                }
                MaskEffectKind::Fill { value, procedural, projection } => {
                    let mut use_procedural = procedural.is_some();
                    if ui.checkbox(&mut use_procedural, "Procedural").changed() {
                        *procedural = use_procedural.then(|| ProceduralParams::new(ProceduralKind::Clouds));
                        changed = true;
                    }
                    if let Some(p) = procedural {
                        prop_row(ui, "Pattern", |ui| egui::ComboBox::from_id_salt("mask_pattern").selected_text(p.kind.label()).show_ui(ui, |ui| {
                            for kind in ProceduralKind::ALL { changed |= ui.selectable_value(&mut p.kind, kind, kind.label()).changed(); }
                        }));
                        changed |= slider_row(ui, "Scale", egui::Slider::new(&mut p.scale, 1.0..=64.0)).changed();
                        changed |= slider_row(ui, "Balance", egui::Slider::new(&mut p.balance, 0.0..=1.0)).changed();
                        changed |= slider_row(ui, "Contrast", egui::Slider::new(&mut p.contrast, 0.0..=1.0)).changed();
                        prop_row(ui, "Seed", |ui| changed |= ui.add(egui::DragValue::new(&mut p.seed)).changed());
                        changed |= ui.checkbox(&mut p.invert, "Invert pattern").changed();
                    } else { changed |= slider_row(ui, "Value", egui::Slider::new(value, 0.0..=1.0)).changed(); }
                    changed |= projection_editor(ui, projection);
                }
                MaskEffectKind::Levels(l) => {
                    changed |= slider_row(ui, "In low", egui::Slider::new(&mut l.in_low, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "In high", egui::Slider::new(&mut l.in_high, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Gamma", egui::Slider::new(&mut l.gamma, 0.1..=4.0)).changed();
                    changed |= slider_row(ui, "Out low", egui::Slider::new(&mut l.out_low, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Out high", egui::Slider::new(&mut l.out_high, 0.0..=1.0)).changed();
                }
                MaskEffectKind::Image { resource, projection, invert } => {
                    if image_row(ui, state, *resource) {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::MaskImage { layer, effect: e.id }, crate::files::IMAGES, false, None);
                    }
                    prop_row(ui, "", |ui| changed |= ui.checkbox(invert, "Invert").changed());
                    changed |= projection_editor(ui, projection);
                    hint(ui, "Uses the image's transparency if it has any, otherwise its brightness.");
                }
                _ => {}
            }
        }
    });
    ui.add_space(4.0);
    if changed {
        if default_name { eff.name = eff.kind.label(); }
        cmds.push(Command::SetMaskEffect { layer, effect: eff });
    }
}

/// Thumbnail, name and size of an image resource, with a Replace button. True if clicked.
/// Value controls of an enabled fill channel (source parameters, then strength). Returns
/// (changed, image picker requested). Shared by fill layers and Fill content effects.
pub(crate) fn source_controls(ui: &mut Ui, state: &mut UiState, kind: ChannelKind, ch: &mut FillChannel) -> (bool, bool) {
    let mut changed = false;
    let mut pick_image = false;
    let kind = &kind;
    if ch.enabled {
        ui.add_space(2.0);
        match &mut ch.source {
            FillSource::Uniform { value } => {
                prop_row(ui, "Value", |ui| changed |= channel_value(ui, *kind, value).changed);
            }
            FillSource::Procedural { params, low, high } => {
                prop_row(ui, "Pattern", |ui| {
                    egui::ComboBox::from_id_salt(("kind", *kind)).selected_text(params.kind.label()).show_ui(ui, |ui| {
                        for k in ProceduralKind::ALL {
                            changed |= ui.selectable_value(&mut params.kind, k, k.label()).changed();
                        }
                    });
                });
                changed |= slider_row(ui, "Scale", egui::Slider::new(&mut params.scale, 1.0..=64.0).step_by(1.0)).changed();
                changed |= slider_row(ui, "Balance", egui::Slider::new(&mut params.balance, 0.0..=1.0)).changed();
                changed |= slider_row(ui, "Contrast", egui::Slider::new(&mut params.contrast, 0.0..=1.0)).changed();
                prop_row(ui, "Seed", |ui| changed |= ui.add(egui::DragValue::new(&mut params.seed)).changed());
                prop_row(ui, "Low", |ui| changed |= channel_value(ui, *kind, low).changed);
                prop_row(ui, "High", |ui| changed |= channel_value(ui, *kind, high).changed);
                prop_row(ui, "", |ui| changed |= ui.checkbox(&mut params.invert, "Invert").changed());
            }
            FillSource::MeshMap { map } => {
                prop_row(ui, "Map", |ui| {
                    egui::ComboBox::from_id_salt(("map", *kind)).selected_text(map.display_name()).show_ui(ui, |ui| {
                        for m in MeshMapKind::ALL {
                            changed |= ui.selectable_value(map, m, m.display_name()).changed();
                        }
                    });
                });
                hint(ui, "Uses the baked map (Ctrl+B); neutral until baked.");
            }
            FillSource::Bitmap { resource } => {
                pick_image |= image_row(ui, state, *resource);
                hint(ui, if kind.is_color() {
                    "Color image: read as sRGB. Tiling and rotation are under Projection."
                } else {
                    "Data image: used as-is (no color conversion). Tiling and rotation are under Projection."
                });
            }
        }
    }
    if ch.enabled {
        // How strongly this channel's result applies (bump, normal tilt, value).
        let (label, range, tip) = match kind {
            ChannelKind::Height => ("Strength", 0.0..=4.0, "Bump strength (0 = flat)"),
            ChannelKind::Normal => ("Strength", 0.0..=4.0, "Normal map strength (0 = flat)"),
            ChannelKind::BaseColor => ("Brightness", 0.0..=2.0, "Multiplies the color"),
            ChannelKind::Emissive => ("Intensity", 0.0..=10.0, "Glow intensity"),
            _ => ("Strength", 0.0..=2.0, "Multiplies the value"),
        };
        let r = slider_row(ui, label, egui::Slider::new(&mut ch.strength, range).max_decimals(2));
        changed |= r.on_hover_text(tip).changed();
    }
    (changed, pick_image)
}

/// UI events only: the app owns independent/solo state changes and their undo transaction.
pub(crate) fn channel_toggles(ui: &mut Ui, channels: &[(ChannelKind, bool)]) -> Option<(ChannelKind, bool)> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3., 4.);
        for &(kind, on) in channels {
            let response = ui.selectable_label(on, kind.short_name()).on_hover_text(format!("{} · Click: toggle · Alt+click: only this channel", kind.display_name()));
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, kind.display_name()));
            if response.clicked() { clicked = Some((kind, ui.input(|i| i.modifiers.alt))); }
        }
    });
    clicked
}

pub(crate) fn image_row(ui: &mut Ui, state: &mut UiState, resource: subcraft_core::ResourceId) -> bool {
    let Some(r) = state.session.resources().get(resource).cloned() else {
        ui.label(RichText::new(format!("{}  Image missing from the project", icons::WARNING)).color(theme::WARNING));
        return secondary_button(ui, "Choose image…").clicked();
    };
    let tex = state
        .image_thumbs
        .entry(resource)
        .or_insert_with(|| {
            let (w, h, rgba) = r.bitmap.preview_rgba8(96);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            ui.ctx().load_texture(format!("image-{}", resource.0), img, egui::TextureOptions::LINEAR)
        })
        .clone();
    let mut clicked = false;
    ui.horizontal(|ui| {
        let size = tex.size_vec2();
        let scale = 48.0 / size.x.max(size.y);
        ui.image((tex.id(), size * scale));
        ui.vertical(|ui| {
            ui.label(RichText::new(&r.name).color(theme::TEXT));
            ui.label(RichText::new(format!("{} × {}", r.bitmap.width, r.bitmap.height)).color(TEXT_DIM).font(theme::font(11.0)));
            clicked = secondary_button(ui, "Replace…").clicked();
        });
    });
    clicked
}

/// Mode / scale / rotation (+ sharpness or axis) editor shared by fill layers and image masks.
pub(crate) fn projection_editor(ui: &mut Ui, proj: &mut Projection) -> bool {
    let mut changed = false;
    prop_row(ui, "Mode", |ui| {
        changed |= segmented(ui, &mut proj.mode, &[(ProjectionMode::Uv, "UV"), (ProjectionMode::Triplanar, "Tri-planar"), (ProjectionMode::Planar, "Planar")]);
    });
    // Tiling: X and Y linked by default (lock), separate when unlocked. The lock is UI state,
    // remembered per editor; it starts unlocked if the two values already differ.
    let lock_id = ui.id().with("tiling_lock");
    let mut locked = ui.data(|d| d.get_temp::<bool>(lock_id)).unwrap_or(proj.scale[0] == proj.scale[1]);
    prop_row(ui, "Tiling", |ui| {
        fn tiling(v: &mut f32) -> egui::DragValue<'_> {
            egui::DragValue::new(v).speed(0.05).range(0.01..=256.0).max_decimals(2)
        }
        if locked {
            if ui.add(tiling(&mut proj.scale[0])).on_hover_text("Repeats across the surface (X and Y together)").changed() {
                proj.scale[1] = proj.scale[0];
                changed = true;
            }
        } else {
            ui.label(RichText::new("X").color(TEXT_DIM));
            changed |= ui.add(tiling(&mut proj.scale[0])).changed();
            ui.label(RichText::new("Y").color(TEXT_DIM));
            changed |= ui.add(tiling(&mut proj.scale[1])).changed();
        }
        if ui.checkbox(&mut locked, "Link").on_hover_text("Same tiling for X and Y; untick to set them separately").changed() {
            if locked && proj.scale[1] != proj.scale[0] {
                proj.scale[1] = proj.scale[0];
                changed = true;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(lock_id, locked));
    changed |= slider_row(ui, "Rotation", egui::Slider::new(&mut proj.rotation_deg, -180.0..=180.0).suffix("°")).changed();
    prop_row(ui, "Offset", |ui| {
        fn offset(v: &mut f32) -> egui::DragValue<'_> {
            egui::DragValue::new(v).speed(0.005).range(-16.0..=16.0).max_decimals(3)
        }
        ui.label(RichText::new("X").color(TEXT_DIM));
        changed |= ui.add(offset(&mut proj.offset[0])).changed();
        ui.label(RichText::new("Y").color(TEXT_DIM));
        changed |= ui.add(offset(&mut proj.offset[1])).changed();
    });
    if proj.mode == ProjectionMode::Triplanar {
        changed |= slider_row(ui, "Sharpness", egui::Slider::new(&mut proj.triplanar_sharpness, 1.0..=32.0)).changed();
    }
    if proj.mode == ProjectionMode::Planar {
        prop_row(ui, "Axis", |ui| {
            changed |= segmented(ui, &mut proj.planar_axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]);
        });
    }
    let default = Projection { mode: proj.mode, planar_axis: proj.planar_axis, ..Projection::default() };
    if *proj != default && ui.small_button("Reset tiling, offset and rotation").clicked() {
        *proj = default;
        changed = true;
    }
    changed
}

#[cfg(test)]
mod generator_tests {
    use super::*;
    #[test]
    fn channel_buttons_solo_hide_rows_and_rotation_sits_directly_below_tiling() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.session.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
        let layer = state.session.selection().layer.unwrap();
        state.session.execute(Command::ToggleFillChannel { layer, effect: None, channel: ChannelKind::BaseColor, solo: true }).unwrap();
        let draw = |state: &mut UiState, events, modifiers| {
            let mut events: Vec<egui::Event> = events; events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(430., 1100.))), events, ..Default::default() }, |ui| show(state, ui));
            out.textures_delta.clear(); out
        };
        draw(&mut state, vec![], Default::default());
        let out = draw(&mut state, vec![], Default::default());
        let pos = |out: &egui::FullOutput, text: &str| out.shapes.iter().find_map(|s| match &s.shape { egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size()*0.5), _ => None });
        assert!(pos(&out, "Base Color").is_some() && pos(&out, "Height").is_none());
        assert!(pos(&out, "Tiling").unwrap().y < pos(&out, "Rotation").unwrap().y);
        assert!(pos(&out, "Rotation").unwrap().y < pos(&out, "Offset").unwrap().y);
        let p = pos(&out, "height").unwrap(); let modifiers = egui::Modifiers { alt: true, ..Default::default() };
        let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers };
        draw(&mut state, vec![egui::Event::PointerMoved(p), button(true)], modifiers);
        draw(&mut state, vec![button(false)], modifiers);
        let out = draw(&mut state, vec![], Default::default());
        assert!(pos(&out, "Height").is_some() && pos(&out, "Base Color").is_none());
    }
    #[test]
    fn generator_next_seed_dispatches_a_undoable_edit_and_shows_pattern_size() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        let layer = state.session.selection().layer.unwrap();
        state.session.execute(Command::AddMask { layer, white: false }).unwrap();
        state.session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).unwrap();
        let draw = |state: &mut UiState, events| {
            let mut commands = Vec::new(); let effect = state.session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].clone();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420., 850.))), events, ..Default::default() }, |ui| effect_editor(ui, state, layer, &effect, &mut commands));
            out.textures_delta.clear(); (out, commands)
        };
        let (out, _) = draw(&mut state, vec![]);
        assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Pattern size")));
        for label in ["Natural edge wear", "Spread", "Variation"] {
            assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == label)));
        }
        let pos = out.shapes.iter().find_map(|s| match &s.shape { egui::Shape::Text(t) if t.galley.text() == "Next" => Some(t.pos + t.galley.size()*0.5), _ => None }).unwrap();
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        draw(&mut state, vec![egui::Event::PointerMoved(pos), button(true)]);
        let (_, commands) = draw(&mut state, vec![button(false)]);
        assert_eq!(commands.len(), 1);
        for command in commands { state.session.execute(command).unwrap(); }
        let seed = |state: &UiState| match state.session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].kind { MaskEffectKind::Generator(g) => g.noise.seed, _ => panic!() };
        assert_eq!(seed(&state), 2); state.session.execute(Command::Undo).unwrap(); assert_eq!(seed(&state), 1);
    }
}

