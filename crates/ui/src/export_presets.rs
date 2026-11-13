//! Export preset bar and editor in the Export Textures window: pick, create, duplicate, rename,
//! delete, import and export presets, and edit a user preset's output maps (file name pattern,
//! format, bit depth, channel packing, padding, size, normal format).
//!
//! A thin layer: the model, validation, storage and Painter import live in `subcraft_export` /
//! `subcraft_app::export_presets`; this file only draws them and forwards edits.

use egui::{RichText, Ui};
use subcraft_app::export_presets::ExportPresets;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_export::{Converted, ExportTemplate, OutputMap, Padding, Source};
use subcraft_image::{BitDepth, Container, FileFormat};

use crate::i18n::tr;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::caption;

/// Editor state kept between frames.
#[derive(Default)]
pub struct PresetEditor {
    /// Key of the preset the draft belongs to.
    draft_of: String,
    /// The edited user preset; may be invalid while typing (then not saved, error shown).
    draft: Option<ExportTemplate>,
    /// Selected output map in the editor's list.
    pub map: usize,
    /// Name being typed into the preset name field.
    name_edit: Option<String>,
    pub error: Option<String>,
    /// Ask before deleting this preset id.
    confirm_delete: Option<String>,
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
}


/// Preset picker row with New / Duplicate / Delete / Import / Export. Returns a status message.
pub fn preset_bar(ui: &mut Ui, presets: &mut ExportPresets, key: &mut String, ed: &mut PresetEditor) -> Option<String> {
    let mut status = None;
    let current = presets.find(key).cloned().unwrap_or_else(|| presets.builtin()[0].clone());
    *key = current.key().to_string();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("export_preset").width(230.0).selected_text(&current.name).show_ui(ui, |ui| {
            ui.label(caption("Built-in (read-only)"));
            for p in presets.builtin() {
                if ui.selectable_label(*key == p.key(), &p.name).clicked() {
                    *key = p.key().to_string();
                }
            }
            ui.separator();
            ui.label(caption("My presets"));
            if presets.user().is_empty() {
                ui.label(dim("None yet: duplicate a built-in or click New"));
            }
            for p in presets.user() {
                if ui.selectable_label(*key == p.key(), &p.name).clicked() {
                    *key = p.key().to_string();
                }
            }
        });
        let mut select = |r: Result<ExportTemplate, String>, what: &str| match r {
            Ok(p) => {
                status = Some(format!("{what} preset \"{}\"", p.name));
                *key = p.key().to_string();
                ed.map = 0;
            }
            Err(e) => status = Some(format!("Preset: {e}")),
        };
        if ui.button(tr("New")).on_hover_text(tr("New preset with one Base Color map")).clicked() {
            select(presets.create(), "Created");
        }
        if ui.button(tr("Duplicate")).on_hover_text(tr("Copy this preset into My presets so it can be edited")).clicked() {
            let k = current.key().to_string();
            select(presets.duplicate(&k), "Created");
        }
        let deletable = !current.is_builtin();
        if ui.add_enabled(deletable, egui::Button::new(tr("Delete"))).on_disabled_hover_text(tr("Built-in templates can't be deleted")).clicked() {
            ed.confirm_delete = current.id.clone();
        }
        if ui.button(tr("Import…")).on_hover_text(tr("SubCraft preset (.json) or Substance 3D Painter export preset (.spexp / export JSON)")).clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("Export preset", &["json", "spexp"]).pick_file()
        {
            match presets.import(&path) {
                Ok((added, warnings)) => {
                    if let Some(p) = added.first() {
                        *key = p.key().to_string();
                        ed.map = 0;
                    }
                    let names: Vec<&str> = added.iter().map(|p| p.name.as_str()).collect();
                    status = Some(if warnings.is_empty() { format!("Imported {}", names.join(", ")) } else { format!("Imported {} ({} note(s): {})", names.join(", "), warnings.len(), warnings[0]) });
                }
                Err(e) => status = Some(format!("Could not import preset: {e}")),
            }
        }
        if ui.button(tr("Export…")).on_hover_text(tr("Save this preset as a JSON file")).clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("SubCraft export preset", &["json"]).set_file_name(format!("{}.json", subcraft_export::sanitize(&current.name))).save_file()
        {
            status = Some(match presets.export_to(key, &path) {
                Ok(()) => format!("Saved preset to {}", path.display()),
                Err(e) => format!("Could not save preset: {e}"),
            });
        }
    });
    if let Some(id) = ed.confirm_delete.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Delete preset \"{}\"? This removes its file from the library.", current.name)).color(theme::WARNING));
            if ui.button(tr("Delete")).clicked() {
                status = Some(match presets.delete(&id) {
                    Ok(()) => {
                        *key = presets.builtin()[0].key().to_string();
                        format!("Deleted preset \"{}\"", current.name)
                    }
                    Err(e) => format!("Preset: {e}"),
                });
                ed.confirm_delete = None;
            }
            if ui.button(tr("Cancel")).clicked() {
                ed.confirm_delete = None;
            }
        });
    }
    status
}

