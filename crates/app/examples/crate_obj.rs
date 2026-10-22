//! Writes the sample crate as an OBJ (a stand-in high poly for scripted bake captures).
//! cargo run -p subcraft-app --example crate_obj -- <out.obj>
fn main() -> Result<(), String> {
    let out = std::env::args().nth(1).ok_or("expected output .obj path")?;
    subcraft_mesh::import::write_obj(&subcraft_mesh::primitives::sample_crate(), std::path::Path::new(&out)).map_err(|e| e.to_string())
}
