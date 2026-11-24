//! Cook a MaterialCraft file and write each output as a PNG (sRGB for color usages).
//! `cargo run -p subcraft-materialcraft --example mc_cook -- <file> <out_dir> [size] [id=value ...]`

use std::collections::BTreeMap;
use subcraft_materialcraft::{Material, ParamValue};

fn srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, out, rest @ ..] = args.as_slice() else {
        return Err("usage: mc_cook <file> <out_dir> [size] [id=value ...]".into());
    };
    let size = rest.first().and_then(|s| s.parse().ok()).unwrap_or(512);
    let material = Material::open_file(std::path::Path::new(file))?;
    for warning in material.warnings() {
        eprintln!("warning: {warning}");
    }
    for graph in material.graphs() {
        println!("graph {} `{}` outputs {:?}", graph.index, graph.name, graph.usages);
    }
    let params = material.params(material.entry)?;
    for param in &params {
        println!("param {} ({}) = {:?} [{}..{}] {:?}", param.id, param.label, param.default, param.min, param.max, param.options);
    }
    let mut values = BTreeMap::new();
    for pair in rest.iter().filter(|s| s.contains('=')) {
        let (id, value) = pair.split_once('=').unwrap_or_default();
        let value: f32 = value.parse()?;
        values.insert(id.to_string(), ParamValue::Float(value));
    }
    let started = std::time::Instant::now();
    let outputs = material.cook(material.entry, &values, size)?;
    println!("cooked in {:?}", started.elapsed());
    std::fs::create_dir_all(out)?;
    for output in outputs {
        let color = matches!(output.usage.as_str(), "baseColor" | "diffuse" | "emissive" | "specular");
        let mut rgb = image::RgbImage::new(output.width, output.height);
        for (i, px) in rgb.pixels_mut().enumerate() {
            let t = &output.texels[i * output.channels..(i + 1) * output.channels];
            let v = |c: usize| {
                let x = if output.channels == 1 { t[0] } else { t[c] };
                ((if color { srgb(x) } else { x.clamp(0.0, 1.0) }) * 255.0).round() as u8
            };
            *px = image::Rgb([v(0), v(1), v(2)]);
        }
        let path = std::path::Path::new(out).join(format!("{}_{}.png", output.usage, output.identifier));
        rgb.save(&path)?;
        println!("{} -> {}", output.usage, path.display());
    }
    Ok(())
}
