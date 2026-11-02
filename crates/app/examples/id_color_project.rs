//! Writes a .subcraft project of the default crate with a baked ID map and an orange fill masked
//! by an ID Color Mask whose colors are picked through the real eyedropper path (ray pick + ID
//! map sample), so the app can open it (`subcraft out.subcraft --screenshot x.png`).
//! cargo run -p subcraft-app --example id_color_project -- <out.subcraft> [size]
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind};
use subcraft_mesh::{Bvh, primitives};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?;
    let size = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024u32);
    let mesh = primitives::sample_crate();
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    // The trim texture set (slot 1) has several mesh elements, so several ID colors.
    let ts = s.document().texture_sets.iter().find(|t| t.material_slot == 1).map(|t| t.id).ok_or("no trim set")?;
    s.execute(Command::SelectTextureSet(ts)).map_err(|e| e.to_string())?;
    let slot = 1;
    let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
        &BakeSettings { size, maps: vec![MeshMapKind::Id], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
    s.set_mesh_maps(ts, maps);

    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| e.to_string())?;
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.9, 0.35, 0.05))) }).map_err(|e| e.to_string())?;
    s.layer_action(layer, subcraft_app::part_workflow::LayerAction::IdColorMask).map_err(|e| e.to_string())?;
    let effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;

    // Click a few points across the front of the crate, as the eyedropper does.
    let b = mesh.bounds;
    let center = (b.min + b.max) * 0.5;
    let eye = center + Vec3::new(0.6, 0.5, 1.0).normalize() * b.diagonal() * 1.3;
    let cam = Camera::perspective(eye, center, Vec3::Y, 40.0, 0.01, 100.0, [800, 800]);
    let mut picked = Vec::new();
    for y in (100..700).step_by(40) { for x in (100..700).step_by(40) {
        let p = Vec2::new(x as f32, y as f32);
        if picked.len() >= 3 { break; }
        // Clicks on the body (another texture set) are refused, like in the viewport.
        if let Ok(Some(c)) = s.pick_id_color(layer, effect, &cam, p) { picked.push(c); }
    } }
    let colors = match &s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].kind {
        MaskEffectKind::IdColor(id) => id.colors.len(),
        _ => 0,
    };
    s.update_composites();
    let img = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().to_dense();
    let orange = img.as_chunks::<4>().0.iter().filter(|p| (p[0] - 0.9).abs() < 1e-3 && (p[2] - 0.05).abs() < 1e-3).count();
    s.save(Some(std::path::Path::new(out)))?;
    println!("picked {picked:?} ({colors} colors); {orange} of {} texels selected -> {out}", img.len() / 4);
    Ok(())
}
