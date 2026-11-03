//! 3D view extras: polygon wireframe controls, camera import and camera bookmarks.
//! The pure parts (camera conversion, bookmarks) are tested without a window.

use egui::{RichText, Ui};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use subcraft_mesh::camera::CameraSpec;

use crate::orbit::OrbitCamera;
use crate::theme::TEXT_DIM;
use crate::viewport_bar::ViewportPrefs;

/// A saved viewpoint: everything the orbit camera needs, with a name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraBookmark {
    pub name: String,
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
    pub orthographic: bool,
}

impl CameraBookmark {
    pub fn capture(name: impl Into<String>, camera: &OrbitCamera) -> Self {
        Self {
            name: name.into(),
            target: camera.target.to_array(),
            yaw: camera.yaw,
            pitch: camera.pitch,
            distance: camera.distance,
            fov_y_deg: camera.fov_y_deg,
            orthographic: camera.orthographic,
        }
    }

    /// Move `camera` to this viewpoint (the scene radius stays the model's).
    pub fn apply(&self, camera: &mut OrbitCamera) {
        let v = Vec3::from_array(self.target);
        if !v.is_finite() || !self.yaw.is_finite() || !self.pitch.is_finite() || !self.distance.is_finite() || !(1.0..=170.0).contains(&self.fov_y_deg) {
            return;
        }
        camera.target = v;
        camera.yaw = self.yaw;
        camera.pitch = self.pitch.clamp(-1.55, 1.55);
        camera.distance = self.distance.clamp(camera.scene_radius * 0.05, camera.scene_radius * 50.0);
        camera.fov_y_deg = self.fov_y_deg;
        camera.orthographic = self.orthographic;
    }

    /// The viewpoint of an imported camera, seen from the model's `camera` (for its scene size).
    /// The orbit camera has no roll, so a rolled camera is imported level. The target is placed
    /// on the camera's view axis at the model's distance, so the eye is exactly the camera's
    /// position unless that distance is outside the zoom limits.
    pub fn from_spec(spec: &CameraSpec, model_center: Vec3, camera: &OrbitCamera) -> Self {
        let forward = spec.forward().normalize_or(Vec3::NEG_Z);
        let ahead = (model_center - spec.position).dot(forward);
        let distance = if ahead > camera.scene_radius * 0.05 { ahead } else { (model_center - spec.position).length().max(camera.scene_radius) };
        let distance = distance.clamp(camera.scene_radius * 0.05, camera.scene_radius * 50.0);
        let back = -forward;
        Self {
            name: spec.name.clone(),
            target: (spec.position + forward * distance).to_array(),
            yaw: back.x.atan2(back.z),
            pitch: back.y.clamp(-1.0, 1.0).asin().clamp(-1.55, 1.55),
            distance,
            fov_y_deg: spec.fov_y_deg.clamp(1.0, 170.0),
            orthographic: spec.orthographic,
        }
    }
}

/// Linear RGBA of the wireframe (the preference stores the sRGB color the picker shows).
pub fn wire_color_linear(prefs: &ViewportPrefs) -> [f32; 3] {
    subcraft_core::color::srgb3_to_linear(prefs.wire_color.map(|c| c.clamp(0.0, 1.0)))
}

/// What the uploaded wireframe was built from: mesh identity, merge angle, linear color bits, opacity bits.
pub type WireKey = (usize, u32, [u32; 3], u32);

/// Upload the polygon wireframe when the mesh or its settings changed. Cheap when nothing did.
pub fn sync_wireframe(
    renderer: &mut subcraft_render::ViewportRenderer,
    device: &egui_wgpu::wgpu::Device,
    prefs: &ViewportPrefs,
    mesh: &std::sync::Arc<subcraft_mesh::Mesh>,
    uploaded: &mut Option<WireKey>,
) {
    if !prefs.wireframe {
        return;
    }
    let angle = prefs.wire_angle.clamp(0.0, 90.0);
    let color = wire_color_linear(prefs);
    let key = (std::sync::Arc::as_ptr(mesh) as usize, angle.to_bits(), color.map(f32::to_bits), prefs.wire_opacity.to_bits());
    if *uploaded == Some(key) {
        return;
    }
    renderer.set_wireframe(device, &subcraft_mesh::topology::polygon_edges(mesh, angle), color, prefs.wire_opacity);
    *uploaded = Some(key);
}

