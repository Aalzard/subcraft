//! Painter's polygon-fill selections (`DataActionDecal` hits) rebuilt on the mesh.
//!
//! A hit is a click (or rectangle) in Painter's viewport, stored as a screen position (0..1,
//! origin bottom-left, as OpenGL) and the inverse of the view-projection matrix at the time
//! (column major). Casting the same ray against the mesh finds the triangle Painter selected; the
//! hit's granularity grows it to Painter's fill mode (triangle, mesh element, UV island). Verified
//! on a real project: with this convention every click of its folder masks lands on the mesh,
//! while the transposed or y-flipped readings miss up to two thirds of them.

use std::sync::Arc;

use glam::{Mat4, Vec3, Vec4};
use subcraft_image::TiledImage;
use subcraft_materials::sample::TextureSetGeometry;
use subcraft_mesh::spp::document::{BrushInfo, DecalHit, StrokeInfo};
use subcraft_mesh::{Bvh, Mesh};

/// What a polygon fill hit selects, from Painter's granularity id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    Triangle,
    /// Painter's "mesh fill": the whole mesh object (the model file's object/sub-mesh), joined
    /// with every object it shares connected geometry with.
    Object,
    UvIsland,
}

impl Granularity {
    pub fn from_id(id: i64) -> Self {
        match id {
            2 => Granularity::Object,
            3 => Granularity::UvIsland,
            // 0 is a single triangle; 1 (polygon/quad) is treated as its triangle.
            _ => Granularity::Triangle,
        }
    }
}

/// Rectangles narrower or lower than this (screen units) are clicks.
const MIN_RECT: f32 = 0.01;

/// Barycentric weights of `q` in screen triangle `v` (x, y), when inside it.
fn barycentric(v: &[Vec3; 3], q: glam::Vec2) -> Option<Vec3> {
    let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
    if area.abs() < 1e-12 {
        return None;
    }
    let w0 = ((v[1].x - q.x) * (v[2].y - q.y) - (v[2].x - q.x) * (v[1].y - q.y)) / area;
    let w1 = ((v[2].x - q.x) * (v[0].y - q.y) - (v[0].x - q.x) * (v[2].y - q.y)) / area;
    let w = Vec3::new(w0, w1, 1.0 - w0 - w1);
    (w.min_element() >= 0.0).then_some(w)
}

/// Per-triangle object ids where objects that share a connected element get one id (the smallest).
/// Measured on DBAL-A2: a piece welded to another object's mesh is filled with it in Painter
/// (Plane.003 with Plane_002.002: Base Color 0.40 -> 0.05 there), while an object made of separate
/// pieces is still filled whole (filling by connected piece alone tripled Metallic on one ring).
pub fn join_connected(objects: &[u32], elements: &[u32]) -> Vec<u32> {
    use std::collections::HashMap;
    let mut parent: HashMap<u32, u32> = HashMap::new();
    fn find(p: &mut HashMap<u32, u32>, x: u32) -> u32 {
        let up = *p.get(&x).unwrap_or(&x);
        if up == x {
            return x;
        }
        let root = find(p, up);
        p.insert(x, root);
        root
    }
    let mut first: HashMap<u32, u32> = HashMap::new();
    for (&o, &e) in objects.iter().zip(elements) {
        let other = *first.entry(e).or_insert(o);
        let (a, b) = (find(&mut parent, o), find(&mut parent, other));
        if a != b {
            parent.insert(a.max(b), a.min(b));
        }
    }
    objects.iter().map(|&o| find(&mut parent, o)).collect()
}

/// Mesh, ray tracer and connectivity used to replay polygon fills.
pub struct Picker {
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
    objects: Vec<u32>,
    islands: Vec<u32>,
}

impl Picker {
    pub fn new(mesh: Arc<Mesh>) -> Self {
        let bvh = Bvh::build(&mesh);
        // Triangles without an object fall back to their connected element (offset past the objects).
        let elements = subcraft_mesh::topology::mesh_elements(&mesh);
        let objects: Vec<u32> = (0..mesh.triangle_count()).map(|t| mesh.tri_object.get(t).copied().flatten().unwrap_or(1 << 30 | elements[t])).collect();
        let objects = join_connected(&objects, &elements);
        let islands = subcraft_mesh::topology::uv_islands(&mesh);
        Self { mesh, bvh, objects, islands }
    }

    /// World-space ray through a screen point (0..1, origin bottom-left) of a stored camera.
    fn ray(inv: &Mat4, p: [f32; 2]) -> (Vec3, Vec3) {
        let ndc = (2.0 * p[0] - 1.0, 2.0 * p[1] - 1.0);
        let un = |z: f32| {
            let v = *inv * Vec4::new(ndc.0, ndc.1, z, 1.0);
            v.truncate() / v.w
        };
        let (a, b) = (un(-1.0), un(1.0));
        (a, (b - a).normalize_or_zero())
    }

    fn grow(&self, seeds: &[u32], g: Granularity) -> Vec<u32> {
        let ids = match g {
            Granularity::Triangle => return seeds.to_vec(),
            Granularity::Object => &self.objects,
            Granularity::UvIsland => &self.islands,
        };
        let mut wanted: Vec<u32> = seeds.iter().map(|&t| ids[t as usize]).collect();
        wanted.sort_unstable();
        wanted.dedup();
        (0..ids.len() as u32).filter(|&t| wanted.binary_search(&ids[t as usize]).is_ok()).collect()
    }

