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

// ----- imported cameras (Painter's camera dropdown) ----------------------------------------------

/// The spec of an imported project camera (for the orbit camera conversion).
pub fn scene_camera_spec(c: &subcraft_doc::SceneCamera) -> CameraSpec {
    CameraSpec {
        name: c.name.clone(),
        position: Vec3::from_array(c.position),
        rotation: glam::Quat::from_array(c.rotation).normalize(),
        fov_y_deg: c.fov_y_deg,
        orthographic: c.orthographic,
        aspect: c.aspect,
    }
}

/// Whether two orbit cameras show the same place (target, angles, distance), lens aside.
fn same_place(a: &OrbitCamera, b: &OrbitCamera) -> bool {
    let close = |x: f32, y: f32| (x - y).abs() <= 1e-4 * (1.0 + x.abs().max(y.abs()));
    (a.target - b.target).length() <= 1e-4 * (1.0 + a.target.length()) && close(a.yaw, b.yaw) && close(a.pitch, b.pitch) && close(a.distance, b.distance)
}

/// Whether the active imported camera's lens was changed since it was chosen (Painter shows its
/// name in italics with a star until Restore).
pub fn camera_modified(state: &crate::state::UiState) -> bool {
    match (state.active_camera, state.camera_applied) {
        (Some(_), Some(applied)) => (state.camera.fov_y_deg - applied.fov_y_deg).abs() > 1e-4 || state.camera.orthographic != applied.orthographic,
        _ => false,
    }
}

/// Look through imported camera `index`, or the Default camera (`None`, the view it had before
/// an imported camera was chosen).
pub fn select_camera(state: &mut crate::state::UiState, index: Option<usize>) {
    match index.and_then(|i| state.session.scene_cameras().get(i).cloned().map(|c| (i, c))) {
        Some((i, cam)) => {
            if state.active_camera.is_none() {
                state.default_camera = Some(state.camera);
            }
            let center = state.session.scene().mesh.bounds.center();
            CameraBookmark::from_spec(&scene_camera_spec(&cam), center, &state.camera).apply(&mut state.camera);
            state.recovery.prefs.viewport.orthographic = state.camera.orthographic;
            state.active_camera = Some(i);
            state.camera_applied = Some(state.camera);
        }
        None => {
            if state.active_camera.is_some()
                && let Some(default) = state.default_camera.take()
            {
                state.camera = default;
                state.recovery.prefs.viewport.orthographic = default.orthographic;
            }
            state.active_camera = None;
            state.camera_applied = None;
            state.camera_locked = false;
        }
    }
}

/// Restore: back to the imported camera's own values (undoes lens changes and navigation).
pub fn restore_camera(state: &mut crate::state::UiState) {
    if let Some(i) = state.active_camera {
        let locked = state.camera_locked;
        let default = state.default_camera;
        select_camera(state, Some(i));
        state.default_camera = default;
        state.camera_locked = locked;
    }
}

/// Next (+1) or previous (-1) camera, Default camera included, wrapping around.
pub fn cycle_camera(state: &mut crate::state::UiState, step: i32) {
    let n = state.session.scene_cameras().len() as i32;
    if n == 0 {
        return;
    }
    // Position 0 is the Default camera, 1..=n the imported ones.
    let at = state.active_camera.map_or(0, |i| i as i32 + 1);
    let next = (at + step).rem_euclid(n + 1);
    select_camera(state, if next == 0 { None } else { Some(next as usize - 1) });
}

/// After the 3D view's input: a locked camera snaps back to its imported place and lens; moving an
/// unlocked one hands over to the Default camera (Painter), keeping the new view.
pub fn follow_camera(state: &mut crate::state::UiState) {
    let (Some(i), Some(applied)) = (state.active_camera, state.camera_applied) else { return };
    if i >= state.session.scene_cameras().len() {
        // The cameras were replaced (another project, a mesh reload): forget the choice.
        state.active_camera = None;
        state.camera_applied = None;
        state.camera_locked = false;
        return;
    }
    if state.camera_locked {
        state.camera = applied;
        state.recovery.prefs.viewport.orthographic = applied.orthographic;
    } else if !same_place(&state.camera, &applied) {
        state.active_camera = None;
        state.camera_applied = None;
    }
}

/// Name shown for camera choice `index` (None = Default camera), with Painter's star when the
/// active imported camera was modified.
pub fn camera_label(state: &crate::state::UiState, index: Option<usize>) -> String {
    match index.and_then(|i| state.session.scene_cameras().get(i)) {
        None => tr("Default camera").to_string(),
        Some(c) if state.active_camera == index && camera_modified(state) => format!("{}*", c.name),
        Some(c) => c.name.clone(),
    }
}

