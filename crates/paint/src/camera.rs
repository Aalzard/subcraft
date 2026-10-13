//! Viewport camera as seen by the paint engine.
//!
//! Screen space: pixels, origin top-left, y down. Projection matrices follow wgpu conventions
//! (right-handed view looking down −Z, NDC depth 0..1).

use glam::{Mat4, Vec2, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Object space → view space.
    pub view: Mat4,
    pub proj: Mat4,
    /// Viewport size in pixels.
    pub viewport: [u32; 2],
}

impl Camera {
    pub fn perspective(eye: Vec3, target: Vec3, up: Vec3, fov_y_deg: f32, near: f32, far: f32, viewport: [u32; 2]) -> Self {
        let aspect = viewport[0] as f32 / viewport[1].max(1) as f32;
        Self {
            view: Mat4::look_at_rh(eye, target, up),
            proj: Mat4::perspective_rh(fov_y_deg.to_radians(), aspect, near, far),
            viewport,
        }
    }

    /// Orthographic camera showing `half_height` object units above and below the target.
    pub fn orthographic(eye: Vec3, target: Vec3, up: Vec3, half_height: f32, near: f32, far: f32, viewport: [u32; 2]) -> Self {
        let aspect = viewport[0] as f32 / viewport[1].max(1) as f32;
        let hw = half_height * aspect;
        Self {
            view: Mat4::look_at_rh(eye, target, up),
            proj: Mat4::orthographic_rh(-hw, hw, -half_height, half_height, near, far),
            viewport,
        }
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }

    pub fn is_orthographic(&self) -> bool {
        self.proj.w_axis.w == 1.0
    }

    pub fn eye(&self) -> Vec3 {
        self.view.inverse().w_axis.truncate()
    }

    pub fn forward(&self) -> Vec3 {
        -self.view.inverse().z_axis.truncate().normalize_or_zero()
    }

    pub fn right(&self) -> Vec3 {
        self.view.inverse().x_axis.truncate().normalize_or_zero()
    }

    /// Distance in front of the camera along the view axis.
    #[inline]
    pub fn view_depth(&self, p: Vec3) -> f32 {
        -self.view.transform_point3(p).z
    }

    /// Screen position (pixels) and view depth of an object-space point; `None` behind the eye.
    #[inline]
    pub fn project(&self, p: Vec3) -> Option<Vec3> {
        let clip = self.view_proj() * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let s = self.ndc_to_screen(Vec2::new(ndc.x, ndc.y));
        Some(Vec3::new(s.x, s.y, self.view_depth(p)))
    }

    #[inline]
    pub fn ndc_to_screen(&self, ndc: Vec2) -> Vec2 {
        Vec2::new((ndc.x * 0.5 + 0.5) * self.viewport[0] as f32, (0.5 - ndc.y * 0.5) * self.viewport[1] as f32)
    }

    /// Object-space ray (origin on the near plane, unit direction) through a screen position.
    pub fn ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(2.0 * screen.x / self.viewport[0] as f32 - 1.0, 1.0 - 2.0 * screen.y / self.viewport[1] as f32);
        let inv = self.view_proj().inverse();
        let unproject = |z: f32| {
            let v = inv * Vec4::new(ndc.x, ndc.y, z, 1.0);
            v.truncate() / v.w
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        (near, (far - near).normalize_or_zero())
    }

    /// Screen pixels per object unit at a view depth.
    pub fn pixels_per_unit(&self, depth: f32) -> f32 {
        let f = self.proj.y_axis.y * self.viewport[1] as f32 * 0.5;
        if self.is_orthographic() { f } else { f / depth.max(1e-6) }
    }

    /// Direction in which the camera projects onto `p`.
    pub fn projection_dir(&self, p: Vec3) -> Vec3 {
        if self.is_orthographic() { self.forward() } else { (p - self.eye()).normalize_or_zero() }
    }

    /// The camera mirrored across the plane `axis = center` (object space). Painting through
    /// the mirrored camera with the same screen positions yields mirrored stamps.
    pub fn mirrored(&self, axis: usize, center: f32) -> Camera {
        let mut r = Mat4::IDENTITY;
        r.col_mut(axis)[axis] = -1.0;
        r.w_axis[axis] = 2.0 * center;
        Camera { view: self.view * r, ..*self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_ray_agree() {
        let cam = Camera::perspective(Vec3::new(1.0, 2.0, 5.0), Vec3::ZERO, Vec3::Y, 45.0, 0.1, 100.0, [640, 480]);
        let p = Vec3::new(0.3, -0.2, 0.1);
        let s = cam.project(p).unwrap();
        let (o, d) = cam.ray(Vec2::new(s.x, s.y));
        let closest = o + d * (p - o).dot(d);
        assert!(closest.distance(p) < 1e-4, "{closest} vs {p}");
        assert!((s.z - cam.view_depth(p)).abs() < 1e-6);
        let center = cam.project(Vec3::ZERO).unwrap();
        assert!((center.x - 320.0).abs() < 1e-3 && (center.y - 240.0).abs() < 1e-3);
    }

    #[test]
    fn mirrored_camera_sees_mirrored_points_at_the_same_pixel() {
        let cam = Camera::perspective(Vec3::new(1.0, 2.0, 5.0), Vec3::ZERO, Vec3::Y, 45.0, 0.1, 100.0, [640, 480]);
        let m = cam.mirrored(0, 0.25);
        let p = Vec3::new(0.9, 0.3, -0.4);
        let mp = Vec3::new(2.0 * 0.25 - p.x, p.y, p.z);
        let (a, b) = (cam.project(p).unwrap(), m.project(mp).unwrap());
        assert!(a.distance(b) < 1e-3, "{a} vs {b}");
        assert!((m.eye() - Vec3::new(0.5 - 1.0, 2.0, 5.0)).length() < 1e-4);
    }

    #[test]
    fn orthographic_pixels_per_unit_is_constant() {
        let cam = Camera::orthographic(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 2.0, 0.1, 10.0, [400, 400]);
        assert!(cam.is_orthographic());
        assert_eq!(cam.pixels_per_unit(1.0), cam.pixels_per_unit(9.0));
        assert!((cam.pixels_per_unit(1.0) - 100.0).abs() < 1e-3);
    }
}
