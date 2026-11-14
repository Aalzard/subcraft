//! Properties of the selected layer: name, fill channels + projection, mask and its effects.

use crate::i18n::tr;
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
use crate::theme::{self, ACCENT, TEXT_DIM, TEXT_FAINT, VALUE_W};
use crate::widgets::{channel_value, empty_state, fill_width, hint, icon_button, panel, primary_button, prop_row, secondary_button, section, section_with, segmented, slider_row};

/// Properties is context-sensitive like Substance Painter's: while a paint layer (or a paint
/// effect) is the target, or the brush settings were asked for, it shows the brush settings; any
/// other layer shows its own properties.
pub fn shows_brush(state: &UiState) -> bool {
    if !state.properties_hosts_brush {
        return false;
    }
    let session = &state.session;
    let paints_effect = matches!(session.brush.target, subcraft_paint::PaintTarget::ContentEffect(_));
    let paint_content = session.selected_layer().is_some_and(|l| l.kind() == subcraft_doc::LayerKind::Paint)
        && session.layer_target() == subcraft_app::LayerTarget::Content
        && session.selected_effect().is_none();
    state.brush_pinned || paint_content || paints_effect
}

/// The docked Properties tab. While the right-click popup over a view shows the same content, it
/// shows a note instead: drawing the widgets twice in one frame gave them the same ids, so both
/// copies took a click (QA 2026-11: a pen-pressure toggle clicked in the popup was flipped back
/// by the panel, and nothing changed).
pub fn show_docked(state: &mut UiState, ui: &mut Ui) {
    if state.view_props.is_some() {
        ui.add_space(8.0);
        ui.label(RichText::new(tr("Shown in the popup over the view (Esc closes it)")).color(TEXT_DIM()));
        return;
    }
    show(state, ui);
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    if shows_brush(state) {
        super::brush::show(state, ui);
        return;
    }
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
    // The part whose row is under the cursor in the list: the 2D view highlights it.
    let mut hovered_part: Option<u32> = None;

    egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| panel(ui, |ui| {
        // Painter's instance list: the source and every instance, each a jump to that layer.
        cmds.extend(super::layers::instance_section(&state.session, ui, id));
        // Name, with the layer type as an icon.
        ui.horizontal(|ui| {
            let icon = match &layer.content {
                LayerContent::Paint(p) if p.path.is_some() => icons::CURVE,
                LayerContent::Paint(_) => icons::PAINT_BRUSH,
                LayerContent::Fill(_) => icons::PAINT_BUCKET,
                LayerContent::Group(_) => icons::FOLDER_OPEN,
            };
            ui.label(RichText::new(icon).font(theme::font(18.0)).color(ACCENT()));
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
            } else if !r.has_focus() && *buf != layer.name {
                // The name changed under the field (Undo, the Layers panel, PhotoCraft): follow
                // it, or the next focus change would push the old name back as a new rename.
                *buf = layer.name.clone();
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
                    None => hint(ui, tr("Show this layer only on some pieces of the model (a blade, a handle…): pick them, then paint or fill as usual.")),
                    Some(p) => {
                        let shown = mesh_parts.iter().filter(|m| p.parts.contains(&m.id)).count();
                        let state_text = if p.enabled { format!("Visible on {shown} of {} parts.", mesh_parts.len()) } else { "Off: the layer shows everywhere.".into() };
                        hint(ui, &state_text);
                        ui.horizontal(|ui| {
                            let all: std::collections::BTreeSet<u32> = mesh_parts.iter().map(|m| m.id).collect();
                            if ui.small_button(tr("All")).clicked() {
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, all.clone())) });
                            }
                            if ui.small_button(tr("None")).clicked() {
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, [])) });
                            }
                            if ui.small_button(tr("Invert")).clicked() {
                                let inv = all.difference(&p.parts).copied();
                                part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, inv)) });
                            }
                        });
                        egui::ScrollArea::vertical().id_salt("part-list").max_height(160.0).min_scrolled_height(0.0).show(ui, |ui| {
                            // Press a box and drag over others to give them the same state (Painter);
                            // Alt+click keeps only that row.
                            let drag_id = egui::Id::new("part-list-drag");
                            let (down, released, alt) = ui.input(|i| (i.pointer.primary_down(), i.pointer.primary_released(), i.modifiers.alt));
                            let mut drag: Option<bool> = ui.data(|d| d.get_temp(drag_id));
                            if !down && !released {
                                drag = None;
                            }
                            for m in &mesh_parts {
                                let was = p.parts.contains(&m.id);
                                let mut on = was;
                                let r = ui.checkbox(&mut on, format!("{}  ·  {} triangles", m.name, m.triangles))
                                    .on_hover_text("Drag over several boxes to set them together · Alt+click: only this one");
                                if r.contains_pointer() {
                                    hovered_part = Some(m.id);
                                }
                                if alt {
                                    if r.clicked() {
                                        part_cmds.push(Command::SetParts { layer: id, parts: Some(PartMask::for_kind(part_kind, [m.id])) });
                                    }
                                    continue;
                                }
                                if drag.is_none() && down && r.is_pointer_button_down_on() {
                                    drag = Some(!was);
                                }
                                // While a drag runs it sets the rows; the click that ends it must not toggle back.
                                let want = match drag {
                                    Some(target) if r.contains_pointer() => target,
                                    Some(_) => was,
                                    None => on,
                                };
                                if want != was {
                                    let mut next = p.clone();
                                    if want {
                                        next.parts.insert(m.id);
                                    } else {
                                        next.parts.remove(&m.id);
                                    }
                                    next.enabled = true;
                                    part_cmds.push(Command::SetParts { layer: id, parts: Some(next) });
                                }
                            }
                            match drag {
                                Some(target) => ui.data_mut(|d| { d.insert_temp(drag_id, target); }),
                                None => ui.data_mut(|d| { d.remove::<bool>(drag_id); }),
                            }
                        });
                    }
                }
                if mesh_parts.len() == 1 && part_kind == subcraft_core::PartKind::Connected {
                    hint(ui, tr("This texture set has one connected piece. Use UV islands in Parts for smaller regions."));
                }
            },
        );
        cmds.extend(part_head);
        cmds.extend(part_cmds);

        match &layer.content {
            LayerContent::Paint(p) if p.path.is_some() => {
                let path = state.pen.preview(&state.session).cloned().unwrap_or_else(|| p.path.as_ref().unwrap().clone());
                section(ui, "pen", tr("Editable pen path"), true, |ui| crate::pen::properties(state, ui, &path, now));
            }
            LayerContent::Paint(p) => {
                section(ui, "paint", tr("Paint layer"), true, |ui| {
                    if p.channels.is_empty() {
                        hint(ui, tr("Empty. Paint in the viewport with the Brush tool."));
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for k in p.channels.keys() {
                                crate::widgets::badge(ui, k.display_name(), TEXT_DIM());
                            }
                        });
                    }
                    if !p.channels.is_empty() && p.text.is_none() {
                        let size = state.session.scene().mesh.bounds.extent().max_element().max(1e-6);
                        let key = ui.id().with("world_blur_radius");
                        let mut radius = ui.data_mut(|d| *d.get_temp_mut_or(key, size * 0.05));
                        prop_row(ui, "World blur", |ui| {
                            ui.add(egui::DragValue::new(&mut radius).range(size * 0.001..=size * 0.5).speed(size * 0.002).max_decimals(3))
                                .on_hover_text("Blur radius in model units over the surface, so it is even where the UVs stretch. Does not cross hard edges.");
                            if ui.button("Blur").on_hover_text("Blur this layer's painted channels in world space (one undo step; Normal paint is left alone)").clicked() {
                                match state.session.world_blur_layer(id, radius) {
                                    Ok(()) => state.set_status("World-space blur applied", now),
                                    Err(e) => state.set_status(format!("World-space blur: {e}"), now),
                                }
                            }
                        });
                        ui.data_mut(|d| d.insert_temp(key, radius));
                    }
                });
            }
            LayerContent::Group(g) => {
                section(ui, "folder", tr("Folder"), true, |ui| {
                    ui.label(RichText::new(format!("{} layers inside", g.children.len())).color(TEXT_DIM()));
                });
            }
            LayerContent::Fill(f) => {
                block_header(ui, tr("Fill"));
                {
                    let mut proj = f.projection;
                    if fill_properties(ui, &mut proj) {
                        cmds.push(Command::SetFillProjection { layer: id, projection: proj });
                    }
                    if proj.mode == ProjectionMode::Warp {
                        crate::warp_view::settings_ui(ui, &mut state.warp);
                    }
                    if proj.has_transform3() {
                        crate::projection_gizmo::settings_ui(ui, &mut state.gizmo);
                    }
                }
                block_header(ui, tr("Material"));
                {
                let states: Vec<_> = channels.iter().map(|k| (*k, f.channels.get(k).is_some_and(|c| c.enabled))).collect();
                let addable: Vec<ChannelKind> = ADDABLE_CHANNELS.into_iter().filter(|k| !channels.contains(k)).collect();
                match material_buttons(ui, &states, &addable) {
                    Some(ChannelAction::Toggle(channel, solo)) => cmds.push(Command::ToggleFillChannel { layer: id, effect: None, channel, solo }),
                    Some(ChannelAction::Add(channel)) => cmds.push(Command::AddFillChannel { layer: id, channel }),
                    None => {}
                }
                hint(ui, tr("Tip: drag a texture from Shelf › Textures onto a channel below."));
                let anchor_index = subcraft_doc::AnchorIndex::build(&state.session.selected_texture_set().layers);
                for kind in &channels {
                    let mut ch = f.channels.get(kind).cloned().unwrap_or(FillChannel::disabled(FillSource::uniform(kind.default_paint_value())));
                    if !ch.enabled { continue; }
                    let mut changed = false;
                    let mut pick_image = false;
                    let row = crate::widgets::card(ui, theme::channel_accent(*kind), |ui| {
                    ui.horizontal(|ui| {
                        // The channel's image slot: its texture, or an empty "+" box to click or drop on.
                        let bitmap = match ch.source { FillSource::Bitmap { resource } => Some(resource), _ => None };
                        if map_slot(ui, state, bitmap, *kind).clicked() { pick_image = true; }
                        ui.label(RichText::new(kind.display_name()).font(theme::font_strong(13.0)).color(theme::TEXT()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let current = match ch.source {
                                FillSource::Uniform { .. } => 0,
                                FillSource::Procedural { .. } => 1,
                                FillSource::MeshMap { .. } => 2,
                                FillSource::Bitmap { .. } => 3,
                                FillSource::Anchor { .. } => 4,
                            };
                            let mut sel = current;
                            egui::ComboBox::from_id_salt(("src", *kind))
                                .width(110.0)
                                .selected_text(["Uniform", "Procedural", "Mesh map", "Image", "Anchor"][current])
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut sel, 0, tr("Uniform"));
                                    ui.selectable_value(&mut sel, 1, tr("Procedural"));
                                    ui.selectable_value(&mut sel, 2, tr("Mesh map"));
                                    ui.selectable_value(&mut sel, 3, tr("Image"));
                                    ui.selectable_value(&mut sel, 4, tr("Anchor")).on_hover_text(tr("A channel or mask captured by an anchor point in a lower layer"));
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
                                    4 => {
                                        let candidates = anchor_index.candidates(subcraft_doc::RefSite::Fill { layer: id, channel: *kind });
                                        let input = super::anchor::default_input(&candidates, *kind)
                                            .unwrap_or_else(|| subcraft_doc::AnchorInput::new(subcraft_core::EffectId::nil(), *kind));
                                        ch.source = FillSource::Anchor { input };
                                    }
                                    // Picked after the layout closure (needs the session).
                                    _ => pick_image = true,
                                }
                                changed |= sel != 3;
                            }
                        });
                    });
                    crate::widgets::card_header_rule(ui);
                    if let FillSource::Anchor { input } = &mut ch.source {
                        let mut pick = Some(*input);
                        if super::anchor::picker(ui, &anchor_index, subcraft_doc::RefSite::Fill { layer: id, channel: *kind }, &mut pick, None, &channels, *kind)
                            && let Some(p) = pick
                        {
                            *input = p;
                            changed = true;
                        }
                        hint(ui, tr("Reads the anchor's value and coverage. Color and gray convert automatically."));
                    }
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
                        ui.painter().rect_stroke(row.response.rect, 0.0, egui::Stroke::new(1.5, ACCENT()), egui::StrokeKind::Inside);
                    }
                    // A file dragged in from the system file browser onto this channel's row.
                    let over_row = ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| row.response.rect.contains(p));
                    if over_row && ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
                        ui.painter().rect_stroke(row.response.rect, 0.0, egui::Stroke::new(1.5, ACCENT()), egui::StrokeKind::Inside);
                    }
                    if over_row && let Some(path) = ui.ctx().input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())).filter(|p| !p.as_os_str().is_empty()) {
                        state.drops_taken = true;
                        texture_drop = Some((*kind, path));
                    }
                    if let Some(d) = row.response.dnd_release_payload::<crate::panels::shelf::ShelfDrag>() {
                        match *d {
                            crate::panels::shelf::ShelfDrag::Texture(i)=>if let Some(path)=state.textures.items.get(i).map(|t|t.path.clone()) {texture_drop=Some((*kind,path));},
                            crate::panels::shelf::ShelfDrag::Starter(i)=>starter_drop=Some((i,*kind)),
                            _=>{},
                        }
                    }
                    if changed {
                        cmds.push(Command::SetFillChannel { layer: id, channel: *kind, fill: ch });
                    }
                }
                }
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
                    hint(ui, tr("Masks hide parts of the layer. Start white (all visible) or black (all hidden)."));
                    ui.horizontal(|ui| {
                        if secondary_button(ui, tr("Add white mask")).clicked() {
                            cmds.push(Command::AddMask { layer: id, white: true });
                        }
                        if secondary_button(ui, tr("Add black mask")).clicked() {
                            cmds.push(Command::AddMask { layer: id, white: false });
                        }
                    });
                }
                Some(mask) => {
                    hint(ui, tr("Paint the mask with the mask toggle in the tool strip, or stack generators below."));
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        let mut add = |ui: &mut Ui, label: &str, kind: MaskEffectKind| {
                            if ui.add(egui::Button::new(RichText::new(format!("{}  {label}", icons::PLUS)).font(theme::font(12.0)))).clicked() {
                                cmds.push(Command::AddMaskEffect { layer: id, kind });
                            }
                        };
                        add(ui, "Edges", MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)));
                        add(ui, "Dirt", MaskEffectKind::Generator(Generator::new(GeneratorKind::Cavity)));
                        add(ui, "Ambient Occlusion", MaskEffectKind::Generator(Generator::new(GeneratorKind::AmbientOcclusion)));
                        add(ui, "Directional", MaskEffectKind::Generator(Generator::new(GeneratorKind::Directional)));
                        add(ui, "Gradient", MaskEffectKind::Generator(Generator::new(GeneratorKind::PositionGradient)));
                        add(
                            ui,
                            "Noise",
                            MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Grunge)), projection: Projection::default() },
                        );
                        add(ui, "Levels", MaskEffectKind::Levels(Levels::default()));
                        add(ui, "Invert", MaskEffectKind::Invert);
                        add(ui, "Compare Mask", MaskEffectKind::Compare(Default::default()));
                        add(ui, "ID Color", MaskEffectKind::IdColor(Default::default()));
                        super::effects::add_mask_filters(ui, id, &mut cmds);
                        ui.menu_button(tr("Anchor"), |ui| {
                            let index = subcraft_doc::AnchorIndex::build(&state.session.selected_texture_set().layers);
                            super::effects::add_mask_anchor_items(ui, id, &index, &mut cmds);
                        });
                        if ui.add(egui::Button::new(RichText::new(format!("{}  Image", icons::PLUS)).font(theme::font(12.0))))
                            .on_hover_text(tr("A black & white image or a logo with transparency"))
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
    state.uv_parts.list_hover = hovered_part;
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

