//! Writes a finished-looking multi-part crate project for native screenshots: Old Wood on the
//! body set and Worn Painted Metal on the trim set (both from the built-in smart materials).
//! cargo run -p subcraft-app --example showcase_project -- <out.subcraft> [size] [body smart material] [trim smart material]
use subcraft_app::Session;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, primitives};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .subcraft path")?;
    let size = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024u32);
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot, t.name.clone())).collect();
    let smart = subcraft_assets::smart::smart_materials();
    for (i, (ts, slot, name)) in sets.iter().enumerate() {
        println!("texture set {name}");
        s.execute(subcraft_app::Command::SelectTextureSet(*ts)).map_err(|e| format!("{e:?}"))?;
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: *slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
        s.set_mesh_maps(*ts, maps);
        let wanted = args.get(2 + usize::from(i != 0)).map(String::as_str).unwrap_or(if i == 0 { "Old Wood" } else { "Worn Painted Metal" });
        let m = smart.iter().find(|m| m.name == wanted).ok_or("smart material missing")?;
        s.add_smart_material(m).map_err(|e| format!("{e:?}"))?;
    }
    s.update_composites();
    s.save(Some(std::path::Path::new(out)))?;
    println!("wrote {out}");
    Ok(())
}
