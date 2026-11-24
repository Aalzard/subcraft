//! Write the small `.mcmat` SubCraft's tests open: MaterialCraft's "Material outputs" template
//! with the base color exposed as a `tint` parameter and the roughness as `rough`, authored with
//! MaterialCraft's own commands. `cargo run -p subcraft-materialcraft --example mc_fixture -- <out.mcmat>`

use materialcraft_core::Package;
use materialcraft_engine::{Session, commands, material_cmds, samples};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: mc_fixture <out.mcmat>")?;
    let mut package = Package::new("Tinted surface");
    package.graphs.push(samples::template("pbr", "Tinted_surface", 8)?);
    let mut session = Session::new(package);
    session.graph = 0;
    let uniforms: Vec<u64> = session.pkg.graphs[0].nodes.iter().filter(|n| n.def == "uniform").map(|n| n.id.0).collect();
    // Template order: basecolor, normal, roughness, metallic, height, ambientocclusion.
    commands::execute(&mut session, "graph.exposed.add", &json!({"id": "tint", "label": "Tint", "group": "Color", "default": {"t": "color", "v": [0.6, 0.2, 0.1, 1.0]}, "bind": {"node": uniforms[0], "param": "color"}}))?;
    commands::execute(&mut session, "graph.exposed.add", &json!({"id": "rough", "label": "Roughness", "bind": {"node": uniforms[2], "param": "color"}}))?;
    let bytes = material_cmds::material_bytes(&session, 0)?;
    std::fs::write(&out, &bytes)?;
    println!("wrote {} ({} bytes)", out, bytes.len());
    Ok(())
}
