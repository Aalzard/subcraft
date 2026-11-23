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
    /// Per-texel data, built the first time something reads it (issue #628: built for every
    /// tile of every texture set, 32 bytes per texel, although plain paint and uniform fills
    /// never read it). Stays `None` when no triangle touches the tile.
    texels: OnceLock<Option<TileTexels>>,
    /// What building `texels` needs; `None` when no triangle touches the tile.
    source: Option<Arc<SetSource>>,
    regions: Option<Arc<RegionIds>>,
    /// Procedural fields evaluated on this tile (see [`super::field`]).
    pub(crate) fields: super::field::FieldCache,
    /// Other derived values of this tile, such as evaluated layer masks (see [`super::derived`]).
    pub derived: super::derived::DerivedCache,
}

/// Shared triangle tables keep each covered texel's selection storage at one u32.
#[derive(Debug)]
struct RegionIds {
    parts: Vec<u32>,
    islands: OnceLock<Vec<u32>>,
    polygons: OnceLock<Vec<u32>>,
    /// Polygon faces and edges for loop selection, built on first use.
    loops: OnceLock<subcraft_mesh::topology::PolygonTopology>,
    mesh: Arc<Mesh>,
}

#[derive(Debug)]
struct TileTexels {
    /// Object position normalized to the mesh bounds (min corner = 0, divided by max extent).
    position: Box<[[f32; 3]]>,
    /// Interpolated shading normal (object space, unit length).
    normal: Box<[[f32; 3]]>,
    /// 1 = covered by a triangle of the texture set (inside or rim), 0 = gutter.
    coverage: Box<[f32]>,
    /// Original triangle index, resolved through shared region tables; `NO_PART` in the gutter.
    part: Box<[u32]>,
}

const NO_PART: u32 = u32::MAX;

