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

/// Which GPU to prefer. Stored as a string (`automatic`, `high_performance`, `name:<adapter>`) so
/// an unknown value from a newer build degrades to Automatic instead of failing the whole load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GpuChoice {
    #[default]
    Automatic,
    HighPerformance,
    /// Adapter whose name contains this text (case-insensitive).
    Named(String),
}
impl GpuChoice {
    pub fn label(&self) -> String {
        match self { Self::Automatic => "Automatic".into(), Self::HighPerformance => "High performance".into(), Self::Named(n) => n.clone() }
    }
    fn encode(&self) -> String {
        match self { Self::Automatic => "automatic".into(), Self::HighPerformance => "high_performance".into(), Self::Named(n) => format!("name:{n}") }
    }
    fn decode(text: &str) -> Self {
        match text.strip_prefix("name:") {
            Some(n) if !n.trim().is_empty() => Self::Named(n.trim().to_string()),
            _ if text == "high_performance" => Self::HighPerformance,
            _ => Self::Automatic,
        }
    }
}
impl Serialize for GpuChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(&self.encode()) }
}
impl<'de> Deserialize<'de> for GpuChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Any non-string value is treated as Automatic too.
        Ok(match serde_json::Value::deserialize(d)? { serde_json::Value::String(t) => Self::decode(&t), _ => Self::Automatic })
    }
}

