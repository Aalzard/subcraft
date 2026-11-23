//! Contact sheet of smart masks on the baked default model: one cell per mask, orange where the
//! mask selects, over grey, with plain diffuse shading.
//! cargo run -p subcraft-app --example mask_sheet -- <out.png> <size> <columns> <mask name>...
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::Bvh;

fn nearest(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let x = ((uv.x * img.width() as f32) as u32).min(img.width() - 1);
    let y = ((uv.y * img.height() as f32) as u32).min(img.height() - 1);
    img.get(x, y)
}

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .png")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let columns: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let names: Vec<String> = args.iter().skip(3).cloned().collect();
    let lib = subcraft_assets::Library::builtin();
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let bvh = Bvh::build(&mesh);
    let (cw, ch) = (420u32, 360u32);
    let rows = names.len().div_ceil(columns);
    let (w, h) = (cw * columns as u32, ch * rows as u32);
    let mut px = vec![0.0f32; (w * h * 3) as usize];
    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let (target, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    let eye = target + Vec3::new(0.9, 0.55, 1.0).normalize() * radius * 2.6 / 1.7;
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let light = Vec3::new(0.7, 0.45, 0.45).normalize();

    for (n, name) in names.iter().enumerate() {
        let mask = lib.smart_masks.iter().find(|m| m.name == *name).ok_or(format!("no smart mask '{name}'"))?;
        let mut s = Session::default_model();
        let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
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
            for (color, masked) in [([0.35, 0.36, 0.38], false), ([0.9, 0.35, 0.05], true)] {
                s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
                let layer = s.selection().layer.ok_or("the fill is selected")?;
                s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))) }).map_err(err)?;
                if masked {
                    s.execute(Command::AddMask { layer, white: false }).map_err(err)?;
                    s.apply_smart_mask(mask).map_err(err)?;
                }
            }
        }
        s.update_composites();
        let albedo: Vec<TiledImage> = sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone()).collect();
        let (cx, cy) = ((n % columns) as u32 * cw, (n / columns) as u32 * ch);
        for y in 0..ch {
            for x in 0..cw {
                let ndc = Vec2::new((x as f32 + 0.5) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / ch as f32 * 2.0);
                let dir = (forward + right * ndc.x * 0.5 * cw as f32 / ch as f32 + up * ndc.y * 0.5).normalize();
                let o = (((cy + y) * w + cx + x) * 3) as usize;
                let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                    px[o..o + 3].copy_from_slice(&[0.16f32, 0.17, 0.19].map(|v| v.powf(1.0 / 2.2)));
                    continue;
                };
                let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap_or(0);
                let base = Vec3::from_slice(&nearest(&albedo[set], p.uv)[..3]).max(Vec3::ZERO);
                let c = base * (0.35 + 0.75 * p.normal.dot(light).max(0.0));
                px[o..o + 3].copy_from_slice(&c.to_array().map(|v| v.clamp(0.0, 1.0).powf(1.0 / 2.2)));
            }
        }
        println!("{name}");
    }
    save_image(std::path::Path::new(&out), w, h, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(())
}
