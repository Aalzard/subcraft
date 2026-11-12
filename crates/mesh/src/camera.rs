//! Camera import: a position, a rotation and a field of view read from a glTF or FBX file (on its
//! own, or the mesh file of a project) or a small JSON file. Pure parsing, no viewport types.
//!
//! JSON forms accepted: one camera object, an array of them, or `{"cameras": [...]}`. A camera is
//! `{"name": "front", "position": [x,y,z], <orientation>, "fov": 40}` where the orientation is one of
//! `"rotation": [x,y,z,w]` (quaternion), `"euler_deg": [x,y,z]` (rotations about X, then Y, then Z,
//! in degrees) or `"target": [x,y,z]` (look at this point, +Y up). `fov` is the vertical field of view
//! in degrees (default 40). A camera looks down its local -Z axis, like glTF.

use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Debug, PartialEq)]
pub struct CameraSpec {
    pub name: String,
    pub position: Vec3,
    /// Camera orientation; the camera looks along `rotation * -Z`.
    pub rotation: Quat,
    pub fov_y_deg: f32,
    pub orthographic: bool,
    /// Picture aspect (width / height) when the file gives one.
    pub aspect: Option<f32>,
}

impl CameraSpec {
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("not valid JSON: {0}")]
    Json(String),
    #[error("not a valid glTF file: {0}")]
    Gltf(String),
    #[error("camera {0}: {1}")]
    Field(usize, String),
    #[error("not a valid FBX file: {0}")]
    Fbx(String),
    #[error("cannot read the file: {0}")]
    Read(String),
    #[error("the file contains no cameras")]
    NoCameras,
}

fn vec3(v: &serde_json::Value, i: usize, what: &str) -> Result<Vec3, CameraError> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(|| CameraError::Field(i, format!("\"{what}\" must be three numbers")))?;
    let mut out = [0.0f32; 3];
    for (o, n) in out.iter_mut().zip(a) {
        *o = n.as_f64().filter(|x| x.is_finite()).ok_or_else(|| CameraError::Field(i, format!("\"{what}\" must be three numbers")))? as f32;
    }
    Ok(Vec3::from_array(out))
}

fn one_json(v: &serde_json::Value, i: usize) -> Result<CameraSpec, CameraError> {
    let position = vec3(v.get("position").ok_or_else(|| CameraError::Field(i, "missing \"position\"".into()))?, i, "position")?;
    let rotation = if let Some(q) = v.get("rotation") {
        let a = q.as_array().filter(|a| a.len() == 4).ok_or_else(|| CameraError::Field(i, "\"rotation\" must be four numbers x, y, z, w".into()))?;
        let f: Vec<f32> = a.iter().filter_map(|n| n.as_f64().map(|x| x as f32)).collect();
        if f.len() != 4 || f.iter().any(|x| !x.is_finite()) || f.iter().all(|x| *x == 0.0) {
            return Err(CameraError::Field(i, "\"rotation\" must be a non-zero quaternion".into()));
        }
        Quat::from_xyzw(f[0], f[1], f[2], f[3]).normalize()
    } else if let Some(e) = v.get("euler_deg") {
        let e = vec3(e, i, "euler_deg")?;
        Quat::from_euler(glam::EulerRot::XYZ, e.x.to_radians(), e.y.to_radians(), e.z.to_radians())
    } else if let Some(t) = v.get("target") {
        let dir = vec3(t, i, "target")? - position;
        if dir.length() < 1e-9 {
            return Err(CameraError::Field(i, "\"target\" equals \"position\"".into()));
        }
        look_rotation(dir)
    } else {
        Quat::IDENTITY
    };
    let fov = match v.get("fov") {
        None => 40.0,
        Some(f) => f.as_f64().map(|x| x as f32).filter(|x| *x > 0.0 && *x < 180.0).ok_or_else(|| CameraError::Field(i, "\"fov\" must be between 0 and 180 degrees".into()))?,
    };
    Ok(CameraSpec {
        aspect: None,
        name: v.get("name").and_then(|n| n.as_str()).map(str::to_owned).unwrap_or_else(|| format!("Camera {}", i + 1)),
        position,
        rotation,
        fov_y_deg: fov,
        orthographic: v.get("orthographic").and_then(|b| b.as_bool()).unwrap_or(false),
    })
}