    /// Triangles selected by one hit. `through` also takes every triangle behind the first one
    /// along a click's ray (Painter's paint-through).
    pub fn select(&self, hit: &DecalHit, through: bool) -> Vec<u32> {
        let inv = Mat4::from_cols_array(&hit.proj_view_inv);
        let g = Granularity::from_id(hit.granularity);
        // A rectangle under ~1% of the view on a side is a click with the rectangle tool: Painter
        // selects what is under it (48 of DBAL-A2's 140 rectangles, which selected nothing).
        let points: Vec<[f32; 2]> = match hit.points.as_slice() {
            [a, b] if (a[0] - b[0]).abs() < MIN_RECT || (a[1] - b[1]).abs() < MIN_RECT => vec![[(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]],
            p => p.to_vec(),
        };
        let seeds: Vec<u32> = match points.as_slice() {
            [p] => {
                let (o, d) = Self::ray(&inv, *p);
                let mut seeds = Vec::new();
                let mut t0 = 0.0;
                while let Some(h) = self.bvh.intersect(o, d, t0, f32::INFINITY) {
                    seeds.push(h.tri);
                    if !through {
                        break;
                    }
                    t0 = h.t + 1e-5;
                }
                seeds
            }
            [a, b] => self.in_rect(&inv, *a, *b),
            _ => Vec::new(),
        };
        self.grow(&seeds, g)
    }

    /// Visible triangles whose centroid projects inside the screen rectangle `a`..`b`.
    fn in_rect(&self, inv: &Mat4, a: [f32; 2], b: [f32; 2]) -> Vec<u32> {
        let view_proj = inv.inverse();
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        let inside = |q: glam::Vec2| q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1;
        let mut out = Vec::new();
        let mut candidates = Vec::new();
        for t in 0..self.mesh.triangle_count() {
            let clip = self.mesh.tri_positions(t).map(|p| view_proj * p.extend(1.0));
            if clip.iter().any(|c| c.w.abs() < 1e-12) {
                continue;
            }
            let v = clip.map(|c| Vec3::new(c.x / c.w * 0.5 + 0.5, c.y / c.w * 0.5 + 0.5, c.z / c.w));
            if v.iter().all(|p| p.x < x0) || v.iter().all(|p| p.x > x1) || v.iter().all(|p| p.y < y0) || v.iter().all(|p| p.y > y1) {
                continue;
            }
            // A point of the triangle inside the rectangle (Painter takes the triangles the
            // rectangle touches, not only those whose centre it holds: large faces were missed).
            let c = (v[0] + v[1] + v[2]) / 3.0;
            let corners = [glam::Vec2::new(x0, y0), glam::Vec2::new(x1, y0), glam::Vec2::new(x1, y1), glam::Vec2::new(x0, y1), glam::Vec2::new((x0 + x1) * 0.5, (y0 + y1) * 0.5)];
            let at = if inside(c.truncate()) {
                Some(c)
            } else if let Some(p) = v.iter().find(|p| inside(p.truncate())) {
                Some(*p + (c - *p) * 0.05)
            } else {
                corners.iter().find_map(|&q| barycentric(&v, q).map(|w| v[0] * w.x + v[1] * w.y + v[2] * w.z))
            };
            if let Some(q) = at {
                candidates.push((t as u32, q.x, q.y, q.z));
            }
        }
        if candidates.is_empty() {
            return out;
        }
        // Visible: not behind the nearest surface at the centroid's pixel (one z-buffer per hit).
        let depth = ScreenDepth::render(&self.mesh, &view_proj, [x0, y0, x1, y1]);
        for (t, sx, sy, z) in candidates {
            if depth.visible(sx, sy, z) {
                out.push(t);
            }
        }
        out
    }

    /// Triangles selected by a hit made in the 2D view: the triangle under a UV point, or those
    /// whose UV centroid lies in a UV rectangle, grown by the granularity.
    pub fn select_uv(&self, hit: &DecalHit, geometry: &TextureSetGeometry) -> Vec<u32> {
        let g = Granularity::from_id(hit.granularity);
        let uv = |p: [f32; 2]| glam::Vec2::new(p[0], 1.0 - p[1]);
        let seeds: Vec<u32> = match hit.points.as_slice() {
            [p] => {
                let q = uv(*p);
                let res = geometry.resolution;
                let (x, y) = (((q.x * res as f32) as i64).clamp(0, res as i64 - 1) as u32, ((q.y * res as f32) as i64).clamp(0, res as i64 - 1) as u32);
                let size = subcraft_image::TILE_SIZE;
                let tile = geometry.tile((y / size * geometry.tiles_x + x / size) as usize);
                tile.triangle(((y % size) * size + x % size) as usize).map(|t| vec![t as u32]).unwrap_or_default()
            }
            [a, b] => {
                let (a, b) = (uv(*a), uv(*b));
                let (lo, hi) = (a.min(b), a.max(b));
                (0..self.mesh.triangle_count() as u32)
                    .filter(|&t| {
                        let u = self.mesh.tri_uvs(t as usize);
                        let c = (u[0] + u[1] + u[2]) / 3.0;
                        c.x >= lo.x && c.x <= hi.x && c.y >= lo.y && c.y <= hi.y
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        self.grow(&seeds, g)
    }

    /// Replay hits in order: the last hit that selects a triangle sets its value. Hits made in
    /// the 2D view need `geometry`; symmetry copies of 3D hits go through the mirrored camera.
    pub fn triangle_values(&self, hits: &[DecalHit], geometry: Option<&TextureSetGeometry>) -> Vec<Option<f32>> {
        let mut values = vec![None; self.mesh.triangle_count()];
        for hit in hits {
            let mut all = vec![hit.clone()];
            if let Some(d) = hit.duplicate {
                let m = Mat4::from_cols_array(&d);
                let mut c = hit.clone();
                if hit.uv_space {
                    c.points = hit.points.iter().map(|p| {
                        let q = m.transform_point3(Vec3::new(p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, 0.0));
                        [q.x * 0.5 + 0.5, q.y * 0.5 + 0.5]
                    }).collect();
                } else {
                    // World = M * world_mirrored: the mirrored camera's inverse is M * inverse.
                    c.proj_view_inv = (m * Mat4::from_cols_array(&hit.proj_view_inv)).to_cols_array();
                }
                all.push(c);
            }
            for h in &all {
                let tris = match (h.uv_space, geometry) {
                    (true, Some(g)) => self.select_uv(h, g),
                    (true, None) => Vec::new(),
                    (false, _) => self.select(h, h.through),
                };
                for t in tris {
                    values[t as usize] = Some(h.value);
                }
            }
        }
        values
    }
}

/// Nearest clip-space depth over a screen rectangle (0..1 screen units, y up), rasterized from
/// every triangle; for the visibility of rectangle selections.
struct ScreenDepth {
    rect: [f32; 4],
    size: usize,
    depth: Vec<f32>,
    /// Depth tolerance: 0.2% of the mesh diagonal in clip-space depth units.
    tolerance: f32,
}

impl ScreenDepth {
    const SIZE: usize = 512;

    fn render(mesh: &Mesh, view_proj: &Mat4, rect: [f32; 4]) -> Self {
        let size = Self::SIZE;
        let mut depth = vec![f32::INFINITY; size * size];
        let (w, h) = ((rect[2] - rect[0]).max(1e-6), (rect[3] - rect[1]).max(1e-6));
        let to_px = |sx: f32, sy: f32| ((sx - rect[0]) / w * size as f32, (sy - rect[1]) / h * size as f32);
        for t in 0..mesh.triangle_count() {
            let clip = mesh.tri_positions(t).map(|p| *view_proj * p.extend(1.0));
            if clip.iter().any(|c| c.w <= 1e-9) {
                continue;
            }
            let v = clip.map(|c| {
                let (x, y) = to_px(c.x / c.w * 0.5 + 0.5, c.y / c.w * 0.5 + 0.5);
                Vec3::new(x, y, c.z / c.w)
            });
            let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
            if area.abs() < 1e-12 {
                continue;
            }
            let x0 = v.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let x1 = (v.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil() as i64).min(size as i64 - 1);
            let y0 = v.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let y1 = (v.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil() as i64).min(size as i64 - 1);
            if x1 < x0 as i64 || y1 < y0 as i64 {
                continue;
            }
            for y in y0..=y1 as usize {
                for x in x0..=x1 as usize {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let b0 = ((v[1].x - px) * (v[2].y - py) - (v[2].x - px) * (v[1].y - py)) / area;
                    let b1 = ((v[2].x - px) * (v[0].y - py) - (v[0].x - px) * (v[2].y - py)) / area;
                    let b2 = 1.0 - b0 - b1;
                    if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                        continue;
                    }
                    let z = b0 * v[0].z + b1 * v[1].z + b2 * v[2].z;
                    let d = &mut depth[y * size + x];
                    if z < *d {
                        *d = z;
                    }
                }
            }
        }
        let dz = Vec3::new(view_proj.x_axis.z, view_proj.y_axis.z, view_proj.z_axis.z).length();
        Self { rect, size, depth, tolerance: 2e-3 * mesh.bounds.diagonal() * dz }
    }

    fn visible(&self, sx: f32, sy: f32, z: f32) -> bool {
        let (w, h) = ((self.rect[2] - self.rect[0]).max(1e-6), (self.rect[3] - self.rect[1]).max(1e-6));
        let x = (((sx - self.rect[0]) / w * self.size as f32) as usize).min(self.size - 1);
        let y = (((sy - self.rect[1]) / h * self.size as f32) as usize).min(self.size - 1);
        // The nearest depth around the pixel (centroids sit inside their own triangle's coverage).
        let mut nearest = f32::INFINITY;
        for yy in y.saturating_sub(1)..=(y + 1).min(self.size - 1) {
            for xx in x.saturating_sub(1)..=(x + 1).min(self.size - 1) {
                nearest = nearest.min(self.depth[yy * self.size + xx]);
            }
        }
        z <= nearest + self.tolerance
    }
}

/// A mask paint raster (value, alpha) holding per-triangle values: texels of triangles with a
/// value get it at full alpha, the rest stay transparent.
pub fn raster(geometry: &TextureSetGeometry, values: &[Option<f32>]) -> TiledImage {
    let res = geometry.resolution;
    raster_over(geometry, values, TiledImage::new(res, res, 2, [0.0; 4]))
}

/// [`raster`] written over an existing (value, alpha) raster: selected texels are replaced.
pub fn raster_over(geometry: &TextureSetGeometry, values: &[Option<f32>], mut image: TiledImage) -> TiledImage {
    for t in 0..geometry.tile_count() {
        if geometry.tile_triangles(t).iter().all(|&tri| values.get(tri as usize).copied().flatten().is_none()) {
            continue;
        }
        let tile = geometry.tile(t);
        let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * 2];
        image.read_tile(t, &mut buf);
        let mut any = false;
        for i in 0..subcraft_image::TILE_TEXELS {
            if let Some(v) = tile.triangle(i).and_then(|tri| values.get(tri).copied().flatten()) {
                buf[i * 2] = v;
                buf[i * 2 + 1] = 1.0;
                any = true;
            }
        }
        if any {
            image.write_tile(t, &buf);
        }
    }
    image
}

/// Painter brush strokes replayed through SubCraft's brush engine onto a mask raster.
///
/// Painter records each stroke as screen positions (0..1, origin bottom-left) with the view and
/// (OpenGL) projection matrices of the moment, and the brush size as a fraction of the mesh's
/// bounding-box diagonal, which is what SubCraft's brush size means too (in percent). The look of
/// a stamp is SubCraft's (round tip with Painter's hardness), not Painter's own engine.
pub struct StrokeReplay<'a> {
    picker: &'a Picker,
    geometry: Arc<TextureSetGeometry>,
    depth: subcraft_paint::DepthCache,
    /// A scratch set with one layer whose mask holds one Paint effect (the raster being painted).
    set: subcraft_doc::TextureSet,
    layer: subcraft_core::LayerId,
    effect: subcraft_core::EffectId,
    /// Screen size used to replay (only the brush spacing in pixels depends on it).
    pub viewport: u32,
    /// Painter alignment ids map to SubCraft's (same order); `None` keeps them, else forces one.
    pub alignment: Option<subcraft_paint::Alignment>,
    /// Screen stencil of the strokes being replayed.
    pub stencil: Option<Arc<subcraft_paint::Stencil>>,
    /// Stencil of the 2D-view strokes being replayed.
    pub uv_stencil: Option<UvStencil>,
}

impl<'a> StrokeReplay<'a> {
    pub fn new(picker: &'a Picker, geometry: Arc<TextureSetGeometry>) -> Self {
        let res = geometry.resolution;
        let mut set = subcraft_doc::TextureSet::new("replay", geometry.material_slot, res);
        let tc = set.clone();
        let mut layer = subcraft_doc::Layer::new_fill("replay", &tc);
        let mut mask = subcraft_doc::Mask::new(res, false);
        let effect = subcraft_doc::MaskEffect::new(subcraft_doc::MaskEffectKind::Paint { image: TiledImage::new(res, res, 2, [0.0; 4]) });
        let effect_id = effect.id;
        mask.effects.push(Arc::new(effect));
        layer.mask = Some(mask);
        let layer_id = set.insert_above(None, layer);
        Self { picker, geometry, depth: subcraft_paint::DepthCache::new(), set, layer: layer_id, effect: effect_id, viewport: std::env::var("SUBCRAFT_SPP_REPLAY_VIEWPORT").ok().and_then(|v| v.parse().ok()).unwrap_or(1024), alignment: match std::env::var("SUBCRAFT_SPP_REPLAY_ALIGN").as_deref() {
            Ok("wrap") => Some(subcraft_paint::Alignment::TangentWrap),
            Ok("planar") => Some(subcraft_paint::Alignment::TangentPlanar),
            Ok("camera") => Some(subcraft_paint::Alignment::Camera),
            Ok("uv") => Some(subcraft_paint::Alignment::Uv),
            _ => None,
        }, stencil: None, uv_stencil: None }
    }

    fn set_image(&mut self, image: TiledImage) {
        let effect = self.effect;
        if let Some(m) = self.set.find_mut(self.layer).and_then(|l| l.mask.as_mut()) {
            for e in &mut m.effects {
                if e.id == effect {
                    Arc::make_mut(e).kind = subcraft_doc::MaskEffectKind::Paint { image: image.clone() };
                }
            }
        }
    }

    fn image(&self) -> TiledImage {
        let l = self.set.find(self.layer).expect("replay layer");
        match l.mask.as_ref().and_then(|m| m.effects.first()).map(|e| &e.kind) {
            Some(subcraft_doc::MaskEffectKind::Paint { image }) => image.clone(),
            _ => unreachable!("replay effect is a paint effect"),
        }
    }

    /// The camera of a recorded stroke, with Painter's OpenGL depth range mapped to SubCraft's 0..1.
    fn camera(&self, s: &StrokeInfo) -> subcraft_paint::Camera {
        let gl_to_wgpu = Mat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 1.0]);
        subcraft_paint::Camera { view: Mat4::from_cols_array(&s.view), proj: gl_to_wgpu * Mat4::from_cols_array(&s.projection), viewport: [self.viewport, self.viewport] }
    }

