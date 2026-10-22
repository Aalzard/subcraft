//! Material evaluation: deterministic procedural patterns and per-tile fill-source sampling with
//! UV / triplanar / planar projection, shared by fill layers, mask fills and the brush material.

pub mod noise;
pub mod filter;
pub mod organic;
pub mod pattern;
pub mod sample;

pub use sample::{GeometryCache, MeshMaps, SampleContext, TextureSetGeometry, TileGeometry, eval_fill_tile};
