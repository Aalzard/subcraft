//! Lighting environments: procedural studio / sky presets and imported HDRIs.
//!
//! HDRIs (equirectangular, `.hdr` / `.exr` or LDR images) are prefiltered on the CPU into a mip
//! chain of increasingly blurred levels (sampled by roughness in the viewport) plus 9 spherical
//! harmonics coefficients for diffuse irradiance. Equirect convention: `u = atan2(z, x)/2π + 0.5`,
//! `v = acos(y)/π` (v = 0 at the zenith).

use std::path::Path;
use std::sync::Arc;

use glam::Vec3;
use rayon::prelude::*;
use subcraft_core::AssetId;
use subcraft_image::{Bitmap, Wrap};

/// Parametric sky + studio lights (the default viewport look).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProceduralEnv {
    pub ground: [f32; 3],
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
    pub irradiance_up: [f32; 3],
    pub irradiance_down: [f32; 3],
    /// Intensities of the reflected ring panels, overhead light and key softbox.
    pub panels: f32,
    pub top: f32,
    pub key_softbox: f32,
    /// Direct key light.
    pub light_intensity: f32,
    pub light_color: [f32; 3],
}

impl ProceduralEnv {
    pub fn studio() -> Self {
        Self {
            ground: [0.10, 0.09, 0.08],
            horizon: [0.42, 0.43, 0.46],
            zenith: [0.20, 0.27, 0.40],
            irradiance_up: [0.32, 0.35, 0.40],
            irradiance_down: [0.13, 0.12, 0.11],
            panels: 14.0,
            top: 10.0,
            key_softbox: 30.0,
            light_intensity: 1.5,
            light_color: [1.0, 0.96, 0.9],
        }
    }
}

#[derive(Clone, Debug)]
pub struct HdriLevel {
    pub width: u32,
    pub height: u32,
    /// Linear RGBA f32.
    pub rgba: Vec<f32>,
}

/// Prefiltered HDRI: level 0 sharp, last level very blurred.
#[derive(Clone, Debug)]
pub struct PrefilteredHdri {
    pub levels: Vec<HdriLevel>,
    /// Irradiance SH coefficients already convolved with the cosine lobe and divided by π, so
    /// `Σ sh[i]·Y_i(n)` is the diffuse "sky color" seen by normal `n`.
    pub sh: [[f32; 3]; 9],
    /// Brightest direction (sun) and its normalized color, for the direct key light.
    pub sun_direction: [f32; 3],
    pub sun_color: [f32; 3],
}

#[derive(Clone, Debug)]
pub enum EnvironmentSource {
    Procedural(ProceduralEnv),
    Hdri(Arc<PrefilteredHdri>),
}

#[derive(Clone, Debug)]
pub struct Environment {
    pub id: AssetId,
    pub name: String,
    pub source: EnvironmentSource,
}

fn env(name: &str, p: ProceduralEnv) -> Environment {
    Environment { id: AssetId::from_name(&format!("environment/{name}")), name: name.into(), source: EnvironmentSource::Procedural(p) }
}

