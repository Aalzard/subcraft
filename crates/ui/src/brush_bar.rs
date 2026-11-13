//! Compact viewport brush controls, editing the session's existing tool state.
use egui::{RichText, Ui};
use subcraft_paint::{BrushSettings, StrokeOptions};

use crate::{
    icons,
    state::Tool,
    theme::{self, ACCENT, BORDER_STRONG, TEXT, TEXT_DIM},
    widgets,
};

pub(crate) fn active(tool: Tool) -> bool {
    matches!(tool, Tool::Brush | Tool::Eraser | Tool::PolygonFill | Tool::Clone | Tool::Smudge)
}

/// Scrubbable values and slider popovers keep the row quiet.
pub(crate) fn show(
    ui: &mut Ui,
    tool: &mut Tool,
    settings: &mut BrushSettings,
    options: &mut StrokeOptions,
    view: &mut crate::viewport_bar::ViewportPrefs,
    width: f32,
) {
    // Tool selection lives in the left tool rail only (as in Painter); this row holds the
    // stroke options and the brush values of the active tool.
    ui.spacing_mut().item_spacing.x = 9.0;
    stroke_options(ui, options, view);
    // Its own button where the row has room; always in the symmetry settings menu as well.
    if shows_display_button(width) {
        display_menu(ui, view);
    }
    let brush = *tool != Tool::PolygonFill;
    if width < FULL_MIN_WIDTH {
        let response = widgets::icon_button(ui, icons::SLIDERS, "Brush settings");
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| menu_controls(ui, settings, options, brush));
        if width >= 250.0 {
            ui.add_enabled_ui(brush, |ui| size(ui, &mut settings.size));
        }
        if width >= 380.0 {
            percent(ui, "Opacity", &mut settings.stroke_opacity);
        }
    } else {
        ui.add_enabled_ui(brush, |ui| size(ui, &mut settings.size));
        ui.add_enabled_ui(brush, |ui| {
            popup_percent(ui, "Flow", &mut settings.flow, "Paint per stamp", false)
        });
        percent(ui, "Opacity", &mut settings.stroke_opacity);
        if shows_spacing_and_distance(width) {
            ui.add_enabled_ui(brush, |ui| spacing(ui, &mut settings.spacing));
            ui.add_enabled_ui(options.lazy_distance_px > 0.0, |ui| distance(ui, &mut options.lazy_distance_px));
        }
        ui.add_enabled_ui(brush, |ui| {
            popup_percent(ui, "Hardness", &mut settings.hardness, "Tip edge", true)
        });
    }
}

/// Polygon fill's element choice (Triangle, Polygon, Mesh, UV island, Object).
pub(crate) fn fill_mode_combo(ui: &mut Ui, mode: &mut subcraft_paint::PolygonFillMode, id: &str) -> egui::Response {
    use subcraft_paint::PolygonFillMode;
    let tip = fill_mode_tip;
    egui::ComboBox::from_id_salt(id)
        .selected_text(mode.label())
        .width(96.0)
        .show_ui(ui, |ui| {
            for m in PolygonFillMode::ALL {
                ui.selectable_value(mode, m, m.label()).on_hover_text(tip(m));
            }
        })
        .response
        .on_hover_text("Polygon fill: what a click or a box fills")
}