/// Wireframe toggle (left click) with color, opacity and merge-angle settings (right click).
pub fn wireframe_control(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    let response = crate::widgets::icon_toggle(ui, crate::icons::CUBE_WIRE, prefs.wireframe, "Polygon wireframe · click to show/hide · right-click for color and opacity");
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, prefs.wireframe, "Polygon wireframe"));
    if response.clicked() {
        prefs.wireframe = !prefs.wireframe;
    }
    egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.label(RichText::new("Polygon wireframe").color(TEXT_DIM));
        ui.checkbox(&mut prefs.wireframe, "Show wireframe");
        ui.horizontal(|ui| {
            ui.label("Color");
            ui.color_edit_button_rgb(&mut prefs.wire_color);
        });
        ui.add(egui::Slider::new(&mut prefs.wire_opacity, 0.05..=1.0).text("Opacity"));
        ui.add(egui::Slider::new(&mut prefs.wire_angle, 0.0..=90.0).text("Merge angle").suffix("°"))
            .on_hover_text("Triangle pairs from the same polygon whose normals differ less than this are drawn as one polygon");
    });
}

/// Camera menu: bookmarks and camera import.
pub fn camera_menu(ui: &mut Ui, state: &mut crate::state::UiState) {
    let now = ui.input(|i| i.time);
    let response = ui.menu_button(crate::icons::CAMERA, |ui| {
        if ui.button("Import camera…").on_hover_text("Load a camera (position, rotation, field of view) from a glTF/GLB file or a simple JSON file").clicked() {
            ui.close();
            return Action::Import;
        }
        if ui.button("Save current view as bookmark").clicked() {
            ui.close();
            return Action::Save;
        }
        let mut action = Action::None;
        let bookmarks = &state.recovery.prefs.viewport.camera_bookmarks;
        if !bookmarks.is_empty() {
            ui.separator();
        }
        for (i, b) in bookmarks.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(&b.name).clicked() {
                    action = Action::Apply(i);
                    ui.close();
                }
                if ui.small_button(crate::icons::TRASH).on_hover_text("Delete bookmark").clicked() {
                    action = Action::Delete(i);
                }
            });
        }
        action
    });
    response.response.on_hover_text("Camera bookmarks and camera import");
    match response.inner.unwrap_or(Action::None) {
        Action::None => {}
        Action::Save => {
            let n = state.recovery.prefs.viewport.camera_bookmarks.len() + 1;
            let b = CameraBookmark::capture(format!("View {n}"), &state.camera);
            state.recovery.prefs.viewport.camera_bookmarks.push(b);
            state.set_status("Saved camera bookmark", now);
        }
        Action::Apply(i) => {
            if let Some(b) = state.recovery.prefs.viewport.camera_bookmarks.get(i).cloned() {
                b.apply(&mut state.camera);
                state.recovery.prefs.viewport.orthographic = b.orthographic;
            }
        }
        Action::Delete(i) => {
            let list = &mut state.recovery.prefs.viewport.camera_bookmarks;
            if i < list.len() {
                list.remove(i);
            }
        }
        Action::Import => import_camera(state, now),
    }
}

enum Action {
    None,
    Import,
    Save,
    Apply(usize),
    Delete(usize),
}