/// The image slot of a filter that reads one (Gradient Dynamic's Gradient Source). Returns
/// whether the image button was clicked; clearing it marks `changed`.
pub(crate) fn filter_image_row(ui: &mut Ui, state: &mut UiState, filter: &mut subcraft_doc::Filter, changed: &mut bool) -> bool {
    let subcraft_doc::Filter::GradientDynamic { source, .. } = filter else { return false };
    let mut pick = false;
    match filter_resource(*source) {
        Some(id) => {
            if image_row(ui, state, id) {
                pick = true;
            }
            if ui.small_button(tr("Remove image")).clicked() {
                *source = subcraft_core::ResourceId::nil();
                *changed = true;
            }
        }
        None => {
            if secondary_button(ui, tr("Choose gradient image…")).clicked() {
                pick = true;
            }
        }
    }
    pick
}

fn filter_resource(id: subcraft_core::ResourceId) -> Option<subcraft_core::ResourceId> {
    (id != subcraft_core::ResourceId::nil()).then_some(id)
}

/// Painter's Use Custom Grunge for the generators with a breakup: an image slot replacing the
/// noise. Returns whether the image button was clicked.
fn custom_grunge_row(ui: &mut Ui, state: &mut UiState, g: &mut subcraft_doc::Generator, changed: &mut bool) -> bool {
    if !matches!(g.kind, GeneratorKind::Edges | GeneratorKind::Cavity | GeneratorKind::Concave) {
        return false;
    }
    let mut pick = false;
    prop_row(ui, tr("Custom grunge"), |ui| {
        match g.painter.and_then(|p| p.custom_grunge) {
            Some(id) => {
                if image_row(ui, state, id) {
                    pick = true;
                }
                if ui.small_button(tr("Remove image")).clicked() {
                    if let Some(p) = g.painter.as_mut() {
                        p.custom_grunge = None;
                    }
                    *changed = true;
                }
            }
            None => {
                if secondary_button(ui, tr("Choose image…")).on_hover_text(tr("An image replaces the breakup noise, projected over the model at the pattern size")).clicked() {
                    pick = true;
                }
            }
        }
    });
    pick
}

/// A new seed for Painter's Random button: different on every click.
pub(crate) fn random_seed() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    (h.finish() % 100_000) as u32
}

/// Controls of a generator in a layer's content stack: kind, level and contrast, the breakup
/// seed and Invert.
pub(crate) fn content_generator_controls(ui: &mut Ui, state: &mut UiState, g: &mut subcraft_doc::Generator) -> bool {
    let mut changed = generator_kind_row(ui, g);
    if g.kind.uses_shape() {
        // The position, UV and conversion kinds have their own settings (an image for Grayscale
        // Conversion is picked on a mask generator; here it shows the one it has).
        return changed | shape_generator_controls(ui, state, g).0;
    }
    changed |= level_controls(ui, g);
    changed |= painter_controls(ui, g);
    prop_row(ui, tr("Seed"), |ui| {
        changed |= ui.add(egui::DragValue::new(&mut g.noise.seed).speed(1.)).changed();
        if ui.small_button(tr("Random")).on_hover_text(tr("Pick a new random seed")).clicked() {
            g.noise.seed = random_seed();
            changed = true;
        }
    });
    prop_row(ui, "", |ui| changed |= ui.checkbox(&mut g.invert, tr("Invert")).changed());
    changed
}

/// The Generator dropdown of a generator mask effect.
fn generator_kind_row(ui: &mut Ui, g: &mut subcraft_doc::Generator) -> bool {
    let mut picked = None;
    prop_row(ui, tr("Generator"), |ui| egui::ComboBox::from_id_salt("generator_kind").selected_text(g.kind.label()).show_ui(ui, |ui| {
        for kind in [GeneratorKind::Edges, GeneratorKind::Cavity, GeneratorKind::AmbientOcclusion, GeneratorKind::Concave, GeneratorKind::Directional, GeneratorKind::PositionGradient, GeneratorKind::MaskEditor,
            GeneratorKind::Distance3D, GeneratorKind::LinearGradient3D, GeneratorKind::UvRandomColor, GeneratorKind::GrayscaleConversion,
            GeneratorKind::UvBorderDistance, GeneratorKind::UvChecker, GeneratorKind::UvTexelDensity]
            .into_iter().chain(MeshMapKind::ALL.into_iter().map(GeneratorKind::MeshMap)) {
            if ui.selectable_label(g.kind == kind, kind.label()).clicked() && g.kind != kind { picked = Some(kind); }
        }
    }));
    if let Some(kind) = picked { g.set_kind(kind); }
    picked.is_some()
}

/// Controls of the position, UV and conversion generators. Returns whether anything changed and
/// whether the Grayscale Conversion image button was clicked.
fn shape_generator_controls(ui: &mut Ui, state: &mut UiState, g: &mut subcraft_doc::Generator) -> (bool, bool) {
    let mut s = g.shape_settings();
    let before = s;
    let mut pick = false;
    let point = |ui: &mut Ui, label: &'static str, p: &mut [f32; 3]| {
        for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
            slider_row(ui, &format!("{} {name}", tr(label)), egui::Slider::new(&mut p[axis], 0.0..=1.0));
        }
    };
    match g.kind {
        GeneratorKind::Distance3D => {
            point(ui, "Position", &mut s.point);
            slider_row(ui, tr("Radius"), egui::Slider::new(&mut s.radius, 0.0..=2.0)).on_hover_text(tr("Length of the fade, as a share of the model's longest side"));
            slider_row(ui, tr("Offset"), egui::Slider::new(&mut s.offset, 0.0..=2.0)).on_hover_text(tr("Distance kept fully white before the fade starts"));
            slider_row(ui, tr("Contrast"), egui::Slider::new(&mut s.contrast, 0.0..=1.0));
            hint(ui, tr("Positions are in the model's bounding box: 0 to 1 on each axis, like the colours of the Position map."));
        }
        GeneratorKind::LinearGradient3D => {
            slider_row(ui, tr("Balance"), egui::Slider::new(&mut s.balance, 0.0..=1.0));
            slider_row(ui, tr("Contrast"), egui::Slider::new(&mut s.contrast, 0.0..=1.0));
            point(ui, "3D Position Start", &mut s.start);
            point(ui, "3D Position End", &mut s.end);
            hint(ui, tr("Positions are in the model's bounding box: 0 to 1 on each axis, like the colours of the Position map."));
        }
        GeneratorKind::UvRandomColor => {
            prop_row(ui, tr("Seed"), |ui| {
                ui.add(egui::DragValue::new(&mut s.seed).speed(1.0));
                if ui.small_button(tr("Random")).clicked() {
                    s.seed = s.seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) >> 8;
                }
            });
            slider_row(ui, tr("Balance"), egui::Slider::new(&mut s.balance, 0.0..=1.0));
            slider_row(ui, tr("Contrast"), egui::Slider::new(&mut s.contrast, 0.0..=1.0));
            hint(ui, tr("One random gray per UV island."));
        }
        GeneratorKind::UvBorderDistance => {
            slider_row(ui, tr("Balance"), egui::Slider::new(&mut s.balance, 0.0..=1.0));
            slider_row(ui, tr("Contrast"), egui::Slider::new(&mut s.contrast, 0.0..=1.0));
            slider_row(ui, tr("Smoothness"), egui::Slider::new(&mut s.smoothness, 0.0..=1.0));
            slider_row(ui, tr("Distance"), egui::Slider::new(&mut s.distance, 0.0..=subcraft_doc::ShapeSettings::MAX_BORDER_DISTANCE).suffix(" px"))
                .on_hover_text(tr("How far from a UV island border the falloff reaches"));
        }
        GeneratorKind::UvChecker => {
            let color = |ui: &mut Ui, label: &'static str, c: &mut [f32; 3]| {
                prop_row(ui, tr(label), |ui| {
                    let mut srgb = subcraft_core::color::linear3_to_srgb(*c);
                    if ui.color_edit_button_rgb(&mut srgb).changed() {
                        *c = subcraft_core::color::srgb3_to_linear(srgb);
                    }
                });
            };
            prop_row(ui, tr("Seed"), |ui| {
                ui.add(egui::DragValue::new(&mut s.seed).speed(1.0));
                if ui.small_button(tr("Random")).clicked() {
                    s.seed = s.seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) >> 8;
                }
            });
            prop_row(ui, tr("Checker Color Mode"), |ui| {
                ui.selectable_value(&mut s.checker_random, false, tr("Uniform"));
                ui.selectable_value(&mut s.checker_random, true, tr("Random"));
            });
            ui.add_enabled_ui(!s.checker_random, |ui| color(ui, "Checker Color", &mut s.checker_color));
            slider_row(ui, tr("Checker Grid Tile"), egui::Slider::new(&mut s.checker_tile, 1.0..=64.0).integer());
            slider_row(ui, tr("Checker Grid Opacity"), egui::Slider::new(&mut s.grid_opacity, 0.0..=1.0));
            color(ui, "UV Borders Color", &mut s.border_color);
            slider_row(ui, tr("UV Borders Width"), egui::Slider::new(&mut s.border_width, 0.0..=16.0).suffix(" px"));
        }
        GeneratorKind::UvTexelDensity => {
            for (label, c) in [("Color Low", &mut s.density_low), ("Color Medium", &mut s.density_medium), ("Color High", &mut s.density_high)] {
                prop_row(ui, tr(label), |ui| {
                    let mut srgb = subcraft_core::color::linear3_to_srgb(*c);
                    if ui.color_edit_button_rgb(&mut srgb).changed() {
                        *c = subcraft_core::color::srgb3_to_linear(srgb);
                    }
                });
            }
            hint(ui, tr("Medium is where the texture spans the model's longest side once; Low and High are two octaves below and above."));
        }
        GeneratorKind::GrayscaleConversion => {
            match s.texture {
                Some(id) => {
                    if image_row(ui, state, id) {
                        pick = true;
                    }
                }
                None => {
                    if secondary_button(ui, tr("Choose image…")).clicked() {
                        pick = true;
                    }
                }
            }
            prop_row(ui, tr("Grayscale type"), |ui| {
                egui::ComboBox::from_id_salt("grayscale_conversion_type").selected_text(s.grayscale.label()).show_ui(ui, |ui| {
                    for t in subcraft_doc::GrayscaleType::ALL {
                        ui.selectable_value(&mut s.grayscale, t, t.label());
                    }
                });
            });
            slider_row(ui, tr("Balance"), egui::Slider::new(&mut s.balance, 0.0..=1.0));
            slider_row(ui, tr("Contrast"), egui::Slider::new(&mut s.contrast, 0.0..=1.0));
            slider_row(ui, tr("Tile"), egui::Slider::new(&mut s.tile, 0.1..=32.0).logarithmic(true));
            slider_row(ui, tr("Rotation"), egui::Slider::new(&mut s.rotation, -180.0..=180.0).suffix("\u{00b0}"));
            prop_row(ui, "", |ui| { ui.checkbox(&mut s.safe_rotation, tr("Safe Rotation")).on_hover_text(tr("Rotation in 45 degree steps")); });
        }
        _ => {}
    }
    let mut changed = false;
    prop_row(ui, "", |ui| changed |= ui.checkbox(&mut g.invert, tr("Invert")).changed());
    if s != before {
        g.shape = Some(s);
        changed = true;
    }
    (changed, pick)
}