/// Polygon fill options for the contextual toolbar (3D and 2D views): the element a click or a
/// box fills as a row of buttons (a menu when the row is narrow), visible-only boxes in 3D, and
/// where the fill lands on the selected layer.
pub(crate) fn fill_options(ui: &mut Ui, mode: &mut subcraft_paint::PolygonFillMode, visible_only: Option<&mut bool>, destination: Option<subcraft_app::LayerTarget>, id: &str) {
    use subcraft_paint::PolygonFillMode;
    ui.label(RichText::new("Fill").color(TEXT_DIM));
    if ui.available_width() >= 330.0 {
        for m in PolygonFillMode::ALL {
            if ui.selectable_label(*mode == m, m.label()).on_hover_text(fill_mode_tip(m)).clicked() {
                *mode = m;
            }
        }
    } else {
        fill_mode_combo(ui, mode, id);
    }
    if let Some(v) = visible_only {
        let r = widgets::icon_toggle(ui, icons::EYE, *v, "Visible polygons only: boxes skip back faces and hidden triangles (click to toggle)");
        if r.clicked() {
            *v = !*v;
        }
    }
    match destination {
        Some(subcraft_app::LayerTarget::Mask) => {
            ui.label(RichText::new("→ mask").color(ACCENT)).on_hover_text(
                "Fills the selected layer's mask: on a fill layer or folder (a smart material) the layer then shows only on the filled elements. A black mask is added if the layer has none.",
            );
        }
        Some(subcraft_app::LayerTarget::Content) => {}
        None => {
            ui.label(RichText::new("select a paint or fill layer").color(theme::WARNING));
        }
    }
}

/// What each polygon fill mode fills, for tooltips.
pub(crate) fn fill_mode_tip(m: subcraft_paint::PolygonFillMode) -> &'static str {
    use subcraft_paint::PolygonFillMode;
    match m {
        PolygonFillMode::Triangle => "One triangle",
        PolygonFillMode::Polygon => "The quad or n-gon the model was made of",
        PolygonFillMode::MeshElement => "A connected piece of the mesh",
        PolygonFillMode::UvIsland => "A UV island",
        PolygonFillMode::Object => "A whole object of the model",
    }
}

/// Narrowest strip that shows every brush value instead of the "Brush settings" menu.
pub(crate) const FULL_MIN_WIDTH: f32 = 640.0;

/// Spacing and the lazy mouse Distance sit on the bar where there is room for them: on the
/// brush bar's own row from 900 points, and beside the view controls from 1400. Elsewhere they
/// are in the Properties (Spacing) and in the symmetry settings menu (Distance).
pub(crate) fn shows_spacing_and_distance(width: f32) -> bool {
    (900.0..crate::viewport_bar::INLINE_MIN_WIDTH).contains(&width) || width >= 1400.0
}

/// Lazy mouse distance the button switches on when none was used before (the D shortcut's value).
pub(crate) const DEFAULT_LAZY_DISTANCE: f32 = 20.0;

/// Mirror axes and radial symmetry as one on / off state (Painter's Symmetry button).
pub(crate) fn symmetry_on(options: &StrokeOptions) -> bool {
    options.symmetry_on()
}

/// Switch symmetry off, remembering the axes in `saved`, or back on with the remembered axes
/// (mirror X when there are none).
pub(crate) fn toggle_symmetry(options: &mut StrokeOptions, saved: &mut Option<([bool; 3], bool)>) {
    options.toggle_symmetry(saved);
}

/// Switch the lazy mouse off, remembering its distance in `saved`, or back on with it.
pub(crate) fn toggle_lazy(options: &mut StrokeOptions, saved: &mut Option<f32>) {
    if options.lazy_distance_px > 0.0 {
        *saved = Some(options.lazy_distance_px);
        options.lazy_distance_px = 0.0;
    } else {
        options.lazy_distance_px = saved.filter(|d| *d > 0.0).unwrap_or(DEFAULT_LAZY_DISTANCE);
    }
}

