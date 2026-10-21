//! Writes demo OBJ models for New Project captures: `tiled_props.obj` (three crates on three UDIM
//! tiles, two materials, three objects) and `high_poly.obj` (the sample crate, no UVs needed).
//! cargo run -p subcraft-app --example udim_models -- <folder>
use glam::{Vec2, Vec3};
use subcraft_mesh::{Corner, MeshBuilder, primitives};

fn main() -> Result<(), String> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).ok_or("expected an output folder")?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cube = primitives::cube(1.0);
    let mut b = MeshBuilder::new("Tiled Props");
    let (metal, wood) = (b.material("Metal"), b.material("Wood"));
    for (i, (offset, uv_shift, material)) in [(Vec3::new(-1.6, 0.0, 0.0), Vec2::ZERO, metal), (Vec3::ZERO, Vec2::new(1.0, 0.0), metal), (Vec3::new(1.6, 0.0, 0.0), Vec2::new(0.0, -1.0), wood)].into_iter().enumerate() {
        b.object(&format!("Crate_{}", ["A", "B", "C"][i]));
        for t in 0..cube.triangle_count() {
            let (p, uv) = (cube.tri_positions(t), cube.tri_uvs(t));
            let n = cube.geometric_normal(t);
            let corner = |k: usize| Corner { position: p[k] + offset, normal: Some(n), uv: Some(uv[k] + uv_shift) };
            b.push_triangle(material, [corner(0), corner(1), corner(2)]);
        }
    }
    let mesh = b.build().map_err(|e| e.to_string())?;
    subcraft_mesh::import::write_obj(&mesh, &dir.join("tiled_props.obj")).map_err(|e| e.to_string())?;
    subcraft_mesh::import::write_obj(&primitives::sample_crate(), &dir.join("high_poly.obj")).map_err(|e| e.to_string())?;
    println!("wrote {}", dir.display());
    Ok(())
}
