//! Multi-scale curvature for the Curvature generator's detail sizes ([`subcraft_doc::CurvatureDetail`]).
//!
//! Fine detail is the baked curvature map itself. Medium and coarse detail are the same map
//! averaged over larger UV neighbourhoods: a box-filtered grid of fixed size (so a detail size
//! means the same fraction of the texture at every resolution), smoothed once with a [1 2 1]
//! kernel and read back bilinearly. A grid is built once per baked map (shared by every tile and
//! every generator reading it) and kept while that map's storage is alive.

use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;
use subcraft_doc::{CurvatureDetail, CurvatureMode};
use subcraft_image::{TILE_TEXELS, TiledImage};
use subcraft_materials::sample::TileGeometry;

/// Grid sizes (cells per side) of the medium and coarse scales: about 8 and 32 texels of a 2048
/// map per cell.
pub const MEDIUM_GRID: u32 = 256;
pub const COARSE_GRID: u32 = 64;
/// Averaging flattens curvature; these gains bring a scale's typical contrast back near the fine
/// map's, so weight 1 reads as "as strong as the fine detail".
const MEDIUM_GAIN: f32 = 2.0;
const COARSE_GAIN: f32 = 4.0;
/// Baked maps whose grids stay cached (a few texture sets' curvature).
const CACHE_ENTRIES: usize = 8;

/// A smoothed, box-filtered copy of a map's first component.
#[derive(Debug)]
pub struct Grid {
    size: u32,
    data: Vec<f32>,
}

impl Grid {
    /// Box-filter the first component of `img` into `size` x `size` cells (fewer when the map is
    /// smaller), then smooth once with [1 2 1] in each direction.
    pub fn build(img: &TiledImage, size: u32) -> Self {
        let (w, h) = (img.width().max(1), img.height().max(1));
        let g = size.min(w).min(h).max(1);
        let comps = img.comps();
        let cell_x = |x: u32| (x as u64 * g as u64 / w as u64) as u32;
        let cell_y = |y: u32| (y as u64 * g as u64 / h as u64) as u32;
        // Per tile: sums over the cells it touches; tiles never write shared state.
        let partials: Vec<(u32, u32, u32, Vec<f32>, Vec<u32>)> = (0..img.tile_count())
            .into_par_iter()
            .map(|ti| {
                let r = img.tile_rect(ti);
                let (cx0, cy0) = (cell_x(r.x0 as u32), cell_y(r.y0 as u32));
                let (cx1, cy1) = (cell_x((r.x1 - 1) as u32), cell_y((r.y1 - 1) as u32));
                let cw = cx1 - cx0 + 1;
                let mut sum = vec![0.0f32; (cw * (cy1 - cy0 + 1)) as usize];
                let mut count = vec![0u32; sum.len()];
                let mut buf = vec![0.0f32; TILE_TEXELS * comps];
                img.read_tile(ti, &mut buf);
                let ts = subcraft_image::TILE_SIZE as usize;
                for y in r.y0..r.y1 {
                    let row = (cell_y(y as u32) - cy0) * cw;
                    let ly = (y - r.y0) as usize;
                    for x in r.x0..r.x1 {
                        let k = (row + cell_x(x as u32) - cx0) as usize;
                        sum[k] += buf[(ly * ts + (x - r.x0) as usize) * comps];
                        count[k] += 1;
                    }
                }
                (cx0, cy0, cw, sum, count)
            })
            .collect();
        let n = (g * g) as usize;
        let (mut sum, mut count) = (vec![0.0f32; n], vec![0u32; n]);
        for (cx0, cy0, cw, s, c) in partials {
            for (i, (v, k)) in s.iter().zip(&c).enumerate() {
                let (x, y) = (cx0 + i as u32 % cw, cy0 + i as u32 / cw);
                let at = (y * g + x) as usize;
                sum[at] += v;
                count[at] += k;
            }
        }
        let mut data: Vec<f32> = sum.iter().zip(&count).map(|(s, &c)| if c > 0 { s / c as f32 } else { 0.5 }).collect();
        smooth(&mut data, g as usize);
        Self { size: g, data }
    }

    /// Bilinear value at texture coordinates `uv` (texel centres aligned, edges clamped).
    #[inline]
    pub fn sample(&self, uv: [f32; 2]) -> f32 {
        let n = self.size as usize;
        let max = (n - 1) as f32;
        let fx = (uv[0] * n as f32 - 0.5).clamp(0.0, max);
        let fy = (uv[1] * n as f32 - 0.5).clamp(0.0, max);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| self.data[y * n + x];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
        let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
        top + (bot - top) * ty
    }
}

/// One [1 2 1] / 4 pass along x then y, edges clamped.
fn smooth(data: &mut [f32], n: usize) {
    if n < 2 {
        return;
    }
    let mut tmp = vec![0.0f32; data.len()];
    for y in 0..n {
        for x in 0..n {
            let (l, r) = (x.saturating_sub(1), (x + 1).min(n - 1));
            tmp[y * n + x] = (data[y * n + l] + 2.0 * data[y * n + x] + data[y * n + r]) * 0.25;
        }
    }
    for y in 0..n {
        let (u, d) = (y.saturating_sub(1), (y + 1).min(n - 1));
        for x in 0..n {
            data[y * n + x] = (tmp[u * n + x] + 2.0 * tmp[y * n + x] + tmp[d * n + x]) * 0.25;
        }
    }
}