/// Rotation whose -Z axis points along `dir`, with +Y as far up as possible.
pub fn look_rotation(dir: Vec3) -> Quat {
    let f = dir.normalize();
    let up = if f.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
    let r = up.cross(-f).normalize();
    let u = (-f).cross(r);
    Quat::from_mat3(&glam::Mat3::from_cols(r, u, -f))
}

pub fn parse_json(text: &str) -> Result<Vec<CameraSpec>, CameraError> {
    let root: serde_json::Value = serde_json::from_str(text).map_err(|e| CameraError::Json(e.to_string()))?;
    let list: Vec<&serde_json::Value> = match &root {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Object(o) => match o.get("cameras") {
            Some(serde_json::Value::Array(a)) => a.iter().collect(),
            Some(_) => return Err(CameraError::Json("\"cameras\" must be an array".into())),
            None => vec![&root],
        },
        _ => return Err(CameraError::Json("expected an object or an array".into())),
    };
    if list.is_empty() {
        return Err(CameraError::NoCameras);
    }
    list.into_iter().enumerate().map(|(i, v)| one_json(v, i)).collect()
}

/// Cameras of a glTF / GLB file (bytes), with node transforms applied. Buffers are not needed.
pub fn parse_gltf(bytes: &[u8]) -> Result<Vec<CameraSpec>, CameraError> {
    let gltf = crate::import::open_gltf(bytes).map_err(CameraError::Gltf)?;
    let mut out = Vec::new();
    let mut stack: Vec<(gltf::Node, Mat4)> = match gltf.default_scene().or_else(|| gltf.scenes().next()) {
        Some(scene) => scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect(),
        None => gltf.nodes().map(|n| (n, Mat4::IDENTITY)).collect(),
    };
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(cam) = node.camera() {
            let (_, rotation, position) = world.to_scale_rotation_translation();
            let (fov, ortho, aspect) = match cam.projection() {
                gltf::camera::Projection::Perspective(p) => (p.yfov().to_degrees(), false, p.aspect_ratio()),
                gltf::camera::Projection::Orthographic(o) => (40.0, true, (o.ymag() > 0.0).then(|| o.xmag() / o.ymag())),
            };
            out.push(CameraSpec {
                name: node.name().or(cam.name()).map(str::to_owned).unwrap_or_else(|| format!("Camera {}", out.len() + 1)),
                position,
                rotation: rotation.normalize(),
                fov_y_deg: fov,
                orthographic: ortho,
                aspect: aspect.filter(|a| a.is_finite() && *a > 0.0),
            });
        }
        stack.extend(node.children().map(|c| (c, world)));
    }
    if out.is_empty() {
        return Err(CameraError::NoCameras);
    }
    out.reverse(); // stack order is reversed relative to the file
    Ok(out)
}

/// Cameras of an FBX file (bytes), in the same convention as glTF: Y up, looking down local -Z.
pub fn parse_fbx(bytes: &[u8]) -> Result<Vec<CameraSpec>, CameraError> {
    let opts = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        // FBX cameras look down +X; this re-expresses them like glTF cameras (-Z forward, +Y up).
        target_camera_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        ..Default::default()
    };
    let scene = ufbx::load_memory(bytes, opts).map_err(|e| CameraError::Fbx(format!("{:?}", e.description)))?;
    let mut out = Vec::new();
    for cam in scene.cameras.iter() {
        for node in cam.element.instances.iter() {
            let m = &node.node_to_world;
            let col = |x: f64, y: f64, z: f64| Vec3::new(x as f32, y as f32, z as f32);
            let (right, up, back) = (col(m.m00, m.m10, m.m20), col(m.m01, m.m11, m.m21), col(m.m02, m.m12, m.m22));
            let position = col(m.m03, m.m13, m.m23);
            if !position.is_finite() || right.length() < 1e-9 || up.length() < 1e-9 || back.length() < 1e-9 {
                continue;
            }
            let rotation = Quat::from_mat3(&glam::Mat3::from_cols(right.normalize(), up.normalize(), back.normalize())).normalize();
            let fov = cam.field_of_view_deg.y as f32;
            let aspect = cam.aspect_ratio as f32;
            let name = if node.element.name.is_empty() { cam.element.name.to_string() } else { node.element.name.to_string() };
            out.push(CameraSpec {
                name: if name.is_empty() { format!("Camera {}", out.len() + 1) } else { name },
                position,
                rotation: if rotation.is_finite() { rotation } else { Quat::IDENTITY },
                fov_y_deg: if fov.is_finite() && fov > 0.0 { fov } else { 40.0 },
                orthographic: cam.projection_mode == ufbx::ProjectionMode::Orthographic,
                aspect: (aspect.is_finite() && aspect > 0.0).then_some(aspect),
            });
        }
    }
    if out.is_empty() {
        return Err(CameraError::NoCameras);
    }
    Ok(out)
}

