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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cameras_round_trip_and_bad_numbers_are_flagged() {
        let c = SceneCamera { name: "Shot 1".into(), position: [1.0, 2.0, 3.0], rotation: [0.0, 0.7071, 0.0, 0.7071], fov_y_deg: 30.0, orthographic: false, aspect: Some(1.5) };
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
