//! Orbit camera for the 3D viewport, shared by rendering and painting (identical matrices).

use glam::{EulerRot, Mat4, Quat, Vec3};
use subcraft_app::Camera;
use subcraft_mesh::Aabb;
use subcraft_render::ViewParams;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
    /// Scene scale for clip planes.
    pub scene_radius: f32,
    /// Orthographic projection: `distance` sets the view size and the eye stays behind the scene.
    pub orthographic: bool,
    /// Radians around the view axis (Painter's free rotation; 0 keeps the horizon level).
    pub roll: f32,
}

/// A fixed view for scripted screenshots (`--camera yaw,pitch,zoom[,x,y,z]`): angles in degrees,
/// zoom divides the framing distance, the target moves by (x, y, z) times the box's half diagonal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub zoom: f32,
    pub offset: Vec3,
}

impl CameraView {
    pub fn parse(s: &str) -> Option<Self> {
        let v: Vec<f32> = s.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
        let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
        (v.len() == 3 || v.len() == 6).then(|| Self { yaw_deg: v[0], pitch_deg: v[1], zoom: v[2].max(1e-3), offset: Vec3::new(at(3), at(4), at(5)) })
    }
}

impl OrbitCamera {
    /// [`OrbitCamera::framing`], then `view` applied when given.
    pub fn framing_with(bounds: &Aabb, view: Option<&CameraView>) -> Self {
        let mut c = Self::framing(bounds);
        if let Some(v) = view {
            c.yaw = v.yaw_deg.to_radians();
            c.pitch = v.pitch_deg.to_radians().clamp(-1.55, 1.55);
            c.distance /= v.zoom;
            c.target += v.offset * c.scene_radius;
        }
        c
    }

    /// Frame a bounding box from a three-quarter view.
    pub fn framing(bounds: &Aabb) -> Self {
        let radius = (bounds.diagonal() * 0.5).max(1e-3);
        let fov = 40.0f32;
        Self {
            target: bounds.center(),
            yaw: 35f32.to_radians(),
            pitch: 22f32.to_radians(),
            distance: radius / (fov.to_radians() * 0.5).sin() * 1.1,
            fov_y_deg: fov,
            scene_radius: radius,
            orthographic: false,
            roll: 0.0,
        }
    }

    /// Distance from the target to the eye. Orthographic views keep the eye outside the scene
    /// so zooming in never clips the model.
    pub fn eye_distance(&self) -> f32 {
        if self.orthographic { self.distance + self.scene_radius * 3.0 } else { self.distance }
    }

    /// Half the visible height at the target (orthographic view size; matches perspective there).
    fn half_height(&self) -> f32 {
        self.distance * (self.fov_y_deg.to_radians() * 0.5).tan()
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(sy * cp, sp, cy * cp) * self.eye_distance()
    }