    /// A 3D-view stroke with Painter's UV alignment: each point is cast into the scene (the first
    /// surface it meets, as Painter paints only what it sees) and its dab is stamped in UV around
    /// that point, sized in UV like a 2D-view stroke. Points that meet another texture set or
    /// nothing paint nothing here, and the path is cut where it changes UV island. The screen
    /// stencil is read at each texel's position seen from the stroke's camera.
    fn uv_aligned(&self, image: TiledImage, s: &StrokeInfo, brush: &BrushInfo, opacity: f32, erase: bool) -> TiledImage {
        let proj_view = Mat4::from_cols_array(&s.projection) * Mat4::from_cols_array(&s.view);
        let inv = proj_view.inverse();
        let slot = self.geometry.material_slot;
        let mut runs: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut last_island = None;
        for p in &s.points {
            let (o, d) = Picker::ray(&inv, *p);
            let hit = self.picker.bvh.intersect(o, d, 0.0, f32::INFINITY).filter(|h| self.picker.mesh.tri_material.get(h.tri as usize) == Some(&slot));
            let Some(h) = hit else {
                last_island = None;
                continue;
            };
            let uv = self.picker.mesh.tri_uvs(h.tri as usize);
            let q = uv[0] * (1.0 - h.u - h.v) + uv[1] * h.u + uv[2] * h.v;
            let island = self.picker.islands[h.tri as usize];
            if last_island != Some(island) {
                runs.push(Vec::new());
            }
            last_island = Some(island);
            runs.last_mut().unwrap().push([q.x, 1.0 - q.y]);
        }
        if runs.is_empty() {
            return image;
        }
        let paths: Vec<StrokeInfo> = runs.into_iter().map(|points| StrokeInfo { uv_space: true, points, pressure: Vec::new(), ..s.clone() }).collect();
        let refs: Vec<&StrokeInfo> = paths.iter().collect();
        let res = image.width();
        let vp = self.viewport;
        let stencil = self.stencil.clone();
        let geometry = &self.geometry;
        let stencil_at = |x: usize, y: usize| -> f32 {
            let Some(st) = &stencil else { return 1.0 };
            let (x, y) = (x as u32, y as u32);
            let tile = geometry.tile(((y / subcraft_image::TILE_SIZE) * geometry.tiles_x + x / subcraft_image::TILE_SIZE) as usize);
            let i = ((y % subcraft_image::TILE_SIZE) * subcraft_image::TILE_SIZE + x % subcraft_image::TILE_SIZE) as usize;
            if tile.triangle(i).is_none() || x >= res || y >= res {
                return 0.0;
            }
            let world = Vec3::from(tile.position(i)) * geometry.max_extent + geometry.bounds.min;
            let c = proj_view * world.extend(1.0);
            let ndc = c.truncate() / c.w;
            st.value_at(glam::Vec2::new((ndc.x * 0.5 + 0.5) * vp as f32, (0.5 - ndc.y * 0.5) * vp as f32), [vp, vp])
        };
        uv_dabs(image, &refs, brush, opacity, erase, &stencil_at)
    }