fn import_camera(state: &mut crate::state::UiState, now: f64) {
    let Some(path) = rfd::FileDialog::new().set_title("Import camera").add_filter("Camera (glTF, GLB, JSON)", &["gltf", "glb", "json"]).pick_file() else { return };
    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| subcraft_mesh::camera::parse_any(&bytes).map_err(|e| e.to_string())) {
        Ok(cameras) => {
            let center = state.session.scene().mesh.bounds.center();
            let bookmarks: Vec<_> = cameras.iter().map(|c| CameraBookmark::from_spec(c, center, &state.camera)).collect();
            if let Some(first) = bookmarks.first() {
                first.apply(&mut state.camera);
                state.recovery.prefs.viewport.orthographic = first.orthographic;
            }
            let count = bookmarks.len();
            state.recovery.prefs.viewport.camera_bookmarks.extend(bookmarks);
            state.set_status(format!("Imported {count} camera(s); the first one is active, all are in the Camera menu"), now);
        }
        Err(e) => state.set_status(format!("Camera import: {e}"), now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::Aabb;

    fn model() -> (OrbitCamera, Vec3) {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        (OrbitCamera::framing(&b), b.center())
    }

    #[test]
    fn imported_camera_puts_the_eye_at_the_position_looking_along_its_axis() {
        let (mut cam, center) = model();
        let spec = &subcraft_mesh::camera::parse_json(r#"{"name":"c","position":[4,2,6],"target":[0,0,0],"fov":25}"#).unwrap()[0];
        let b = CameraBookmark::from_spec(spec, center, &cam);
        b.apply(&mut cam);
        assert!((cam.eye() - Vec3::new(4.0, 2.0, 6.0)).length() < 1e-4, "{:?}", cam.eye());
        assert_eq!(cam.fov_y_deg, 25.0);
        // The model's center is exactly in the middle of the screen.
        let s = cam.paint_camera([800, 600]).project(center).unwrap();
        assert!((s.x - 400.0).abs() < 0.5 && (s.y - 300.0).abs() < 0.5, "{s:?}");
    }

    #[test]
    fn camera_looking_away_still_gives_a_usable_view_and_rolls_are_dropped() {
        let (mut cam, center) = model();
        // Behind the model, looking away from it, and rolled 40 degrees.
        let rot = glam::Quat::from_rotation_y(std::f32::consts::PI) * glam::Quat::from_rotation_z(40f32.to_radians());
        let spec = CameraSpec { name: "away".into(), position: Vec3::new(0.0, 0.0, -9.0), rotation: rot, fov_y_deg: 50.0, orthographic: false };
        let b = CameraBookmark::from_spec(&spec, center, &cam);
        assert!(b.distance >= cam.scene_radius * 0.05 && b.distance.is_finite());
        b.apply(&mut cam);
        assert!(cam.eye().is_finite() && cam.view().is_finite());
        // Looks along the camera's axis even though that points away from the model.
        let fwd = (cam.target - cam.eye()).normalize();
        assert!((fwd - spec.forward()).length() < 1e-4, "{fwd:?} vs {:?}", spec.forward());
    }

    #[test]
    fn capture_and_apply_round_trip_through_json() {
        let (cam, _) = model();
        let mut moved = cam;
        moved.orbit(120.0, -40.0);
        moved.zoom(0.5);
        let saved = CameraBookmark::capture("shot", &moved);
        let json = serde_json::to_string(&saved).unwrap();
        let loaded: CameraBookmark = serde_json::from_str(&json).unwrap();
        let mut restored = cam;
        loaded.apply(&mut restored);
        assert_eq!(restored, moved);
    }

    #[test]
    fn bad_bookmarks_do_not_move_the_camera() {
        let (mut cam, _) = model();
        let before = cam;
        let mut b = CameraBookmark::capture("x", &cam);
        b.target = [f32::NAN, 0.0, 0.0];
        b.apply(&mut cam);
        b.target = [0.0; 3];
        b.fov_y_deg = 0.0;
        b.apply(&mut cam);
        assert_eq!(cam, before);
    }

    #[test]
    fn old_preference_files_get_wireframe_and_bookmark_defaults() {
        let prefs: ViewportPrefs = serde_json::from_str(r#"{"scale_percent":50}"#).unwrap();
        assert_eq!(prefs.scale_percent, 50);
        assert!(!prefs.wireframe && prefs.camera_bookmarks.is_empty());
        assert!((0.0..=1.0).contains(&prefs.wire_opacity));
        let lin = wire_color_linear(&prefs);
        assert!(lin.iter().all(|c| (0.0..=1.0).contains(c)));
    }
}
