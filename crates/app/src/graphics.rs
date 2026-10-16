//! Startup graphics preference. Backend/device selection belongs to the frontend.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsApi {
    DirectX12,
    OpenGl,
    #[default]
    #[serde(other)]
    Automatic,
}
impl GraphicsApi {
    pub fn label(self) -> &'static str {
        match self { Self::Automatic => "Automatic", Self::DirectX12 => "DirectX 12", Self::OpenGl => "OpenGL" }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings { pub schema: u32, pub api: GraphicsApi }
impl Default for GraphicsSettings { fn default() -> Self { Self { schema: 1, api: GraphicsApi::Automatic } } }
impl GraphicsSettings {
    pub fn path() -> Option<PathBuf> { crate::recovery::data_dir().map(|d| d.join("graphics.json")) }
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Could not read graphics settings: {e}")),
        };
        let settings: Self = serde_json::from_str(&text).map_err(|e| format!("Invalid graphics settings: {e}"))?;
        if settings.schema != 1 { return Err("Graphics settings use an unsupported version; using Automatic.".into()); }
        Ok(settings)
    }
    pub fn save(api: GraphicsApi, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Graphics settings have no parent folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!("graphics-{}.tmp", subcraft_core::ObjectId::new()));
        let bytes = serde_json::to_vec(&Self { schema: 1, api }).map_err(|e| e.to_string())?;
        let result = (|| {
            use std::io::Write;
            let mut f = std::fs::File::create(&temporary)?;
            f.write_all(&bytes)?; f.sync_all()?; drop(f);
            std::fs::rename(&temporary, path)
        })();
        if let Err(e) = result { let _ = std::fs::remove_file(temporary); return Err(format!("Could not save graphics settings: {e}")); }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_old_unknown_and_future_settings_are_safe() {
        let root = std::env::temp_dir().join(format!("subcraft-graphics-{}", subcraft_core::ObjectId::new()));
        let path = root.join("graphics.json");
        assert_eq!(GraphicsSettings::load(&path).unwrap().api, GraphicsApi::Automatic);
        std::fs::create_dir_all(&root).unwrap();
        for text in ["{}", r#"{"schema":1,"api":"future_api"}"#] {
            std::fs::write(&path, text).unwrap(); assert_eq!(GraphicsSettings::load(&path).unwrap().api, GraphicsApi::Automatic);
        }
        for text in [r#"{"schema":2,"api":"open_gl"}"#, "broken"] {
            std::fs::write(&path, text).unwrap(); assert!(GraphicsSettings::load(&path).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preferences_persist_before_gpu_creation_and_replace_atomically() {
        let root = std::env::temp_dir().join(format!("subcraft-graphics-{}", subcraft_core::ObjectId::new()));
        let path = root.join("graphics.json");
        for api in [GraphicsApi::DirectX12, GraphicsApi::OpenGl, GraphicsApi::Automatic] {
            GraphicsSettings::save(api, &path).unwrap(); assert_eq!(GraphicsSettings::load(&path).unwrap().api, api);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        }
        // A failed write must leave the previous file intact.
        assert!(GraphicsSettings::save(GraphicsApi::OpenGl, &path.join("child")).is_err());
        assert_eq!(GraphicsSettings::load(&path).unwrap().api, GraphicsApi::Automatic);
        std::fs::remove_dir_all(root).unwrap();
    }
}