    /// Paint (or erase) `strokes` with `brush` onto `image` (value, alpha) and return the result.
    pub fn replay(&mut self, image: TiledImage, strokes: &[StrokeInfo], brush: &BrushInfo, opacity: f32, erase: bool) -> TiledImage {
        self.set_image(image);
        let alignment = self.alignment.unwrap_or(match brush.alignment {
            0 => subcraft_paint::Alignment::TangentWrap,
            1 => subcraft_paint::Alignment::TangentPlanar,
            2 => subcraft_paint::Alignment::Camera,
            _ => subcraft_paint::Alignment::Uv,
        });
        let settings = subcraft_paint::BrushSettings {
            size: brush.size * 100.0 * brush_scale(),
            flow: brush.flow,
            stroke_opacity: opacity,
            spacing: brush.spacing.max(0.02),
            hardness: brush.hardness.unwrap_or(0.0),
            alignment,
            backface_culling: brush.culling,
            ..subcraft_paint::BrushSettings::default()
        };
        let material = subcraft_paint::BrushMaterial { mask_value: brush.value, ..subcraft_paint::BrushMaterial::default() };
        let mode = if erase { subcraft_paint::StrokeMode::Erase } else { subcraft_paint::StrokeMode::Paint };
        let target = subcraft_paint::PaintTarget::MaskEffect(self.effect);
        // Symmetry copies: a mirrored camera for 3D strokes (as SubCraft's own symmetry), the
        // mirrored UV path (around the centre of the UV square) for 2D ones.
        let mut copies: Vec<StrokeInfo> = Vec::new();
        for s in strokes.iter().filter(|s| s.duplicate.is_some()) {
            let m = Mat4::from_cols_array(&s.duplicate.unwrap());
            let mut c = s.clone();
            c.duplicate = None;
            if s.uv_space {
                c.points = s.points.iter().map(|p| {
                    let q = m.transform_point3(Vec3::new(p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, 0.0));
                    [q.x * 0.5 + 0.5, q.y * 0.5 + 0.5]
                }).collect();
            } else {
                c.view = (Mat4::from_cols_array(&s.view) * m).to_cols_array();
            }
            copies.push(c);
        }
        let all: Vec<&StrokeInfo> = strokes.iter().chain(copies.iter()).collect();
        let uv_strokes: Vec<&StrokeInfo> = all.iter().copied().filter(|s| s.uv_space).collect();
        if !uv_strokes.is_empty() {
            let image = self.image();
            self.set_image(uv_paint(image, &uv_strokes, brush, opacity, erase, self.uv_stencil.as_ref()));
        }
        if brush.alignment == 3 && uv_dabs_enabled() {
            for s in all.iter().filter(|s| !s.uv_space) {
                let image = self.image();
                let image = self.uv_aligned(image, s, brush, opacity, erase);
                self.set_image(image);
            }
            return self.image();
        }
        for (k, s) in all.iter().enumerate().filter(|(_, s)| !s.uv_space) {
            let camera = self.camera(s);
            let ctx = subcraft_paint::PaintContext { mesh: &self.picker.mesh, bvh: &self.picker.bvh, geometry: &self.geometry, camera: &camera, depth: &self.depth };
            let size = match screen_brush() {
                Some(k) => screen_brush_size(s, brush.size * k, &self.picker.mesh),
                None => settings.size,
            };
            let settings = subcraft_paint::BrushSettings { seed: s.seed.wrapping_add(k as u32), size, ..settings };
            let Ok(stroke) = subcraft_paint::Stroke::begin(&self.set, self.layer, target, mode, settings, subcraft_paint::StrokeOptions::default(), &material) else { continue };
            let mut stroke = stroke.with_stencil(self.stencil.clone());
            let vp = self.viewport as f32;
            let screen: Vec<glam::Vec2> = s.points.iter().map(|p| glam::Vec2::new(p[0] * vp, (1.0 - p[1]) * vp)).collect();
            if s.pressure.is_empty() {
                let _ = stroke.add_points(&mut self.set, &ctx, &screen);
            } else {
                for (i, p) in screen.iter().enumerate() {
                    if let Some(pr) = s.pressure.get(i) {
                        stroke.set_pressure(*pr);
                    }
                    let _ = stroke.add_point(&mut self.set, &ctx, *p);
                }
            }
            let _ = stroke.finish(&mut self.set, &ctx);
        }
        self.image()
    }
}