/// Painter's three buttons: Symmetry on / off, its settings menu, and Lazy mouse on / off.
fn stroke_options(ui: &mut Ui, options: &mut StrokeOptions, view: &mut crate::viewport_bar::ViewportPrefs) {
    let saved_id = egui::Id::new("brush_bar_saved_stroke_options");
    let (mut saved_symmetry, mut saved_lazy): (Option<([bool; 3], bool)>, Option<f32>) = ui.data(|d| d.get_temp(saved_id)).unwrap_or((None, None));
    let on = symmetry_on(options);
    let lazy = options.lazy_distance_px > 0.0;
    let settings = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let toggle = widgets::icon_toggle(ui, icons::SYMMETRY, on, if on { "Symmetry is on · click to switch it off (L)" } else { "Symmetry is off · click to switch it on (L)" });
            toggle.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, "Symmetry"));
            if toggle.clicked() {
                toggle_symmetry(options, &mut saved_symmetry);
            }
            // A narrow button beside the toggle, as in Painter: it opens the settings menu.
            let (rect, settings) = ui.allocate_exact_size(egui::vec2(12.0, 26.0), egui::Sense::click());
            let color = if settings.hovered() { TEXT } else { TEXT_DIM };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icons::GEAR, theme::font(11.0), color);
            settings.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Symmetry settings"));
            settings.on_hover_text("Symmetry settings").on_hover_cursor(egui::CursorIcon::PointingHand)
        })
        .inner;
    egui::Popup::menu(&settings)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new("Mirror symmetry").color(TEXT_DIM));
            ui.horizontal(|ui| {
                for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                    let value = &mut options.symmetry[i];
                    if ui
                        .selectable_label(*value, axis)
                        .on_hover_text(format!("Mirror across {axis} through the model center"))
                        .clicked()
                    {
                        *value = !*value;
                    }
                }
            });
            mirror_positions(ui, options);
            ui.add_space(4.0);
            radial_options(ui, options);
            symmetry_display(ui, view);
            ui.separator();
            ui.label(RichText::new("Lazy Mouse").color(TEXT_DIM));
            ui.spacing_mut().slider_width = 155.0;
            ui.add(widgets::typed_slider(egui::Slider::new(&mut options.lazy_distance_px, 0.0..=100.0).suffix(" px")));
            ui.label(RichText::new("Distance the brush trails the cursor · 0 px = off").small().color(TEXT_DIM));
            ui.separator();
            display_options(ui, view);
        });
    let toggle = widgets::icon_toggle(ui, icons::CURVE, lazy, if lazy { "Lazy mouse is on · click to switch it off (D)" } else { "Lazy mouse is off · click to switch it on (D)" });
    toggle.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, lazy, "Lazy mouse"));
    if toggle.clicked() {
        toggle_lazy(options, &mut saved_lazy);
    }
    ui.data_mut(|d| d.insert_temp(saved_id, (saved_symmetry, saved_lazy)));
}

/// Painter's brush display menu: the brush preview style under the cursor and the pen pressure
/// curve, both kept with the preferences.
fn display_menu(ui: &mut Ui, prefs: &mut crate::viewport_bar::ViewportPrefs) {
    let icon = if prefs.brush_cursor == crate::viewport_bar::BrushCursor::Crosshair { icons::CROSSHAIR } else { icons::CURSOR };
    let response = widgets::icon_button(ui, icon, "Brush preview and pressure curve");
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(200.0);
            display_options(ui, prefs);
        });
}

/// Brush preview style and pressure curve choices (the display menu and the symmetry settings
/// menu both show them).
fn display_options(ui: &mut Ui, prefs: &mut crate::viewport_bar::ViewportPrefs) {
    ui.label(RichText::new("Brush preview").color(TEXT_DIM));
    for style in crate::viewport_bar::BrushCursor::ALL {
        ui.radio_value(&mut prefs.brush_cursor, style, style.label()).on_hover_text(crate::brush_cursor::tip(style));
    }
    ui.add_space(4.0);
    ui.label(RichText::new("Pressure curve").color(TEXT_DIM));
    for curve in subcraft_paint::PressureCurve::ALL {
        ui.radio_value(&mut prefs.pressure_curve, curve, curve.label()).on_hover_text(pressure_tip(curve));
    }
}

/// The brush display menu gets its own toolbar button from this width (where Spacing and
/// Distance also sit beside the view controls); narrower rows reach it in the symmetry settings.
pub(crate) fn shows_display_button(width: f32) -> bool {
    width >= 1400.0
}

/// What each pressure curve does, for tooltips.
pub(crate) fn pressure_tip(curve: subcraft_paint::PressureCurve) -> &'static str {
    use subcraft_paint::PressureCurve;
    match curve {
        PressureCurve::Linear => "Pen pressure as it comes",
        PressureCurve::EaseIn => "Light pressure does less; full strength needs a firm press",
        PressureCurve::EaseInOut => "Soft at both ends: light touches stay light, firm presses reach full strength sooner",
    }
}