impl TileGeometry {
    /// A tile without any surface (all texels uncovered).
    pub fn empty(index: usize, x0: u32, y0: u32, resolution: u32) -> Self {
        Self { index, x0, y0, resolution, texels: OnceLock::new(), source: None, regions: None, fields: Default::default(), derived: Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_none()
    }

    /// Whether the per-texel data has been built (tests and diagnostics).
    pub fn texels_built(&self) -> bool {
        self.texels.get().is_some_and(Option::is_some)
    }

    #[inline]
    fn texels(&self) -> Option<&TileTexels> {
        let source = self.source.as_ref()?;
        self.texels.get_or_init(|| Some(source.build(self.index, self.x0, self.y0))).as_ref()
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
        self.texels().map_or([0.0; 3], |t| t.position[i])
    }

    #[inline]
    pub fn normal(&self, i: usize) -> [f32; 3] {
        self.texels().map_or([0.0; 3], |t| t.normal[i])
    }

    #[inline]
    pub fn coverage(&self, i: usize) -> f32 {
        self.texels().map_or(0.0, |t| t.coverage[i])
    }

    /// Mesh part ([`subcraft_mesh::topology::mesh_elements`]) under texel `i`, if any.
    #[inline]
    pub fn part(&self, i: usize) -> Option<u32> {
        self.region(i, subcraft_core::PartKind::Connected)
    }

    pub fn triangle(&self, i: usize) -> Option<usize> {
        let t = self.texels()?.part[i];
        (t != NO_PART).then_some(t as usize)
    }

    pub fn region(&self, i: usize, kind: subcraft_core::PartKind) -> Option<u32> {
        let triangle = self.texels()?.part[i];
        if triangle == NO_PART { return None; }
        let regions = self.regions.as_ref()?;
        let t = triangle as usize;
        Some(match kind {
            subcraft_core::PartKind::Connected => regions.parts[t],
            subcraft_core::PartKind::UvIsland => regions.islands.get_or_init(|| subcraft_mesh::topology::uv_islands(&regions.mesh))[t],
            subcraft_core::PartKind::Object => regions.mesh.tri_object.get(t).copied().flatten()?,
            subcraft_core::PartKind::Material => regions.mesh.tri_material[t],
        })
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
    /// Triangles per tile, shared with the tiles that build their texels from it.
    source: Arc<SetSource>,
    tiles: Box<[OnceLock<Arc<TileGeometry>>]>,
    /// Mesh part per triangle (computed on first use).
    parts: OnceLock<Arc<RegionIds>>,
    edges: Arc<OnceLock<super::edge_distance::SurfaceEdges>>,
    /// Meters of surface per UV unit of this set's triangles (computed on first use).
    uv_meters: OnceLock<f32>,
    /// Whole-set results a caller derived from this geometry (the drips of a Dripping Rust
    /// generator), kept while the geometry lives. A handful at most.
    shared: Mutex<Vec<Arc<dyn std::any::Any + Send + Sync>>>,
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
        let source = Arc::new(SetSource { mesh: mesh.clone(), resolution, bounds_min: bounds.min, max_extent, bins });
        Self {
            mesh,
            material_slot,
            resolution,
            tiles_x,
            bounds,
            max_extent,
            source,
            tiles: (0..n).map(|_| OnceLock::new()).collect(),
            parts: OnceLock::new(),
            edges: Arc::new(OnceLock::new()),
            uv_meters: OnceLock::new(),
            shared: Mutex::new(Vec::new()),
        }
    }

    /// A whole-set result of type `T` kept with this geometry: the one `matches` accepts, or
    /// `make()` (kept in its place; the oldest of more than [`Self::SHARED_LIMIT`] is dropped).
    /// `make` only creates the entry, under the lock: slow work belongs in a `OnceLock` inside it.
    pub fn shared<T: std::any::Any + Send + Sync>(&self, matches: impl Fn(&T) -> bool, make: impl FnOnce() -> T) -> Arc<T> {
        let mut list = self.shared.lock();
        if let Some(found) = list.iter().filter_map(|e| e.clone().downcast::<T>().ok()).find(|e| matches(e)) {
            return found;
        }
        let made = Arc::new(make());
        if list.len() >= Self::SHARED_LIMIT {
            list.remove(0);
        }
        list.push(made.clone());
        made
    }

    pub const SHARED_LIMIT: usize = 4;

    /// The real-world scale of this set's surface for Physical size fills: meters of model
    /// surface per UV unit (the square root of the summed 3D area over the summed UV area of the
    /// set's triangles) and the model's longest extent. A set
    /// without UV area reports 1 m per UV unit.
    pub fn surface_scale(&self) -> subcraft_doc::SurfaceScale {
        let uv_meters = *self.uv_meters.get_or_init(|| {
            let (mut world, mut uv) = (0.0f64, 0.0f64);
            for t in self.mesh.material_triangles(self.material_slot) {
                let [a, b, c] = self.mesh.tri_positions(t);
                world += f64::from((b - a).cross(c - a).length()) * 0.5;
                let [p, q, r] = self.mesh.tri_uvs(t);
                uv += f64::from(((q - p).perp_dot(r - p)).abs()) * 0.5;
            }
            let unit = f64::from(self.mesh.meters_per_unit());
            if uv > 1e-12 && world > 0.0 { ((world / uv).sqrt() * unit) as f32 } else { 1.0 }
        });
        subcraft_doc::SurfaceScale { uv_meters, extent_meters: self.max_extent * self.mesh.meters_per_unit() }
    }

    pub fn mesh(&self) -> &Arc<Mesh> {
        &self.mesh
    }

    /// Parameter-independent distance field, shared with the bounded procedural tile cache.
    pub fn edge_distance_tile(&self, geo: &TileGeometry) -> Arc<super::field::Field> {
        geo.fields.get_or_compute(super::field::FieldKey::edge_distance(), || {
            let edges = self.edges.get_or_init(|| super::edge_distance::SurfaceEdges::new(&self.mesh));
            (0..TILE_TEXELS).map(|i| geo.triangle(i).map_or(f32::INFINITY, |t|
                edges.distance(t, Vec3::from_array(geo.position(i))))).collect()
        })
    }

    /// Distance to rounded (bevelled) edges ([`super::edge_distance::SurfaceEdges::rounded_distance`]),
    /// cached like [`Self::edge_distance_tile`].
    pub fn rounded_edge_distance_tile(&self, geo: &TileGeometry) -> Arc<super::field::Field> {
        geo.fields.get_or_compute(super::field::FieldKey::rounded_edge_distance(), || {
            let edges = self.edges.get_or_init(|| super::edge_distance::SurfaceEdges::new(&self.mesh));
            (0..TILE_TEXELS).map(|i| geo.triangle(i).map_or(f32::INFINITY, |t|
                edges.rounded_distance(t, Vec3::from_array(geo.position(i))))).collect()
        })
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Bytes of per-texel data built so far (diagnostics).
    pub fn memory_bytes(&self) -> usize {
        self.tiles.iter().filter_map(|t| t.get()).filter(|g| g.texels_built()).count() * TILE_TEXELS * 32
    }

    /// Triangles whose (rim-expanded) UV footprint touches tile `index`.
    pub fn tile_triangles(&self, index: usize) -> &[u32] {
        &self.source.bins[index]
    }

    /// Mesh part of every triangle ([`subcraft_mesh::topology::mesh_elements`]).
    pub fn triangle_parts(&self) -> &[u32] {
        &self.regions().parts
    }

    fn regions(&self) -> &Arc<RegionIds> {
        self.parts.get_or_init(|| Arc::new(RegionIds {
            parts: subcraft_mesh::topology::mesh_elements(&self.mesh),
            islands: OnceLock::new(), polygons: OnceLock::new(), loops: OnceLock::new(), mesh: self.mesh.clone(),
        }))
    }

    pub fn triangle_islands(&self) -> &[u32] {
        self.regions().islands.get_or_init(|| subcraft_mesh::topology::uv_islands(&self.mesh))
    }

    /// Source polygon (quad / n-gon) of every triangle ([`subcraft_mesh::topology::polygons`]),
    /// recovered with the 3D wireframe's default 30 degree angle limit.
    pub fn triangle_polygons(&self) -> &[u32] {
        self.regions().polygons.get_or_init(|| subcraft_mesh::topology::polygons(&self.mesh, 30.0))
    }

    /// The polygons of [`Self::triangle_polygons`] with their edges, for polygon loops.
    pub fn polygon_topology(&self) -> &subcraft_mesh::topology::PolygonTopology {
        self.regions().loops.get_or_init(|| subcraft_mesh::topology::polygon_topology(&self.mesh, self.triangle_polygons()))
    }

    pub fn triangle_region(&self, triangle: usize, kind: subcraft_core::PartKind) -> Option<u32> {
        Some(match kind {
            subcraft_core::PartKind::Connected => *self.triangle_parts().get(triangle)?,
            subcraft_core::PartKind::UvIsland => *self.triangle_islands().get(triangle)?,
            subcraft_core::PartKind::Object => self.mesh.tri_object.get(triangle).copied().flatten()?,
            subcraft_core::PartKind::Material => *self.mesh.tri_material.get(triangle)?,
        })
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
        if self.source.bins[index].is_empty() {
            return TileGeometry::empty(index, x0, y0, self.resolution);
        }
        TileGeometry { index, x0, y0, resolution: self.resolution, texels: OnceLock::new(), source: Some(self.source.clone()), regions: Some(self.regions().clone()), fields: Default::default(), derived: Default::default() }
    }
}

/// What a tile needs to build its per-texel data on first read.
#[derive(Debug)]
struct SetSource {
    mesh: Arc<Mesh>,
    resolution: u32,
    bounds_min: Vec3,
    max_extent: f32,
    bins: Vec<Vec<u32>>,
}

impl SetSource {
    fn build(&self, index: usize, x0: u32, y0: u32) -> TileTexels {
        let region = IRect::new(x0 as i32, y0 as i32, (x0 + TILE_SIZE) as i32, (y0 + TILE_SIZE) as i32);
        let s = rasterize_surface_region(
            &self.mesh,
            self.bins[index].iter().map(|&t| t as usize),
            self.resolution,
            self.resolution,
            GEOMETRY_EXPAND,
            region,
        );
        let mut position = vec![[0.0f32; 3]; TILE_TEXELS].into_boxed_slice();
        let mut normal = vec![[0.0f32; 3]; TILE_TEXELS].into_boxed_slice();
        let mut coverage = vec![0.0f32; TILE_TEXELS].into_boxed_slice();
        let mut part = vec![NO_PART; TILE_TEXELS].into_boxed_slice();
        for ly in 0..s.height as usize {
            for lx in 0..s.width as usize {
                let si = ly * s.width as usize + lx;
                if !s.covered(si) {
                    continue;
                }
                let di = ly * TILE_SIZE as usize + lx;
                let p = Vec3::from_slice(&s.position[si][..3]);
                position[di] = ((p - self.bounds_min) / self.max_extent).to_array();
                normal[di] = [s.normal[si][0], s.normal[si][1], s.normal[si][2]];
                coverage[di] = 1.0;
                part[di] = s.triangle[si];
            }
        }
        TileTexels { position, normal, coverage, part }
    }
}

/// Shared cache of [`TextureSetGeometry`] per (material slot, resolution) for one mesh.
#[derive(Debug)]
pub struct GeometryCache {
    mesh: Arc<Mesh>,
    sets: Mutex<HashMap<(u32, u32), Arc<TextureSetGeometry>>>,
    edges: Arc<OnceLock<super::edge_distance::SurfaceEdges>>,
}

impl GeometryCache {
    pub fn new(mesh: Arc<Mesh>) -> Self {
        Self { mesh, sets: Mutex::new(HashMap::new()), edges: Arc::new(OnceLock::new()) }
    }

    pub fn mesh(&self) -> &Arc<Mesh> {
        &self.mesh
    }

    /// Bytes of per-texel tile data built so far, over every texture set (diagnostics).
    pub fn memory_bytes(&self) -> usize {
        self.sets.lock().values().map(|g| g.memory_bytes()).sum()
    }

    pub fn get(&self, material_slot: u32, resolution: u32) -> Arc<TextureSetGeometry> {
        self.sets
            .lock()
            .entry((material_slot, resolution))
            .or_insert_with(|| {
                let mut set = TextureSetGeometry::new(self.mesh.clone(), material_slot, resolution);
                set.edges = self.edges.clone();
                Arc::new(set)
            })
            .clone()
    }
}
