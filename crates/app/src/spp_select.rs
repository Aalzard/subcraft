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
    MeshElement,
    UvIsland,
}

impl Granularity {
    pub fn from_id(id: i64) -> Self {
        match id {
            2 => Granularity::MeshElement,
            3 => Granularity::UvIsland,
            // 0 is a single triangle; 1 (polygon/quad) is treated as its triangle.
            _ => Granularity::Triangle,
        }
    }
}

/// Mesh, ray tracer and connectivity used to replay polygon fills.
pub struct Picker {
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
    parts: Vec<u32>,
    islands: Vec<u32>,
}

impl Picker {
    pub fn new(mesh: Arc<Mesh>) -> Self {
        let bvh = Bvh::build(&mesh);
        let parts = subcraft_mesh::topology::mesh_elements(&mesh);
        let islands = subcraft_mesh::topology::uv_islands(&mesh);
        Self { mesh, bvh, parts, islands }
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
            Granularity::MeshElement => &self.parts,
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
        let seeds: Vec<u32> = match hit.points.as_slice() {
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
        let mut out = Vec::new();
        for t in 0..self.mesh.triangle_count() {
            let p = self.mesh.tri_positions(t);
            let c = (p[0] + p[1] + p[2]) / 3.0;
            let clip = view_proj * c.extend(1.0);
            if clip.w.abs() < 1e-12 {
                continue;
            }
            let (sx, sy) = (clip.x / clip.w * 0.5 + 0.5, clip.y / clip.w * 0.5 + 0.5);
            if sx < x0 || sx > x1 || sy < y0 || sy > y1 {
                continue;
            }
            // Visible: the ray through the centroid's screen point reaches this triangle first.
            let (o, d) = Self::ray(inv, [sx, sy]);
            if self.bvh.intersect(o, d, 0.0, f32::INFINITY).is_some_and(|h| h.tri as usize == t || (o + d * h.t - c).length() < 1e-4 * self.mesh.bounds.extent().max_element()) {
                out.push(t as u32);
            }
        }
        out
    }

    /// Replay hits in order: the last hit that selects a triangle sets its value.
    pub fn triangle_values(&self, hits: &[DecalHit]) -> Vec<Option<f32>> {
        let mut values = vec![None; self.mesh.triangle_count()];
        for hit in hits {
            for t in self.select(hit, hit.through) {
                values[t as usize] = Some(hit.value);
            }
        }
        values
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
        } }
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
            size: brush.size * 100.0,
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
        for (k, s) in strokes.iter().enumerate() {
            let camera = self.camera(s);
            let ctx = subcraft_paint::PaintContext { mesh: &self.picker.mesh, bvh: &self.picker.bvh, geometry: &self.geometry, camera: &camera, depth: &self.depth };
            let settings = subcraft_paint::BrushSettings { seed: s.seed.wrapping_add(k as u32), ..settings };
            let Ok(mut stroke) = subcraft_paint::Stroke::begin(&self.set, self.layer, target, mode, settings, subcraft_paint::StrokeOptions::default(), &material) else { continue };
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

/// One step of a mask raster: a polygon fill or a stroke action.
#[derive(Clone, Debug)]
pub enum RasterOp {
    Fill(Vec<DecalHit>),
    Strokes { strokes: Vec<StrokeInfo>, brush: BrushInfo, opacity: f32, erase: bool },
}

/// Paint `ops` in order into a new (value, alpha) raster at the geometry's resolution.
pub fn paint_raster(picker: &Picker, geometry: Arc<TextureSetGeometry>, ops: &[RasterOp]) -> TiledImage {
    let res = geometry.resolution;
    let mut image = TiledImage::new(res, res, 2, [0.0; 4]);
    let mut replay: Option<StrokeReplay> = None;
    for op in ops {
        image = match op {
            RasterOp::Fill(hits) => raster_over(&geometry, &picker.triangle_values(hits), image),
            RasterOp::Strokes { strokes, brush, opacity, erase } => {
                replay.get_or_insert_with(|| StrokeReplay::new(picker, geometry.clone())).replay(image, strokes, brush, *opacity, *erase)
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
        DecalHit { value, granularity, points, proj_view_inv: (proj * view).inverse().to_cols_array(), through: false }
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
    fn rectangles_take_visible_triangles_and_later_hits_win() {
        let p = cube_picker();
        let all = p.select(&hit(vec![[0.0, 0.0], [1.0, 1.0]], 0, 1.0), false);
        assert_eq!(all.len(), 32, "only the front face is visible: {all:?}");
        let values = p.triangle_values(&[hit(vec![[0.55, 0.6]], 2, 1.0), hit(vec![[0.55, 0.6]], 3, 0.0)]);
        assert_eq!(values.iter().filter(|v| **v == Some(1.0)).count(), 160);
        assert_eq!(values.iter().filter(|v| **v == Some(0.0)).count(), 32);
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
        let filled = raster(&geometry, &p.triangle_values(&[hit(vec![[0.55, 0.6]], 3, 1.0)]));
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
        let stroke = StrokeInfo { points: (0..20).map(|i| [0.3 + i as f32 * 0.02, 0.5]).collect(), pressure: Vec::new(), view: view.to_cols_array(), projection: proj.to_cols_array(), seed: 1 };
        let brush = BrushInfo { size: 0.1, size_space: 1, flow: 1.0, spacing: 0.1, pressure_size: false, value: 1.0, hardness: Some(0.5), tip: "shape".into(), alignment: 0, culling: true };
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
        let erased = replay.replay(painted, &[StrokeInfo { points: (0..20).map(|i| [0.3 + i as f32 * 0.02, 0.5]).collect(), pressure: Vec::new(), view: view.to_cols_array(), projection: proj.to_cols_array(), seed: 2 }], &brush, 1.0, true);
        assert!((0..64).all(|y| (0..64).all(|x| erased.get(x, y)[1] < 0.5)), "erasing the same stroke clears it");
        let _ = Vec2::ZERO;
    }
}
