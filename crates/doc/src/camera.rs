//! Cameras imported from the project's mesh file (Painter's camera dropdown in the 3D view).
//! Plain data saved with the project; the viewport turns one into its orbit camera.

use serde::{Deserialize, Serialize};

/// One camera of the mesh file: where it is, where it looks and its lens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneCamera {
    pub name: String,
    /// World position.
    pub position: [f32; 3],
    /// Orientation as a quaternion `[x, y, z, w]`; the camera looks down its local -Z, +Y up.
    pub rotation: [f32; 4],
    /// Vertical field of view in degrees.
    pub fov_y_deg: f32,
    pub orthographic: bool,
    /// Picture aspect (width / height) when the file gives one (Painter's "Display ratio").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect: Option<f32>,
}

impl Default for SceneCamera {
    fn default() -> Self {
        Self { name: "Camera".into(), position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], fov_y_deg: 40.0, orthographic: false, aspect: None }
    }
}

impl SceneCamera {
    /// Whether every number is usable (finite, a non-zero rotation, a field of view in 1..170).
    pub fn is_valid(&self) -> bool {
        self.position.iter().chain(&self.rotation).all(|v| v.is_finite())
            && self.rotation.iter().any(|v| *v != 0.0)
            && (1.0..=170.0).contains(&self.fov_y_deg)
            && self.aspect.is_none_or(|a| a.is_finite() && a > 0.0)
    }
}

/// The 3D view's own camera and the 2D view's pan and zoom, saved with the project so it opens
/// where it was left (issue #321). Orbit form: the eye sits `distance` from `target`, turned
/// `yaw` about the vertical, `pitch` above the horizon and `roll` about the view axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Distance from the target to the eye (the zoom; the view size when orthographic).
    pub distance: f32,
    /// Vertical field of view in degrees.
    pub fov_y_deg: f32,
    pub orthographic: bool,
    /// The point orbits turn around (Painter: the point under the cursor when the orbit began).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f32; 3]>,
    /// 2D view zoom and pan (points).
    pub uv_zoom: f32,
    pub uv_offset: [f32; 2],
}

impl Default for ViewState {
    fn default() -> Self {
        Self { target: [0.0; 3], yaw: 0.0, pitch: 0.0, roll: 0.0, distance: 1.0, fov_y_deg: 40.0, orthographic: false, pivot: None, uv_zoom: 1.0, uv_offset: [0.0; 2] }
    }
}

impl ViewState {
    /// Whether it can be restored: finite numbers, a positive distance and 2D zoom, a field of
    /// view in 1..170. A broken one is ignored and the project opens with the default framing.
    pub fn is_valid(&self) -> bool {
        let finite = self.target.iter().chain(&self.uv_offset).chain(self.pivot.iter().flatten()).chain([&self.yaw, &self.pitch, &self.roll, &self.distance, &self.uv_zoom]).all(|v| v.is_finite());
        finite && self.distance > 0.0 && self.uv_zoom > 0.0 && (1.0..=170.0).contains(&self.fov_y_deg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_states_round_trip_and_broken_ones_are_flagged() {
        let v = ViewState { target: [0.5, 1.0, -2.0], yaw: 0.7, pitch: -0.3, roll: 0.1, distance: 6.5, fov_y_deg: 35.0, orthographic: true, pivot: Some([1.0, 2.0, 3.0]), uv_zoom: 2.5, uv_offset: [-40.0, 12.0] };
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<ViewState>(&text).unwrap(), v);
        assert!(v.is_valid());
        assert!(ViewState::default().is_valid());
        assert_eq!(serde_json::from_str::<ViewState>("{}").unwrap(), ViewState::default());
        assert!(!ViewState { distance: 0.0, ..v }.is_valid());
        assert!(!ViewState { fov_y_deg: 0.5, ..v }.is_valid());
        assert!(!ViewState { uv_zoom: -1.0, ..v }.is_valid());
        assert!(!ViewState { target: [f32::NAN, 0.0, 0.0], ..v }.is_valid());
        assert!(!ViewState { pivot: Some([f32::INFINITY, 0.0, 0.0]), ..v }.is_valid());
    }

    #[test]
    fn cameras_round_trip_and_bad_numbers_are_flagged() {
        let c = SceneCamera { name: "Shot 1".into(), position: [1.0, 2.0, 3.0], rotation: [0.0, std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2], fov_y_deg: 30.0, orthographic: false, aspect: Some(1.5) };
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<SceneCamera>(&text).unwrap(), c);
        assert!(c.is_valid());
        let empty: SceneCamera = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, SceneCamera::default());
        assert!(empty.is_valid());
        assert!(!SceneCamera { fov_y_deg: 0.0, ..c.clone() }.is_valid());
        assert!(!SceneCamera { rotation: [0.0; 4], ..c.clone() }.is_valid());
        assert!(!SceneCamera { position: [f32::NAN, 0.0, 0.0], ..c.clone() }.is_valid());
        assert!(!SceneCamera { aspect: Some(-1.0), ..c }.is_valid());
    }
}