/// A blending-mode dropdown row of a Mask Editor group.
fn editor_blend_row(ui: &mut Ui, id: &str, mode: &mut subcraft_doc::BlendMode) -> bool {
    let mut changed = false;
    prop_row(ui, tr("Blending Mode"), |ui| {
        egui::ComboBox::from_id_salt(("editor_blend", id)).selected_text(mode.label()).show_ui(ui, |ui| {
            for m in subcraft_doc::BlendMode::available_for_effect(false) {
                changed |= ui.selectable_value(mode, m, m.label()).changed();
            }
        });
    });
    changed
}

fn editor_group<R>(ui: &mut Ui, title: &'static str, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    egui::CollapsingHeader::new(RichText::new(tr(title)).font(theme::font_strong(12.0)).color(theme::TEXT())).id_salt(("mask_editor_group", title)).default_open(false).show(ui, body).body_returned
}

/// Painter's Mask Editor: the four global controls, one opacity per contribution, then one
/// collapsible group per contribution in the same order. Returns whether anything changed and the
/// texture slot (0 = Texture, 1 = Texture 2) whose image button was clicked.
fn mask_editor_controls(ui: &mut Ui, state: &mut UiState, g: &mut subcraft_doc::Generator) -> (bool, Option<u8>) {
    use subcraft_doc::{CurvatureMode, EditorDirections, GrayscaleType, MaskEditor};
    let mut ed = g.editor_settings();
    let before = ed;
    let mut pick = None;
    let unit = |ui: &mut Ui, label: &'static str, v: &mut f32| { slider_row(ui, tr(label), egui::Slider::new(v, 0.0..=1.0)); };
    let signed = |ui: &mut Ui, label: &'static str, v: &mut f32| { slider_row(ui, tr(label), egui::Slider::new(v, -1.0..=1.0)); };
    let toggle = |ui: &mut Ui, label: &'static str, v: &mut bool| { prop_row(ui, "", |ui| { ui.checkbox(v, tr(label)); }); };
    toggle(ui, "Global Invert", &mut ed.global_invert);
    slider_row(ui, tr("Global Blur"), egui::Slider::new(&mut ed.global_blur, 0.0..=MaskEditor::MAX_BLUR).suffix(" px"))
        .on_hover_text(tr("Blurs the blended contributions before Balance and Contrast (radius in texture pixels)."));
    unit(ui, "Global Balance", &mut ed.global_balance);
    unit(ui, "Global Contrast", &mut ed.global_contrast);
    ui.add_space(4.0);
    unit(ui, "Texture Opacity", &mut ed.texture.opacity);
    unit(ui, "Texture 2 Opacity", &mut ed.texture2.opacity);
    unit(ui, "Ambient Occlusion Opacity", &mut ed.occlusion.opacity);
    unit(ui, "Curvature Opacity", &mut ed.curvature.opacity);
    unit(ui, "World Space Normal Opacity", &mut ed.world_normal.opacity);
    unit(ui, "Position Gradient Opacity", &mut ed.position.opacity);
    unit(ui, "Thickness Opacity", &mut ed.thickness.opacity);
    ui.add_space(4.0);
    for (slot, title, texture) in [(0u8, "Texture", &mut ed.texture), (1, "Texture 2", &mut ed.texture2)] {
        editor_group(ui, title, |ui| {
            match texture.resource {
                Some(id) => { if image_row(ui, state, id) { pick = Some(slot); } }
                None => { if secondary_button(ui, tr("Choose image…")).clicked() { pick = Some(slot); } }
            }
            if texture.resource.is_some() && ui.small_button(tr("Remove image")).clicked() { texture.resource = None; }
            toggle(ui, "Invert", &mut texture.invert);
            prop_row(ui, tr("Grayscale Conversion"), |ui| {
                egui::ComboBox::from_id_salt(("editor_gray", slot)).selected_text(texture.grayscale.label()).show_ui(ui, |ui| {
                    for t in GrayscaleType::ALL { ui.selectable_value(&mut texture.grayscale, t, t.label()); }
                });
            });
            editor_blend_row(ui, title, &mut texture.blend);
            slider_row(ui, tr("Scale"), egui::Slider::new(&mut texture.scale, 0.1..=32.0).logarithmic(true));
            signed(ui, "Contrast", &mut texture.contrast);
            signed(ui, "Brightness", &mut texture.brightness);
            toggle(ui, "Triplanar", &mut texture.triplanar);
            ui.add_enabled_ui(texture.triplanar, |ui| unit(ui, "Triplanar Contrast", &mut texture.triplanar_contrast));
            toggle(ui, "Non-Square Tiling", &mut texture.non_square);
        });
    }
    editor_group(ui, "Ambient Occlusion", |ui| {
        toggle(ui, "Invert", &mut ed.occlusion.invert);
        editor_blend_row(ui, "ao", &mut ed.occlusion.blend);
        unit(ui, "Balance", &mut ed.occlusion.balance);
        unit(ui, "Contrast", &mut ed.occlusion.contrast);
    });
    editor_group(ui, "Curvature", |ui| {
        toggle(ui, "Invert", &mut ed.curvature.invert);
        editor_blend_row(ui, "curvature", &mut ed.curvature.blend);
        prop_row(ui, tr("Mode"), |ui| {
            egui::ComboBox::from_id_salt("editor_curvature_mode").selected_text(ed.curvature.mode.label()).show_ui(ui, |ui| {
                for m in CurvatureMode::ALL { ui.selectable_value(&mut ed.curvature.mode, m, m.label()); }
            });
        });
        signed(ui, "Contrast", &mut ed.curvature.contrast);
        signed(ui, "Brightness", &mut ed.curvature.brightness);
    });
    for (title, group) in [("World Space Normal", &mut ed.world_normal), ("Position Gradient", &mut ed.position)] {
        editor_group(ui, title, |ui| {
            toggle(ui, "Invert", &mut group.invert);
            editor_blend_row(ui, title, &mut group.blend);
            unit(ui, "Balance", &mut group.balance);
            unit(ui, "Contrast", &mut group.contrast);
            signed(ui, "Brightness", &mut group.brightness);
            for (axis, label) in group.axes.iter_mut().zip(EditorDirections::AXIS_LABELS) {
                unit(ui, label, &mut axis.amount);
            }
            for (axis, label) in group.axes.iter_mut().zip(EditorDirections::AXIS_LABELS) {
                ui.push_id((title, label), |ui| {
                    editor_group(ui, label, |ui| {
                        toggle(ui, "Invert", &mut axis.invert);
                        editor_blend_row(ui, label, &mut axis.blend);
                    });
                });
            }
        });
    }
    editor_group(ui, "Thickness", |ui| {
        toggle(ui, "Invert", &mut ed.thickness.invert);
        signed(ui, "Contrast", &mut ed.thickness.contrast);
        signed(ui, "Brightness", &mut ed.thickness.brightness);
    });
    let changed = ed != before;
    if changed { g.editor = Some(ed); }
    (changed, pick)
}

/// Level / Softness / Breakup under Painter's names for the kinds Painter has: Dirt shows Dirt Level
/// and Dirt Contrast (more dirt to the right), Metal Edge Wear Wear Level and Wear Contrast,
/// Ambient Occlusion and mesh maps Balance and Contrast (Contrast 0 = the smooth map itself). The
/// stored values stay the threshold and softness, so older projects read the same.
fn level_controls(ui: &mut Ui, g: &mut subcraft_doc::Generator) -> bool {
    let mut changed = false;
    let natural_edges = g.kind == GeneratorKind::Edges && g.edge_spread.is_some();
    if g.kind.uses_painter_names() {
        // Edges' level already grows the wear; the occlusion/map thresholds grow the mask downwards.
        let inverted = g.kind != GeneratorKind::Edges;
        let (level_label, level_tip, contrast_label, contrast_tip) = match g.kind {
            GeneratorKind::Cavity => (tr("Dirt Level"), "Amount of dirt: how far it creeps out of the occluded areas (Painter's Dirt Level).", tr("Dirt Contrast"), "Sharpness of the dirt's falloff: 0 = a soft gradient, 1 = a hard edge."),
            GeneratorKind::Edges => (tr("Wear Level"), if natural_edges { "Amount of wear: how much of the Spread reach is worn (0 = none, 1 = all of it), and which baked convex detail shows (Painter's Wear Level)." } else { "Amount of wear on the baked convex edges (Painter's Wear Level)." }, tr("Wear Contrast"), "Sharpness of the wear boundary: 0 = soft, 1 = crisp."),
            _ => (tr("Balance"), "Shifts the map towards white (more mask) or black (less).", tr("Contrast"), "0 = the map itself, smooth; 1 = a hard threshold at the balance."),
        };
        let mut level = if inverted { 1.0 - g.level } else { g.level };
        if slider_row(ui, level_label, egui::Slider::new(&mut level, 0.0..=1.0).max_decimals(3)).on_hover_text(level_tip).changed() {
            g.level = if inverted { 1.0 - level } else { level };
            changed = true;
        }
        let mut contrast = (1.0 - g.softness).clamp(0.0, 1.0);
        if slider_row(ui, contrast_label, egui::Slider::new(&mut contrast, 0.0..=1.0)).on_hover_text(contrast_tip).changed() {
            g.softness = (1.0 - contrast).max(0.001);
            changed = true;
        }
    } else {
        changed |= slider_row(ui, tr("Level"), egui::Slider::new(&mut g.level, 0.0..=1.0).max_decimals(3)).on_hover_text("Threshold for the generator's source values.").changed();
        changed |= slider_row(ui, tr("Softness"), egui::Slider::new(&mut g.softness, 0.0..=1.0)).changed();
    }
    let breakup = if matches!(g.kind, GeneratorKind::Cavity | GeneratorKind::Edges) { tr("Grunge Amount") } else { tr("Breakup") };
    changed |= slider_row(ui, breakup, egui::Slider::new(&mut g.noise_amount, 0.0..=1.0)).on_hover_text("How much the grunge breaks the mask up (0 = smooth).").changed();
    changed
}

/// Substance Painter's extra generator controls that each kind actually uses ([`subcraft_doc::PainterControls`]):
/// Dirt has Edges masking and a grunge floor, natural edge wear has AO masking, Curvature weight and
/// Edges smoothness, and Grain speckles every kind except the plain curvature threshold. Untouched
/// generators keep `painter: None` (their exact older look); the first change stores the controls.
fn painter_controls(ui: &mut Ui, g: &mut subcraft_doc::Generator) -> bool {
    let natural_edges = g.kind == GeneratorKind::Edges && g.edge_spread.is_some();
    let mut pc = g.painter.unwrap_or_default();
    let before = pc;
    if g.kind == GeneratorKind::Cavity {
        slider_row(ui, tr("Edges Masking"), egui::Slider::new(&mut pc.edges_masking, 0.0..=1.0))
            .on_hover_text("Keeps dirt off convex edges (Painter's Dirt: Edges masking). Reads the baked curvature.");
        slider_row(ui, tr("Grunge Floor"), egui::Slider::new(&mut pc.grunge_floor, 0.0..=1.0))
            .on_hover_text("A faint layer of grunge over open surfaces where the dirt level leaves none, like Painter's dirt at higher Grunge Amount. Needs Grunge Amount above 0.");
    }
    if natural_edges {
        slider_row(ui, tr("Ambient Occlusion Masking"), egui::Slider::new(&mut pc.ao_masking, 0.0..=1.0))
            .on_hover_text("Removes wear where the surface is occluded (Painter's Metal Edge Wear: Ambient Occlusion Masking). Reads the baked AO.");
        slider_row(ui, tr("Curvature Weight"), egui::Slider::new(&mut pc.curvature_weight, 0.0..=2.0))
            .on_hover_text("Strength of the baked curvature detail next to the geometric edge band (Painter: Curvature Weight). 1 = default.");
        slider_row(ui, tr("Edges Smoothness"), egui::Slider::new(&mut pc.smoothness, 0.0..=1.0))
            .on_hover_text("Extra softening of the wear boundary (Painter: Edges Smoothness).");
    }
    if g.kind != GeneratorKind::Edges || natural_edges {
        if g.kind != GeneratorKind::AmbientOcclusion && !matches!(g.kind, GeneratorKind::MeshMap(_)) {
            prop_row(ui, "", |ui| {
                ui.checkbox(&mut pc.grunge_map, tr("Grunge map"))
                    .on_hover_text(tr("Break the mask up with the bundled photo-like dirt texture, as Painter's generators do. Off: a smooth procedural noise."));
            });
        }
        slider_row(ui, tr("Grain"), egui::Slider::new(&mut pc.grain, 0.0..=1.0))
            .on_hover_text("Fine grain that chips wear and speckles dirt instead of smooth bands. Fades in with the grunge amount: 0 stays smooth.");
    }
    if pc == before {
        return false;
    }
    g.painter = Some(pc);
    true
}