/// With `SUBCRAFT_SPP_BRUSH_SPACE=object` only: SubCraft brush size (percent of the bounding-box
/// diagonal) per unit of Painter's `brushSize` x 100. 0.5 matched best under that reading.
/// `SUBCRAFT_SPP_BRUSH_SCALE` overrides it for experiments.
fn brush_scale() -> f32 {
    std::env::var("SUBCRAFT_SPP_BRUSH_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.5)
}

/// 3D-view strokes' `brushSize` scales with the stroke's view height: the brush diameter is
/// `SCREEN_BRUSH` x `brushSize` x the view height. Measured on DBAL-A2: read against the
/// bounding-box diagonal instead, black mask strokes beside a stencilled label grew over it and
/// cut "DBAL-A" to "RAL-A"; read against the view the label stays whole. The factor is
/// calibrated, not derived: at 1024 without solved detail the sum of the Base Color, Roughness
/// and Metallic differences over the three sample projects is 0.2529 at 1.5, 0.2535 at 1 and
/// 0.2544 at 2. Larger brushes clean DBAL-A2's slot dirt as Painter does but cut into its QR code;
/// no single factor fits both. `SUBCRAFT_SPP_BRUSH_SPACE=object` restores the diagonal reading
/// (times [`brush_scale`]), `screen:k` sets the factor.
fn screen_brush() -> Option<f32> {
    match std::env::var("SUBCRAFT_SPP_BRUSH_SPACE").ok().as_deref() {
        Some("object") => None,
        Some(v) => Some(v.strip_prefix("screen").and_then(|r| r.strip_prefix(':')).and_then(|k| k.parse().ok()).unwrap_or(SCREEN_BRUSH)),
        None => Some(SCREEN_BRUSH),
    }
}

const SCREEN_BRUSH: f32 = 1.5;

/// SubCraft brush size (percent of the mesh's bounding-box diagonal) of a brush whose diameter is
/// `fraction` of the stroke's view height, at the depth of the mesh centre for perspective views.
pub fn screen_brush_size(s: &StrokeInfo, fraction: f32, mesh: &Mesh) -> f32 {
    let proj = Mat4::from_cols_array(&s.projection);
    let view = Mat4::from_cols_array(&s.view);
    let orthographic = proj.w_axis.w.abs() > 0.5;
    let height = if orthographic {
        2.0 / proj.y_axis.y.abs().max(1e-6)
    } else {
        let depth = -view.transform_point3(mesh.bounds.center()).z;
        2.0 * depth.max(1e-6) / proj.y_axis.y.abs().max(1e-6)
    };
    fraction * height / mesh.bounds.diagonal().max(1e-6) * 100.0
}

/// Experiment, off by default (`SUBCRAFT_SPP_UV_DABS=1`): stamp strokes with Painter's UV
/// alignment in UV units like 2D-view strokes ([`StrokeReplay`]) instead of with SubCraft's 3D
/// brush. It fits several DBAL-A2 details (a QR code beside large black stamps, dirt cleaned off a
/// lens) and its Metallic (0.0542 -> 0.0532 at 1024), but CarInteriro's Roughness (0.0504 ->
/// 0.0523) and Street_cyberpunk's Base Color (0.0180 -> 0.0186) get worse at every scale tried
/// (0.08-0.3), and Painter documents alignment as the stamps' orientation only.
fn uv_dabs_enabled() -> bool {
    std::env::var("SUBCRAFT_SPP_UV_DABS").is_ok_and(|v| v == "1")
}

/// Stroke diameter in UV units per unit of Painter's brush size, for strokes painted in the 2D view.
pub fn uv_brush_scale() -> f32 {
    std::env::var("SUBCRAFT_SPP_UV_BRUSH").ok().and_then(|v| v.parse().ok()).unwrap_or(UV_BRUSH_SCALE)
}

/// Calibrated on a project painted in the 2D view (Street_cyberpunk: line widths match best at
/// 0.15; 1.0 doubles the Base Color difference). Painter seems to size 2D strokes by its 2D
/// viewport, whose zoom is not stored.
const UV_BRUSH_SCALE: f32 = 0.15;

/// Strokes painted in Painter's 2D view, stamped straight into the (value, alpha) raster: round
/// stamps along the UV path at the brush spacing, each adding `flow` with the brush falloff, the
/// stroke's coverage capped at `opacity`.
pub fn uv_paint(image: TiledImage, strokes: &[&StrokeInfo], brush: &BrushInfo, opacity: f32, erase: bool, stencil: Option<&UvStencil>) -> TiledImage {
    let res = image.width() as f32;
    let stencil_at = |x: usize, y: usize| stencil.map_or(1.0, |s| s.value_at(glam::Vec2::new((x as f32 + 0.5) / res, (y as f32 + 0.5) / res)));
    uv_dabs(image, strokes, brush, opacity, erase, &stencil_at)
}

/// Stamps along UV paths (points in Painter's UV, v up) with a round tip `brush.size` x
/// [`uv_brush_scale`] of the UV square across; `stencil_at(x, y)` scales each texel's paint.
pub fn uv_dabs(mut image: TiledImage, strokes: &[&StrokeInfo], brush: &BrushInfo, opacity: f32, erase: bool, stencil_at: &dyn Fn(usize, usize) -> f32) -> TiledImage {
    let res = image.width() as usize;
    let radius = (brush.size * uv_brush_scale() * 0.5 * res as f32).max(0.5);
    let spacing = (brush.spacing.max(0.02) * 2.0 * radius).max(0.5);
    let hardness = brush.hardness.unwrap_or(0.0);
    for s in strokes {
        let pts: Vec<glam::Vec2> = s.points.iter().map(|p| glam::Vec2::new(p[0] * res as f32, (1.0 - p[1]) * res as f32)).collect();
        let mut stamps = Vec::new();
        if let Some(first) = pts.first() {
            stamps.push(*first);
            let mut carry = 0.0f32;
            for w in pts.windows(2) {
                let len = (w[1] - w[0]).length();
                let mut d = spacing - carry;
                while d <= len {
                    stamps.push(w[0].lerp(w[1], d / len.max(1e-9)));
                    d += spacing;
                }
                carry = (carry + len) % spacing;
            }
        }
        let mut cov: std::collections::HashMap<(usize, usize), f32> = std::collections::HashMap::new();
        for c in &stamps {
            let (x0, x1) = ((c.x - radius).floor().max(0.0) as usize, ((c.x + radius).ceil() as usize).min(res - 1));
            let (y0, y1) = ((c.y - radius).floor().max(0.0) as usize, ((c.y + radius).ceil() as usize).min(res - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = (glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - *c).length() / radius;
                    let a = subcraft_paint::falloff(d, hardness) * brush.flow;
                    if a > 0.0 {
                        let e = cov.entry((x, y)).or_default();
                        *e += (1.0 - *e) * a;
                    }
                }
            }
        }
        for ((x, y), c) in cov {
            let c = c.min(1.0) * opacity * stencil_at(x, y);
            if c <= 0.0 {
                continue;
            }
            let p = image.get(x as u32, y as u32);
            let (v, a) = (p[0], p[1]);
            let px = if erase {
                [v, a * (1.0 - c), 0.0, 0.0]
            } else {
                let na = c + a * (1.0 - c);
                [if na > 1e-6 { (brush.value * c + v * a * (1.0 - c)) / na } else { 0.0 }, na, 0.0, 0.0]
            };
            image.set(x as u32, y as u32, px);
        }
    }
    image
}

/// Stencil of a stroke made in Painter's 2D view: the action's 3x3 `projMatrix` (column major)
/// takes the texel's UV (Painter's, v up) straight to stencil UV (v up), as the stroke points
/// are UVs. On DBAL-A2 this puts the "CAGE / P/N / S/N" stencil text beside the QR code turned
/// a quarter, as Painter shows it.
#[derive(Clone, Debug)]
pub struct UvStencil {
    pub bitmap: Arc<subcraft_image::Bitmap>,
    pub matrix: [f32; 9],
}

impl UvStencil {
    /// Paint factor at SubCraft UV `uv` (origin top-left): image brightness x alpha, 0 outside.
    pub fn value_at(&self, uv: glam::Vec2) -> f32 {
        let m = &self.matrix;
        let (x, y) = (uv.x, 1.0 - uv.y);
        let (u, v) = (m[0] * x + m[3] * y + m[6], m[1] * x + m[4] * y + m[7]);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return 0.0;
        }
        let t = self.bitmap.sample(u, 1.0 - v, subcraft_image::Wrap::Clamp, false);
        ((0.2126 * t[0] + 0.7152 * t[1] + 0.0722 * t[2]) * t[3]).clamp(0.0, 1.0)
    }
}