/// The editor for the selected preset (read-only for built-ins). `mesh` names the preview file.
pub fn preset_editor(ui: &mut Ui, presets: &mut ExportPresets, key: &str, ed: &mut PresetEditor, mesh: &str) -> Option<String> {
    let current = presets.find(key).cloned()?;
    let editable = !current.is_builtin();
    if ed.draft_of != key || !editable {
        ed.draft = editable.then(|| current.clone());
        ed.draft_of = key.to_string();
        ed.name_edit = None;
        ed.error = None;
    }
    let mut draft = ed.draft.clone().unwrap_or_else(|| current.clone());
    let mut status = None;
    if !editable {
        ui.label(dim("Built-in templates are read-only: Duplicate to make an editable copy."));
    }
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new("preset_opts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(dim("Name"));
            let name = ed.name_edit.get_or_insert_with(|| current.name.clone());
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(240.0));
            if editable && r.lost_focus() && *name != current.name {
                match presets.rename(key, name) {
                    Ok(p) => {
                        status = Some(format!("Renamed preset to \"{}\"", p.name));
                        draft.name = p.name.clone();
                        ed.name_edit = Some(p.name);
                    }
                    Err(e) => ed.error = Some(e),
                }
            }
            ui.end_row();
            ui.label(dim("Padding"));
            padding_combo(ui, "preset_padding", &mut draft.padding, Some("Export window setting"));
            ui.end_row();
            ui.label(dim("Size"));
            size_combo(ui, "preset_size", &mut draft.size, "Texture set size");
            ui.end_row();
            ui.label(dim("Normal maps"));
            normal_combo(ui, "preset_normal", &mut draft.normal_directx, "Per map");
            ui.end_row();
        });
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        // Output map list.
        ui.vertical(|ui| {
            ui.set_width(220.0);
            ui.label(caption(&format!("Output maps ({})", draft.maps.len())));
            egui::Frame::new().fill(theme::FIELD).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("preset_maps").max_height(300.0).show(ui, |ui| {
                    ui.set_min_width(212.0);
                    for (i, m) in draft.maps.iter().enumerate() {
                        let text = RichText::new(format!("{}  ·  {}", m.suffix, m.format.label()));
                        let r = ui.selectable_label(ed.map == i, text).on_hover_text(packing_summary(m));
                        if r.clicked() {
                            ed.map = i;
                        }
                    }
                });
            });
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(tr("+ Map")).on_hover_text(tr("Add an RGB map")).clicked() {
                        let taken: Vec<&str> = draft.maps.iter().map(|m| m.suffix.as_str()).collect();
                        let suffix = (1..).map(|n| format!("Map{n}")).find(|s| !taken.contains(&s.as_str())).expect("unbounded");
                        draft.maps.push(OutputMap { pattern: Some("$mesh_$textureSet_$suffix".into()), ..OutputMap::new(&suffix, FileFormat::Png8, vec![Source::ch(ChannelKind::BaseColor, 0), Source::ch(ChannelKind::BaseColor, 1), Source::ch(ChannelKind::BaseColor, 2)], true) });
                        ed.map = draft.maps.len() - 1;
                    }
                    let has = ed.map < draft.maps.len();
                    if ui.add_enabled(has, egui::Button::new(tr("Copy")).small()).clicked() {
                        let mut m = draft.maps[ed.map].clone();
                        let taken: Vec<&str> = draft.maps.iter().map(|m| m.suffix.as_str()).collect();
                        m.suffix = (2..).map(|n| format!("{}{n}", m.suffix)).find(|s| !taken.contains(&s.as_str())).expect("unbounded");
                        draft.maps.insert(ed.map + 1, m);
                        ed.map += 1;
                    }
                    if ui.add_enabled(has, egui::Button::new(tr("Remove")).small()).clicked() {
                        draft.maps.remove(ed.map);
                        ed.map = ed.map.saturating_sub(1);
                    }
                    if ui.add_enabled(has && ed.map > 0, egui::Button::new(tr("Up")).small()).clicked() {
                        draft.maps.swap(ed.map, ed.map - 1);
                        ed.map -= 1;
                    }
                    if ui.add_enabled(has && ed.map + 1 < draft.maps.len(), egui::Button::new(tr("Down")).small()).clicked() {
                        draft.maps.swap(ed.map, ed.map + 1);
                        ed.map += 1;
                    }
                });
            });
        });
        ui.separator();
        // Selected map.
        ui.vertical(|ui| {
            ed.map = ed.map.min(draft.maps.len().saturating_sub(1));
            let Some(map) = draft.maps.get_mut(ed.map) else {
                ui.label(dim("No output maps: add one with + Map."));
                return;
            };
            ui.add_enabled_ui(editable, |ui| map_editor(ui, map, mesh));
        });
    });
    if let Some(e) = &ed.error {
        ui.label(RichText::new(e).color(theme::WARNING));
    }
    if editable && Some(&draft) != ed.draft.as_ref() {
        draft.name = current.name.clone();
        ed.draft = Some(draft.clone());
        match presets.update(draft) {
            Ok(_) => ed.error = None,
            Err(e) => ed.error = Some(format!("Not saved: {e}")),
        }
    }
    status
}

