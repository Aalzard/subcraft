//! SubCraft core vocabulary.
//!
//! This crate is the bottom of the dependency graph. It knows nothing about the UI toolkit,
//! the GPU, file formats or the document model. It defines identifiers, the texture-channel and
//! mesh-map vocabulary shared by every subsystem, color-space math and small geometry helpers.

pub mod channel;
pub mod color;
pub mod ids;
pub mod meshmap;
pub mod rect;

pub use glam;
pub use half::f16;

pub use channel::{ChannelKind, ChannelValue};
pub use ids::*;
pub use meshmap::MeshMapKind;
pub use rect::IRect;