pub fn builtin() -> Vec<Environment> {
    let studio = ProceduralEnv::studio();
    vec![
        env("Studio", studio),
        env("Soft Studio", ProceduralEnv { panels: 6.0, top: 14.0, key_softbox: 10.0, light_intensity: 1.0, irradiance_up: [0.4, 0.42, 0.45], ..studio }),
        env(
            "Sunset",
            ProceduralEnv {
                ground: [0.08, 0.05, 0.04],
                horizon: [0.9, 0.45, 0.2],
                zenith: [0.12, 0.16, 0.35],
                irradiance_up: [0.25, 0.22, 0.3],
                irradiance_down: [0.12, 0.07, 0.05],
                panels: 0.0,
                top: 0.0,
                key_softbox: 40.0,
                light_intensity: 2.2,
                light_color: [1.0, 0.62, 0.35],
            },
        ),
        env(
            "Overcast",
            ProceduralEnv {
                ground: [0.18, 0.18, 0.17],
                horizon: [0.7, 0.72, 0.75],
                zenith: [0.62, 0.65, 0.7],
                irradiance_up: [0.55, 0.57, 0.6],
                irradiance_down: [0.2, 0.2, 0.19],
                panels: 0.0,
                top: 3.0,
                key_softbox: 0.0,
                light_intensity: 0.35,
                light_color: [0.95, 0.97, 1.0],
            },
        ),
        env(
            "Night Studio",
            ProceduralEnv {
                ground: [0.01, 0.01, 0.012],
                horizon: [0.03, 0.035, 0.05],
                zenith: [0.01, 0.012, 0.02],
                irradiance_up: [0.04, 0.045, 0.06],
                irradiance_down: [0.01, 0.01, 0.01],
                panels: 6.0,
                top: 0.0,
                key_softbox: 50.0,
                light_intensity: 2.5,
                light_color: [0.8, 0.88, 1.0],
            },
        ),
    ]
}

/// Direction for equirect texel coordinates (0..1).
pub fn dir_from_uv(u: f32, v: f32) -> Vec3 {
    let phi = (u - 0.5) * std::f32::consts::TAU;
    let theta = v * std::f32::consts::PI;
    Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin())
}

pub fn uv_from_dir(d: Vec3) -> (f32, f32) {
    (d.z.atan2(d.x) / std::f32::consts::TAU + 0.5, d.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI)
}

/// Real SH basis, bands 0..2.
pub fn sh_basis(d: Vec3) -> [f32; 9] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
    ]
}

const BASE_W: u32 = 512;
const LEVELS: usize = 6;

fn downsample(l: &HdriLevel) -> HdriLevel {
    let (w, h) = ((l.width / 2).max(1), (l.height / 2).max(1));
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut s = 0.0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((2 * x + dx).min(l.width - 1), (2 * y + dy).min(l.height - 1));
                    s += l.rgba[((sy * l.width + sx) * 4 + c) as usize];
                }
                rgba[((y * w + x) * 4 + c) as usize] = s * 0.25;
            }
        }
    }
    HdriLevel { width: w, height: h, rgba }
}

/// Blur with a Gaussian of `sigma` texels, widened horizontally by 1/cos(latitude) so the blur
/// is roughly isotropic on the sphere; horizontal wrap-around.
fn blur(l: &HdriLevel, sigma: f32) -> HdriLevel {
    let (w, h) = (l.width as i32, l.height as i32);
    let gauss = |s: f32| -> Vec<f32> {
        let r = (s * 2.5).ceil().max(1.0) as i32;
        let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * s * s)).exp()).collect();
        let sum: f32 = k.iter().sum();
        k.into_iter().map(|v| v / sum).collect()
    };
    let mut tmp = vec![0.0f32; l.rgba.len()];
    tmp.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(y, row)| {
        let lat = ((y as f32 + 0.5) / h as f32 - 0.5) * std::f32::consts::PI;
        let sx = (sigma / lat.cos().max(0.08)).min(w as f32 / 4.0);
        let k = gauss(sx);
        let r = (k.len() / 2) as i32;
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (i, wgt) in k.iter().enumerate() {
                let sxp = (x + i as i32 - r).rem_euclid(w);
                for c in 0..4 {
                    acc[c] += l.rgba[((y as i32 * w + sxp) * 4) as usize + c] * wgt;
                }
            }
            row[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&acc);
        }
    });
    let k = gauss(sigma);
    let r = (k.len() / 2) as i32;
    let mut out = vec![0.0f32; l.rgba.len()];
    out.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (i, wgt) in k.iter().enumerate() {
                let sy = (y as i32 + i as i32 - r).clamp(0, h - 1);
                for c in 0..4 {
                    acc[c] += tmp[((sy * w + x) * 4) as usize + c] * wgt;
                }
            }
            row[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&acc);
        }
    });
    HdriLevel { width: l.width, height: l.height, rgba: out }
}

