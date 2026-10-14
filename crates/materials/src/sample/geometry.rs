//! Per-tile surface geometry of a texture set: what a fill needs to know about each texel.
//!
//! Built lazily per 64² tile from [`rasterize_surface_region`] (1-texel conservative rim, so
//! bilinear display filtering never reads unevaluated gutter texels) and cached per
//! texture set (material slot) + resolution. Uncovered tiles cost no texel storage.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use glam::Vec3;
use parking_lot::Mutex;
use subcraft_image::{TILE_SIZE, TILE_TEXELS};
use subcraft_mesh::uv_raster::{rasterize_surface_region, triangle_rect};
use subcraft_mesh::{Aabb, Mesh};

use subcraft_core::IRect;

/// Rim width (texels) rasterized around UV islands.
pub const GEOMETRY_EXPAND: f32 = 1.0;

/// Surface data of one tile. Texel `i` is at `(x0 + i % 64, y0 + i / 64)`.
#[derive(Debug)]
pub struct TileGeometry {
    pub index: usize,
    pub x0: u32,
    pub y0: u32,
    pub resolution: u32,
    /// Per-texel data; `None` when no triangle touches the tile.
    texels: Option<TileTexels>,
}

#[derive(Debug)]
struct TileTexels {
    /// Object position normalized to the mesh bounds (min corner = 0, divided by max extent).
    position: Box<[[f32; 3]]>,
    /// Interpolated shading normal (object space, unit length).
    normal: Box<[[f32; 3]]>,
    /// 1 = covered by a triangle of the texture set (inside or rim), 0 = gutter.
    coverage: Box<[f32]>,
    /// Mesh part (connected piece) of the texel's triangle; `NO_PART` in the gutter.
    part: Box<[u32]>,
}

const NO_PART: u32 = u32::MAX;

impl TileGeometry {
    /// A tile without any surface (all texels uncovered).
    pub fn empty(index: usize, x0: u32, y0: u32, resolution: u32) -> Self {
        Self { index, x0, y0, resolution, texels: None }
    }

    pub fn is_empty(&self) -> bool {
        self.texels.is_none()
    }

    /// Texture-space texel coordinates of texel `i`.
    #[inline]
    pub fn texel_xy(&self, i: usize) -> (u32, u32) {
        (self.x0 + i as u32 % TILE_SIZE, self.y0 + i as u32 / TILE_SIZE)
    }

    /// UV of texel `i`'s center.
    #[inline]
    pub fn uv(&self, i: usize) -> [f32; 2] {
        let (x, y) = self.texel_xy(i);
        let r = self.resolution as f32;
        [(x as f32 + 0.5) / r, (y as f32 + 0.5) / r]
    }

    #[inline]
    pub fn position(&self, i: usize) -> [f32; 3] {
        self.texels.as_ref().map_or([0.0; 3], |t| t.position[i])
    }

    #[inline]
    pub fn normal(&self, i: usize) -> [f32; 3] {
        self.texels.as_ref().map_or([0.0; 3], |t| t.normal[i])
    }

    #[inline]
    pub fn coverage(&self, i: usize) -> f32 {
        self.texels.as_ref().map_or(0.0, |t| t.coverage[i])
    }

    /// Mesh part ([`subcraft_mesh::topology::mesh_elements`]) under texel `i`, if any.
    #[inline]
    pub fn part(&self, i: usize) -> Option<u32> {
        self.texels.as_ref().map(|t| t.part[i]).filter(|&p| p != NO_PART)
    }
}

/// Lazily built per-tile geometry for one texture set at one resolution.
pub struct TextureSetGeometry {
    mesh: Arc<Mesh>,
    pub material_slot: u32,
    pub resolution: u32,
    pub tiles_x: u32,
    /// Mesh bounds (the whole mesh, so positions agree across texture sets).
    pub bounds: Aabb,
    /// Largest bounds extent; positions are divided by it.
    pub max_extent: f32,
    bins: Vec<Vec<u32>>,
    tiles: Box<[OnceLock<Arc<TileGeometry>>]>,
    /// Mesh part per triangle (computed on first use).
    parts: OnceLock<Vec<u32>>,
}

impl std::fmt::Debug for TextureSetGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureSetGeometry")
            .field("material_slot", &self.material_slot)
            .field("resolution", &self.resolution)
            .finish()
    }
}