fn packing_summary(m: &OutputMap) -> String {
    let slots = slot_names(m.components.len());
    m.components.iter().zip(slots).map(|(s, n)| format!("{n} = {}", s.label())).collect::<Vec<_>>().join("\n")
}

fn slot_names(n: usize) -> &'static [&'static str] {
    match n {
        1 => &["L"],
        2 => &["L", "A"],
        3 => &["R", "G", "B"],
        _ => &["R", "G", "B", "A"],
    }
}

fn map_editor(ui: &mut Ui, map: &mut OutputMap, mesh: &str) {
    egui::Grid::new("preset_map").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(dim("Suffix"));
        ui.add(egui::TextEdit::singleline(&mut map.suffix).desired_width(200.0));
        ui.end_row();
        ui.label(dim("File name"));
        ui.vertical(|ui| {
            let mut pattern = map.pattern.clone().unwrap_or_else(|| "$mesh_$textureSet_$suffix".into());
            if ui.add(egui::TextEdit::singleline(&mut pattern).desired_width(260.0).font(egui::TextStyle::Monospace)).on_hover_text(tr("Tokens: $mesh, $textureSet, $udim, $suffix")).changed() {
                map.pattern = Some(pattern.clone());
            }
            let name = subcraft_export::expand_pattern(&pattern, mesh, "TextureSet", None, &map.suffix);
            ui.label(RichText::new(format!("{name}.{}", map.format.extension())).monospace().small().color(TEXT_DIM));
        });
        ui.end_row();
        ui.label(dim("Format"));
        ui.horizontal(|ui| {
            let mut format = Some(map.format);
            format_combo(ui, "map", &mut format, &mut map.dither, None);
            map.format = format.unwrap_or(map.format);
        });
        ui.end_row();
        ui.label(dim("Channels"));
        ui.horizontal(|ui| {
            let mut n = map.components.len();
            for (v, label) in [(1, "Gray"), (2, "Gray+A"), (3, "RGB"), (4, "RGBA")] {
                ui.selectable_value(&mut n, v, label);
            }
            if n != map.components.len() {
                resize_components(&mut map.components, n);
            }
            ui.checkbox(&mut map.srgb, "sRGB color").on_hover_text(tr("Encode color channels (Base Color, Emissive) to sRGB in 8/16-bit files; float files stay linear"));
        });
        ui.end_row();
        ui.label(dim("Padding"));
        padding_combo(ui, "map_padding", &mut map.padding, Some("Preset setting"));
        ui.end_row();
        ui.label(dim("Size"));
        size_combo(ui, "map_size", &mut map.size, "Preset setting");
        ui.end_row();
        ui.label(dim("Normal"));
        normal_combo(ui, "map_normal", &mut map.normal_directx, "Preset setting");
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(caption("Channel packing"));
    egui::Grid::new("preset_packing").num_columns(5).spacing([6.0, 4.0]).show(ui, |ui| {
        let names = slot_names(map.components.len());
        for (i, src) in map.components.iter_mut().enumerate() {
            ui.label(RichText::new(names[i]).strong().color(slot_color(names[i])));
            source_editor(ui, i, src);
            ui.end_row();
        }
    });
    if map.format == FileFormat::Jpeg && map.components.len() == 4 {
        ui.label(dim("JPEG has no alpha: A is dropped."));
    }
}

