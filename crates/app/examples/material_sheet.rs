//! Contact sheet of one procedural material (ADR-0047) on the baked default model: one column per
//! parameter set, shaded with a small PBR model (two lights and a gradient sky) so roughness,
//! metal and relief read.
//! cargo run -p subcraft-app --example material_sheet -- <out.png> <size> <zoom> <material name> [param=value,param=r:g:b ...]...
//! The first column is always the material at its defaults.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::{ParamKind, ParamValue};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::Bvh;

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0);
    let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let [a, b, c, d] = [img.get(x0, y0), img.get(x1, y0), img.get(x0, y1), img.get(x1, y1)];
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

/// A soft studio sky: bright above, a warm horizon, dark below.
fn sky(d: Vec3, blur: f32) -> Vec3 {
    let t = (d.y * (1.0 - 0.7 * blur)).clamp(-1.0, 1.0);
    let top = Vec3::new(0.85, 0.92, 1.05);
    let horizon = Vec3::new(0.62, 0.58, 0.54);
    let ground = Vec3::new(0.12, 0.11, 0.10);
    if t > 0.0 { horizon.lerp(top, t.powf(0.6)) } else { horizon.lerp(ground, (-t).powf(0.5)) }
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .png")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let zoom: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let name = args.get(3).ok_or("expected a material name")?;
    let mut columns: Vec<String> = vec![String::new()];
    columns.extend(args.iter().skip(4).cloned());
    let lib = subcraft_assets::Library::builtin();
    let material = lib.smart_materials.iter().find(|m| m.name == *name || m.name == format!("Procedural {name}")).ok_or("no such material")?;

    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let bvh = Bvh::build(&mesh);
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let mut roots = Vec::new();
    for &(ts, slot) in &sets {
        s.execute(Command::SetResolution { texture_set: Some(ts), resolution: size }).map_err(err)?;
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], use_gpu: false, ..Default::default() },
            &Progress::new(),
        )
        .map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        s.add_smart_material(material).map_err(err)?;
        roots.push((ts, s.selection().layer.ok_or("the material is selected")?));
    }

    let (cw, ch) = (520u32, 440u32);
    let w = cw * columns.len() as u32;
    let mut px = vec![0.0f32; (w * ch * 3) as usize];
    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let (target, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    let eye = target + Vec3::new(0.9, 0.55, 1.0).normalize() * radius * 2.6 / zoom;
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let lights = [(Vec3::new(0.7, 0.45, 0.45).normalize(), Vec3::splat(2.2)), (Vec3::new(-0.6, 0.5, 0.2).normalize(), Vec3::new(0.5, 0.55, 0.7))];

    for (col, spec) in columns.iter().enumerate() {
        for &(_, root) in &roots {
            s.execute(Command::ResetMaterialParams(root)).map_err(err)?;
            for pair in spec.split(',').filter(|p| !p.is_empty()) {
                let (id, text) = pair.split_once('=').ok_or("expected param=value")?;
                let layer = s.document().texture_sets.iter().find_map(|t| t.find(root)).ok_or("material layer")?;
                let kind = layer.material.as_ref().and_then(|m| m.param(id)).ok_or(format!("no parameter '{id}'"))?.kind;
                let value = match kind {
                    ParamKind::Color { .. } => {
                        let c: Vec<f32> = text.split(':').map(|v| v.parse().unwrap_or(0.0)).collect();
                        ParamValue::Color([c[0], c[1], c[2]])
                    }
                    ParamKind::Toggle { .. } => ParamValue::Toggle(text == "1" || text == "true"),
                    _ => ParamValue::Number(text.parse().map_err(err)?),
                };
                s.execute(Command::SetMaterialParam { layer: root, param: id.into(), value }).map_err(err)?;
            }
        }
        let t = std::time::Instant::now();
        s.update_composites();
        println!("column {col} [{spec}]: composite {:.2}s", t.elapsed().as_secs_f32());
        // SHEET_SAVE=<prefix>: also save each column as a project (<prefix><column>.subcraft), to
        // open the same surface in the real viewport (`material_viewport capture`).
        if let Ok(prefix) = std::env::var("SHEET_SAVE") {
            s.save(Some(&std::path::PathBuf::from(format!("{prefix}{col}.subcraft")))).map_err(err)?;
        }
        let get = |kind: ChannelKind| -> Vec<TiledImage> { sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(kind).unwrap().clone()).collect() };
        let (albedo, rough, metal, height) = (get(ChannelKind::BaseColor), get(ChannelKind::Roughness), get(ChannelKind::Metallic), get(ChannelKind::Height));
        // Sheen (cloth): only when the material brought the channels.
        let opt = |kind: ChannelKind| -> Vec<Option<TiledImage>> { sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(kind).cloned()).collect() };
        let (sheen_c, sheen_o, sheen_r) = (opt(ChannelKind::SheenColor), opt(ChannelKind::SheenOpacity), opt(ChannelKind::SheenRoughness));
        for y in 0..ch {
            for x in 0..cw {
                let ndc = Vec2::new((x as f32 + 0.5) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / ch as f32 * 2.0);
                let dir = (forward + right * ndc.x * 0.5 * cw as f32 / ch as f32 + up * ndc.y * 0.5).normalize();
                let o = (((y * w) + col as u32 * cw + x) * 3) as usize;
                let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                    px[o..o + 3].copy_from_slice(&[0.16f32, 0.17, 0.19].map(|v| v.powf(1.0 / 2.2)));
                    continue;
                };
                let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap_or(0);
                let base = Vec3::from_slice(&bilinear(&albedo[set], p.uv)[..3]).max(Vec3::ZERO);
                let r = bilinear(&rough[set], p.uv)[0].clamp(0.03, 1.0);
                let m = bilinear(&metal[set], p.uv)[0].clamp(0.0, 1.0);
                // Relief: the Height channel's slope in UV, through the tangent frame.
                let e = 1.0 / size as f32;
                let h = |du: f32, dv: f32| bilinear(&height[set], p.uv + Vec2::new(du, dv))[0];
                let (dhu, dhv) = ((h(e, 0.0) - h(-e, 0.0)) / (2.0 * e), (h(0.0, e) - h(0.0, -e)) / (2.0 * e));
                let t = p.tangent.truncate();
                let bt = p.tangent.w * p.normal.cross(t);
                let n = (p.normal - (t * dhu + bt * dhv) * 0.02).normalize_or(p.normal);
                let view = -dir;
                let nv = n.dot(view).max(1e-3);
                let f0 = Vec3::splat(0.04).lerp(base, m);
                let diffuse = base * (1.0 - m);
                let a = r * r;
                let sheen = match (&sheen_c[set], &sheen_o[set]) {
                    (Some(col), Some(op)) => Vec3::from_slice(&bilinear(col, p.uv)[..3]).clamp(Vec3::ZERO, Vec3::ONE) * bilinear(op, p.uv)[0].clamp(0.0, 1.0),
                    _ => Vec3::ZERO,
                };
                let sr = sheen_r[set].as_ref().map_or(0.5, |i| bilinear(i, p.uv)[0]).clamp(0.07, 1.0);
                let mut c = Vec3::ZERO;
                for (l, radiance) in lights {
                    let nl = n.dot(l).max(0.0);
                    if nl <= 0.0 { continue; }
                    let hv = (l + view).normalize();
                    let nh = n.dot(hv).max(0.0);
                    let d = a * a / (std::f32::consts::PI * ((nh * nh) * (a * a - 1.0) + 1.0).powi(2)).max(1e-6);
                    let k = (r + 1.0).powi(2) / 8.0;
                    let g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
                    let f = f0 + (Vec3::ONE - f0) * (1.0 - hv.dot(view).max(0.0)).powi(5);
                    c += (diffuse / std::f32::consts::PI + f * (d * g / (4.0 * nl * nv).max(1e-4)).min(30.0)) * radiance * nl;
                    // The viewport's sheen lobe: Charlie distribution, Neubelt visibility.
                    let inv = 1.0 / (sr * sr);
                    let ds = (2.0 + inv) * (1.0 - nh * nh).max(0.0).powf(inv * 0.5) / (2.0 * std::f32::consts::PI);
                    c += sheen * ds / (4.0 * (nl + nv - nl * nv)).max(1e-4) * radiance * nl;
                }
                // Sheen under the sky: strongest at grazing angles.
                c += sheen * sky(n, 1.0) * 0.55 * (0.15 + 0.85 * (1.0 - nv).powi(3));
                // Ambient: diffuse sky plus a reflection blurred by roughness.
                let fresnel = f0 + (Vec3::splat((1.0 - r).max(0.04)).max(f0) - f0) * (1.0 - nv).powi(5);
                let refl = dir - 2.0 * dir.dot(n) * n;
                c += diffuse * sky(n, 1.0) * 0.55 + fresnel * sky(refl, r) * (1.0 - 0.5 * r);
                // Filmic-ish tone curve, then display gamma.
                let mapped = c.map(|v| v / (1.0 + v * 0.6));
                px[o..o + 3].copy_from_slice(&mapped.to_array().map(|v| v.max(0.0).powf(1.0 / 2.2)));
            }
        }
    }
    save_image(std::path::Path::new(&out), w, ch, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}