/// Where Painter samples a fill's image for texel UV `uv` (SubCraft's, origin top-left): in
/// Painter's UV (origin bottom-left) the image's centre sits at 0.5 + translation, and the image
/// is scaled and rotated (radians) around it. Image coordinates are returned top-left. Measured
/// on DBAL-A2's placed images: the QR code (no rotation) is centred at (0.352, 0.266) and the
/// sticker (rotation 4.74) at (0.592, 0.736), both 0.5 + translation within a texel at 1024.
pub fn painter_image_uv(t: &subcraft_mesh::spp::document::UvTransform, uv: glam::Vec2) -> glam::Vec2 {
    let p = glam::Vec2::new(uv.x, 1.0 - uv.y);
    let rotation = t.rotation * rotation_sign();
    let (s, c) = rotation.sin_cos();
    let d = (p - glam::Vec2::splat(0.5) - glam::Vec2::from(t.offset)) * glam::Vec2::from(t.scale);
    let q = glam::Vec2::new(d.x * c - d.y * s, d.x * s + d.y * c) + glam::Vec2::splat(0.5);
    glam::Vec2::new(q.x, 1.0 - q.y)
}

/// Experiment: `SUBCRAFT_SPP_ROT_SIGN=-1` turns Painter's UV rotation the other way.
pub fn rotation_sign() -> f32 {
    if std::env::var("SUBCRAFT_SPP_ROT_SIGN").is_ok_and(|v| v == "-1") { -1.0 } else { 1.0 }
}

/// A bitmap placed once (not tiled) on the UV square, baked into a mask raster (value = image
/// brightness, alpha 1 inside the image, transparent outside).
pub fn placed_bitmap(geometry: &TextureSetGeometry, bitmap: &subcraft_image::Bitmap, t: &subcraft_mesh::spp::document::UvTransform) -> TiledImage {
    let res = geometry.resolution;
    let mut image = TiledImage::new(res, res, 2, [0.0; 4]);
    for tile in 0..geometry.tile_count() {
        let geo = geometry.tile(tile);
        if geo.is_empty() {
            continue;
        }
        let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * 2];
        let mut any = false;
        for i in 0..subcraft_image::TILE_TEXELS {
            let q = painter_image_uv(t, glam::Vec2::from(geo.uv(i)));
            if (0.0..=1.0).contains(&q.x) && (0.0..=1.0).contains(&q.y) {
                let px = bitmap.sample(q.x, q.y, subcraft_image::Wrap::Clamp, false);
                buf[i * 2] = px[0];
                buf[i * 2 + 1] = 1.0;
                any = true;
            }
        }
        if any {
            image.write_tile(tile, &buf);
        }
    }
    image
}

/// One step of a mask raster: a polygon fill or a stroke action.
#[derive(Clone, Debug)]
pub enum RasterOp {
    Fill(Vec<DecalHit>),
    /// `stencil` masks strokes made in the 3D view; `uv_stencil` (the image and Painter's matrix)
    /// those made in the 2D view.
    Strokes { strokes: Vec<StrokeInfo>, brush: BrushInfo, opacity: f32, erase: bool, stencil: Option<Arc<subcraft_paint::Stencil>>, uv_stencil: Option<UvStencil> },
}

/// SubCraft's screen stencil for Painter's stencil matrix (clip-space screen position to stencil
/// UV, v up), on the square replay viewport.
pub fn stencil_of(bitmap: Arc<subcraft_image::Bitmap>, m: &[f32; 9]) -> subcraft_paint::Stencil {
    // u = a·x + c·y + e and v = b·x + d·y + f for clip-space x, y.
    let (a, b, c, d, e, f) = (m[0], m[1], m[3], m[4], m[6], m[7]);
    let sx = (a * a + b * b).sqrt().max(1e-6);
    let sy = (c * c + d * d).sqrt().max(1e-6);
    // The image center (u, v) = (0.5, 0.5) in clip space, then in viewport fractions (y down).
    let det = a * d - b * c;
    let (cx, cy) = if det.abs() > 1e-9 { ((d * (0.5 - e) - c * (0.5 - f)) / det, (a * (0.5 - f) - b * (0.5 - e)) / det) } else { (0.0, 0.0) };
    let mut st = subcraft_paint::Stencil::new(bitmap);
    st.center = glam::Vec2::new(cx * 0.5 + 0.5, 0.5 - cy * 0.5);
    // One UV unit spans 2/s clip units = 1/s viewport fractions.
    st.scale = 1.0 / (2.0 * sy);
    st.rotation = b.atan2(a);
    let _ = sx;
    st
}

