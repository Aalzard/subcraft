//! Import Painter smart materials (`.spsm`) into the sample crate and report what came in.
//! `cargo run --release -p subcraft-app --example spsm_import -- a.spsm b.spsm ...`
use subcraft_core::ChannelKind;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let mut s = subcraft_app::Session::new(subcraft_mesh::primitives::sample_crate(), 512);
        if std::env::var_os("SUBCRAFT_SPSM_LAYERS").is_none() && arg == "-" { continue; }
        let t = std::time::Instant::now();
        match s.import_painter_smart_material(path) {
            Ok((name, count)) => {
                s.update_composites();
                let ts = s.selected_texture_set().id;
                let base = s.composite(ts).and_then(|c| c.channel(ChannelKind::BaseColor)).cloned();
                let (mut sum, mut n) = ([0.0f64; 3], 0u64);
                if let Some(img) = &base {
                    for y in (0..img.height()).step_by(4) {
                        for x in (0..img.width()).step_by(4) {
                            let p = img.get(x, y);
                            for c in 0..3 { sum[c] += p[c] as f64; }
                            n += 1;
                        }
                    }
                }
                let mean = sum.map(|v| v / n.max(1) as f64);
                println!("OK  {:<40} {:>3} layers in {:?} ({:.1}s); mean base colour {:.3} {:.3} {:.3}; undo: {:?}",
                    path.file_name().unwrap().to_string_lossy(), count, name, t.elapsed().as_secs_f32(), mean[0], mean[1], mean[2], s.history().entries().last().map(|e| e.label.clone()));
            }
            Err(e) => println!("ERR {}: {e}", path.display()),
        }
        if std::env::var_os("SUBCRAFT_SPSM_LAYERS").is_some() {
            fn show(l: &subcraft_doc::Layer, depth: usize) {
                let kind = match &l.content { subcraft_doc::LayerContent::Fill(_) => "fill", subcraft_doc::LayerContent::Paint(_) => "paint", subcraft_doc::LayerContent::Group(_) => "folder", _ => "other" };
                println!("{:indent$}- {} [{kind}] visible={} opacity={:?} mask={}", "", l.name, l.visible, l.blend.values().next().map(|b| b.opacity), l.mask.as_ref().map_or("none".to_string(), |m| format!("{} effects, enabled {}", m.effects.len(), m.enabled)), indent = depth * 2);
                for c in l.children() { show(c, depth + 1); }
            }
            if let Some(l) = s.selected_layer() { show(l, 1); }
        }
    }
}