/// Axis position of each enabled mirror plane: its offset from the model center along its axis.
fn mirror_positions(ui: &mut Ui, options: &mut StrokeOptions) {
    if !options.symmetry.contains(&true) {
        return;
    }
    ui.horizontal(|ui| {
        ui.label("Axis position");
        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            if options.symmetry[i] {
                ui.add(widgets::typed_drag(egui::DragValue::new(&mut options.mirror_offset[i]).speed(0.01).max_decimals(3).prefix(format!("{axis} "))))
                    .on_hover_text(format!("Move the {axis} mirror plane away from the model center (object units); drag its handle with the manipulator"));
            }
        }
        if options.mirror_offset.iter().any(|v| *v != 0.0) && ui.small_button("Reset").on_hover_text("Mirror planes back through the model center").clicked() {
            options.mirror_offset = [0.0; 3];
        }
    });
}

/// Symmetry display options (Painter's display group): plane, intersection and axis guides,
/// the cursor at every copy, hide while painting, and the axis-position manipulator.
fn symmetry_display(ui: &mut Ui, view: &mut crate::viewport_bar::ViewportPrefs) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut view.symmetry_plane, "Plane").on_hover_text("Each active mirror plane as a translucent sheet");
        ui.checkbox(&mut view.symmetry_guides, "Guides").on_hover_text("Intersection and radial axis lines");
    });
    let guides = view.symmetry_guides;
    let d = &mut view.symmetry_display;
    ui.add_enabled_ui(guides, |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.show_intersection, "Intersection").on_hover_text("Where the mirror plane cuts the model");
            ui.checkbox(&mut d.show_axis, "Axis").on_hover_text("Radial axis and one spoke per copy");
        });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut view.symmetry_cursor, "Cursor").on_hover_text("Show the brush cursor at every copy");
        ui.checkbox(&mut view.symmetry_cursor_hide_painting, "Hide while painting").on_hover_text("Hide the copy cursors and the guides during a stroke");
    });
    let d = &mut view.symmetry_display;
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.manipulator, "Manipulator").on_hover_text("Drag the handles in the 3D view to move the mirror planes or the radial axis");
        ui.add_enabled_ui(d.manipulator, |ui| {
            ui.spacing_mut().slider_width = 70.0;
            ui.add(egui::Slider::new(&mut d.manipulator_size, crate::symmetry_view::SymmetryDisplay::SIZE_RANGE).text("size"));
        });
    });
}

/// Radial symmetry: on/off, axis, copy count and an optional axis offset from the model center.
fn radial_options(ui: &mut Ui, options: &mut StrokeOptions) {
    let radial = &mut options.radial;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Radial symmetry").color(TEXT_DIM));
        ui.checkbox(&mut radial.enabled, "")
            .on_hover_text("Repeat the stroke around an axis through the model center");
    });
    ui.add_enabled_ui(radial.enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("Axis");
            for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                if ui
                    .selectable_label(radial.axis as usize == i, axis)
                    .on_hover_text(format!("Rotate copies around {axis}"))
                    .clicked()
                {
                    radial.axis = i as u8;
                }
            }
        });
        ui.spacing_mut().slider_width = 130.0;
        ui.add(
            widgets::typed_slider(egui::Slider::new(&mut radial.count, 2..=subcraft_paint::MAX_SYMMETRY_COPIES)
                .text("copies")),
        );
        ui.horizontal(|ui| {
            ui.label("Offset");
            for v in &mut radial.center_offset {
                ui.add(widgets::typed_drag(egui::DragValue::new(v).speed(0.01).max_decimals(3)));
            }
        })
        .response
        .on_hover_text("Move the axis away from the model center (object units)");
        ui.checkbox(&mut radial.flip, "Flip copy")
            .on_hover_text("Mirror every other copy, so neighbouring copies face each other");
    });
    let mirrors = options.symmetry.iter().filter(|&&m| m).count();
    let effective = options.radial.effective_count(mirrors);
    if options.radial.enabled && mirrors > 0 {
        let text = if effective < options.radial.count {
            format!("{} copies with mirror (capped at {})", options.copies(), subcraft_paint::MAX_SYMMETRY_COPIES)
        } else {
            format!("{} copies with mirror", options.copies())
        };
        ui.label(RichText::new(text).small().color(TEXT_DIM));
    }
}

