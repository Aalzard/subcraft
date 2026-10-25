//! Mesh processing: canonical mesh, importers, tangent space, BVH, topology, primitives.

pub mod assbin;
pub mod bvh;
pub mod import;
pub mod mesh;
pub mod primitives;
pub mod regroup;
pub mod serialize;
pub mod tangent;
pub mod topology;
pub mod uv_raster;

pub use bvh::{Bvh, RayHit};
pub use import::{ImportError, ImportReport, SUPPORTED_EXTENSIONS, import_mesh, import_mesh_for_baking};
pub use assbin::{import_assbin as import_assbin_bytes, import_assbin_pure};
pub use mesh::{Aabb, BuildError, Corner, MaterialSlot, Mesh, MeshBuilder, MeshObject, Submesh, SurfacePoint};