/// The Curvature generator's mode and detail sizes ([`subcraft_doc::CurvatureDetail`]). Untouched
/// generators keep `curvature: None` (the baked map itself); the first change stores the settings.
fn curvature_controls(ui: &mut Ui, g: &mut subcraft_doc::Generator) -> bool {
    use subcraft_doc::{CurvatureDetail, CurvatureDetailMode};
    let mut d = g.curvature.unwrap_or_default();
    let before = d;
    prop_row(ui, tr("Mode"), |ui| {
        egui::ComboBox::from_id_salt("curvature_mode").selected_text(tr(d.mode.label())).show_ui(ui, |ui| {
            for mode in CurvatureDetailMode::ALL {
                ui.selectable_value(&mut d.mode, mode, tr(mode.label()));
            }
        })
        .response
    })
    .on_hover_text("Both: the baked map (convex bright, concave dark). Edges only: convex edges on black. Cavities only: concave creases on black.");
    let max = CurvatureDetail::MAX_WEIGHT;
    slider_row(ui, tr("Fine detail"), egui::Slider::new(&mut d.fine, 0.0..=max))
        .on_hover_text("Weight of the baked curvature itself: small bevels and sharp edges. 1 = the map as baked.");
    slider_row(ui, tr("Medium detail"), egui::Slider::new(&mut d.medium, 0.0..=max))
        .on_hover_text("Weight of the curvature averaged over about 1/256 of the texture: wider bevels and grooves.");
    slider_row(ui, tr("Coarse detail"), egui::Slider::new(&mut d.coarse, 0.0..=max))
        .on_hover_text("Weight of the curvature averaged over about 1/64 of the texture: large forms and broad rounded edges.");
    if d == before {
        return false;
    }
    g.curvature = Some(d);
    true
}

pub(crate) fn effect_editor(ui: &mut Ui, state: &mut UiState, layer: subcraft_core::LayerId, e: &Arc<MaskEffect>, cmds: &mut Vec<Command>) {
    let mut eff = (**e).clone();
    let default_name = eff.name == eff.kind.label();
    let mut changed = false;
    crate::widgets::card(ui, theme::ACCENT_DIM(), |ui| {
        ui.horizontal(|ui| {
            let (icon, tip) = if eff.visible { (icons::EYE, "Hide effect") } else { (icons::EYE_SLASH, "Show effect") };
            if icon_button(ui, icon, tip).clicked() {
                eff.visible = !eff.visible;
                changed = true;
            }
            ui.label(RichText::new(&eff.name).font(theme::font_strong(12.5)).color(if eff.visible { theme::TEXT() } else { TEXT_FAINT() }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, icons::X, "Remove effect").clicked() {
                    cmds.push(Command::RemoveMaskEffect { layer, effect: eff.id });
                }
            });
        });
        if eff.visible {
            let anchored = matches!(eff.kind, MaskEffectKind::Anchor | MaskEffectKind::AnchorRef { .. })
                || matches!(&eff.kind, MaskEffectKind::Generator(g) if g.accepts_input());
            let anchor_index = if anchored { subcraft_doc::AnchorIndex::build(&state.session.selected_texture_set().layers) } else { Default::default() };
            let channels: Vec<ChannelKind> = if anchored { state.session.selected_texture_set().channel_kinds().collect() } else { Vec::new() };
            let site = subcraft_doc::RefSite::Mask { layer, effect: eff.id };
            if !matches!(eff.kind, MaskEffectKind::Anchor) {
            prop_row(ui, tr("Blend"), |ui| {
                egui::ComboBox::from_id_salt(("effect_blend", eff.id)).selected_text(eff.blend.label()).show_ui(ui, |ui| {
                    for mode in subcraft_doc::BlendMode::available_for_effect(false) {
                        changed |= ui.selectable_value(&mut eff.blend, mode, mode.label()).changed();
                    }
                });
            });
            changed |= slider_row(ui, tr("Opacity"), egui::Slider::new(&mut eff.opacity, 0.0..=1.0)).changed();
            }
            match &mut eff.kind {
                MaskEffectKind::Anchor => {
                    prop_row(ui, tr("Name"), |ui| {
                        if anchor_name_field(ui, eff.id, &mut eff.name) {
                            changed = true;
                        }
                    });
                    super::anchor::anchor_info(ui, &anchor_index, eff.id, true);
                }
                MaskEffectKind::AnchorRef { input, levels } => {
                    let mut pick = Some(*input);
                    if super::anchor::picker(ui, &anchor_index, site, &mut pick, None, &channels, ChannelKind::Height) && let Some(p) = pick {
                        *input = p;
                        changed = true;
                    }
                    let mut use_levels = levels.is_some();
                    if ui.checkbox(&mut use_levels, tr("Levels")).changed() {
                        *levels = use_levels.then(Levels::default);
                        changed = true;
                    }
                    if let Some(l) = levels {
                        changed |= slider_row(ui, tr("In low"), egui::Slider::new(&mut l.in_low, 0.0..=1.0)).changed();
                        changed |= slider_row(ui, tr("In high"), egui::Slider::new(&mut l.in_high, 0.0..=1.0)).changed();
                        changed |= slider_row(ui, tr("Gamma"), egui::Slider::new(&mut l.gamma, 0.1..=4.0)).changed();
                        changed |= slider_row(ui, tr("Out low"), egui::Slider::new(&mut l.out_low, 0.0..=1.0)).changed();
                        changed |= slider_row(ui, tr("Out high"), egui::Slider::new(&mut l.out_high, 0.0..=1.0)).changed();
                    }
                    hint(ui, tr("Gray value of the anchor (color channels by luminance), times its coverage. Stack Levels or Invert above it to remap."));
                }
                MaskEffectKind::Generator(g) if g.kind == GeneratorKind::MaskEditor => {
                    changed |= generator_kind_row(ui, g);
                    let (edited, pick) = mask_editor_controls(ui, state, g);
                    changed |= edited;
                    if let Some(slot) = pick {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::GeneratorTexture { layer, effect: e.id, slot }, crate::files::IMAGES, false, None);
                    }
                }
                MaskEffectKind::Generator(g) if g.kind.uses_shape() => {
                    changed |= generator_kind_row(ui, g);
                    let (edited, pick) = shape_generator_controls(ui, state, g);
                    changed |= edited;
                    if pick {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::GeneratorTexture { layer, effect: e.id, slot: 2 }, crate::files::IMAGES, false, None);
                    }
                }
                MaskEffectKind::Generator(g) => {
                    changed |= generator_kind_row(ui, g);
                    if g.accepts_input() {
                        let mut input = g.input;
                        if super::anchor::picker(ui, &anchor_index, site, &mut input, Some("Baked mesh map"), &channels, ChannelKind::Height) {
                            g.input = input;
                            changed = true;
                        }
                        if g.input.is_some() {
                            hint(ui, tr("The anchor replaces the baked map this generator reads (curvature for Edges and Concave, occlusion for Dirt and Ambient Occlusion)."));
                        }
                    }
                    if g.kind == GeneratorKind::PositionGradient {
                        prop_row(ui, tr("Axis"), |ui| for axis in subcraft_doc::GradientAxis::ALL {
                            changed |= ui.selectable_value(&mut g.axis, axis, axis.label()).changed();
                        });
                    }
                    if g.kind == GeneratorKind::Edges {
                        let mut natural = g.edge_spread.is_some();
                        if ui.checkbox(&mut natural, tr("Natural edge wear")).on_hover_text(tr("Surface-constrained spread with an irregular boundary. Older projects retain their original wear until enabled.")).changed() {
                            g.edge_spread = natural.then(subcraft_doc::EdgeSpread::default); changed = true;
                        }
                        // Spread is always offered (wear made without natural wear, such as the
                        // built-in smart materials' chip lines, used to hide it): moving it turns
                        // natural edge wear on with that reach.
                        let mut distance = g.edge_spread.map_or(subcraft_doc::EdgeSpread::default().distance, |s| s.distance);
                        let natural_on = g.edge_spread.is_some();
                        if slider_row(ui, tr("Spread"), egui::Slider::new(&mut distance, 0.0..=12.5).suffix("%"))
                            .on_hover_text(if natural_on { "How far wear can reach from an exposed edge along the surface, as a percentage of the model's longest dimension. Level decides how much of that reach is worn. Follows connected edges, not UV islands." } else { "How far wear can reach from an exposed edge along the surface. Moving it turns on natural edge wear (this wear currently follows the baked curvature only)." })
                            .changed()
                        {
                            g.edge_spread.get_or_insert_with(subcraft_doc::EdgeSpread::default).distance = distance;
                            changed = true;
                        }
                        if let Some(spread) = &mut g.edge_spread {
                            changed |= slider_row(ui, tr("Variation"), egui::Slider::new(&mut spread.variation, 0.0..=1.0)).on_hover_text(tr("Irregular, chipped boundary, independent of Breakup. Zero gives a clean geometric band.")).changed();
                            if !spread.level_reach {
                                prop_row(ui, "", |ui| {
                                    if ui.checkbox(&mut spread.level_reach, tr("Level controls the amount")).on_hover_text("This wear was made when Level only selected baked detail and Spread alone set the reach. Turn on to make Level the amount of wear within Spread.").changed() { changed = true; }
                                });
                            }
                        }
                    }
                    if g.kind == GeneratorKind::MeshMap(MeshMapKind::Curvature) {
                        changed |= curvature_controls(ui, g);
                    }
                    changed |= level_controls(ui, g);
                    let mut size = g.pattern_size();
                    if slider_row(ui, tr("Pattern size"), egui::Slider::new(&mut size, 1.0..=100.0).logarithmic(true).suffix("%")).changed() {
                        g.set_pattern_size(size); changed = true;
                    }
                    changed |= painter_controls(ui, g);
                    if custom_grunge_row(ui, state, g, &mut changed) {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::GeneratorGrunge { layer, effect: e.id }, crate::files::IMAGES, false, None);
                    }
                    prop_row(ui, tr("Seed"), |ui| {
                        changed |= ui.add(egui::DragValue::new(&mut g.noise.seed).speed(1.)).changed();
                        if ui.small_button(tr("Next")).on_hover_text(tr("Try the next deterministic pattern seed")).clicked() {
                            g.noise.seed = g.noise.seed.wrapping_add(1); changed = true;
                        }
                        if ui.small_button(tr("Random")).on_hover_text(tr("Pick a new random seed")).clicked() {
                            g.noise.seed = random_seed(); changed = true;
                        }
                    });
                    for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                        let mut percent = g.offset[axis] * 100.0;
                        if slider_row(ui, &format!("Offset {name}"), egui::Slider::new(&mut percent, -100.0..=100.0).suffix("%").max_decimals(1))
                            .on_hover_text("Slides the breakup pattern through the model along this axis, as a percentage of the model's longest dimension. The seed and pattern size stay the same.").changed() {
                            g.offset[axis] = percent / 100.0; changed = true;
                        }
                    }
                    if g.kind != GeneratorKind::Edges || g.edge_spread.is_none() {
                        changed |= ui.checkbox(&mut g.organic_breakup, tr("Reduce repetition")).changed();
                    }
                    prop_row(ui, "", |ui| changed |= ui.checkbox(&mut g.invert, tr("Invert")).changed());
                    if g.kind == GeneratorKind::Directional {
                        // Painter's Light: the direction as two angles, plus attenuation.
                        let (mut h, mut v) = subcraft_doc::light_angles(g.direction);
                        let a = slider_row(ui, tr("Horizontal Angle"), egui::Slider::new(&mut h, -180.0..=180.0).suffix("\u{00b0}")).changed();
                        let b = slider_row(ui, tr("Vertical Angle"), egui::Slider::new(&mut v, -90.0..=90.0).suffix("\u{00b0}")).changed();
                        if a || b {
                            g.direction = subcraft_doc::light_direction(h, v);
                            changed = true;
                        }
                        prop_row(ui, tr("Direction"), |ui| { for value in &mut g.direction {
                            changed |= ui.add(egui::DragValue::new(value).range(-1.0..=1.0).speed(0.01)).changed();
                        } });
                        let mut att = g.shape.map_or(0.0, |s| s.attenuation);
                        if slider_row(ui, tr("Light Attenuation"), egui::Slider::new(&mut att, 0.0..=1.0)).on_hover_text(tr("Fades the light toward the side of the model facing away from it")).changed() {
                            g.shape = Some(subcraft_doc::ShapeSettings { attenuation: att, ..g.shape_settings() });
                            changed = true;
                        }
                    }
                    // Micro effect: the same mask also writes Normal / Height of the layers below.
                    let mut micro_on = g.micro.is_some_and(|m| m.enabled);
                    prop_row(ui, "", |ui| {
                        if ui.checkbox(&mut micro_on, tr("Micro effect (Normal / Height)")).on_hover_text(tr("Also applies this generator's pattern, grain included, to the Normal and Height of the layers below it: wear chips the surface, dirt fills it. Only channels that already hold a normal or height map change; none is created. Costs extra only while on.")).changed() {
                            g.set_micro_enabled(micro_on); changed = true;
                        }
                    });
                    if let Some(m) = g.micro.as_mut().filter(|m| m.enabled) {
                        changed |= slider_row(ui, tr("Micro amount"), egui::Slider::new(&mut m.amount, 0.0..=1.0)).on_hover_text(tr("Height change at full mask. Normals follow the height slope.")).changed();
                        prop_row(ui, tr("Micro direction"), |ui| for d in [subcraft_doc::MicroDirection::Raise, subcraft_doc::MicroDirection::Carve] {
                            changed |= ui.selectable_value(&mut m.direction, d, match d { subcraft_doc::MicroDirection::Raise => tr("Raise"), subcraft_doc::MicroDirection::Carve => tr("Carve") }).changed();
                        });
                        prop_row(ui, "", |ui| changed |= ui.checkbox(&mut m.grain_follows, tr("Grain follows generator")).on_hover_text(tr("Pit the micro surface with the generator's fine grain, so it is as speckled as the mask. Off: a smooth surface that follows the mask only.")).changed());
                    }
                }
                MaskEffectKind::Fill { value, procedural, projection } => {
                    let mut use_procedural = procedural.is_some();
                    if ui.checkbox(&mut use_procedural, tr("Procedural")).changed() {
                        *procedural = use_procedural.then(|| ProceduralParams::new(ProceduralKind::Clouds));
                        changed = true;
                    }
                    if let Some(p) = procedural {
                        prop_row(ui, tr("Pattern"), |ui| egui::ComboBox::from_id_salt("mask_pattern").selected_text(p.kind.label()).show_ui(ui, |ui| {
                            for kind in ProceduralKind::ALL { changed |= ui.selectable_value(&mut p.kind, kind, kind.label()).changed(); }
                        }));
                        changed |= slider_row(ui, tr("Scale"), egui::Slider::new(&mut p.scale, 1.0..=64.0)).changed();
                        changed |= slider_row(ui, tr("Balance"), egui::Slider::new(&mut p.balance, 0.0..=1.0)).changed();
                        changed |= slider_row(ui, tr("Contrast"), egui::Slider::new(&mut p.contrast, 0.0..=1.0)).changed();
                        prop_row(ui, tr("Seed"), |ui| changed |= ui.add(egui::DragValue::new(&mut p.seed)).changed());
                        changed |= ui.checkbox(&mut p.invert, tr("Invert pattern")).changed();
                        changed |= scratch_controls(ui, p, ("mask_scratch", 0));
                    } else { changed |= slider_row(ui, tr("Value"), egui::Slider::new(value, 0.0..=1.0)).changed(); }
                    changed |= projection_editor(ui, projection);
                    changed |= projection_options(ui, projection);
                }
                MaskEffectKind::Levels(l) => {
                    let key = subcraft_app::layer_previews::PreviewKey { layer, channel: ChannelKind::Roughness, mask: true, effect: Some(e.id), size: 0 };
                    changed |= levels_editor(ui, state, key, l, false);
                }
                MaskEffectKind::Compare(c) => {
                    let channels: Vec<ChannelKind> = state.session.selected_texture_set().channel_kinds().collect();
                    changed |= compare_editor(ui, c, &channels);
                }
                MaskEffectKind::Filter(filter) => {
                    changed |= super::filter::editor(ui, filter, None);
                    if filter_image_row(ui, state, filter, &mut changed) {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::FilterImage { layer, effect: e.id, mask: true }, crate::files::IMAGES, false, None);
                    }
                    hint(ui, tr("Filters the mask below this effect; opacity mixes the result with that input."));
                }
                MaskEffectKind::Image { resource, projection, invert } => {
                    if image_row(ui, state, *resource) {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::MaskImage { layer, effect: e.id }, crate::files::IMAGES, false, None);
                    }
                    prop_row(ui, "", |ui| changed |= ui.checkbox(invert, tr("Invert")).changed());
                    changed |= projection_editor(ui, projection);
                    changed |= projection_options(ui, projection);
                    hint(ui, tr("Uses the image's transparency if it has any, otherwise its brightness."));
                }
                MaskEffectKind::IdColor(id) => changed |= id_color_editor(ui, state, layer, e.id, id, cmds),
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