fn menu_controls(ui: &mut Ui, s: &mut BrushSettings, options: &mut StrokeOptions, brush: bool) {
    ui.add_enabled_ui(brush, |ui| {
        size(ui, &mut s.size);
        ui.horizontal(|ui| percent(ui, "Flow", &mut s.flow));
        ui.horizontal(|ui| percent(ui, "Hardness", &mut s.hardness));
        spacing(ui, &mut s.spacing);
    });
    ui.horizontal(|ui| percent(ui, "Opacity", &mut s.stroke_opacity));
    ui.add_enabled_ui(options.lazy_distance_px > 0.0, |ui| distance(ui, &mut options.lazy_distance_px));
}

/// A frameless drag value like [`scrub`], with its own range and suffix.
fn value(ui: &mut Ui, text: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, speed: f32, suffix: &str, help: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        label(ui, text);
        ui.scope(|ui| {
            let widgets = &mut ui.visuals_mut().widgets;
            for visuals in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
                visuals.weak_bg_fill = egui::Color32::TRANSPARENT;
                visuals.bg_fill = egui::Color32::TRANSPARENT;
                visuals.bg_stroke = egui::Stroke::NONE;
            }
            ui.visuals_mut().override_text_color = Some(TEXT);
            ui.spacing_mut().button_padding.x = 1.0;
            ui.spacing_mut().interact_size.x = 36.0;
            ui.add(widgets::typed_drag(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix).max_decimals(0)))
        })
        .inner
        .on_hover_text(help)
    })
    .inner
}

/// Spacing between stamps, shown as a percentage of the brush diameter (2-200).
fn spacing(ui: &mut Ui, v: &mut f32) {
    let mut display = *v * 100.0;
    if value(ui, "Spacing", &mut display, 2.0..=200.0, 0.5, "%", "Distance between stamps, as a percentage of the brush diameter").changed() {
        *v = (display * 0.01).clamp(0.02, 2.0);
    }
}

/// Lazy mouse distance in pixels; greyed out while the lazy mouse is off.
fn distance(ui: &mut Ui, v: &mut f32) {
    let mut display = if *v > 0.0 { *v } else { DEFAULT_LAZY_DISTANCE };
    if value(ui, "Distance", &mut display, 1.0..=100.0, 0.5, " px", "Lazy mouse: how far the brush trails the cursor").changed() && *v > 0.0 {
        *v = display.clamp(1.0, 100.0);
    }
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font(11.5)).color(TEXT_DIM));
}

/// Only the resting drag button is frameless; text-entry mode retains a readable field.
fn scrub(ui: &mut Ui, value: &mut f32, size: bool) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for visuals in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            visuals.weak_bg_fill = egui::Color32::TRANSPARENT;
            visuals.bg_fill = egui::Color32::TRANSPARENT;
            visuals.bg_stroke = egui::Stroke::NONE;
        }
        ui.visuals_mut().override_text_color = Some(if size { ACCENT } else { TEXT });
        ui.spacing_mut().button_padding.x = 1.0;
        ui.spacing_mut().interact_size.x = 36.0;
        ui.add(widgets::typed_drag(
            egui::DragValue::new(value)
                .speed(if size { 0.1 } else { 0.5 })
                .range(if size { 0.1..=100.0 } else { 0.0..=100.0 })
                .suffix("%")
                .max_decimals(usize::from(size)),
        ))
    })
    .inner
    .on_hover_text("Drag left/right to adjust · Shift for precision · click to type")
}