fn slot_color(slot: &str) -> egui::Color32 {
    match slot {
        "R" => egui::Color32::from_rgb(230, 110, 110),
        "G" => egui::Color32::from_rgb(120, 200, 120),
        "B" => egui::Color32::from_rgb(120, 150, 240),
        _ => TEXT_DIM,
    }
}

fn resize_components(c: &mut Vec<Source>, n: usize) {
    match (c.len(), n) {
        // Gray(+A) → RGB(A): copy the gray source into R, G and B.
        (1 | 2, 3 | 4) => {
            let alpha = (c.len() == 2).then(|| c[1]);
            let g = c[0];
            *c = vec![g, g, g];
            if n == 4 {
                c.push(alpha.unwrap_or(Source::Constant { value: 1.0 }));
            }
        }
        // RGB(A) → Gray(+A): keep R (and A).
        (3 | 4, 1 | 2) => {
            let alpha = (c.len() == 4).then(|| c[3]);
            c.truncate(1);
            if n == 2 {
                c.push(alpha.unwrap_or(Source::Constant { value: 1.0 }));
            }
        }
        _ => c.resize(n, Source::Constant { value: 1.0 }),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Channel,
    Gray,
    Normal,
    Height,
    Baked,
    Converted,
    Constant,
    View,
    Alpha,
}

impl Kind {
    fn of(s: &Source) -> Self {
        match s {
            Source::Channel { .. } => Kind::Channel,
            Source::Gray { .. } => Kind::Gray,
            Source::Normal { .. } => Kind::Normal,
            Source::Height => Kind::Height,
            Source::MeshMap { .. } => Kind::Baked,
            Source::Converted { .. } => Kind::Converted,
            Source::Constant { .. } => Kind::Constant,
            Source::View2d { .. } => Kind::View,
            Source::Alpha => Kind::Alpha,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Channel => "Channel",
            Kind::Gray => "Grayscale",
            Kind::Normal => "Normal",
            Kind::Height => "Height",
            Kind::Baked => "Mesh map",
            Kind::Converted => "Converted",
            Kind::Constant => "Constant",
            Kind::View => "2D view",
            Kind::Alpha => "Alpha",
        }
    }

    fn default_source(self, slot: usize) -> Source {
        let c = slot.min(2) as u8;
        match self {
            Kind::Channel => Source::ch(ChannelKind::Roughness, 0),
            Kind::Gray => Source::Gray { kind: ChannelKind::BaseColor, invert: false },
            Kind::Normal => Source::Normal { component: c, directx: false },
            Kind::Height => Source::Height,
            Kind::Baked => Source::MeshMap { kind: MeshMapKind::AmbientOcclusion, component: 0, invert: false },
            Kind::Converted => Source::Converted { map: Converted::Glossiness, component: 0 },
            Kind::Constant => Source::Constant { value: if slot == 3 { 1.0 } else { 0.0 } },
            Kind::View => Source::View2d { component: c },
            Kind::Alpha => Source::Alpha,
        }
    }
}

fn component_combo(ui: &mut Ui, id: (usize, &str), component: &mut u8, count: usize) {
    const C: [&str; 4] = ["R", "G", "B", "A"];
    if count <= 1 {
        *component = 0;
        ui.label(dim("—"));
        return;
    }
    egui::ComboBox::from_id_salt(id).width(44.0).selected_text(C[(*component as usize).min(3)]).show_ui(ui, |ui| {
        for (i, c) in C.iter().enumerate().take(count) {
            ui.selectable_value(component, i as u8, *c);
        }
    });
}

fn source_editor(ui: &mut Ui, slot: usize, src: &mut Source) {
    let mut kind = Kind::of(src);
    egui::ComboBox::from_id_salt((slot, "kind")).width(96.0).selected_text(kind.label()).show_ui(ui, |ui| {
        for k in [Kind::Channel, Kind::Gray, Kind::Normal, Kind::Height, Kind::Baked, Kind::Converted, Kind::Constant, Kind::View, Kind::Alpha] {
            ui.selectable_value(&mut kind, k, k.label());
        }
    });
    if kind != Kind::of(src) {
        *src = kind.default_source(slot);
    }
    match src {
        Source::Channel { kind, component, invert } => {
            channel_combo(ui, slot, kind);
            // Colour channels also offer A: the stack's coverage (0 where a transparent set is unpainted).
            // Every channel with three components offers them (Coat Normal was left with R only).
            let n = if kind.is_color() { 4 } else { kind.components() };
            component_combo(ui, (slot, "comp"), component, n);
            ui.checkbox(invert, "Invert");
        }
        Source::Gray { kind, invert } => {
            channel_combo(ui, slot, kind);
            ui.label(dim("luminance"));
            ui.checkbox(invert, "Invert");
        }
        Source::Normal { component, directx } => {
            egui::ComboBox::from_id_salt((slot, "nfmt")).width(120.0).selected_text(if *directx { "DirectX (Y-)" } else { "OpenGL (Y+)" }).show_ui(ui, |ui| {
                ui.selectable_value(directx, false, "OpenGL (Y+)");
                ui.selectable_value(directx, true, "DirectX (Y-)");
            });
            component_combo(ui, (slot, "comp"), component, 3);
            ui.label(dim("+ height"));
        }
        Source::Height => {
            ui.label(dim("8/16-bit: 0.5 + h/2"));
            ui.label("");
            ui.label("");
        }
        Source::MeshMap { kind, component, invert } => {
            egui::ComboBox::from_id_salt((slot, "mesh")).width(140.0).selected_text(kind.display_name()).show_ui(ui, |ui| {
                for k in MeshMapKind::ALL {
                    ui.selectable_value(kind, k, k.display_name());
                }
            });
            component_combo(ui, (slot, "comp"), component, kind.components());
            ui.checkbox(invert, "Invert");
        }
        Source::Converted { map, component } => {
            egui::ComboBox::from_id_salt((slot, "conv")).width(140.0).selected_text(map.label()).show_ui(ui, |ui| {
                for c in Converted::ALL {
                    ui.selectable_value(map, c, c.label());
                }
            });
            component_combo(ui, (slot, "comp"), component, map.components());
            ui.label("");
        }
        Source::Constant { value } => {
            ui.add(egui::DragValue::new(value).range(0.0..=1.0).speed(0.01).fixed_decimals(2));
            ui.label("");
            ui.label("");
        }
        Source::View2d { component } => {
            ui.label(dim("what the 2D view shows"));
            component_combo(ui, (slot, "comp"), component, 3);
            ui.label("");
        }
        Source::Alpha => {
            ui.label(dim("Opacity × coverage"));
            ui.label("");
            ui.label("");
        }
    }
}

fn channel_combo(ui: &mut Ui, slot: usize, kind: &mut ChannelKind) {
    egui::ComboBox::from_id_salt((slot, "ch")).width(140.0).selected_text(kind.display_name()).show_ui(ui, |ui| {
        for k in ChannelKind::ALL {
            ui.selectable_value(kind, k, k.display_name());
        }
    });
}

/// Painter's padding names. `Texels` (dilate, keep the rest of the gutter) is SubCraft's own.
pub(crate) fn padding_label(p: Padding) -> &'static str {
    match p {
        Padding::None => "No padding",
        Padding::Infinite => "Dilation infinite",
        Padding::Texels(_) => "Dilation only",
        Padding::Background(_) => "Dilation + background color",
        Padding::Diffusion(_) => "Dilation + diffusion",
    }
}