    /// Camera orientation: looking down its -Z, +Y up (yaw about the world vertical, then
    /// pitch, then roll about the view axis).
    pub fn orientation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch) * Quat::from_rotation_z(self.roll)
    }

    pub fn view(&self) -> Mat4 {
        // A level camera keeps the world up (bit for bit the matrices it always had); a rolled or
        // overturned one (free rotation) takes its own up.
        let up = if self.roll == 0.0 && self.pitch.abs() < std::f32::consts::FRAC_PI_2 { Vec3::Y } else { self.orientation() * Vec3::Y };
        Mat4::look_at_rh(self.eye(), self.target, up)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let d = self.eye_distance();
        let near = (d - self.scene_radius * 2.0).max(d * 0.01).max(1e-4);
        let far = d + self.scene_radius * 4.0;
        if self.orthographic {
            let (h, w) = (self.half_height(), self.half_height() * aspect.max(1e-3));
            return Mat4::orthographic_rh(-w, w, -h, h, near, far);
        }
        Mat4::perspective_rh(self.fov_y_deg.to_radians(), aspect.max(1e-3), near, far)
    }

    /// Paint camera for a viewport of `size` physical pixels.
    pub fn paint_camera(&self, size: [u32; 2]) -> Camera {
        Camera { view: self.view(), proj: self.proj(size[0] as f32 / size[1].max(1) as f32), viewport: size }
    }

    pub fn view_params(&self, size: [u32; 2]) -> ViewParams {
        let mut p = ViewParams::new(self.view(), self.proj(size[0] as f32 / size[1].max(1) as f32));
        // Key light follows the camera loosely (over the viewer's right shoulder).
        let (sy, cy) = (self.yaw + 0.6).sin_cos();
        p.light_dir = Vec3::new(sy * 0.6, 0.75, cy * 0.6).normalize();
        p
    }

    /// Constrained rotation (Painter's default): about the world vertical and the horizon, which
    /// stays level (a roll left by free rotation is undone).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.roll = 0.0;
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.55, 1.55);
    }

    /// Free rotation: about the camera's own up and right axes, so the view can roll and go over
    /// the top of the model.
    pub fn free_orbit(&mut self, dx: f32, dy: f32) {
        let q = (self.orientation() * Quat::from_rotation_y(-dx * 0.008) * Quat::from_rotation_x(-dy * 0.008)).normalize();
        let (yaw, x, roll) = q.to_euler(EulerRot::YXZ);
        if yaw.is_finite() && x.is_finite() && roll.is_finite() {
            self.yaw = yaw;
            self.pitch = -x;
            self.roll = roll;
        }
    }

    /// [`OrbitCamera::free_orbit`] turning about `pivot`.
    pub fn free_orbit_around(&mut self, pivot: Vec3, dx: f32, dy: f32) {
        let before = glam::Mat3::from_mat4(self.view()).transpose();
        self.free_orbit(dx, dy);
        self.turn_about(pivot, before);
    }

    /// [`OrbitCamera::orbit`] around `pivot` instead of the target: the camera turns rigidly about
    /// that point, so it stays where it is on screen (Painter orbits around the point under the
    /// cursor, issue #115). With `pivot == target` this is exactly `orbit`.
    pub fn orbit_around(&mut self, pivot: Vec3, dx: f32, dy: f32) {
        let before = glam::Mat3::from_mat4(self.view()).transpose();
        self.orbit(dx, dy);
        self.turn_about(pivot, before);
    }

    /// Set the view angles, turning about `pivot` (snapped orbit).
    pub fn set_angles_around(&mut self, pivot: Vec3, yaw: f32, pitch: f32) {
        let before = glam::Mat3::from_mat4(self.view()).transpose();
        self.yaw = yaw;
        self.pitch = pitch.clamp(-1.55, 1.55);
        self.turn_about(pivot, before);
    }

    /// Move the target by the rotation that took the camera axes `before` to the current ones.
    fn turn_about(&mut self, pivot: Vec3, before: glam::Mat3) {
        let after = glam::Mat3::from_mat4(self.view()).transpose();
        let turn = after * before.transpose();
        self.target = pivot + turn * (self.target - pivot);
    }

    /// Pan by a screen delta in pixels (keeps the point under the cursor fixed at the target depth).
    pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
        let units_per_px = 2.0 * self.half_height() / viewport_height.max(1.0);
        let view = self.view().inverse();
        let right = view.x_axis.truncate();
        let up = view.y_axis.truncate();
        self.target += (-right * dx + up * dy) * units_per_px;
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(self.scene_radius * 0.05, self.scene_radius * 50.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_sees_the_whole_box_and_paint_camera_matches_render() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let cam = OrbitCamera::framing(&b);
        let pc = cam.paint_camera([800, 600]);
        for corner in [b.min, b.max, Vec3::new(1.0, -1.0, 1.0), Vec3::new(-1.0, 1.0, -1.0)] {
            let s = pc.project(corner).unwrap();
            assert!(s.x > 0.0 && s.x < 800.0 && s.y > 0.0 && s.y < 600.0, "{corner} -> {s}");
        }
        let vp = cam.view_params([800, 600]);
        // Same matrix up to rounding (sin/cos of constant angles may be folded at compile time on one path).
        assert!((vp.proj * vp.view).abs_diff_eq(pc.view_proj(), 1e-5));
    }

    #[test]
    fn camera_view_from_the_command_line() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let v = CameraView::parse("90, 60, 2, 0.5, 0, 0").unwrap();
        let cam = OrbitCamera::framing_with(&b, Some(&v));
        let plain = OrbitCamera::framing(&b);
        assert!((cam.yaw - 90f32.to_radians()).abs() < 1e-6 && (cam.pitch - 60f32.to_radians()).abs() < 1e-6);
        assert!((cam.distance * 2.0 - plain.distance).abs() < 1e-5);
        assert!((cam.target - Vec3::new(0.5 * plain.scene_radius, 0.0, 0.0)).length() < 1e-6);
        assert_eq!(OrbitCamera::framing_with(&b, None), plain);
        assert!(CameraView::parse("1,2").is_none() && CameraView::parse("a,b,c").is_none());
    }

    #[test]
    fn free_rotation_rolls_and_goes_over_the_top_and_constrained_levels_it_again() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let level = OrbitCamera::framing(&b);
        // A level camera keeps the world up: the matrix it always had (the test's own call can
        // round differently once inlined, so compare to within float noise).
        assert!(level.view().abs_diff_eq(Mat4::look_at_rh(level.eye(), level.target, Vec3::Y), 1e-5));
        assert!((level.orientation() * Vec3::NEG_Z - (level.target - level.eye()).normalize()).length() < 1e-5, "orientation looks at the target");
        // Free rotation over the top: the eye stays at its distance and the target stays centred.
        let mut cam = level;
        for _ in 0..60 {
            cam.free_orbit(0.0, 10.0);
            let pc = cam.paint_camera([800, 600]);
            assert!(pc.view_proj().is_finite());
            assert!(((cam.eye() - cam.target).length() - cam.eye_distance()).abs() < 1e-3);
            let s = pc.project(cam.target).unwrap();
            assert!((s.x - 400.0).abs() < 1e-2 && (s.y - 300.0).abs() < 1e-2, "target off centre: {s}");
        }
        // Horizontal free rotation with a pitch rolls the view.
        let mut tilted = level;
        tilted.free_orbit(0.0, 60.0);
        tilted.free_orbit(120.0, 0.0);
        assert!(tilted.roll.abs() > 1e-3, "rotating about the camera's own up after a pitch rolls it: {}", tilted.roll);
        // Constrained rotation levels the horizon.
        tilted.orbit(1.0, 0.0);
        assert_eq!(tilted.roll, 0.0);
        assert!(tilted.view().abs_diff_eq(Mat4::look_at_rh(tilted.eye(), tilted.target, Vec3::Y), 1e-5));
    }

    #[test]
    fn orbit_pan_zoom() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let mut cam = OrbitCamera::framing(&b);
        let d = cam.distance;
        cam.zoom(0.5);
        assert!((cam.distance - d * 0.5).abs() < 1e-5);
        cam.orbit(0.0, 10_000.0);
        assert!(cam.pitch <= 1.55);
        let t = cam.target;
        cam.pan(10.0, 0.0, 600.0);
        assert!(cam.target != t);
        assert!((cam.eye() - cam.target).length() - cam.distance < 1e-4);
    }

    #[test]
    fn orbiting_around_a_point_keeps_it_where_it_is_on_screen() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let mut cam = OrbitCamera::framing(&b);
        let pivot = Vec3::new(0.7, -0.4, 0.9);
        let before = cam.paint_camera([800, 600]).project(pivot).unwrap();
        let depth = cam.paint_camera([800, 600]).view_depth(pivot);
        for (dx, dy) in [(40.0, 0.0), (-15.0, 25.0), (120.0, -60.0)] {
            cam.orbit_around(pivot, dx, dy);
            let pc = cam.paint_camera([800, 600]);
            let now = pc.project(pivot).unwrap();
            assert!((now.truncate() - before.truncate()).length() < 0.05, "{before} -> {now}");
            assert!((pc.view_depth(pivot) - depth).abs() < 1e-3, "the distance to the pivot is kept");
        }
        // Around the target it is the plain orbit.
        let (mut a, mut c) = (OrbitCamera::framing(&b), OrbitCamera::framing(&b));
        a.orbit(30.0, 12.0);
        c.orbit_around(c.target, 30.0, 12.0);
        assert!((a.target - c.target).length() < 1e-5 && a.yaw == c.yaw && a.pitch == c.pitch);
        // Snapped angles turn about the pivot too.
        let mut s = OrbitCamera::framing(&b);
        let at = s.paint_camera([800, 600]).project(pivot).unwrap();
        s.set_angles_around(pivot, std::f32::consts::FRAC_PI_2, 0.0);
        assert!((s.paint_camera([800, 600]).project(pivot).unwrap().truncate() - at.truncate()).length() < 0.05);
        assert_eq!((s.yaw, s.pitch), (std::f32::consts::FRAC_PI_2, 0.0));
    }

    #[test]
    fn orthographic_keeps_size_at_the_target_and_never_clips_when_zoomed_in() {
        let b = Aabb { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        let persp = OrbitCamera::framing(&b);
        let ortho = OrbitCamera { orthographic: true, ..persp };
        let (pp, po) = (persp.paint_camera([800, 600]), ortho.paint_camera([800, 600]));
        assert!(po.is_orthographic() && !pp.is_orthographic());
        // Same on-screen size at the target, so switching doesn't jump.
        let (a, c) = (po.pixels_per_unit(1.0), pp.pixels_per_unit(persp.distance));
        assert!((a - c).abs() / c < 1e-3, "{a} vs {c}");
        // Zoomed far in, every corner is still in front of the camera.
        let mut close = ortho;
        close.zoom(0.05);
        let pc = close.paint_camera([800, 600]);
        for corner in [b.min, b.max] {
            let clip = pc.view_proj() * corner.extend(1.0);
            let depth = clip.z / clip.w;
            assert!((0.0..1.0).contains(&depth), "{corner} clip depth {depth}");
        }
        // Painting and rendering still share one matrix.
        let vp = close.view_params([800, 600]);
        // Same matrix up to rounding (sin/cos of constant angles may be folded at compile time on one path).
        assert!((vp.proj * vp.view).abs_diff_eq(pc.view_proj(), 1e-5));
    }
}