fn size(ui: &mut Ui, value: &mut f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        label(ui, "Size");
        scrub(ui, value, true)
            .on_hover_text("Brush diameter as a percentage of the model diagonal");
    });
}

fn percent(ui: &mut Ui, text: &str, value: &mut f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        label(ui, text);
        let mut display = *value * 100.0;
        if scrub(ui, &mut display, false).changed() {
            *value = display * 0.01;
        }
    });
}

fn popup_percent(ui: &mut Ui, name: &str, value: &mut f32, help: &str, tip: bool) {
    // Each popover has its own stable ID: changing its numeric label must not close it.
    ui.push_id(name, |ui| {
        let font = theme::font(11.5);
        let label = ui
            .painter()
            .layout_no_wrap(name.into(), font.clone(), TEXT_DIM);
        let number = ui
            .painter()
            .layout_no_wrap(format!("{:.0}%", *value * 100.0), font, TEXT);
        // Reserve maximum-width values so the row doesn't jump while scrubbing the slider.
        let width = label.size().x + 47.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
        let enabled = ui.is_enabled();
        let painter = ui.painter();
        let color = if response.hovered() && enabled {
            ACCENT
        } else {
            TEXT_DIM
        };
        painter.galley(
            egui::pos2(rect.left(), rect.center().y - label.size().y / 2.0),
            label,
            TEXT_DIM,
        );
        painter.galley(
            egui::pos2(
                rect.right() - 12.0 - number.size().x,
                rect.center().y - number.size().y / 2.0,
            ),
            number,
            TEXT,
        );
        if tip {
            let center = egui::pos2(rect.right() - 4.0, rect.center().y);
            painter.circle_stroke(center, 4.0, egui::Stroke::new(1.0, color));
            painter.circle_filled(center, *value * 3.5, color);
        } else {
            let y = rect.bottom() - 3.0;
            painter.hline(rect.x_range(), y, egui::Stroke::new(2.0, BORDER_STRONG));
            painter.hline(
                rect.left()..=rect.left() + rect.width() * *value,
                y,
                egui::Stroke::new(2.0, ACCENT),
            );
            painter.text(
                egui::pos2(rect.right() - 4.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                icons::CARET_DOWN,
                theme::font(10.0),
                color,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                format!("{name} {:.0}%", *value * 100.0),
            )
        });
        let response = response
            .on_hover_text(format!("{help} · click for slider"))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.label(RichText::new(help).color(TEXT_DIM));
                ui.spacing_mut().slider_width = 160.0;
                let mut display = *value * 100.0;
                if ui
                    .add(widgets::typed_slider(egui::Slider::new(&mut display, 0.0..=100.0).suffix("%")))
                    .changed()
                {
                    *value = display * 0.01;
                }
            });
    });
}

/// Material channels a texture set can add from the channel switches' "+" menu.
const ADDABLE: [subcraft_core::ChannelKind; 9] = {
    use subcraft_core::ChannelKind::*;
    [BaseColor, Height, Roughness, Metallic, Normal, AmbientOcclusion, Emissive, Opacity, Specular]
};

fn channel_label(kind: subcraft_core::ChannelKind) -> &'static str {
    use subcraft_core::ChannelKind::*;
    match kind {
        BaseColor => "Color",
        Height => "Height",
        Roughness => "Rough",
        Metallic => "Metal",
        Normal => "Normal",
        AmbientOcclusion => "AO",
        Emissive => "Emissive",
        Opacity => "Opacity",
        Specular => "Spec",
        other => other.short_name(),
    }
}