/// ID Color Mask (ADR-0041): picked swatches with remove buttons, a 3D-view eyedropper,
/// tolerance/softness/invert. Swatch edits are their own undo steps (`SetIdColors`).
fn id_color_editor(ui: &mut Ui, state: &mut UiState, layer: subcraft_core::LayerId, effect: subcraft_core::EffectId, id: &mut subcraft_doc::IdColorMask, cmds: &mut Vec<Command>) -> bool {
    let mut changed = false;
    let texture_set = state.session.selection().texture_set;
    if !state.session.has_id_map(texture_set) {
        hint(ui, tr("No ID map baked: this mask stays black. Bake an ID map in Bake Mesh Maps."));
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, c) in id.colors.iter().enumerate() {
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let fill = egui::Color32::from_rgb(byte(c[0]), byte(c[1]), byte(c[2]));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, fill);
            if icon_button(ui, icons::X, "Remove this ID color").clicked() {
                let mut colors = id.colors.clone();
                colors.remove(i);
                cmds.push(Command::SetIdColors { layer, effect, colors });
            }
        }
        if id.colors.is_empty() { ui.label(RichText::new(tr("No colors picked")).font(theme::font(12.0)).color(TEXT_FAINT())); }
    });
    let active = state.picking_id_color == Some((layer, effect));
    let label = if active { "Picking: click the model (Esc to stop)" } else { "Pick from model" };
    if ui.add(egui::Button::new(RichText::new(format!("{}  {label}", icons::CROSSHAIR)).font(theme::font(12.0))).selected(active))
        .on_hover_text(tr("Click parts in the 3D view to add their ID color"))
        .clicked()
    {
        state.picking_id_color = if active { None } else { Some((layer, effect)) };
        if !active { state.picking_parts = false; }
    }
    let mut hardness = 1.0 - id.softness;
    if slider_row(ui, tr("Hardness"), egui::Slider::new(&mut hardness, 0.0..=1.0).max_decimals(3)).on_hover_text(tr("1 cuts hard at the tolerance; lower values fade out beyond it")).changed() {
        id.softness = (1.0 - hardness).clamp(0.0, 1.0);
        changed = true;
    }
    changed |= slider_row(ui, tr("Tolerance"), egui::Slider::new(&mut id.tolerance, 0.0..=1.0).max_decimals(3)).on_hover_text(tr("Largest RGB distance from a picked color that still counts as a full match")).changed();
    changed |= slider_row(ui, tr("Output value"), egui::Slider::new(&mut id.output, 0.0..=1.0)).on_hover_text(tr("Gray written where the colors match")).changed();
    prop_row(ui, tr("Background color"), |ui| {
        egui::ComboBox::from_id_salt("selection_background").selected_text(tr(id.background.label())).show_ui(ui, |ui| {
            for b in subcraft_doc::SelectionBackground::ALL {
                changed |= ui.selectable_value(&mut id.background, b, tr(b.label())).changed();
            }
        });
    });
    // Older projects' invert switch: shown only while it is on, so it can be turned off.
    if id.invert {
        prop_row(ui, "", |ui| changed |= ui.checkbox(&mut id.invert, tr("Invert")).changed());
    }
    changed
}

/// Painter's Levels: the component dropdown (colour channels), the input histogram, the five
/// handles of the shown component, then Auto, Invert and Clamp. `key` names the effect whose
/// input the histogram shows.
pub(crate) fn levels_editor(ui: &mut Ui, state: &mut UiState, key: subcraft_app::layer_previews::PreviewKey, levels: &mut Levels, color: bool) -> bool {
    let mut changed = false;
    let id = egui::Id::new(("levels_component", key.effect));
    let mut component: Option<usize> = if color { ui.ctx().data(|d| d.get_temp(id)).unwrap_or(None) } else { None };
    if color {
        const NAMES: [&str; 4] = ["RGB", "R", "G", "B"];
        let at = component.map_or(0, |c| c + 1);
        prop_row(ui, tr("Component"), |ui| {
            egui::ComboBox::from_id_salt(id).selected_text(NAMES[at]).show_ui(ui, |ui| {
                for (i, name) in NAMES.iter().enumerate() {
                    if ui.selectable_label(i == at, *name).clicked() {
                        component = i.checked_sub(1);
                    }
                }
            });
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, component));
    }
    let histogram = state.layer_previews.histogram(&state.session, key);
    // R, G, B or the luminance (RGB, and gray inputs).
    let shown = component.unwrap_or(3);
    if let Some(h) = &histogram {
        histogram_strip(ui, &h.bins[shown], levels.component(component));
    }
    let mut r = levels.component(component);
    let before = r;
    slider_row(ui, tr("In low"), egui::Slider::new(&mut r.in_low, 0.0..=1.0));
    slider_row(ui, tr("Gamma"), egui::Slider::new(&mut r.gamma, 0.1..=4.0).logarithmic(true));
    slider_row(ui, tr("In high"), egui::Slider::new(&mut r.in_high, 0.0..=1.0));
    slider_row(ui, tr("Out low"), egui::Slider::new(&mut r.out_low, 0.0..=1.0));
    slider_row(ui, tr("Out high"), egui::Slider::new(&mut r.out_high, 0.0..=1.0));
    if r != before {
        levels.set_component(component, r);
        changed = true;
    }
    prop_row(ui, "", |ui| {
        let auto = ui.add_enabled(histogram.as_ref().is_some_and(|h| h.count > 0), egui::Button::new(tr("Auto")))
            .on_hover_text(tr("Input handles at the darkest and brightest input values"));
        if auto.clicked() && let Some(h) = &histogram {
            levels.auto(component, h.min[shown], h.max[shown]);
            changed = true;
        }
        if ui.button(tr("Invert")).on_hover_text(tr("Swap the output handles")).clicked() {
            levels.invert(component);
            changed = true;
        }
        changed |= ui.checkbox(&mut levels.clamp, tr("Clamp")).on_hover_text(tr("Keep values between the input handles; off, they extend past them (HDR)")).changed();
    });
    changed
}

/// A histogram as bars, with the input handles and the gamma midpoint marked.
fn histogram_strip(ui: &mut Ui, bins: &[u32], r: subcraft_doc::LevelsRange) {
    let width = ui.available_width().min(320.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 48.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::FIELD());
    // Scale by the second-largest bin so one huge spike (a flat area) does not flatten the rest.
    let mut sorted: Vec<u32> = bins.to_vec();
    sorted.sort_unstable();
    let top = sorted.iter().rev().nth(1).copied().unwrap_or(0).max(1) as f32;
    let step = rect.width() / bins.len().max(1) as f32;
    for (i, &b) in bins.iter().enumerate() {
        let h = (b as f32 / top).min(1.0) * rect.height();
        let x = rect.left() + i as f32 * step;
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + step.max(1.0), rect.bottom())), 0.0, theme::TEXT_FAINT());
    }
    let mark = |v: f32, color: egui::Color32| {
        let x = rect.left() + v.clamp(0.0, 1.0) * rect.width();
        painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0, color));
    };
    mark(r.in_low, theme::TEXT());
    mark(r.in_high, theme::TEXT());
    // The input value that maps to the output midpoint.
    mark(r.in_low + (r.in_high - r.in_low) * 0.5f32.powf(r.gamma.max(1e-3)), theme::ACCENT());
}

/// Painter's Compare Mask: channel, the left operand / operation / right operand row, then
/// Constant, Tolerance and Hardness.
fn compare_editor(ui: &mut Ui, c: &mut subcraft_doc::CompareMask, channels: &[ChannelKind]) -> bool {
    use subcraft_doc::{CompareOp, CompareOperand};
    let mut changed = false;
    prop_row(ui, tr("Channel"), |ui| {
        egui::ComboBox::from_id_salt("compare_channel").selected_text(c.channel.display_name()).show_ui(ui, |ui| {
            for k in channels {
                changed |= ui.selectable_value(&mut c.channel, *k, k.display_name()).changed();
            }
        });
    });
    let operand = |ui: &mut Ui, id: &str, o: &mut CompareOperand| -> bool {
        let mut changed = false;
        egui::ComboBox::from_id_salt(id).width(96.0).selected_text(tr(o.label())).show_ui(ui, |ui| {
            for v in CompareOperand::ALL {
                changed |= ui.selectable_value(o, v, tr(v.label())).changed();
            }
        });
        changed
    };
    prop_row(ui, tr("Compare"), |ui| {
        changed |= operand(ui, "compare_left", &mut c.left);
        egui::ComboBox::from_id_salt("compare_op").width(110.0).selected_text(tr(c.op.label())).show_ui(ui, |ui| {
            for v in CompareOp::ALL {
                changed |= ui.selectable_value(&mut c.op, v, tr(v.label())).changed();
            }
        });
        changed |= operand(ui, "compare_right", &mut c.right);
    });
    let uses_constant = c.left == CompareOperand::Constant || c.right == CompareOperand::Constant;
    ui.add_enabled_ui(uses_constant, |ui| changed |= slider_row(ui, tr("Constant"), egui::Slider::new(&mut c.constant, 0.0..=1.0)).changed());
    ui.add_enabled_ui(c.op == CompareOp::Within, |ui| changed |= slider_row(ui, tr("Tolerance"), egui::Slider::new(&mut c.tolerance, 0.0..=1.0)).changed());
    changed |= slider_row(ui, tr("Hardness"), egui::Slider::new(&mut c.hardness, 0.0..=1.0))
        .on_hover_text(tr("1 cuts hard where the comparison flips; lower values blend across it"))
        .changed();
    hint(ui, tr("White where the comparison holds. Layers Below is the channel as composited under this layer; This Layer is this layer's own content."));
    changed
}