/// Paint `ops` in order into a new (value, alpha) raster at the geometry's resolution.
pub fn paint_raster(picker: &Picker, geometry: Arc<TextureSetGeometry>, ops: &[RasterOp]) -> TiledImage {
    let res = geometry.resolution;
    let mut image = TiledImage::new(res, res, 2, [0.0; 4]);
    let mut replay: Option<StrokeReplay> = None;
    for op in ops {
        image = match op {
            RasterOp::Fill(hits) => raster_over(&geometry, &picker.triangle_values(hits, Some(&geometry)), image),
            RasterOp::Strokes { strokes, brush, opacity, erase, stencil, uv_stencil } => {
                let r = replay.get_or_insert_with(|| StrokeReplay::new(picker, geometry.clone()));
                r.stencil = stencil.clone();
                r.uv_stencil = uv_stencil.clone();
                r.replay(image, strokes, brush, *opacity, *erase)
            }
        };
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use subcraft_mesh::primitives;

    /// A Painter-style hit: an orthographic camera in front of the cube (+Z), clicked at `p`
    /// (0..1, origin bottom-left), stored as the inverse view-projection in OpenGL clip space.
    fn hit(points: Vec<[f32; 2]>, granularity: i64, value: f32) -> DecalHit {
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::orthographic_rh_gl(-2.0, 2.0, -2.0, 2.0, 0.1, 20.0);
        DecalHit { value, granularity, points, proj_view_inv: (proj * view).inverse().to_cols_array(), through: false, uv_space: false, duplicate: None }
    }

    fn cube_picker() -> Picker {
        Picker::new(Arc::new(primitives::cube(2.0)))
    }

    #[test]
    fn screen_points_are_bottom_up_like_opengl() {
        let h = hit(vec![[0.5, 0.75]], 0, 1.0);
        let (o, d) = Picker::ray(&Mat4::from_cols_array(&h.proj_view_inv), h.points[0]);
        assert!((o.y - 1.0).abs() < 1e-4 && o.x.abs() < 1e-4, "0.75 up the screen is y = +1 for a half height of 2: {o}");
        assert!((d - Vec3::NEG_Z).length() < 1e-4, "looking down -Z: {d}");
    }

    #[test]
    fn granularity_grows_the_clicked_triangle() {
        let p = cube_picker();
        let mesh = p.mesh.clone();
        let front = |t: &u32| mesh.geometric_normal(*t as usize).z > 0.9;
        let tri = p.select(&hit(vec![[0.55, 0.6]], 0, 1.0), false);
        assert_eq!(tri.len(), 1);
        assert!(tri.iter().all(front));
        let island = p.select(&hit(vec![[0.55, 0.6]], 3, 1.0), false);
        assert_eq!(island.len(), 32, "a cube face is one UV island (4x4 quads)");
        assert!(island.iter().all(front));
        assert_eq!(p.select(&hit(vec![[0.55, 0.6]], 2, 1.0), false).len(), 192, "the cube is one mesh element");
        assert!(p.select(&hit(vec![[0.5, 0.95]], 2, 1.0), false).is_empty(), "a click beside the cube selects nothing");
        let through = p.select(&hit(vec![[0.55, 0.6]], 3, 1.0), true);
        assert_eq!(through.len(), 64, "paint-through also takes the back face");
    }

    #[test]
    fn screen_brush_size_is_a_fraction_of_the_view_height() {
        let mesh = subcraft_mesh::primitives::cube(2.0);
        let diag = mesh.bounds.diagonal();
        // Orthographic view 4 units tall (y scale 0.5): a 0.1 brush is 0.4 units across.
        let ortho = Mat4::orthographic_rh_gl(-2.0, 2.0, -2.0, 2.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y);
        let s = StrokeInfo { uv_space: false, points: Vec::new(), pressure: Vec::new(), view: view.to_cols_array(), projection: ortho.to_cols_array(), seed: 0, duplicate: None };
        assert!((screen_brush_size(&s, 0.1, &mesh) - 0.4 / diag * 100.0).abs() < 1e-4);
        // Perspective: the view height at the mesh centre (10 units away, 90 degrees: 20 units).
        let persp = Mat4::perspective_rh_gl(90f32.to_radians(), 1.0, 0.1, 100.0);
        let s = StrokeInfo { projection: persp.to_cols_array(), ..s };
        assert!((screen_brush_size(&s, 0.1, &mesh) - 2.0 / diag * 100.0).abs() < 1e-3);
    }

    #[test]
    fn uv_aligned_strokes_stamp_only_where_they_meet_the_set() {
        let p = cube_picker();
        let geometry = Arc::new(TextureSetGeometry::new(p.mesh.clone(), 0, 64));
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::orthographic_rh_gl(-2.0, 2.0, -2.0, 2.0, 0.1, 20.0);
        let stroke = |points: Vec<[f32; 2]>| StrokeInfo { uv_space: false, duplicate: None, points, pressure: Vec::new(), view: view.to_cols_array(), projection: proj.to_cols_array(), seed: 1 };
        let brush = BrushInfo { size: 0.2, size_space: 1, flow: 1.0, spacing: 0.1, pressure_size: false, value: 1.0, hardness: Some(1.0), tip: "shape".into(), tip_text: None, alignment: 3, culling: true, stencil: None };
        let replay = StrokeReplay::new(&p, geometry.clone());
        let painted = |img: &TiledImage| (0..64).flat_map(|y| (0..64).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y)[1] > 0.5).collect::<Vec<_>>();
        // Across the front face: texels of the front face's island only, around the path.
        let on = replay.uv_aligned(TiledImage::new(64, 64, 2, [0.0; 4]), &stroke((0..10).map(|i| [0.4 + i as f32 * 0.02, 0.5]).collect()), &brush, 1.0, false);
        let texels = painted(&on);
        assert!(!texels.is_empty());
        let island_of = |x: u32, y: u32| {
            let tile = geometry.tile(geometry.tiles_x as usize * (y / 64) as usize + (x / 64) as usize);
            tile.triangle(((y % 64) * 64 + x % 64) as usize).map(|t| p.islands[t])
        };
        let front = p.islands[p.select(&hit(vec![[0.5, 0.5]], 0, 1.0), false)[0] as usize];
        assert!(texels.iter().all(|&(x, y)| island_of(x, y).is_none_or(|i| i == front)), "stays on the front island in UV");
        // Over the background (the cube spans the middle half of the view): nothing at all.
        let off = replay.uv_aligned(TiledImage::new(64, 64, 2, [0.0; 4]), &stroke(vec![[0.05, 0.05], [0.1, 0.05]]), &brush, 1.0, false);
        assert!(painted(&off).is_empty());
    }

    #[test]
    fn uv_stencil_maps_painter_uv_to_the_image() {
        // White left half, black right half; RGBA with full alpha.
        let bitmap = Arc::new(subcraft_image::Bitmap::new_u8(2, 1, 4, vec![255, 255, 255, 255, 0, 0, 0, 255]));
        let identity = UvStencil { bitmap: bitmap.clone(), matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] };
        assert!(identity.value_at(glam::Vec2::new(0.1, 0.5)) > 0.99);
        assert!(identity.value_at(glam::Vec2::new(0.9, 0.5)) < 0.01);
        // A quarter turn (u = 1 - y, v = x): Painter's v (up) picks the image column. The top of
        // the texture (Painter v 0.9) reads the white left half, the bottom the black right half.
        let turned = UvStencil { bitmap: bitmap.clone(), matrix: [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 1.0] };
        assert!(turned.value_at(glam::Vec2::new(0.5, 0.1)) > 0.99);
        assert!(turned.value_at(glam::Vec2::new(0.5, 0.9)) < 0.01);
        // Outside the image: nothing.
        let small = UvStencil { bitmap, matrix: [4.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 1.0] };
        assert_eq!(small.value_at(glam::Vec2::new(0.8, 0.2)), 0.0);
    }

    #[test]
    fn mesh_fill_joins_objects_that_share_connected_geometry() {
        // Objects 0 and 1 are welded through element 7; object 2 has two separate pieces.
        let objects = [0, 0, 1, 1, 2, 2, 3];
        let elements = [7, 7, 7, 8, 9, 10, 11];
        assert_eq!(join_connected(&objects, &elements), vec![0, 0, 0, 0, 2, 2, 3]);
    }

    #[test]
    fn rectangles_take_visible_triangles_and_later_hits_win() {
        let p = cube_picker();
        let all = p.select(&hit(vec![[0.0, 0.0], [1.0, 1.0]], 0, 1.0), false);
        assert_eq!(all.len(), 32, "only the front face is visible: {all:?}");
        let values = p.triangle_values(&[hit(vec![[0.55, 0.6]], 2, 1.0), hit(vec![[0.55, 0.6]], 3, 0.0)], None);
        assert_eq!(values.iter().filter(|v| **v == Some(1.0)).count(), 160);
        assert_eq!(values.iter().filter(|v| **v == Some(0.0)).count(), 32);
    }

    #[test]
    fn small_rectangles_take_the_triangles_they_touch_and_drags_without_area_are_clicks() {
        let p = cube_picker();
        let click = p.select(&hit(vec![[0.55, 0.6]], 0, 1.0), false);
        assert_eq!(click.len(), 1);
        // A drag without height selects what a click at its centre selects.
        assert_eq!(p.select(&hit(vec![[0.549, 0.6], [0.551, 0.6]], 0, 1.0), false), click);
        // A rectangle inside one triangle, away from its centre (each triangle spans 1/8 of the
        // view): that triangle, though its centre is outside the rectangle.
        let inside = p.select(&hit(vec![[0.538, 0.592], [0.552, 0.6]], 0, 1.0), false);
        assert!(!inside.is_empty() && inside.len() <= 2, "{inside:?}");
        assert!(inside.iter().all(|&t| p.mesh.geometric_normal(t as usize).z > 0.9));
    }

    #[test]
    fn raster_and_strokes_land_on_the_selected_texels() {
        let p = cube_picker();
        let geometry = Arc::new(TextureSetGeometry::new(p.mesh.clone(), 0, 64));
        let front_texel = |x: u32, y: u32| {
            let tile = geometry.tile(geometry.tiles_x as usize * (y / 64) as usize + (x / 64) as usize);
            let i = ((y % 64) * 64 + x % 64) as usize;
            tile.triangle(i).map(|t| p.mesh.geometric_normal(t).z > 0.9)
        };
        let filled = raster(&geometry, &p.triangle_values(&[hit(vec![[0.55, 0.6]], 3, 1.0)], None));
        let (mut inside, mut outside) = (0, 0);
        for y in 0..64 {
            for x in 0..64 {
                match front_texel(x, y) {
                    Some(true) => inside += (filled.get(x, y)[1] > 0.99) as usize,
                    Some(false) => outside += (filled.get(x, y)[1] > 0.0) as usize,
                    None => {}
                }
            }
        }
        assert!(inside > 50, "{inside} front texels filled");
        assert_eq!(outside, 0);
        // A horizontal stroke across the front face, with a white mask brush.
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::orthographic_rh_gl(-2.0, 2.0, -2.0, 2.0, 0.1, 20.0);
        let stroke = StrokeInfo { uv_space: false, duplicate: None, points: (0..20).map(|i| [0.3 + i as f32 * 0.02, 0.5]).collect(), pressure: Vec::new(), view: view.to_cols_array(), projection: proj.to_cols_array(), seed: 1 };
        // 0.05 of the 4-unit view x 1.5: 0.3 units across, inside the 2-unit face along the path.
        let brush = BrushInfo { size: 0.05, size_space: 1, flow: 1.0, spacing: 0.1, pressure_size: false, value: 1.0, hardness: Some(0.5), tip: "shape".into(), tip_text: None, alignment: 0, culling: true, stencil: None };
        let mut replay = StrokeReplay::new(&p, geometry.clone());
        let painted = replay.replay(TiledImage::new(64, 64, 2, [0.0; 4]), &[stroke], &brush, 1.0, false);
        let (mut on_front, mut elsewhere) = (0, 0);
        for y in 0..64 {
            for x in 0..64 {
                let a = painted.get(x, y)[1];
                match front_texel(x, y) {
                    Some(true) => on_front += (a > 0.5) as usize,
                    _ => elsewhere += (a > 0.01) as usize,
                }
            }
        }
        assert!(on_front > 20, "{on_front}");
        assert_eq!(elsewhere, 0, "the stroke stays on the visible face");
        let erased = replay.replay(painted, &[StrokeInfo { uv_space: false, duplicate: None, points: (0..20).map(|i| [0.3 + i as f32 * 0.02, 0.5]).collect(), pressure: Vec::new(), view: view.to_cols_array(), projection: proj.to_cols_array(), seed: 2 }], &brush, 1.0, true);
        assert!((0..64).all(|y| (0..64).all(|x| erased.get(x, y)[1] < 0.5)), "erasing the same stroke clears it");
        let _ = Vec2::ZERO;
    }

    #[test]
    fn strokes_from_the_2d_view_and_their_mirror_copies_paint_in_uv_space() {
        let p = cube_picker();
        let geometry = Arc::new(TextureSetGeometry::new(p.mesh.clone(), 0, 64));
        // A vertical line at u = 0.25 (v from bottom to top), mirrored across the middle.
        let mirror = Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)).to_cols_array();
        let stroke = StrokeInfo { uv_space: true, duplicate: Some(mirror), points: vec![[0.25, 0.1], [0.25, 0.9]], pressure: Vec::new(), view: [0.0; 16], projection: [0.0; 16], seed: 0 };
        let brush = BrushInfo { size: 0.5, size_space: 1, flow: 1.0, spacing: 0.1, pressure_size: false, value: 1.0, hardness: Some(0.8), tip: "shape".into(), tip_text: None, alignment: 3, culling: true, stencil: None };
        let mut replay = StrokeReplay::new(&p, geometry);
        let img = replay.replay(TiledImage::new(64, 64, 2, [0.0; 4]), &[stroke], &brush, 1.0, false);
        let a = |u: f32, v: f32| img.get((u * 64.0) as u32, ((1.0 - v) * 64.0) as u32)[1];
        assert!(a(0.25, 0.5) > 0.9 && a(0.75, 0.5) > 0.9, "the line and its mirror copy");
        assert!(a(0.5, 0.5) < 0.01 && a(0.1, 0.5) < 0.01 && a(0.25, 0.97) < 0.01, "nothing beside or past the ends");
        // UV polygon fill: the island under a UV point.
        let island = DecalHit { value: 1.0, granularity: 3, points: vec![[0.5, 0.5]], proj_view_inv: [0.0; 16], through: false, uv_space: true, duplicate: None };
        let geometry = TextureSetGeometry::new(p.mesh.clone(), 0, 64);
        let selected = p.triangle_values(&[island], Some(&geometry)).iter().filter(|v| v.is_some()).count();
        assert!(selected == 32 || selected == 0, "one cube face (or a gap between islands): {selected}");
    }

    #[test]
    fn painter_uv_placement_matches_the_affine_projection() {
        use subcraft_mesh::spp::document::UvTransform;
        let id = UvTransform { scale: [1.0, 1.0], rotation: 0.0, offset: [0.0, 0.0], tiling: true };
        let q = painter_image_uv(&id, glam::Vec2::new(0.3, 0.8));
        assert!((q - glam::Vec2::new(0.3, 0.8)).length() < 1e-6, "identity: {q}");
        // A sticker 1/10 of the square, shifted, rotated (radians): its centre is at Painter UV
        // 0.5 + translation, i.e. SubCraft UV (0.7, 1 - 0.4).
        let t = UvTransform { scale: [10.0, 10.0], rotation: 0.5, offset: [0.2, -0.1], tiling: false };
        let centre = glam::Vec2::new(0.7, 1.0 - 0.4);
        assert!((painter_image_uv(&t, centre) - glam::Vec2::splat(0.5)).length() < 1e-5);
        // One image width to the right of the centre (Painter U) reaches the image's edge, turned.
        let edge = painter_image_uv(&t, centre + glam::Vec2::new(0.05, 0.0));
        assert!(((edge - glam::Vec2::splat(0.5)).length() - 0.5).abs() < 1e-4, "{edge}");
        // The same map as SubCraft's projection with the converted parameters.
        let p = subcraft_doc::Projection { scale: t.scale, rotation_deg: -t.rotation.to_degrees(), offset: painter_image_uv(&t, glam::Vec2::ZERO).into(), ..Default::default() };
        for uv in [[0.1f32, 0.2], [0.7, 0.4], [0.9, 0.95]] {
            let a = painter_image_uv(&t, glam::Vec2::from(uv));
            let b = glam::Vec2::from(p.transform_2d(uv));
            assert!((a - b).length() < 1e-4, "{a} vs {b}");
        }
    }
}