fn padding_tip(p: Padding) -> &'static str {
    match p {
        Padding::None => tr("Gutter texels keep their composite values"),
        Padding::Infinite => tr("Fill the whole gutter with the nearest island texel (best for mipmaps)"),
        Padding::Texels(_) => tr("Grow islands by the distance; the rest of the gutter keeps its composite values"),
        Padding::Background(_) => tr("Grow islands by the distance, then fill the rest with each map's default color"),
        Padding::Diffusion(_) => tr("Grow islands by the distance, then fill the rest with a smooth blend of the islands around it"),
    }
}

/// Padding menu plus the dilation distance. `inherit` names the `None` entry ("use the
/// setting above"); without it the menu has no such entry.
pub(crate) fn padding_combo(ui: &mut Ui, id: &str, padding: &mut Option<Padding>, inherit: Option<&str>) {
    let text = match (*padding, inherit) {
        (None, Some(i)) => i.to_string(),
        (None, None) => padding_label(Padding::Infinite).to_string(),
        (Some(p), _) => padding_label(p).to_string(),
    };
    // Switching between dilation modes keeps the distance.
    let distance = padding.and_then(Padding::distance).unwrap_or(16);
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id).width(190.0).selected_text(text).show_ui(ui, |ui| {
            if let Some(i) = inherit {
                ui.selectable_value(padding, None, i);
            }
            let modes = [Padding::None, Padding::Infinite, Padding::Texels(distance), Padding::Background(distance), Padding::Diffusion(distance)];
            for mode in modes {
                let on = padding.is_some_and(|p| std::mem::discriminant(&p) == std::mem::discriminant(&mode));
                if ui.selectable_label(on, padding_label(mode)).on_hover_text(padding_tip(mode)).clicked() && !on {
                    *padding = Some(mode);
                }
            }
        });
        if let Some(p) = padding
            && let Some(mut n) = p.distance()
            && ui.add(egui::DragValue::new(&mut n).range(1..=256).suffix(" px")).on_hover_text(tr("Dilation distance")).changed()
        {
            *p = p.with_distance(n);
        }
    });
}