/// Controls of the scratch generator (shown only for that pattern): presets, then one slider
/// per setting with ranges that stay sensible. True if anything changed.
fn scratch_controls(ui: &mut Ui, params: &mut ProceduralParams, id: (&str, u32)) -> bool {
    if params.kind != ProceduralKind::Scratch {
        return false;
    }
    let mut changed = false;
    prop_row(ui, tr("Preset"), |ui| {
        egui::ComboBox::from_id_salt(id).selected_text("Choose…").show_ui(ui, |ui| {
            for preset in subcraft_doc::ScratchPreset::ALL {
                if ui.selectable_label(false, preset.label()).clicked() {
                    preset.apply(params);
                    changed = true;
                }
            }
        });
    });
    let s = &mut params.scratch;
    changed |= slider_row(ui, tr("Direction"), egui::Slider::new(&mut s.angle_deg, 0.0..=180.0).suffix("°")).changed();
    changed |= slider_row(ui, tr("Spread"), egui::Slider::new(&mut s.spread_deg, 0.0..=90.0).suffix("°")).changed();
    changed |= slider_row(ui, tr("Length"), egui::Slider::new(&mut s.length, 0.1..=2.0)).changed();
    changed |= slider_row(ui, tr("Length var."), egui::Slider::new(&mut s.length_var, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Width"), egui::Slider::new(&mut s.width, 0.001..=0.06).logarithmic(true)).changed();
    changed |= slider_row(ui, tr("Width var."), egui::Slider::new(&mut s.width_var, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Density"), egui::Slider::new(&mut s.density, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Curvature"), egui::Slider::new(&mut s.curvature, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Depth falloff"), egui::Slider::new(&mut s.depth_falloff, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Clustering"), egui::Slider::new(&mut s.cluster, 0.0..=1.0)).changed();
    changed |= slider_row(ui, tr("Edge wear"), egui::Slider::new(&mut s.wear, 0.0..=1.0)).changed();
    hint(ui, tr("Edge wear keeps scratches near edges and grooves; it needs baked mesh maps (Ctrl+B)."));
    changed
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
                prop_row(ui, tr("Value"), |ui| changed |= channel_value(ui, *kind, value).changed);
            }
            FillSource::Procedural { params, low, high } => {
                prop_row(ui, tr("Pattern"), |ui| {
                    egui::ComboBox::from_id_salt(("kind", *kind)).selected_text(params.kind.label()).show_ui(ui, |ui| {
                        for k in ProceduralKind::ALL {
                            changed |= ui.selectable_value(&mut params.kind, k, k.label()).changed();
                        }
                    });
                });
                changed |= slider_row(ui, tr("Scale"), egui::Slider::new(&mut params.scale, 1.0..=64.0).step_by(1.0)).changed();
                changed |= slider_row(ui, tr("Balance"), egui::Slider::new(&mut params.balance, 0.0..=1.0)).changed();
                changed |= slider_row(ui, tr("Contrast"), egui::Slider::new(&mut params.contrast, 0.0..=1.0)).changed();
                prop_row(ui, tr("Seed"), |ui| changed |= ui.add(egui::DragValue::new(&mut params.seed)).changed());
                changed |= scratch_controls(ui, params, ("fill_scratch", *kind as u32));
                prop_row(ui, tr("Low"), |ui| changed |= channel_value(ui, *kind, low).changed);
                prop_row(ui, tr("High"), |ui| changed |= channel_value(ui, *kind, high).changed);
                prop_row(ui, "", |ui| changed |= ui.checkbox(&mut params.invert, tr("Invert")).changed());
            }
            FillSource::MeshMap { map } => {
                prop_row(ui, tr("Map"), |ui| {
                    egui::ComboBox::from_id_salt(("map", *kind)).selected_text(map.display_name()).show_ui(ui, |ui| {
                        for m in MeshMapKind::ALL {
                            changed |= ui.selectable_value(map, m, m.display_name()).changed();
                        }
                    });
                });
                hint(ui, crate::keys::native("Uses the baked map (Ctrl+B); neutral until baked."));
            }
            // The anchor picker sits above (it needs the layer and the texture set).
            FillSource::Anchor { .. } => {}
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

/// What a click on the channel chips asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelAction {
    /// Toggle a channel the texture set already has (`true`: Alt-click, solo).
    Toggle(ChannelKind, bool),
    /// Add a channel the texture set does not have yet, and turn it on for this layer.
    Add(ChannelKind),
}

/// Channels a fill layer can gain even though the default texture set lacks them.
pub(crate) const ADDABLE_CHANNELS: [ChannelKind; 2] = [ChannelKind::Emissive, ChannelKind::AmbientOcclusion];

/// UI events only: the app owns independent/solo state changes and their undo transaction.
pub(crate) fn channel_toggles(ui: &mut Ui, channels: &[(ChannelKind, bool)]) -> Option<(ChannelKind, bool)> {
    match channel_chips(ui, channels, &[]) {
        Some(ChannelAction::Toggle(kind, solo)) => Some((kind, solo)),
        _ => None,
    }
}

/// Chips for the set's channels plus "+ name" chips for `addable` channels it lacks.
pub(crate) fn channel_chips(ui: &mut Ui, channels: &[(ChannelKind, bool)], addable: &[ChannelKind]) -> Option<ChannelAction> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4., 6.);
        for &(kind, on) in channels {
            let response = crate::widgets::chip(ui, kind.short_name(), on).on_hover_text(format!("{} · Click: toggle · Alt+click: only this channel", kind.display_name()));
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, kind.display_name()));
            if response.clicked() { clicked = Some(ChannelAction::Toggle(kind, ui.input(|i| i.modifiers.alt))); }
        }
        for &kind in addable {
            let label = format!("+ {}", kind.short_name());
            let response = crate::widgets::chip(ui, &label, false).on_hover_text(format!("Add the {} channel to this texture set and turn it on for this layer (one undo step)", kind.display_name()));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
            if response.clicked() { clicked = Some(ChannelAction::Add(kind)); }
        }
    });
    clicked
}

/// A fill channel's map slot, as in other painters: the channel's image as a small thumbnail, or a
/// dashed "+" box when it has none. Click to choose an image; Shelf textures and files from the
/// system file browser can be dropped on the channel's row.
fn map_slot(ui: &mut Ui, state: &mut UiState, resource: Option<subcraft_core::ResourceId>, kind: ChannelKind) -> egui::Response {
    let side = 30.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    let painter = ui.painter_at(rect);
    let thumb = resource.and_then(|id| {
        let r = state.session.resources().get(id).cloned()?;
        Some(state.image_thumbs.entry(id).or_insert_with(|| {
            let (w, h, rgba) = r.bitmap.preview_rgba8(96);
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            ui.ctx().load_texture(format!("image-{}", id.0), img, egui::TextureOptions::LINEAR)
        }).clone())
    });
    let hovered = response.hovered();
    match thumb {
        Some(tex) => {
            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, if hovered { ACCENT() } else { theme::BORDER_STRONG() }), egui::StrokeKind::Inside);
        }
        None => {
            let color = if hovered { ACCENT() } else { TEXT_DIM() };
            let r = rect.shrink(1.0);
            // Dashed outline: four dashes per side.
            for (a, b) in [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())] {
                for k in 0..4 {
                    let t0 = k as f32 / 4.0 + 0.04;
                    let t1 = t0 + 0.14;
                    painter.line_segment([a.lerp(b, t0), a.lerp(b, t1)], egui::Stroke::new(1.0, color));
                }
            }
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "+", theme::font_strong(16.0), color);
        }
    }
    let tip = if resource.is_some() { format!("{} image · Click to replace, or drop a texture on this row", kind.display_name()) }
        else { format!("No {} image · Click to choose one, or drop a texture (from the Shelf or your files) on this row", kind.display_name()) };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} map", kind.display_name())));
    response.on_hover_text(tip)
}

pub(crate) fn image_row(ui: &mut Ui, state: &mut UiState, resource: subcraft_core::ResourceId) -> bool {
    let Some(r) = state.session.resources().get(resource).cloned() else {
        ui.label(RichText::new(format!("{}  Image missing from the project", icons::WARNING)).color(theme::WARNING()));
        return secondary_button(ui, tr("Choose image…")).clicked();
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
            ui.label(RichText::new(&r.name).color(theme::TEXT()));
            ui.label(RichText::new(format!("{} × {}", r.bitmap.width, r.bitmap.height)).color(TEXT_DIM()).font(theme::font(11.0)));
            clicked = secondary_button(ui, tr("Replace…")).clicked();
        });
    });
    clicked
}

/// Painter-style block title: bold capitals, no caret.
pub(crate) fn block_header(ui: &mut Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text.to_uppercase()).font(theme::font_strong(12.0)).color(theme::TEXT()));
    ui.add_space(4.0);
}

/// Labelled dropdown row (label column, then the combo filling the row). `options` is
/// (value, label); a disabled row shows the current text and nothing to pick.
fn dropdown_row<T: PartialEq + Copy>(ui: &mut Ui, id: &str, label: &str, value: &mut T, options: &[(T, &str)], enabled: bool, why_disabled: &str) -> bool {
    let mut changed = false;
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    prop_row(ui, label, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let r = egui::ComboBox::from_id_salt(("fill_dd", id)).width(ui.available_width().max(60.0)).selected_text(current).show_ui(ui, |ui| {
                for (v, name) in options {
                    if ui.selectable_value(value, *v, *name).changed() { changed = true; }
                }
            });
            if !enabled && !why_disabled.is_empty() { r.response.on_disabled_hover_text(why_disabled); }
        });
    });
    changed
}

/// The rows Painter shows between the Projection dropdown and the UV transformations group:
/// Filtering (every projection), UV Wrap (UV and planar) and Shape crop (tri-planar and planar).
pub(crate) fn projection_options(ui: &mut Ui, proj: &mut Projection) -> bool {
    use subcraft_doc::{FillFiltering, ShapeCrop, UvWrap};
    let mut changed = false;
    changed |= dropdown_row(ui, "filtering", tr("Filtering"), &mut proj.filtering, &FillFiltering::ALL.map(|f| (f, f.label())), true, "");
    if proj.mode != ProjectionMode::Triplanar {
        changed |= dropdown_row(ui, "wrap", tr("UV Wrap"), &mut proj.uv_wrap, &UvWrap::ALL.map(|w| (w, w.label())), true, "");
    }
    if proj.mode != ProjectionMode::Uv {
        changed |= dropdown_row(ui, "crop", tr("Shape crop"), &mut proj.shape_crop, &ShapeCrop::ALL.map(|c| (c, c.label())), true, "");
    }
    changed
}

/// Warp projection: grid size (while no point was moved, as in Painter), the split tools and
/// a reset. The points themselves are edited in the 3D view.
pub(crate) fn warp_rows(ui: &mut Ui, proj: &mut Projection) -> bool {
    use subcraft_doc::{MAX_WARP_SIDE, WarpGrid};
    if proj.mode != ProjectionMode::Warp {
        return false;
    }
    let axis = proj.planar_axis.min(2);
    let mut grid = proj.warp_grid();
    let before = grid;
    let max = (MAX_WARP_SIDE - 1) as u8;
    let flat = grid.is_flat(axis);
    prop_row(ui, "Grid", |ui| {
        let (mut cols, mut rows) = (grid.cols(), grid.rows());
        ui.add_enabled_ui(flat, |ui| {
            let c = ui.add(egui::DragValue::new(&mut cols).range(1..=max).prefix("columns ")).on_disabled_hover_text("Fixed once a point was moved or the grid was split; Reset grid to change it");
            let r = ui.add(egui::DragValue::new(&mut rows).range(1..=max).prefix("rows "));
            if c.changed() || r.changed() {
                grid = WarpGrid::planar(axis, cols, rows);
            }
        });
    });
    prop_row(ui, "Split", |ui| {
        let (mid_c, mid_r) = (grid.cols() as usize / 2, grid.rows() as usize / 2);
        if ui.small_button("Crosswise").on_hover_text("A row and a column through the middle cells").clicked() {
            grid.split_column(mid_c);
            grid.split_row(mid_r);
        }
        if ui.small_button("Horizontally").on_hover_text("A row through the middle cells").clicked() {
            grid.split_row(mid_r);
        }
        if ui.small_button("Vertically").on_hover_text("A column through the middle cells").clicked() {
            grid.split_column(mid_c);
        }
    });
    let mut changed = false;
    if grid != before {
        proj.warp = Some(grid);
        changed = true;
    }
    if proj.warp.is_some() && ui.small_button("Reset grid").on_hover_text("Back to the flat grid of the planar projection").clicked() {
        proj.warp = None;
        changed = true;
    }
    ui.label(RichText::new("Drag the grid points in the 3D view · Shift+V: Transform warp / Edit vertices").small().color(TEXT_DIM()));
    changed
}

