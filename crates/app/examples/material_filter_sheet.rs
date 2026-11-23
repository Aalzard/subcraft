//! Before/after sheet of the material filters (MatFinish / MatFX / PBR Validate) on SubCraft_base.
//! cargo run --release -p subcraft-app --example material_filter_sheet -- <out_prefix> [size] [zoom] [only]
//!
//! Each column is one filter on a plain fill layer: the fill alone on top, the fill with the
//! filter (default settings) below. Ray-cast and lit on the CPU with GGX, the composited Base
//! Color / Roughness / Metallic and the Height turned into a bump exactly like the viewport
//! (slope x 0.02). Writes <prefix>_lit.png, and <prefix>_<name>.png per filter, and prints the
//! composite time of every filter.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, MaterialFilter};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::Bvh;

fn texel(img: &TiledImage, x: i64, y: i64) -> [f32; 4] {
    let (w, h) = (img.width() as i64, img.height() as i64);
    img.get(x.clamp(0, w - 1) as u32, y.clamp(0, h - 1) as u32)
}

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0);
    let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let [a, b, c, d] = [texel(img, x0, y0), texel(img, x0 + 1, y0), texel(img, x0, y0 + 1), texel(img, x0 + 1, y0 + 1)];
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// A soft studio: gray gradient, a bright overhead panel and a warm side panel, blurred by `rough`.
fn environment(dir: Vec3, rough: f32) -> Vec3 {
    let blur = 0.02 + rough * rough * 1.6;
    let sky = Vec3::splat(0.22 + 0.30 * (dir.y * 0.5 + 0.5));
    let panel = |d: Vec3, size: f32, color: Vec3| {
        let c = dir.dot(d.normalize());
        let edge = size - blur * 0.5;
        color * ((c - edge + blur) / (2.0 * blur)).clamp(0.0, 1.0) * (size * size / (size * size + blur * 0.6)).min(1.0)
    };
    sky + panel(Vec3::new(0.2, 1.0, 0.3), 0.80, Vec3::splat(2.6)) + panel(Vec3::new(0.9, 0.25, 0.4), 0.90, Vec3::new(2.2, 2.0, 1.7)) + panel(Vec3::new(-0.8, 0.3, 0.5), 0.93, Vec3::new(0.9, 1.1, 1.4))
}

fn ggx(n: Vec3, v: Vec3, l: Vec3, rough: f32, f0: Vec3) -> Vec3 {
    let h = (v + l).normalize();
    let (nl, nv, nh, vh) = (n.dot(l).max(0.0), n.dot(v).max(1e-3), n.dot(h).max(0.0), v.dot(h).max(0.0));
    if nl <= 0.0 {
        return Vec3::ZERO;
    }
    let a = (rough * rough).max(0.002);
    let a2 = a * a;
    let d = a2 / (std::f32::consts::PI * (nh * nh * (a2 - 1.0) + 1.0).powi(2));
    let k = a / 2.0;
    let g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
    let f = f0 + (Vec3::ONE - f0) * (1.0 - vh).powi(5);
    f * (d * g / (4.0 * nl * nv)) * nl
}

