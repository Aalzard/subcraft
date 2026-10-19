//! CPU view of one textured dab wrapping over a hard box edge (no GPU/window needed).
//! cargo run -p subcraft-app --example brush_wrap -- <out.png> [checker|stripes] [alignment: wrap|planar|camera]
use std::sync::Arc;
use glam::{Vec2, Vec3};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_image::{Bitmap, FileFormat, save_image};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, primitives};
use subcraft_paint::{Alignment, BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, Stroke, StrokeMode, StrokeOptions};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .png path")?;
    let checker = args.get(1).is_none_or(|s| s != "stripes");
    let alignment = match args.get(2).map(String::as_str) { Some("planar") => Alignment::TangentPlanar, Some("camera") => Alignment::Camera, _ => Alignment::TangentWrap };
    let res = 1024;
    let mesh = Arc::new(primitives::atlas_box(Vec3::splat(2.0), 3, "Wrap"));
    let bvh = Bvh::build(&mesh);
    let mut ts = TextureSet::new("T", 0, res); let tc = ts.clone();
    let layer = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let geometry = TextureSetGeometry::new(mesh.clone(), 0, res); let depth = DepthCache::new();
    let tip: Vec<f32> = (0..256 * 256).map(|i| {
        let (x, y) = ((i % 256) / 32, (i / 256) / 32);
        if checker { ((x + y) % 2) as f32 } else { (y % 2) as f32 }
    }).collect();
    let paint_cam = Camera::perspective(Vec3::new(0.0, 4.0, 3.2), Vec3::new(0.0, 0.9, 0.7), Vec3::Y, 30.0, 0.1, 100.0, [800, 800]);
    let settings = BrushSettings { size: 30.0, hardness: 1.0, spacing: 10.0, alignment, ..BrushSettings::default() };
    let material = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(0.9, 0.55, 0.1))]);
    let mut stroke = Stroke::begin(&ts, layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &material)
        .map_err(|e| format!("{e:?}"))?.with_alpha(Some(Arc::new(Bitmap::new_f32(256, 256, 1, tip))));
    let ctx = PaintContext { mesh: &mesh, bvh: &bvh, geometry: &geometry, camera: &paint_cam, depth: &depth };
    let mut edited = ts.clone();
    stroke.add_point(&mut edited, &ctx, paint_cam.project(Vec3::new(0.0, 1.0, 0.8)).unwrap().truncate()).map_err(|e| format!("{e:?}"))?;
    let LayerContent::Paint(p) = &edited.find(layer).unwrap().content else { unreachable!() };
    let color = p.channels.get(&ChannelKind::BaseColor).ok_or("nothing painted")?;
    // Look at the edge from the side so both faces show at similar foreshortening.
    let (w, h) = (900u32, 700u32); let eye = Vec3::new(1.6, 2.4, 3.4); let target = Vec3::new(0.0, 0.8, 0.8);
    let forward = (target - eye).normalize(); let right = forward.cross(Vec3::Y).normalize(); let up = right.cross(forward);
    let light = Vec3::new(0.3, 0.8, 0.6).normalize();
    let mut px = vec![0.0f32; (w * h * 3) as usize];
    for y in 0..h { for x in 0..w {
        let ndc = Vec2::new((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
        let dir = (forward + right * ndc.x * 0.42 * w as f32 / h as f32 + up * ndc.y * 0.42).normalize();
        let c = match bvh.intersect(eye, dir, 0.0, 100.0) {
            Some(hit) => {
                let sp = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let (tx, ty) = (((sp.uv.x * res as f32) as u32).min(res - 1), ((sp.uv.y * res as f32) as u32).min(res - 1));
                let paint = color.get(tx, ty); let a = paint[3];
                let base = [0.08, 0.09, 0.1];
                let shade = 0.35 + 0.65 * sp.normal.dot(light).max(0.0);
                [0, 1, 2].map(|i| ((base[i] * (1.0 - a) + paint[i] * a) * shade).powf(1.0 / 2.2))
            }
            None => [0.16, 0.17, 0.19],
        };
        let o = ((y * w + x) * 3) as usize; px[o..o + 3].copy_from_slice(&c);
    } }
    save_image(std::path::Path::new(out), w, h, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    println!("{alignment:?} {} -> {out}", if checker { "checker" } else { "stripes" });
    Ok(())
}