/// Cylindrical projection: the Angle row (how much of the perimeter one tile covers).
fn wrapped_projection_rows(ui: &mut Ui, proj: &mut Projection) -> bool {
    if proj.mode != ProjectionMode::Cylindrical {
        return false;
    }
    slider_row(ui, tr("Angle"), egui::Slider::new(&mut proj.cylinder_angle, 1.0..=360.0).suffix("\u{00b0}"))
        .on_hover_text(tr("How much of the perimeter the texture covers once"))
        .changed()
}

/// Painter's "3D projection" group: Offset, Rotation and Scale of the projection box, for the
/// 3D projections that have one (tri-planar, planar, spherical, cylindrical).
fn transform3_group(ui: &mut Ui, proj: &mut Projection) -> bool {
    use subcraft_doc::ProjectionTransform as T;
    if !proj.has_transform3() {
        return false;
    }
    let before = proj.transform3;
    fn row(ui: &mut Ui, label: &str, values: &mut [f32; 3], speed: f64, range: std::ops::RangeInclusive<f32>, suffix: &str, hover: &str) {
        prop_row(ui, label, |ui| {
            for v in values.iter_mut() {
                ui.add(egui::DragValue::new(v).speed(speed).range(range.clone()).max_decimals(3).suffix(suffix)).on_hover_text(hover);
            }
        });
    }
    egui::CollapsingHeader::new(RichText::new(tr("3D projection")).font(theme::font_strong(12.0)).color(theme::TEXT())).id_salt("fill_transform3").default_open(false).show(ui, |ui| {
        let t = &mut proj.transform3;
        row(ui, tr("Offset"), &mut t.offset, 0.005, -T::MAX_OFFSET..=T::MAX_OFFSET, "", tr("X, Y, Z in units of the model's bounding box; 0 is centred"));
        row(ui, tr("Rotation"), &mut t.rotation_deg, 0.5, -360.0..=360.0, "\u{00b0}", tr("Degrees about X, Y, Z (applied in that order)"));
        row(ui, tr("Scale"), &mut t.scale, 0.005, T::MIN_SCALE..=T::MAX_SCALE, "", tr("Size of the projection box on X, Y, Z; with Shape crop on, a planar projection reaches as deep as its scale along the projection axis"));
        if ui.add_enabled(!t.is_identity(), egui::Button::new(tr("Reset")).small()).clicked() {
            *t = T::default();
        }
    });
    proj.transform3 != before
}

/// Painter's Backface culling of a planar or cylindrical projection: the toggle, the Angle
/// (planar) and the Hardness.
fn backface_rows(ui: &mut Ui, proj: &mut Projection) -> bool {
    if !matches!(proj.mode, ProjectionMode::Planar | ProjectionMode::Cylindrical) {
        return false;
    }
    let before = proj.backface;
    let mut on = proj.backface.is_some();
    prop_row(ui, "", |ui| {
        if ui.checkbox(&mut on, tr("Backface culling")).on_hover_text(tr("Project only on the surfaces that face the projection")).changed() {
            proj.backface = on.then(subcraft_doc::BackfaceCulling::default);
        }
    });
    let planar = proj.mode == ProjectionMode::Planar;
    if let Some(b) = &mut proj.backface {
        if planar {
            slider_row(ui, tr("Culling angle"), egui::Slider::new(&mut b.angle_deg, 0.0..=180.0).suffix("\u{00b0}"))
                .on_hover_text(tr("Surfaces turned further than this from the projection get nothing"));
        }
        slider_row(ui, tr("Culling hardness"), egui::Slider::new(&mut b.hardness, 0.0..=1.0))
            .on_hover_text(tr("1 cuts at the angle; lower values fade the projection out before it"));
    }
    proj.backface != before
}

/// Painter's Symmetry group of a fill projection (3D projections only): Enable symmetry, the
/// type (Mirror / Radial) and its settings.
fn symmetry_group(ui: &mut Ui, proj: &mut Projection) -> bool {
    use subcraft_doc::FillSymmetry;
    let before = proj.symmetry;
    let enabled = proj.is_3d();
    egui::CollapsingHeader::new(RichText::new(tr("Symmetry")).font(theme::font_strong(12.0)).color(theme::TEXT())).id_salt("fill_symmetry").default_open(false).show(ui, |ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let mut on = !proj.symmetry.is_none();
            let r = ui.checkbox(&mut on, tr("Enable symmetry"));
            if !enabled {
                r.on_disabled_hover_text(tr("Symmetry needs a 3D projection (not UV)"));
            } else if r.changed() {
                proj.symmetry = if on { FillSymmetry::mirror() } else { FillSymmetry::None };
            }
            if proj.symmetry.is_none() {
                return;
            }
            let mut radial = matches!(proj.symmetry, FillSymmetry::Radial { .. });
            prop_row(ui, tr("Symmetry type"), |ui| {
                let a = ui.selectable_value(&mut radial, false, tr("Mirror")).changed();
                let b = ui.selectable_value(&mut radial, true, tr("Radial")).changed();
                if a || b {
                    proj.symmetry = if radial { FillSymmetry::radial() } else { FillSymmetry::mirror() };
                }
            });
            match &mut proj.symmetry {
                FillSymmetry::Mirror { axis, position } => {
                    prop_row(ui, tr("Axis"), |ui| { segmented(ui, axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]); });
                    slider_row(ui, tr("Axis position"), egui::Slider::new(position, 0.0..=1.0));
                }
                FillSymmetry::Radial { axis, count, flip, center } => {
                    prop_row(ui, tr("Axis"), |ui| { segmented(ui, axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]); });
                    slider_row(ui, tr("Copy count"), egui::Slider::new(count, 1..=FillSymmetry::MAX_COPIES).logarithmic(true));
                    prop_row(ui, "", |ui| { ui.checkbox(flip, tr("Flip copy")).on_hover_text(tr("Mirror every other copy")); });
                    for (i, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                        slider_row(ui, &format!("{} {name}", tr("Axis position")), egui::Slider::new(&mut center[i], 0.0..=1.0));
                    }
                }
                FillSymmetry::None => {}
            }
        });
    });
    proj.symmetry != before
}

/// Tri-planar Hardness 0..1 (1 = hard cut between the three planes), stored as the blend exponent.
fn hardness_row(ui: &mut Ui, proj: &mut Projection) -> bool {
    let mut hardness = proj.hardness();
    let changed = slider_row(ui, tr("Hardness"), egui::Slider::new(&mut hardness, 0.0..=1.0).max_decimals(3))
        .on_hover_text(tr("How sharply the three projection planes meet: 0 blends them widely, 1 cuts"))
        .changed();
    if changed {
        proj.set_hardness(hardness);
    }
    changed
}

/// The FILL block of Painter's Properties: Projection / Filtering / UV Wrap / Shape crop /
/// Hardness rows and the collapsible UV transformations group. The Scale mode (Physical Size)
/// does not exist in the engine yet and is shown disabled.
pub(crate) fn fill_properties(ui: &mut Ui, proj: &mut Projection) -> bool {
    let mut changed = false;
    changed |= dropdown_row(ui, "projection", tr("Projection"), &mut proj.mode,
        &[(ProjectionMode::Uv, "UV projection"), (ProjectionMode::Triplanar, "Tri-planar projection"), (ProjectionMode::Planar, "Planar projection"),
            (ProjectionMode::Spherical, "Spherical projection"), (ProjectionMode::Cylindrical, "Cylindrical projection"), (ProjectionMode::Warp, "Warp projection")], true, "");
    changed |= projection_options(ui, proj);
    changed |= warp_rows(ui, proj);
    if proj.mode == ProjectionMode::Triplanar {
        changed |= hardness_row(ui, proj);
    }
    changed |= wrapped_projection_rows(ui, proj);
    changed |= transform3_group(ui, proj);
    changed |= backface_rows(ui, proj);
    changed |= symmetry_group(ui, proj);
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new(tr("UV transformations")).font(theme::font_strong(12.0)).color(theme::TEXT())).default_open(true).show(ui, |ui| {
        let mut scale_mode = 0u8;
        dropdown_row(ui, "scale", tr("Scale"), &mut scale_mode, &[(0u8, "Tiling")], false, "Only tiling is supported");
        let lock_id = ui.id().with("tiling_lock");
        let mut locked = ui.data(|d| d.get_temp::<bool>(lock_id)).unwrap_or(proj.scale[0] == proj.scale[1]);
        ui.add(egui::Label::new(crate::widgets::row_label(tr("Tiling"))));
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.x = VALUE_W; ui.spacing_mut().button_padding.x = 4.0;
            let sp = ui.spacing().item_spacing.x;
            let w = ((ui.available_width() - 26.0 - 2.0 * sp) / 2.0 - sp - VALUE_W).max(30.0);
            ui.spacing_mut().slider_width = w;
            let mut sx = proj.scale[0];
            if ui.add(egui::Slider::new(&mut sx, 0.01..=16.0).clamping(egui::SliderClamping::Never).max_decimals(2)).changed() {
                proj.scale[0] = sx;
                if locked { proj.scale[1] = sx; }
                changed = true;
            }
            let lock = ui.add(egui::Button::new(RichText::new(if locked { icons::CHECK } else { icons::X }).font(theme::font(12.0))).selected(locked).min_size(egui::vec2(26.0, 24.0)))
                .on_hover_text(tr("Same tiling for X and Y; click to set them separately"));
            if lock.clicked() {
                locked = !locked;
                if locked && proj.scale[1] != proj.scale[0] { proj.scale[1] = proj.scale[0]; changed = true; }
            }
            let mut sy = proj.scale[1];
            if ui.add_enabled(!locked, egui::Slider::new(&mut sy, 0.01..=16.0).clamping(egui::SliderClamping::Never).max_decimals(2)).changed() {
                proj.scale[1] = sy;
                changed = true;
            }
        });
        ui.data_mut(|d| d.insert_temp(lock_id, locked));
        changed |= slider_row(ui, tr("Rotation"), egui::Slider::new(&mut proj.rotation_deg, -180.0..=180.0).suffix("°")).changed();
        // Painter has no UV Offset for tri-planar (the projection moves in 3D instead).
        if proj.mode != ProjectionMode::Triplanar {
            ui.add(egui::Label::new(crate::widgets::row_label(tr("Offset"))));
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = VALUE_W; ui.spacing_mut().button_padding.x = 4.0;
                let sp = ui.spacing().item_spacing.x;
                ui.spacing_mut().slider_width = ((ui.available_width() - sp) / 2.0 - sp - VALUE_W).max(30.0);
                changed |= ui.add(egui::Slider::new(&mut proj.offset[0], -1.0..=1.0).clamping(egui::SliderClamping::Never).max_decimals(3)).changed();
                changed |= ui.add(egui::Slider::new(&mut proj.offset[1], -1.0..=1.0).clamping(egui::SliderClamping::Never).max_decimals(3)).changed();
            });
        }
        if matches!(proj.mode, ProjectionMode::Planar | ProjectionMode::Spherical | ProjectionMode::Cylindrical | ProjectionMode::Warp) {
            prop_row(ui, tr("Axis"), |ui| { changed |= segmented(ui, &mut proj.planar_axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]); });
        }
        let default = reset_transform(proj);
        if *proj != default && ui.small_button(tr("Reset tiling, offset and rotation")).clicked() { *proj = default; changed = true; }
    });
    changed
}

/// `proj` with its tiling, rotation and offset back at their defaults; the projection mode and
/// its options (axis, filtering, wrap, crop, hardness) are kept.
fn reset_transform(proj: &Projection) -> Projection {
    let d = Projection::default();
    Projection { scale: d.scale, rotation_deg: d.rotation_deg, offset: d.offset, ..*proj }
}