/// Prefilter an equirectangular image (LDR images are linearized from sRGB).
pub fn prefilter(src: &Bitmap) -> PrefilteredHdri {
    let linearize = !src.is_hdr();
    let (w, h) = (BASE_W, BASE_W / 2);
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    rgba.par_chunks_mut((w * 4) as usize).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let t = src.sample((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32, Wrap::Repeat, linearize);
            row[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&[t[0].max(0.0), t[1].max(0.0), t[2].max(0.0), 1.0]);
        }
    });
    let mut levels = vec![HdriLevel { width: w, height: h, rgba }];
    for k in 1..LEVELS {
        let down = downsample(&levels[k - 1]);
        levels.push(blur(&down, 1.0 + 0.25 * k as f32));
    }
    // SH from a mid level, weighted by solid angle.
    let l = &levels[3];
    let mut sh = [[0.0f32; 3]; 9];
    let (lw, lh) = (l.width as f32, l.height as f32);
    for y in 0..l.height {
        let theta = (y as f32 + 0.5) / lh * std::f32::consts::PI;
        let d_omega = (std::f32::consts::TAU / lw) * (std::f32::consts::PI / lh) * theta.sin();
        for x in 0..l.width {
            let d = dir_from_uv((x as f32 + 0.5) / lw, (y as f32 + 0.5) / lh);
            let b = sh_basis(d);
            let o = ((y * l.width + x) * 4) as usize;
            for i in 0..9 {
                for c in 0..3 {
                    sh[i][c] += l.rgba[o + c] * b[i] * d_omega;
                }
            }
        }
    }
    // Cosine-lobe convolution (Ramamoorthi & Hanrahan) divided by π.
    let a = [1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25];
    for i in 0..9 {
        for c in 0..3 {
            sh[i][c] *= a[i];
        }
    }
    // Sun: brightest texel of a blurred level.
    let l = &levels[4];
    let (mut best, mut best_lum) = (0usize, -1.0f32);
    for i in 0..(l.width * l.height) as usize {
        let lum = 0.2126 * l.rgba[i * 4] + 0.7152 * l.rgba[i * 4 + 1] + 0.0722 * l.rgba[i * 4 + 2];
        if lum > best_lum {
            best_lum = lum;
            best = i;
        }
    }
    let (sx, sy) = (best as u32 % l.width, best as u32 / l.width);
    let sun = dir_from_uv((sx as f32 + 0.5) / l.width as f32, (sy as f32 + 0.5) / l.height as f32);
    let c = [l.rgba[best * 4], l.rgba[best * 4 + 1], l.rgba[best * 4 + 2]];
    let m = c[0].max(c[1]).max(c[2]).max(1e-6);
    PrefilteredHdri { levels, sh, sun_direction: sun.to_array(), sun_color: [c[0] / m, c[1] / m, c[2] / m] }
}

/// Evaluate the prefiltered irradiance for a normal (CPU mirror of the shader).
pub fn irradiance(hdri: &PrefilteredHdri, n: Vec3) -> [f32; 3] {
    let b = sh_basis(n);
    let mut out = [0.0f32; 3];
    for i in 0..9 {
        for c in 0..3 {
            out[c] += hdri.sh[i][c] * b[i];
        }
    }
    out
}

pub fn import_hdri(path: &Path) -> Result<Environment, String> {
    let bmp = subcraft_image::load_bitmap(path).map_err(|e| e.to_string())?;
    if bmp.width < bmp.height {
        return Err("expected an equirectangular (2:1) panorama".into());
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("HDRI").to_string();
    Ok(Environment {
        id: AssetId::from_name(&format!("environment/import/{}", path.display())),
        name,
        source: EnvironmentSource::Hdri(Arc::new(prefilter(&bmp))),
    })
}