pub(crate) fn size_combo(ui: &mut Ui, id: &str, size: &mut Option<u32>, inherit: &str) {
    let text = size.map_or(inherit.to_string(), |s| format!("{s} × {s}"));
    egui::ComboBox::from_id_salt(id).width(150.0).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(size, None, inherit);
        for s in subcraft_export::EXPORT_SIZES {
            ui.selectable_value(size, Some(s), format!("{s} × {s}"));
        }
    });
}

/// One entry of the bit depth menu: a depth, or 8 bits with dithering.
#[derive(Clone, Copy, PartialEq)]
enum DepthChoice {
    Depth(BitDepth),
    Dithered,
}

impl DepthChoice {
    fn label(self) -> &'static str {
        match self {
            DepthChoice::Depth(d) => d.label(),
            DepthChoice::Dithered => "8-bit + dithering",
        }
    }
}

/// File type and bit depth menus (Painter's two dropdowns). `format` is `None` when the file
/// type follows the template; that entry is offered when `inherit` names it. `dither` is the
/// "8-bit + dithering" choice.
pub(crate) fn format_combo(ui: &mut Ui, id: &str, format: &mut Option<FileFormat>, dither: &mut Option<bool>, inherit: Option<&str>) {
    let container_text = match (*format, inherit) {
        (Some(f), _) => f.container().label(),
        (None, Some(i)) => i,
        (None, None) => "",
    };
    let mut container = format.map(FileFormat::container);
    egui::ComboBox::from_id_salt((id, "container")).width(if inherit.is_some() { 190.0 } else { 80.0 }).selected_text(container_text).show_ui(ui, |ui| {
        if let Some(i) = inherit {
            ui.selectable_value(&mut container, None, i);
        }
        for c in Container::ALL {
            ui.selectable_value(&mut container, Some(c), c.label());
        }
    });
    match container {
        None => {
            *format = None;
            *dither = None;
        }
        Some(c) => {
            let depth = format.map_or(BitDepth::B8, FileFormat::depth);
            let new = FileFormat::from_parts(c, depth);
            let mut choice = if new.is_8bit() && *dither == Some(true) { DepthChoice::Dithered } else { DepthChoice::Depth(new.depth()) };
            egui::ComboBox::from_id_salt((id, "depth")).width(130.0).selected_text(choice.label()).show_ui(ui, |ui| {
                for d in c.depths() {
                    ui.selectable_value(&mut choice, DepthChoice::Depth(*d), d.label());
                    if *d == BitDepth::B8 {
                        ui.selectable_value(&mut choice, DepthChoice::Dithered, DepthChoice::Dithered.label())
                            .on_hover_text(tr("Adds fine noise when writing 8 bits per channel, so smooth gradients don't band"));
                    }
                }
            });
            *format = Some(match choice {
                DepthChoice::Depth(d) => FileFormat::from_parts(c, d),
                DepthChoice::Dithered => FileFormat::from_parts(c, BitDepth::B8),
            });
            *dither = match choice {
                DepthChoice::Dithered => Some(true),
                // Keep "no explicit choice" as it was; an explicit dithered choice is turned off.
                DepthChoice::Depth(_) if *dither == Some(true) => None,
                DepthChoice::Depth(_) => *dither,
            };
        }
    }
}