/// The MATERIAL block's channel buttons in Painter's order and lower-case short names, plus the
/// material-mode box beneath them (disabled: SubCraft has no material resource slot yet).
pub(crate) fn material_buttons(ui: &mut Ui, states: &[(ChannelKind, bool)], addable: &[ChannelKind]) -> Option<ChannelAction> {
    const ORDER: [ChannelKind; 7] = [ChannelKind::BaseColor, ChannelKind::Metallic, ChannelKind::Roughness, ChannelKind::Normal, ChannelKind::Height, ChannelKind::AmbientOcclusion, ChannelKind::Emissive];
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(5.0, 6.0);
        // The usual seven first, then every other channel the texture set has (Opacity and the
        // rest), so each one can be toggled on a fill layer.
        let extra = states.iter().map(|s| s.0).filter(|k| !ORDER.contains(k));
        for kind in ORDER.into_iter().chain(extra) {
            let state = states.iter().find(|s| s.0 == kind).map(|s| s.1);
            let can_add = state.is_none() && addable.contains(&kind);
            let r = ui.add_enabled(state.is_some() || can_add, egui::Button::new(RichText::new(kind.short_name()).font(theme::font(12.0)).color(if state == Some(true) { theme::TEXT() } else { theme::TEXT_DIM() }))
                .selected(state == Some(true)).min_size(egui::vec2(38.0, 22.0)))
                .on_hover_text(format!("{} · Click: toggle · Alt+click: only this channel", kind.display_name()));
            if r.clicked() {
                clicked = Some(if state.is_some() { ChannelAction::Toggle(kind, ui.input(|i| i.modifiers.alt)) } else { ChannelAction::Add(kind) });
            }
        }
    });
    ui.add_space(6.0);
    egui::Frame::new().fill(theme::BG_2()).stroke(egui::Stroke::new(1.0, theme::BORDER())).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Material mode").font(theme::font_strong(12.0)).color(theme::TEXT()));
            ui.label(RichText::new("No Resource Selected").font(theme::font(11.5)).color(theme::TEXT_DIM()));
        });
    }).response.on_hover_text("Material resources are not supported yet: each channel below has its own source");
    ui.add_space(4.0);
    clicked
}

/// The modes [`projection_editor`] offers when the projection is in `current`. Warp's grid is
/// only edited in the 3D view for a fill layer's own projection; on masks, fill effects and the
/// brush material it is listed only while already set (so the panel shows it and can leave it).
pub(crate) fn editor_modes(current: ProjectionMode) -> Vec<(ProjectionMode, &'static str)> {
    let mut modes = vec![(ProjectionMode::Uv, "UV"), (ProjectionMode::Triplanar, "Tri-planar"), (ProjectionMode::Planar, "Planar"), (ProjectionMode::Spherical, "Sphere"), (ProjectionMode::Cylindrical, "Cylinder")];
    if current == ProjectionMode::Warp {
        modes.push((ProjectionMode::Warp, "Warp"));
    }
    modes
}

/// Mode / scale / rotation (+ sharpness or axis) editor shared by image masks, fill effects and
/// the brush material (fill layers use [`fill_properties`]).
pub(crate) fn projection_editor(ui: &mut Ui, proj: &mut Projection) -> bool {
    let mut changed = false;
    prop_row(ui, tr("Mode"), |ui| {
        let modes = editor_modes(proj.mode);
        changed |= segmented(ui, &mut proj.mode, &modes);
    });
    // Tiling: X and Y linked by default (lock), separate when unlocked. The lock is UI state,
    // remembered per editor; it starts unlocked if the two values already differ.
    let lock_id = ui.id().with("tiling_lock");
    let mut locked = ui.data(|d| d.get_temp::<bool>(lock_id)).unwrap_or(proj.scale[0] == proj.scale[1]);
    prop_row(ui, tr("Tiling"), |ui| {
        fn tiling(v: &mut f32) -> egui::DragValue<'_> {
            egui::DragValue::new(v).speed(0.05).range(0.01..=256.0).max_decimals(2)
        }
        if locked {
            if ui.add(tiling(&mut proj.scale[0])).on_hover_text(tr("Repeats across the surface (X and Y together)")).changed() {
                proj.scale[1] = proj.scale[0];
                changed = true;
            }
        } else {
            ui.label(RichText::new("X").color(TEXT_DIM()));
            changed |= ui.add(tiling(&mut proj.scale[0])).changed();
            ui.label(RichText::new("Y").color(TEXT_DIM()));
            changed |= ui.add(tiling(&mut proj.scale[1])).changed();
        }
        if ui.checkbox(&mut locked, tr("Link")).on_hover_text(tr("Same tiling for X and Y; untick to set them separately")).changed() {
            if locked && proj.scale[1] != proj.scale[0] {
                proj.scale[1] = proj.scale[0];
                changed = true;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(lock_id, locked));
    changed |= slider_row(ui, tr("Rotation"), egui::Slider::new(&mut proj.rotation_deg, -180.0..=180.0).suffix("°")).changed();
    if proj.mode != ProjectionMode::Triplanar {
        prop_row(ui, tr("Offset"), |ui| {
            fn offset(v: &mut f32) -> egui::DragValue<'_> {
                egui::DragValue::new(v).speed(0.005).range(-16.0..=16.0).max_decimals(3)
            }
            ui.label(RichText::new("X").color(TEXT_DIM()));
            changed |= ui.add(offset(&mut proj.offset[0])).changed();
            ui.label(RichText::new("Y").color(TEXT_DIM()));
            changed |= ui.add(offset(&mut proj.offset[1])).changed();
        });
    }
    if proj.mode == ProjectionMode::Triplanar {
        changed |= hardness_row(ui, proj);
    }
    if matches!(proj.mode, ProjectionMode::Planar | ProjectionMode::Spherical | ProjectionMode::Cylindrical | ProjectionMode::Warp) {
        prop_row(ui, tr("Axis"), |ui| {
            changed |= segmented(ui, &mut proj.planar_axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]);
        });
    }
    changed |= wrapped_projection_rows(ui, proj);
    changed |= transform3_group(ui, proj);
    changed |= backface_rows(ui, proj);
    changed |= symmetry_group(ui, proj);
    let default = reset_transform(proj);
    if *proj != default && ui.small_button(tr("Reset tiling, offset and rotation")).clicked() {
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
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420., 1000.))), events, ..Default::default() }, |ui| effect_editor(ui, state, layer, &effect, &mut commands));
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
    /// Each generator kind shows the Painter controls it uses; Spread is offered for every Edges
    /// generator (also one made without natural wear, like the smart materials' chip lines).
    #[test]
    fn generator_controls_follow_the_kind_and_spread_is_always_offered_for_edges() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let labels = |g: Generator| -> Vec<String> {
            let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
            let layer = state.session.selection().layer.unwrap();
            state.session.execute(Command::AddMask { layer, white: false }).unwrap();
            state.session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
            let effect = state.session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].clone();
            let mut commands = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420., 1400.))), ..Default::default() }, |ui| effect_editor(ui, &mut state, layer, &effect, &mut commands));
            out.textures_delta.clear();
            assert!(commands.is_empty(), "drawing alone must not edit the generator");
            out.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_string()), _ => None }).collect()
        };
        let has = |l: &[String], s: &str| l.iter().any(|t| t == s);
        let dirt = labels(Generator::new(GeneratorKind::Cavity));
        for s in ["Dirt Level", "Dirt Contrast", "Grunge Amount", "Edges Masking", "Grunge Floor", "Grunge map", "Grain"] { assert!(has(&dirt, s), "Dirt lacks {s}"); }
        for s in ["Spread", "Ambient Occlusion Masking", "Curvature Weight", "Edges Smoothness", "Level", "Softness"] { assert!(!has(&dirt, s), "Dirt shows {s}"); }
        let wear = labels(Generator::new(GeneratorKind::Edges));
        for s in ["Wear Level", "Wear Contrast", "Grunge Amount", "Spread", "Variation", "Ambient Occlusion Masking", "Curvature Weight", "Edges Smoothness", "Grunge map", "Grain"] { assert!(has(&wear, s), "Edges lacks {s}"); }
        assert!(!has(&wear, "Edges Masking"));
        let ao = labels(Generator::new(GeneratorKind::AmbientOcclusion));
        for s in ["Balance", "Contrast", "Breakup"] { assert!(has(&ao, s), "AO lacks {s}"); }
        for s in ["Grunge map", "Edges Masking", "Spread"] { assert!(!has(&ao, s), "AO shows {s}"); }
        let mut chips = Generator::new(GeneratorKind::Edges);
        chips.edge_spread = None;
        let chips = labels(chips);
        assert!(has(&chips, "Spread"), "Spread must stay visible without natural wear");
        for s in ["Variation", "Ambient Occlusion Masking", "Grain"] { assert!(!has(&chips, s), "legacy Edges shows {s}, which it ignores"); }
        let top = labels(Generator::new(GeneratorKind::Directional));
        assert!(has(&top, "Grain") && has(&top, "Level") && !has(&top, "Spread") && !has(&top, "Edges Masking"));
    }
}

/// The Name field of a mask anchor point. The text being typed lives in egui memory while the
/// field has focus: rebuilt from the effect every frame, each typed character was gone by the next
/// frame and the anchor could not be renamed at all. True when a new name was committed.
pub(crate) fn anchor_name_field(ui: &mut Ui, id: subcraft_core::EffectId, name: &mut String) -> bool {
    let buffer = egui::Id::new(("mask_anchor_name", id));
    let mut text: String = ui.data(|d| d.get_temp(buffer)).unwrap_or_else(|| name.clone());
    let r = ui.text_edit_singleline(&mut text);
    if r.has_focus() { ui.data_mut(|d| d.insert_temp(buffer, text.clone())); } else { ui.data_mut(|d| d.remove_temp::<String>(buffer)); }
    if r.lost_focus() && !text.trim().is_empty() && text.trim() != name.as_str() {
        *name = text.trim().to_string();
        return true;
    }
    false
}

#[cfg(test)]
mod anchor_name_tests {
    use super::*;

    /// QA 2026-11: typing in a mask anchor's Name field did nothing.
    #[test]
    fn typing_a_name_and_pressing_enter_renames_the_anchor() {
        let ctx = egui::Context::default();
        let id = subcraft_core::EffectId::new();
        let mut name = String::from("Anchor 1");
        let mut renamed = false;
        let frame = |events: Vec<egui::Event>, name: &mut String, renamed: &mut bool| {
            let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))), ..Default::default() }, |ui| {
                *renamed |= anchor_name_field(ui, id, name);
            });
            out.textures_delta.clear();
        };
        frame(vec![], &mut name, &mut renamed);
        let at = egui::pos2(30.0, 12.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(vec![egui::Event::PointerMoved(at), press(true)], &mut name, &mut renamed);
        frame(vec![press(false)], &mut name, &mut renamed);
        frame(vec![egui::Event::Text(" edges".into())], &mut name, &mut renamed);
        frame(vec![], &mut name, &mut renamed);
        assert!(!renamed, "nothing is committed while typing");
        frame(vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }], &mut name, &mut renamed);
        frame(vec![], &mut name, &mut renamed);
        assert!(renamed, "Enter commits the typed name");
        // The text lands where the click put the cursor, so only its presence is checked.
        assert!(name.contains("edges") && name.len() == "Anchor 1 edges".len(), "{name:?}");
    }
}

#[cfg(test)]
mod material_button_tests {
    use super::*;

    #[test]
    fn fill_material_buttons_include_opacity_and_other_set_channels() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let states = [(ChannelKind::BaseColor, true), (ChannelKind::Roughness, false), (ChannelKind::Opacity, true)];
        let mut first = ctx.run_ui(Default::default(), |ui| { let _ = material_buttons(ui, &states, &[]); });
        first.textures_delta.clear();
        let mut out = ctx.run_ui(Default::default(), |ui| { let _ = material_buttons(ui, &states, &[]); });
        out.textures_delta.clear();
        let has = |t: &str| out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(g) if g.galley.text() == t));
        assert!(has(ChannelKind::Opacity.short_name()), "Opacity gets a toggle on fill layers");
        assert!(has(ChannelKind::BaseColor.short_name()));
    }
}

#[cfg(test)]
mod warp_mode_tests {
    use super::*;

    /// QA 2026-11: masks, fill effects and the brush material offered Warp, whose grid can only
    /// be edited for a fill layer's own projection: picking it left a flat grid with no editor.
    #[test]
    fn warp_is_offered_only_where_its_grid_can_be_edited() {
        for mode in [ProjectionMode::Uv, ProjectionMode::Triplanar, ProjectionMode::Planar, ProjectionMode::Spherical, ProjectionMode::Cylindrical] {
            let modes = editor_modes(mode);
            assert!(modes.iter().all(|(m, _)| *m != ProjectionMode::Warp), "{mode:?}: {modes:?}");
            assert!(modes.iter().any(|(m, _)| *m == mode), "{mode:?} is listed");
        }
        assert!(editor_modes(ProjectionMode::Warp).iter().any(|(m, _)| *m == ProjectionMode::Warp), "a Warp set elsewhere still shows");
        // Drawn: no Warp button for a mask's planar projection; fill layers keep Warp.
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut proj = Projection { mode: ProjectionMode::Planar, ..Projection::default() };
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            projection_editor(ui, &mut proj);
        });
        out.textures_delta.clear();
        let text = format!("{:?}", out.shapes);
        assert!(text.contains("Planar") && !text.contains("Warp"), "the editor draws no Warp button");
        let source = include_str!("properties.rs");
        let fill = &source[source.find("pub(crate) fn fill_properties").unwrap()..];
        assert!(fill[..fill.find("\n}").unwrap()].contains("(ProjectionMode::Warp, \"Warp projection\")"), "fill layers keep Warp");
    }
}