/// The camera dropdown (3D view corner and Display Settings > Camera). Nothing when the project
/// has no imported cameras, like Painter.
pub fn camera_dropdown(ui: &mut Ui, state: &mut crate::state::UiState, id: &str) {
    let count = state.session.scene_cameras().len();
    if count == 0 {
        return;
    }
    let mut chosen = state.active_camera;
    let selected = camera_label(state, chosen);
    let italic = camera_modified(state);
    let text = if italic { RichText::new(selected).italics() } else { RichText::new(selected) };
    egui::ComboBox::from_id_salt(id).selected_text(text).width(150.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut chosen, None, tr("Default camera"));
        for i in 0..count {
            ui.selectable_value(&mut chosen, Some(i), camera_label(state, Some(i)));
        }
    }).response.on_hover_text(tr("Look through a camera imported with the model; navigating switches back to the Default camera unless it is locked"));
    if chosen != state.active_camera {
        select_camera(state, chosen);
    }
}

use crate::i18n::tr;

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
mod camera_tests {
    use super::*;

    fn state_with_cameras() -> crate::state::UiState {
        let mut state = crate::state::UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let dir = std::env::temp_dir().join(format!("subcraft-ui-cams-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gltf = dir.join("cams.gltf");
        std::fs::write(&gltf, r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
            "nodes":[{"name":"Front","camera":0,"translation":[0,0,6]},{"name":"Top","camera":1,"translation":[0,6,0],"rotation":[-0.7071068,0,0,0.7071068]}],
            "cameras":[{"type":"perspective","perspective":{"yfov":0.6,"znear":0.1}},{"type":"perspective","perspective":{"yfov":0.9,"znear":0.1,"aspectRatio":2.0}}]}"#).unwrap();
        assert_eq!(state.session.import_mesh_cameras(&gltf).unwrap(), 2);
        let _ = std::fs::remove_dir_all(&dir);
        state
    }

    #[test]
    fn choosing_a_camera_moves_the_view_and_the_default_camera_comes_back() {
        let mut state = state_with_cameras();
        let start = state.camera;
        select_camera(&mut state, Some(0));
        assert_eq!(state.active_camera, Some(0));
        assert!((state.camera.fov_y_deg - 0.6f32.to_degrees()).abs() < 1e-3, "the camera's lens");
        let eye = state.camera.eye();
        assert!((eye - Vec3::new(0.0, 0.0, 6.0)).length() < 1e-3, "the camera's position: {eye:?}");
        // Next / previous wrap through the Default camera.
        cycle_camera(&mut state, 1);
        assert_eq!(state.active_camera, Some(1));
        cycle_camera(&mut state, 1);
        assert_eq!(state.active_camera, None, "after the last camera comes the Default camera");
        assert_eq!(state.camera, start, "with the view it had before");
        cycle_camera(&mut state, -1);
        assert_eq!(state.active_camera, Some(1));
        assert_eq!(camera_label(&state, None), "Default camera");
    }

    #[test]
    fn navigating_hands_over_to_the_default_camera_unless_locked_and_restore_undoes_lens_changes() {
        let mut state = state_with_cameras();
        select_camera(&mut state, Some(0));
        // A lens change keeps the camera and marks it modified; Restore puts it back.
        state.camera.fov_y_deg = 70.0;
        follow_camera(&mut state);
        assert_eq!(state.active_camera, Some(0));
        assert!(camera_modified(&state) && camera_label(&state, Some(0)) == "Front*");
        restore_camera(&mut state);
        assert!(!camera_modified(&state) && camera_label(&state, Some(0)) == "Front");
        // Locked: navigation is undone at once.
        state.camera_locked = true;
        let applied = state.camera;
        state.camera.yaw += 0.5;
        follow_camera(&mut state);
        assert_eq!((state.active_camera, state.camera), (Some(0), applied));
        // Unlocked: orbiting switches to the Default camera and keeps the new view.
        state.camera_locked = false;
        state.camera.yaw += 0.5;
        let moved = state.camera;
        follow_camera(&mut state);
        assert_eq!((state.active_camera, state.camera), (None, moved));
        // Cameras that disappear (a mesh reload without them) drop the choice.
        select_camera(&mut state, Some(1));
        let obj = std::env::temp_dir().join(format!("subcraft-ui-nocam-{}.obj", std::process::id()));
        std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        state.session.import_mesh_cameras(&obj).unwrap();
        let _ = std::fs::remove_file(&obj);
        follow_camera(&mut state);
        assert_eq!(state.active_camera, None);
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
        let spec = CameraSpec { name: "away".into(), position: Vec3::new(0.0, 0.0, -9.0), rotation: rot, fov_y_deg: 50.0, orthographic: false, aspect: None };
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