/// Cameras stored in a mesh file, for "Import cameras" in New project: glTF, GLB and FBX carry
/// them; other formats have none (Ok with an empty list), so the caller can say so.
pub fn cameras_in_mesh_file(path: &std::path::Path) -> Result<Vec<CameraSpec>, CameraError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "gltf" | "glb" | "fbx") {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path).map_err(|e| CameraError::Read(e.to_string()))?;
    let found = if ext == "fbx" { parse_fbx(&bytes) } else { parse_gltf(&bytes) };
    match found {
        Err(CameraError::NoCameras) => Ok(Vec::new()),
        other => other,
    }
}

/// Parse by content: glTF/GLB when the data starts with `glTF` or a JSON object with `"asset"`,
/// otherwise the simple JSON form.
pub fn parse_any(bytes: &[u8]) -> Result<Vec<CameraSpec>, CameraError> {
    let is_gltf = bytes.starts_with(b"glTF")
        || serde_json::from_slice::<serde_json::Value>(bytes).is_ok_and(|v| v.get("asset").is_some() && v.get("cameras").is_some_and(|c| c.is_array() && v.get("nodes").is_some()));
    if is_gltf { parse_gltf(bytes) } else { parse_json(std::str::from_utf8(bytes).map_err(|e| CameraError::Json(e.to_string()))?) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal ASCII FBX: one camera at (0, 0, 10) turned to look at the origin, 16:9.
    const FBX_CAMERA: &str = r#"; FBX 7.4.0 project file
FBXHeaderExtension:  {
	FBXHeaderVersion: 1003
	FBXVersion: 7400
}
GlobalSettings:  {
	Version: 1000
	Properties70:  {
		P: "UpAxis", "int", "Integer", "",1
		P: "UpAxisSign", "int", "Integer", "",1
		P: "FrontAxis", "int", "Integer", "",2
		P: "FrontAxisSign", "int", "Integer", "",1
		P: "CoordAxis", "int", "Integer", "",0
		P: "CoordAxisSign", "int", "Integer", "",1
		P: "UnitScaleFactor", "double", "Number", "",1
	}
}
Objects:  {
	NodeAttribute: 1001, "NodeAttribute::ShotCam", "Camera" {
		TypeFlags: "Camera"
		Properties70:  {
			P: "AspectWidth", "double", "Number", "",1920
			P: "AspectHeight", "double", "Number", "",1080
			P: "FieldOfView", "FieldOfView", "", "A",40
		}
	}
	Model: 1002, "Model::ShotCam", "Camera" {
		Version: 232
		Properties70:  {
			P: "Lcl Translation", "Lcl Translation", "", "A",0,0,10
			P: "Lcl Rotation", "Lcl Rotation", "", "A",0,90,0
		}
	}
}
Connections:  {
	C: "OO",1002,0
	C: "OO",1001,1002
}
"#;

    #[test]
    fn fbx_cameras_come_in_with_the_gltf_convention() {
        let cams = parse_fbx(FBX_CAMERA.as_bytes()).unwrap();
        assert_eq!(cams.len(), 1);
        let c = &cams[0];
        assert!(c.name.contains("ShotCam"), "{}", c.name);
        assert!((c.position - Vec3::new(0.0, 0.0, 10.0)).length() < 1e-3, "{:?}", c.position);
        assert!((c.forward() - Vec3::NEG_Z).length() < 1e-3, "an FBX camera looking at the origin looks down -Z here: {:?}", c.forward());
        assert!((c.rotation * Vec3::Y - Vec3::Y).length() < 1e-3, "and stays upright");
        assert!(c.fov_y_deg > 5.0 && c.fov_y_deg < 60.0, "vertical field of view {}", c.fov_y_deg);
        assert!((c.aspect.unwrap() - 16.0 / 9.0).abs() < 1e-3, "{:?}", c.aspect);
        assert!(!c.orthographic);
        assert!(matches!(parse_fbx(b"not an fbx"), Err(CameraError::Fbx(_))));
    }

    #[test]
    fn a_mesh_file_without_cameras_gives_an_empty_list() {
        let dir = std::env::temp_dir().join(format!("subcraft-cams-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let obj = dir.join("cube.obj");
        std::fs::write(&obj, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        assert!(cameras_in_mesh_file(&obj).unwrap().is_empty(), "OBJ files hold no cameras");
        let fbx = dir.join("shot.fbx");
        std::fs::write(&fbx, FBX_CAMERA).unwrap();
        assert_eq!(cameras_in_mesh_file(&fbx).unwrap().len(), 1);
        assert!(matches!(cameras_in_mesh_file(&dir.join("missing.glb")), Err(CameraError::Read(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn json_target_looks_at_the_point() {
        let c = &parse_json(r#"{"name":"front","position":[0,1,5],"target":[0,1,0],"fov":30}"#).unwrap()[0];
        assert_eq!(c.name, "front");
        assert_eq!(c.fov_y_deg, 30.0);
        assert!(close(c.forward(), Vec3::NEG_Z), "{:?}", c.forward());
        let up = c.rotation * Vec3::Y;
        assert!(close(up, Vec3::Y));
    }

    #[test]
    fn json_quaternion_and_euler_agree() {
        let q = Quat::from_rotation_y(90f32.to_radians());
        let a = &parse_json(&format!(r#"[{{"position":[0,0,0],"rotation":[{},{},{},{}]}}]"#, q.x, q.y, q.z, q.w)).unwrap()[0];
        let b = &parse_json(r#"{"cameras":[{"position":[0,0,0],"euler_deg":[0,90,0]}]}"#).unwrap()[0];
        assert!(close(a.forward(), Vec3::NEG_X) && close(b.forward(), Vec3::NEG_X), "{:?} {:?}", a.forward(), b.forward());
        assert_eq!(a.fov_y_deg, 40.0, "default fov");
        assert_eq!(a.name, "Camera 1");
    }

    #[test]
    fn json_errors_are_specific() {
        assert!(matches!(parse_json("{"), Err(CameraError::Json(_))));
        assert!(matches!(parse_json("[]"), Err(CameraError::NoCameras)));
        for bad in [
            r#"{"rotation":[0,0,0,1]}"#,
            r#"{"position":[0,0]}"#,
            r#"{"position":[0,0,0],"rotation":[0,0,0,0]}"#,
            r#"{"position":[0,0,0],"fov":200}"#,
            r#"{"position":[1,1,1],"target":[1,1,1]}"#,
        ] {
            assert!(matches!(parse_json(bad), Err(CameraError::Field(0, _))), "{bad}");
        }
    }

    #[test]
    fn looking_straight_down_still_gives_a_valid_rotation() {
        let r = look_rotation(Vec3::NEG_Y);
        assert!(close(r * Vec3::NEG_Z, Vec3::NEG_Y));
        assert!((r.length() - 1.0).abs() < 1e-5);
    }

    const GLTF: &str = r#"{
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "rig", "translation": [10, 0, 0], "children": [1]},
            {"name": "shot", "camera": 0, "translation": [0, 2, 3], "rotation": [0, 0.7071068, 0, 0.7071068]}
        ],
        "cameras": [{"type": "perspective", "perspective": {"yfov": 0.7853982, "znear": 0.1, "aspectRatio": 1.5}}]
    }"#;

    #[test]
    fn gltf_camera_applies_parent_transforms_and_fov() {
        let cams = parse_gltf(GLTF.as_bytes()).unwrap();
        assert_eq!(cams.len(), 1);
        let c = &cams[0];
        assert_eq!(c.name, "shot");
        assert!(close(c.position, Vec3::new(10.0, 2.0, 3.0)), "{:?}", c.position);
        assert!((c.fov_y_deg - 45.0).abs() < 1e-3, "{}", c.fov_y_deg);
        assert!(close(c.forward(), Vec3::NEG_X), "{:?}", c.forward());
        // Detected by content, not by extension.
        assert_eq!(parse_any(GLTF.as_bytes()).unwrap(), cams);
        assert!(matches!(parse_gltf(br#"{"asset":{"version":"2.0"}}"#), Err(CameraError::NoCameras)));
        assert!(matches!(parse_gltf(b"nonsense"), Err(CameraError::Gltf(_))));
    }
}