/// Kind of adapter, without any graphics types (the UI maps `wgpu::DeviceType` onto this).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterClass { Discrete, Integrated, Virtual, Other, Software }
impl AdapterClass {
    fn rank(self) -> u8 { match self { Self::Discrete => 0, Self::Integrated => 1, Self::Virtual => 2, Self::Other => 3, Self::Software => 4 } }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterDesc { pub name: String, pub class: AdapterClass }

/// Environment override: `SUBCRAFT_GPU=<substring of adapter name>`; empty means unset.
pub fn gpu_env_override() -> Option<String> {
    std::env::var("SUBCRAFT_GPU").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Order in which to try adapters (indices into `adapters`). Explicit matches come first
/// (`env` wins over a `Named` preference), then everything else discrete first, integrated next,
/// software renderers last; enumeration order breaks ties. Every adapter appears exactly once,
/// so a bad preference can never leave the app without a candidate.
pub fn order_adapters(adapters: &[AdapterDesc], choice: &GpuChoice, env: Option<&str>) -> Vec<usize> {
    let wanted = env.map(str::trim).filter(|e| !e.is_empty()).map(str::to_lowercase)
        .or_else(|| match choice { GpuChoice::Named(n) => Some(n.to_lowercase()), _ => None });
    let mut order: Vec<usize> = (0..adapters.len()).collect();
    order.sort_by_key(|&i| {
        let explicit = wanted.as_ref().is_some_and(|w| adapters[i].name.to_lowercase().contains(w.as_str()));
        (!explicit, adapters[i].class.rank())
    });
    order
}

/// Try candidates in order until `attempt` succeeds. Returns the chosen index plus the failures
/// that were skipped, or all failures when none worked.
pub fn first_working(order: &[usize], mut attempt: impl FnMut(usize) -> Result<(), String>) -> Result<(usize, Vec<(usize, String)>), Vec<(usize, String)>> {
    let mut skipped = Vec::new();
    for &i in order {
        match attempt(i) { Ok(()) => return Ok((i, skipped)), Err(e) => skipped.push((i, e)) }
    }
    Err(skipped)
}

pub const SCHEMA: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings { pub schema: u32, pub api: GraphicsApi, pub gpu: GpuChoice }
impl Default for GraphicsSettings { fn default() -> Self { Self { schema: SCHEMA, api: GraphicsApi::Automatic, gpu: GpuChoice::Automatic } } }
impl GraphicsSettings {
    pub fn path() -> Option<PathBuf> { crate::recovery::data_dir().map(|d| d.join("graphics.json")) }
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Could not read graphics settings: {e}")),
        };
        let settings: Self = serde_json::from_str(&text).map_err(|e| format!("Invalid graphics settings: {e}"))?;
        if settings.schema == 0 || settings.schema > SCHEMA { return Err("Graphics settings use an unsupported version; using Automatic.".into()); }
        Ok(settings)
    }
    pub fn save(api: GraphicsApi, gpu: &GpuChoice, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Graphics settings have no parent folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!("graphics-{}.tmp", subcraft_core::ObjectId::new()));
        let bytes = serde_json::to_vec(&Self { schema: SCHEMA, api, gpu: gpu.clone() }).map_err(|e| e.to_string())?;
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
        for text in [r#"{"schema":99,"api":"open_gl"}"#, "broken"] {
            std::fs::write(&path, text).unwrap(); assert!(GraphicsSettings::load(&path).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn preferences_persist_before_gpu_creation_and_replace_atomically() {
        let root = std::env::temp_dir().join(format!("subcraft-graphics-{}", subcraft_core::ObjectId::new()));
        let path = root.join("graphics.json");
        for api in [GraphicsApi::DirectX12, GraphicsApi::OpenGl, GraphicsApi::Automatic] {
            GraphicsSettings::save(api, &GpuChoice::Automatic, &path).unwrap(); assert_eq!(GraphicsSettings::load(&path).unwrap().api, api);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        }
        // A failed write must leave the previous file intact.
        assert!(GraphicsSettings::save(GraphicsApi::OpenGl, &GpuChoice::Automatic, &path.join("child")).is_err());
        assert_eq!(GraphicsSettings::load(&path).unwrap().api, GraphicsApi::Automatic);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn a(name: &str, class: AdapterClass) -> AdapterDesc { AdapterDesc { name: name.into(), class } }
    fn laptop() -> Vec<AdapterDesc> {
        vec![a("Intel(R) UHD Graphics", AdapterClass::Integrated), a("NVIDIA GeForce RTX 2080 Super", AdapterClass::Discrete),
             a("Microsoft Basic Render Driver", AdapterClass::Software)]
    }
    #[test]
    fn default_ordering_is_discrete_then_integrated_then_software() {
        for choice in [GpuChoice::Automatic, GpuChoice::HighPerformance] {
            assert_eq!(order_adapters(&laptop(), &choice, None), vec![1, 0, 2]);
        }
        assert!(order_adapters(&[], &GpuChoice::Automatic, None).is_empty());
    }
    #[test]
    fn explicit_choice_goes_first_env_beats_settings_and_unknown_name_is_harmless() {
        assert_eq!(order_adapters(&laptop(), &GpuChoice::Named("uhd".into()), None), vec![0, 1, 2]);
        assert_eq!(order_adapters(&laptop(), &GpuChoice::Named("uhd".into()), Some("rtx")), vec![1, 0, 2]);
        assert_eq!(order_adapters(&laptop(), &GpuChoice::Automatic, Some("BASIC")), vec![2, 1, 0]);
        assert_eq!(order_adapters(&laptop(), &GpuChoice::Named("radeon".into()), None), vec![1, 0, 2]);
        assert_eq!(order_adapters(&laptop(), &GpuChoice::Automatic, Some("  ")), vec![1, 0, 2]);
    }
    #[test]
    fn failing_adapters_are_skipped_until_one_works() {
        let order = order_adapters(&laptop(), &GpuChoice::Automatic, None);
        let mut tried = Vec::new();
        let ok = first_working(&order, |i| { tried.push(i); if i == 1 { Err("surface".into()) } else { Ok(()) } }).unwrap();
        assert_eq!(ok.0, 0); assert_eq!(tried, vec![1, 0]); assert_eq!(ok.1, vec![(1, "surface".to_string())]);
        let all = first_working(&order, |_| Err("bad".into())).unwrap_err();
        assert_eq!(all.len(), 3);
        assert!(first_working(&[], |_| Ok(())).is_err());
    }
    #[test]
    fn gpu_choice_round_trips_and_old_files_still_load() {
        let root = std::env::temp_dir().join(format!("subcraft-graphics-{}", subcraft_core::ObjectId::new()));
        let path = root.join("graphics.json");
        for gpu in [GpuChoice::Automatic, GpuChoice::HighPerformance, GpuChoice::Named("NVIDIA GeForce RTX 2080 Super".into())] {
            GraphicsSettings::save(GraphicsApi::DirectX12, &gpu, &path).unwrap();
            let s = GraphicsSettings::load(&path).unwrap();
            assert_eq!((s.api, s.gpu, s.schema), (GraphicsApi::DirectX12, gpu, SCHEMA));
        }
        // Schema 1 file (no gpu field), unknown gpu value, non-string gpu value.
        for (text, api) in [(r#"{"schema":1,"api":"open_gl"}"#, GraphicsApi::OpenGl), (r#"{"schema":2,"gpu":"quantum"}"#, GraphicsApi::Automatic), (r#"{"schema":2,"gpu":7}"#, GraphicsApi::Automatic), (r#"{"schema":2,"gpu":"name:  "}"#, GraphicsApi::Automatic)] {
            std::fs::write(&path, text).unwrap();
            let s = GraphicsSettings::load(&path).unwrap();
            assert_eq!((s.api, s.gpu), (api, GpuChoice::Automatic), "{text}");
        }
        std::fs::write(&path, r#"{"schema":3,"gpu":"high_performance"}"#).unwrap();
        assert!(GraphicsSettings::load(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
