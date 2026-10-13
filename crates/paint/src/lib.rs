//! Brush engine (ADR-0003): projection painting onto UV-space tiled layers.
//!
//! A [`Stroke`] turns screen samples into surface stamps (BVH ray casts), finds the texels each
//! stamp covers by UV-rasterizing nearby triangles (seam-aware: islands are painted from 3D
//! positions, not from UV adjacency), and writes paint-layer channels or a mask base raster from
//! the pre-stroke snapshot. Visibility comes from a CPU depth map plus backface culling.
//! Undo needs nothing special: the stroke edits the document copy-on-write, so pushing the
//! post-stroke document to `History` costs only the touched tiles.

pub mod camera;
pub mod depth;
pub mod kernel;
pub mod path;
pub mod settings;
pub mod stamp;
pub mod stroke;
pub mod target;

pub use camera::Camera;
pub use depth::{DepthCache, DepthMap};
pub use settings::{Alignment, BrushChannel, BrushMaterial, BrushSettings, PaintTarget, PolygonFillMode, StrokeMode, StrokeOptions, falloff};
pub use stamp::Stamp;
pub use stroke::{PaintContext, Stroke};
pub use target::PaintError;