fn normal_combo(ui: &mut Ui, id: &str, dx: &mut Option<bool>, inherit: &str) {
    let text = match dx {
        None => inherit,
        Some(false) => "OpenGL (Y+)",
        Some(true) => "DirectX (Y-)",
    };
    egui::ComboBox::from_id_salt(id).width(150.0).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(dx, None, inherit);
        ui.selectable_value(dx, Some(false), "OpenGL (Y+)");
        ui.selectable_value(dx, Some(true), "DirectX (Y-)");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_count_changes_keep_gray_and_alpha() {
        let r = Source::ch(ChannelKind::Roughness, 0);
        let a = Source::ch(ChannelKind::Opacity, 0);
        let mut c = vec![r, a];
        resize_components(&mut c, 4);
        assert_eq!(c, [r, r, r, a]);
        resize_components(&mut c, 1);
        assert_eq!(c, [r]);
        resize_components(&mut c, 3);
        assert_eq!(c, [r, r, r]);
        resize_components(&mut c, 2);
        assert_eq!(c[0], r);
        assert_eq!(c[1], Source::Constant { value: 1.0 });
    }

    #[test]
    fn packing_summary_names_every_slot() {
        let m = &ExportTemplate::unity_hdrp().maps[2];
        assert_eq!(packing_summary(m), "R = Metallic\nG = Ambient Occlusion\nB = 0.00\nA = 1 - Roughness");
    }

    #[test]
    fn editor_draws_and_saves_edits_to_user_presets() {
        let dir = std::env::temp_dir().join(format!("subcraft-ui-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = subcraft_assets::UserLibrary::new(&dir);
        let mut presets = ExportPresets::load(Some(&lib));
        let p = presets.duplicate("Unreal Engine (ORM packed)").unwrap();
        let mut ed = PresetEditor::default();
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut key = p.key().to_string();
        for _ in 0..2 {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                preset_bar(ui, &mut presets, &mut key, &mut ed);
                preset_editor(ui, &mut presets, &key, &mut ed, "Crate");
            });
            // No renderer here: drop the font atlas upload (epaint asserts on unapplied deltas in debug).
            out.textures_delta.clear();
        }
        assert!(ed.error.is_none(), "{:?}", ed.error);
        // Simulate an edit through the draft path: change the selected map's format.
        let mut d = ed.draft.clone().unwrap();
        d.maps[0].format = FileFormat::Tiff16;
        presets.update(d).unwrap();
        let reloaded = ExportPresets::load(Some(&lib));
        assert_eq!(reloaded.find(&key).unwrap().maps[0].format, FileFormat::Tiff16);
        std::fs::remove_dir_all(&dir).ok();
    }
}
