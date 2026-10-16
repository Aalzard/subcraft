//! Orbit camera for the 3D viewport, shared by rendering and painting (identical matrices).

use glam::{Mat4, Vec3};
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
}

impl OrbitCamera {
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
        }
    }

    /// Distance from the target to the eye. Orthographic views keep the eye outside the scene
    /// so zooming in never clips the model.
    fn eye_distance(&self) -> f32 {
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

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
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

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch + dy * 0.008).clamp(-1.55, 1.55);
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
        assert_eq!(vp.proj * vp.view, pc.view_proj());
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
        assert_eq!(vp.proj * vp.view, pc.view_proj());
    }
}