impl TextureSetGeometry {
    pub fn new(mesh: Arc<Mesh>, material_slot: u32, resolution: u32) -> Self {
        let tiles_x = resolution.div_ceil(TILE_SIZE);
        let n = (tiles_x * tiles_x) as usize;
        let mut bins = vec![Vec::new(); n];
        for t in mesh.material_triangles(material_slot) {
            let r = triangle_rect(mesh.tri_uvs(t), resolution, resolution, GEOMETRY_EXPAND);
            if r.is_empty() {
                continue;
            }
            let ts = TILE_SIZE as i32;
            for ty in r.y0 / ts..=(r.y1 - 1) / ts {
                for tx in r.x0 / ts..=(r.x1 - 1) / ts {
                    bins[(ty as u32 * tiles_x + tx as u32) as usize].push(t as u32);
                }
            }
        }
        let bounds = mesh.bounds;
        let max_extent = bounds.extent().max_element().max(1e-12);
        Self {
            mesh,
            material_slot,
            resolution,
            tiles_x,
            bounds,
            max_extent,
            bins,
            tiles: (0..n).map(|_| OnceLock::new()).collect(),
            parts: OnceLock::new(),
        }
    }

    pub fn mesh(&self) -> &Arc<Mesh> {
        &self.mesh
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Triangles whose (rim-expanded) UV footprint touches tile `index`.
    pub fn tile_triangles(&self, index: usize) -> &[u32] {
        &self.bins[index]
    }

    /// Mesh part of every triangle ([`subcraft_mesh::topology::mesh_elements`]).
    pub fn triangle_parts(&self) -> &[u32] {
        self.parts.get_or_init(|| subcraft_mesh::topology::mesh_elements(&self.mesh))
    }

    /// Normalize an object-space position into the 0..1 (by max extent) frame.
    pub fn normalize_position(&self, p: Vec3) -> Vec3 {
        (p - self.bounds.min) / self.max_extent
    }

    /// Geometry of tile `index` (built on first use, thread-safe).
    pub fn tile(&self, index: usize) -> Arc<TileGeometry> {
        self.tiles[index].get_or_init(|| Arc::new(self.build_tile(index))).clone()
    }

    fn build_tile(&self, index: usize) -> TileGeometry {
        let (tx, ty) = (index as u32 % self.tiles_x, index as u32 / self.tiles_x);
        let (x0, y0) = (tx * TILE_SIZE, ty * TILE_SIZE);
        let bin = &self.bins[index];
        if bin.is_empty() {
            return TileGeometry::empty(index, x0, y0, self.resolution);
        }
        let region = IRect::new(x0 as i32, y0 as i32, (x0 + TILE_SIZE) as i32, (y0 + TILE_SIZE) as i32);
        let s = rasterize_surface_region(
            &self.mesh,
            bin.iter().map(|&t| t as usize),
            self.resolution,
            self.resolution,
            GEOMETRY_EXPAND,
            region,
        );
        let mut position = vec![[0.0f32; 3]; TILE_TEXELS].into_boxed_slice();
        let mut normal = vec![[0.0f32; 3]; TILE_TEXELS].into_boxed_slice();
        let mut coverage = vec![0.0f32; TILE_TEXELS].into_boxed_slice();
        let mut part = vec![NO_PART; TILE_TEXELS].into_boxed_slice();
        let parts = self.triangle_parts();
        for ly in 0..s.height as usize {
            for lx in 0..s.width as usize {
                let si = ly * s.width as usize + lx;
                if !s.covered(si) {
                    continue;
                }
                let di = ly * TILE_SIZE as usize + lx;
                let p = Vec3::from_slice(&s.position[si][..3]);
                position[di] = self.normalize_position(p).to_array();
                normal[di] = [s.normal[si][0], s.normal[si][1], s.normal[si][2]];
                coverage[di] = 1.0;
                part[di] = parts[s.triangle[si] as usize];
            }
        }
        TileGeometry { index, x0, y0, resolution: self.resolution, texels: Some(TileTexels { position, normal, coverage, part }) }
    }
}

/// Shared cache of [`TextureSetGeometry`] per (material slot, resolution) for one mesh.
#[derive(Debug)]
pub struct GeometryCache {
    mesh: Arc<Mesh>,
    sets: Mutex<HashMap<(u32, u32), Arc<TextureSetGeometry>>>,
}

impl GeometryCache {
    pub fn new(mesh: Arc<Mesh>) -> Self {
        Self { mesh, sets: Mutex::new(HashMap::new()) }
    }

    pub fn mesh(&self) -> &Arc<Mesh> {
        &self.mesh
    }

    pub fn get(&self, material_slot: u32, resolution: u32) -> Arc<TextureSetGeometry> {
        self.sets
            .lock()
            .entry((material_slot, resolution))
            .or_insert_with(|| Arc::new(TextureSetGeometry::new(self.mesh.clone(), material_slot, resolution)))
            .clone()
    }
}