/// Which material channels the brush paints: click switches one on or off, Shift+click paints
/// only that channel (again: all channels). "+" adds a missing PBR channel to the texture set.
pub(crate) fn channel_switches(ui: &mut Ui, state: &mut crate::state::UiState) {
    let channels = state.session.channels();
    let mut material = state.session.brush.material.clone();
    let original = material.clone();
    let shift = ui.input(|i| i.modifiers.shift);
    let mask = state.session.brush.target.is_mask();
    let mut add = None;
    ui.add_enabled_ui(!mask, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for &kind in &channels {
                let on = material.is_enabled(kind);
                let text = RichText::new(channel_label(kind)).font(theme::font(11.5)).color(if on { TEXT } else { TEXT_DIM });
                let help = if mask {
                    "Painting a mask: channels don't apply".to_string()
                } else {
                    format!("{} · click: paint on/off · Shift+click: paint only this channel", kind.display_name())
                };
                if ui.selectable_label(on, text).on_hover_text(help).clicked() {
                    if shift { material.solo(kind, &channels) } else { material.toggle(kind) }
                }
            }
            let missing: Vec<_> = ADDABLE.into_iter().filter(|k| !channels.contains(k)).collect();
            if !missing.is_empty() {
                let response = widgets::icon_button(ui, icons::PLUS, "Add a material channel to this texture set");
                egui::Popup::menu(&response).show(|ui| {
                    for kind in missing {
                        if ui.button(kind.display_name()).clicked() {
                            add = Some(kind);
                        }
                    }
                });
            }
        });
    });
    let now = ui.input(|i| i.time);
    if material != original
        && let Err(e) = state.session.execute(subcraft_app::Command::SetBrushMaterial(material))
    {
        state.set_status(e.to_string(), now);
    }
    if let Some(kind) = add {
        let texture_set = state.session.selection().texture_set;
        match state.session.execute(subcraft_app::Command::AddChannel { texture_set, kind }) {
            Ok(()) => {
                state.gpu_dirty = true;
                state.set_status(format!("{} channel added", kind.display_name()), now);
            }
            Err(e) => state.set_status(e.to_string(), now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetry_button_switches_off_and_restores_the_axes() {
        let mut o = StrokeOptions::default();
        let mut saved = None;
        assert!(!symmetry_on(&o));
        toggle_symmetry(&mut o, &mut saved);
        assert_eq!(o.symmetry, [true, false, false], "first click: mirror X");
        o.symmetry = [false, true, true];
        o.radial.enabled = true;
        toggle_symmetry(&mut o, &mut saved);
        assert!(!symmetry_on(&o) && !o.has_symmetry());
        toggle_symmetry(&mut o, &mut saved);
        assert_eq!((o.symmetry, o.radial.enabled), ([false, true, true], true), "the axes come back");
    }

    #[test]
    fn lazy_button_switches_off_and_restores_the_distance() {
        let mut o = StrokeOptions::default();
        let mut saved = None;
        o.lazy_distance_px = 0.0;
        toggle_lazy(&mut o, &mut saved);
        assert_eq!(o.lazy_distance_px, DEFAULT_LAZY_DISTANCE);
        o.lazy_distance_px = 55.0;
        toggle_lazy(&mut o, &mut saved);
        assert_eq!(o.lazy_distance_px, 0.0);
        toggle_lazy(&mut o, &mut saved);
        assert_eq!(o.lazy_distance_px, 55.0);
    }

    #[test]
    fn spacing_and_distance_show_only_where_the_row_has_room() {
        for (width, shown) in [(300.0, false), (700.0, false), (900.0, true), (1100.0, true), (1200.0, false), (1500.0, true)] {
            assert_eq!(shows_spacing_and_distance(width), shown, "{width}");
        }
        assert!(FULL_MIN_WIDTH < 900.0);
    }

    #[test]
    fn the_display_button_shows_only_on_wide_rows() {
        for (width, shown) in [(300.0, false), (1100.0, false), (1399.0, false), (1400.0, true), (2000.0, true)] {
            assert_eq!(shows_display_button(width), shown, "{width}");
        }
    }

    #[test]
    fn every_pressure_curve_has_its_own_tooltip() {
        let tips: std::collections::BTreeSet<_> = subcraft_paint::PressureCurve::ALL.into_iter().map(pressure_tip).collect();
        assert_eq!(tips.len(), subcraft_paint::PressureCurve::ALL.len());
    }
}
