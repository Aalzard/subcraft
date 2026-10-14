//! Built-in and imported assets: brush alphas, material presets, smart materials, smart masks
//! and lighting environments. Pure data + CPU processing (no GPU, no UI); the shelf panel shows
//! them and the app turns them into commands.

pub mod alphas;
pub mod environment;
pub mod presets;
pub mod smart;
pub mod texture_maps;

pub use alphas::BrushAlpha;
pub use environment::{Environment, EnvironmentSource, HdriLevel, PrefilteredHdri, ProceduralEnv};
pub use presets::{MaterialPreset, SmartMask};
pub use smart::SmartMaterial;

/// Everything the shelf offers.
pub struct Library {
    pub alphas: Vec<BrushAlpha>,
    pub materials: Vec<MaterialPreset>,
    pub smart_materials: Vec<SmartMaterial>,
    pub smart_masks: Vec<SmartMask>,
    pub environments: Vec<Environment>,
}

impl Library {
    pub fn builtin() -> Self {
        Self {
            alphas: alphas::builtin(),
            materials: presets::materials(),
            smart_materials: smart::smart_materials(),
            smart_masks: presets::smart_masks(),
            environments: environment::builtin(),
        }
    }
}

#[cfg(test)]
mod tests;