fn aces(c: Vec3) -> Vec3 {
    let c = c * 0.42;
    ((c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14)).clamp(Vec3::ZERO, Vec3::ONE)
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

struct Case {
    name: &'static str,
    filter: MaterialFilter,
    base: [f32; 3],
    rough: f32,
    metal: f32,
    /// A filter already on the layer in both rows (relief for the tested filter to react to).
    under: Option<MaterialFilter>,
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let zoom: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let only = args.get(3).cloned();
    let all = MaterialFilter::ALL;
    let steel = [0.56, 0.57, 0.58];
    let cases: Vec<Case> = [
        Case { name: "galvanized", filter: all[0], base: steel, rough: 0.35, metal: 1.0, under: None },
        Case { name: "hammered", filter: all[1], base: [0.93, 0.62, 0.52], rough: 0.28, metal: 1.0, under: None },
        Case { name: "powder_coated", filter: all[2], base: [0.50, 0.04, 0.03], rough: 0.25, metal: 0.0, under: None },
        Case { name: "edge_damages", filter: all[3], base: [0.10, 0.17, 0.26], rough: 0.45, metal: 0.0, under: None },
        Case { name: "rust_weathering", filter: all[4], base: steel, rough: 0.35, metal: 1.0, under: None },
        Case { name: "peeling_paint", filter: all[5], base: [0.42, 0.44, 0.30], rough: 0.5, metal: 0.0, under: None },
        Case { name: "pbr_validate", filter: all[6], base: [0.004, 0.004, 0.005], rough: 0.5, metal: 0.0, under: None },
        Case { name: "grinded", filter: all[7], base: steel, rough: 0.3, metal: 1.0, under: None },
        Case { name: "rough", filter: all[8], base: steel, rough: 0.3, metal: 1.0, under: None },
        Case { name: "raw", filter: all[9], base: steel, rough: 0.3, metal: 1.0, under: None },
        Case { name: "detail_edge_wear", filter: all[10], base: [0.10, 0.17, 0.26], rough: 0.45, metal: 0.0, under: None },
        Case {
            name: "detail_edge_wear_on_relief",
            filter: all[10],
            base: [0.10, 0.17, 0.26],
            rough: 0.45,
            metal: 0.0,
            under: Some(MaterialFilter::Hammered { scale: 14.0, depth: 1.0, irregularity: 0.6, roughness_variation: 0.0, seed: 1 }),
        },
        Case { name: "water_drops", filter: all[11], base: [0.30, 0.05, 0.04], rough: 0.4, metal: 0.0, under: None },
    ]
    .into_iter()
    .filter(|c| only.as_deref().is_none_or(|o| c.name.contains(o)))
    .collect();

    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let bvh = Bvh::build(&mesh);
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let mut layers = Vec::new();
    for &(ts, slot) in &sets {
        if size != 2048 {
            s.execute(Command::SetResolution { texture_set: Some(ts), resolution: size }).map_err(err)?;
        }
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], use_gpu: false, ..Default::default() },
            &Progress::new(),
        )
        .map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        layers.push((ts, s.selection().layer.unwrap()));
    }

    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let (target, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    let eye = target + Vec3::new(0.9, 0.55, 1.0).normalize() * radius * 2.6 / zoom;
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let lights = [(Vec3::new(0.7, 0.45, 0.55).normalize(), Vec3::splat(2.4)), (Vec3::new(-0.6, 0.5, 0.2).normalize(), Vec3::new(0.5, 0.6, 0.8))];
    let (cw, ch) = (560u32, 420u32);

    let render = |s: &Session| -> Vec<f32> {
        let get = |kind: ChannelKind| -> Vec<Option<TiledImage>> { sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(kind).cloned()).collect() };
        let (albedo, rough, metal, height, normal) = (get(ChannelKind::BaseColor), get(ChannelKind::Roughness), get(ChannelKind::Metallic), get(ChannelKind::Height), get(ChannelKind::Normal));
        let mut out = vec![0.0f32; (cw * ch * 3) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let mut acc = Vec3::ZERO;
                for (jx, jy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let ndc = Vec2::new((x as f32 + jx) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + jy) / ch as f32 * 2.0);
                    let dir = (forward + right * ndc.x * 0.5 * cw as f32 / ch as f32 + up * ndc.y * 0.5).normalize();
                    let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                        acc += Vec3::splat(0.035);
                        continue;
                    };
                    let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                    let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap_or(0);
                    let sample = |img: &Vec<Option<TiledImage>>, d: [f32; 4]| img[set].as_ref().map_or(d, |i| bilinear(i, p.uv));
                    let c = sample(&albedo, [0.5, 0.5, 0.5, 1.0]);
                    let r = sample(&rough, [0.5; 4])[0].clamp(0.03, 1.0);
                    let m = sample(&metal, [0.0; 4])[0].clamp(0.0, 1.0);
                    let n = sample(&normal, [0.5, 0.5, 1.0, 1.0]);
                    // Height bump, the viewport's construction: central differences in texels.
                    let mut bump = Vec3::Z;
                    if let Some(h) = height[set].as_ref() {
                        let sz = h.width() as f32;
                        let at = |du: f32, dv: f32| bilinear(h, p.uv + Vec2::new(du, dv) / sz)[0];
                        let dhdu = (at(1.0, 0.0) - at(-1.0, 0.0)) * 0.5 * sz;
                        let dhdv = (at(0.0, 1.0) - at(0.0, -1.0)) * 0.5 * sz;
                        bump = Vec3::new(-dhdu * 0.02, dhdv * 0.02, 1.0).normalize();
                    }
                    let base = Vec3::new(n[0] * 2.0 - 1.0, n[1] * 2.0 - 1.0, (n[2] * 2.0 - 1.0).max(1e-3));
                    let tn = Vec3::new(base.x + bump.x, base.y + bump.y, base.z * bump.z).normalize();
                    let t = p.tangent.truncate();
                    let bt = p.tangent.w * p.normal.cross(t);
                    let nw = (t * tn.x + bt * tn.y + p.normal * tn.z).normalize_or(p.normal);
                    let view = -dir;
                    let albedo = Vec3::new(c[0], c[1], c[2]);
                    let f0 = Vec3::splat(0.04).lerp(albedo, m);
                    let diffuse = albedo * (1.0 - m);
                    let mut col = Vec3::ZERO;
                    for (l, k) in lights {
                        col += (diffuse / std::f32::consts::PI * nw.dot(l).max(0.0) + ggx(nw, view, l, r, f0)) * k;
                    }
                    // Image-based part: blurred studio along the reflection, Karis' environment BRDF.
                    let nv = nw.dot(view).max(1e-3);
                    let refl = (2.0 * nv * nw - view).normalize();
                    let c0 = Vec3::new(-1.0, -0.0275, -0.572) * r + Vec3::new(1.0, 0.0425, 1.04);
                    let a004 = (c0.x * c0.x).min((-9.28 * nv).exp2()) * c0.x + c0.y;
                    let (sa, sb) = (-1.04 * a004 + c0.z, 1.04 * a004 + (-0.022 * r + 0.022 * r).max(0.0) + 0.04 * 0.0);
                    col += environment(refl, r) * (f0 * sa + Vec3::splat(sb.max(0.0)) + Vec3::splat(0.0)) + diffuse * environment(nw, 1.0) * 0.55;
                    acc += col;
                }
                let v = aces(acc * 0.25);
                let o = ((y * cw + x) * 3) as usize;
                out[o..o + 3].copy_from_slice(&[v.x.powf(1.0 / 2.2), v.y.powf(1.0 / 2.2), v.z.powf(1.0 / 2.2)]);
            }
        }
        out
    };

    let w = cw * cases.len() as u32;
    let mut sheet = vec![0.0f32; (w * ch * 2 * 3) as usize];
    for (col, case) in cases.iter().enumerate() {
        let mut effects = Vec::new();
        for &(ts, layer) in &layers {
            s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
            let fill = |v: ChannelValue| FillChannel::new(FillSource::uniform(v));
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: fill(ChannelValue::rgb(case.base[0], case.base[1], case.base[2])) }).map_err(err)?;
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Roughness, fill: fill(ChannelValue::scalar(case.rough)) }).map_err(err)?;
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Metallic, fill: fill(ChannelValue::scalar(case.metal)) }).map_err(err)?;
        }
        if let Some(under) = case.under {
            for &(ts, layer) in &layers {
                s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
                s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Material(under) }).map_err(err)?;
                effects.push(s.selected_texture_set().find(layer).unwrap().content_effects.last().unwrap().id);
            }
        }
        s.update_composites();
        let before = render(&s);
        for &(ts, layer) in &layers {
            s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
            s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Material(case.filter) }).map_err(err)?;
            effects.push(s.selected_texture_set().find(layer).unwrap().content_effects.last().unwrap().id);
        }
        let t = std::time::Instant::now();
        s.update_composites();
        println!("{}: composite {:.2}s at {size}", case.name, t.elapsed().as_secs_f32());
        let after = render(&s);
        let mut single = vec![0.0f32; (cw * 2 * ch * 3) as usize];
        for y in 0..ch {
            for x in 0..cw {
                let i = ((y * cw + x) * 3) as usize;
                for (row, img) in [(0u32, &before), (1, &after)] {
                    let o = (((row * ch + y) * w + col as u32 * cw + x) * 3) as usize;
                    sheet[o..o + 3].copy_from_slice(&img[i..i + 3]);
                }
                let o = ((y * cw * 2 + x) * 3) as usize;
                single[o..o + 3].copy_from_slice(&before[i..i + 3]);
                let o = ((y * cw * 2 + cw + x) * 3) as usize;
                single[o..o + 3].copy_from_slice(&after[i..i + 3]);
            }
        }
        save_image(std::path::Path::new(&format!("{prefix}_{}.png", case.name)), cw * 2, ch, 3, &single, FileFormat::Png8).map_err(|e| e.to_string())?;
        for (i, effect) in effects.into_iter().enumerate() {
            let (ts, layer) = layers[i % layers.len()];
            s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
            s.execute(Command::RemoveContentEffect { layer, effect }).map_err(err)?;
        }
    }
    save_image(std::path::Path::new(&format!("{prefix}_lit.png")), w, ch * 2, 3, &sheet, FileFormat::Png8).map_err(|e| e.to_string())?;
    Ok(())
}