type Entry = (TiledImage, u32, Arc<OnceLock<Arc<Grid>>>);

fn cache() -> &'static Mutex<Vec<Entry>> {
    static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// The `size` grid of `img`, built on first use. Holding the image keeps its storage alive, so a
/// copy-on-write edit always gets new storage and can't be mistaken for the cached one.
pub fn grid(img: &TiledImage, size: u32) -> Arc<Grid> {
    let cell = {
        let mut entries = cache().lock().unwrap_or_else(|e| e.into_inner());
        match entries.iter().position(|(i, s, _)| *s == size && i.same_storage(img) && i.same_layout(img)) {
            Some(at) => {
                let e = entries.remove(at);
                let cell = e.2.clone();
                entries.push(e); // most recently used last
                cell
            }
            None => {
                let cell = Arc::new(OnceLock::new());
                entries.push((img.clone(), size, cell.clone()));
                if entries.len() > CACHE_ENTRIES {
                    entries.remove(0);
                }
                cell
            }
        }
    };
    cell.get_or_init(|| Arc::new(Grid::build(img, size))).clone()
}

/// The generator's source value per texel from the fine map values `fine` (the baked curvature
/// of this tile, 0.5 = flat) and the detail settings. Default settings return `fine` unchanged.
pub fn detail_values(detail: &CurvatureDetail, map: Option<&TiledImage>, geo: &TileGeometry, fine: &[[f32; 3]], out: &mut [f32]) {
    debug_assert_eq!(fine.len(), TILE_TEXELS);
    if detail.is_identity() {
        for (o, f) in out.iter_mut().zip(fine) {
            *o = f[0];
        }
        return;
    }
    let weight = |w: f32| if w.is_finite() { w.clamp(0.0, CurvatureDetail::MAX_WEIGHT) } else { 0.0 };
    let (wf, wm, wc) = (weight(detail.fine), weight(detail.medium), weight(detail.coarse));
    let medium = map.filter(|_| wm > 0.0).map(|m| grid(m, MEDIUM_GRID));
    let coarse = map.filter(|_| wc > 0.0).map(|m| grid(m, COARSE_GRID));
    for (i, o) in out.iter_mut().enumerate() {
        let mut dev = wf * (fine[i][0] - 0.5);
        if medium.is_some() || coarse.is_some() {
            let uv = geo.uv(i);
            if let Some(g) = &medium { dev += wm * MEDIUM_GAIN * (g.sample(uv) - 0.5); }
            if let Some(g) = &coarse { dev += wc * COARSE_GAIN * (g.sample(uv) - 0.5); }
        }
        *o = match detail.mode {
            CurvatureMode::Both => 0.5 + dev,
            CurvatureMode::Edges => 2.0 * dev.max(0.0),
            CurvatureMode::Cavities => 2.0 * (-dev).max(0.0),
        }
        .clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(res: u32, period: u32) -> TiledImage {
        let data: Vec<f32> = (0..res * res).map(|i| if ((i % res) / period + (i / res) / period) % 2 == 0 { 1.0 } else { 0.0 }).collect();
        TiledImage::from_dense(res, res, 1, [0.5; 4], &data)
    }

    #[test]
    fn grid_of_a_constant_map_is_that_constant_and_fine_checkers_average_out() {
        let flat = TiledImage::from_dense(512, 512, 1, [0.5; 4], &vec![0.7; 512 * 512]);
        let g = Grid::build(&flat, MEDIUM_GRID);
        assert_eq!(g.size, 256);
        assert!(g.data.iter().all(|v| (v - 0.7).abs() < 1e-3)); // texels are stored as half floats
        // A 1-texel checker vanishes at both scales; a 64-texel one survives the medium scale.
        let fine = Grid::build(&checker(512, 1), MEDIUM_GRID);
        assert!(fine.data.iter().all(|v| (v - 0.5).abs() < 1e-4), "1-texel detail averages to flat");
        let broad = Grid::build(&checker(512, 64), MEDIUM_GRID);
        let (lo, hi) = broad.data.iter().fold((1.0f32, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(lo < 0.05 && hi > 0.95, "broad detail kept: {lo}..{hi}");
        let coarse = Grid::build(&checker(512, 4), COARSE_GRID);
        assert!(coarse.data.iter().all(|v| (v - 0.5).abs() < 1e-4), "4-texel detail is below the coarse scale");
    }

    #[test]
    fn grid_size_never_exceeds_a_small_map_and_sampling_is_bilinear() {
        let g = Grid::build(&checker(32, 16), MEDIUM_GRID);
        assert_eq!(g.size, 32);
        let flat = Grid { size: 2, data: vec![0.0, 1.0, 0.0, 1.0] };
        assert_eq!(flat.sample([0.25, 0.5]), 0.0);
        assert_eq!(flat.sample([0.75, 0.5]), 1.0);
        assert!((flat.sample([0.5, 0.5]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn cache_reuses_a_grid_until_the_map_changes() {
        let mut img = checker(256, 32);
        let a = grid(&img, MEDIUM_GRID);
        let b = grid(&img.clone(), MEDIUM_GRID);
        assert!(Arc::ptr_eq(&a, &b), "same storage: same grid");
        img.set(0, 0, [0.25; 4]);
        let c = grid(&img, MEDIUM_GRID);
        assert!(!Arc::ptr_eq(&a, &c), "an edited map gets a new grid");
    }
}
