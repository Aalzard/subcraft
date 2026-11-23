//! Render check for the Blur Slope options, Non-uniform Blur and Color Match: each case is the
//! Base Color of a flat plane before (left) and after (right) the filter, as a PNG, with the
//! composite time. `cargo run --release -p subcraft-app --example blur_color_sheet -- <prefix> [size]`
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind as C, ChannelValue, color::linear3_to_srgb};
use subcraft_doc::{BlurMap, ContentEffectKind, FillChannel, FillSource, Filter, MatchSource, ProceduralKind, ProceduralParams, SlopeMode, SlopeSource};
use subcraft_image::{FileFormat, save_image};

fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let px = size as f32 / 1024.0;
    let slope = |source, mode| Filter::BlurSlopeMode { intensity: 16.0 * px, scale: 6, seed: 3, samples: 16, source, mode, image: None };
    let shapes = (ProceduralKind::Checker, 6.0, [0.02, 0.02, 0.025], [0.85, 0.8, 0.7]);
    let clouds = (ProceduralKind::Clouds, 5.0, [0.03, 0.005, 0.001], [0.9, 0.3, 0.03]);
    let cells = (ProceduralKind::Cells, 10.0, [0.02, 0.05, 0.1], [0.9, 0.9, 0.85]);
    let rust = (ProceduralKind::Grunge, 4.0, [0.16, 0.05, 0.02], [0.45, 0.2, 0.08]);
    // The images the image-input cases read, written next to the sheets: a swirl to follow
    // (custom noise), a soft spot (blur map: white blurs) and a two-colour sweep (Color Match).
    let image = |name: &str, f: &dyn Fn(f32, f32) -> [f32; 3]| -> Result<String, String> {
        let n = 256u32;
        let data: Vec<f32> = (0..n * n).flat_map(|i| f((i % n) as f32 / n as f32 - 0.5, (i / n) as f32 / n as f32 - 0.5)).collect();
        let path = format!("{prefix}_input_{name}.png");
        save_image(std::path::Path::new(&path), n, n, 3, &data, FileFormat::Png8).map_err(|e| e.to_string())?;
        Ok(path)
    };
    let images = [
        ("swirl", image("swirl", &|x, y| [(x * 9.0).sin() * (y * 9.0).cos() * 0.5 + 0.5; 3])?),
        ("spot", image("spot", &|x, y| [(1.0 - (x * x + y * y).sqrt() * 2.6).clamp(0.0, 1.0); 3])?),
        ("sunset", image("sunset", &|x, y| { let t = (x + y + 1.0) * 0.5; [0.9 - 0.7 * t, 0.35 * (1.0 - t) + 0.1, 0.1 + 0.6 * t] })?),
    ];
    let scratches = (ProceduralKind::Scratches, 6.0, [0.85, 0.8, 0.7], [0.02, 0.02, 0.025]);
    let scratches_dark = (ProceduralKind::Scratches, 6.0, [0.02, 0.02, 0.025], [0.85, 0.8, 0.7]);
    let cases = [
        ("slope_noise_min", shapes, slope(SlopeSource::Noise, SlopeMode::Min), &[][..]),
        ("slope_noise_max", shapes, slope(SlopeSource::Noise, SlopeMode::Max), &[][..]),
        ("slope_input_blur", cells, slope(SlopeSource::Input, SlopeMode::Blur), &[][..]),
        ("slope_input_min", cells, slope(SlopeSource::Input, SlopeMode::Min), &[][..]),
        ("slope_input_max", cells, slope(SlopeSource::Input, SlopeMode::Max), &[][..]),
        ("non_uniform_bright", cells, Filter::NonUniformBlur { radius: 24.0 * px, map: BlurMap::Value, scale: 4, seed: 1, samples: 48, image: None }, &[][..]),
        ("non_uniform_dark", cells, Filter::NonUniformBlur { radius: 24.0 * px, map: BlurMap::InvertedValue, scale: 4, seed: 1, samples: 48, image: None }, &[][..]),
        ("non_uniform_noise", shapes, Filter::NonUniformBlur { radius: 24.0 * px, map: BlurMap::Noise, scale: 3, seed: 1, samples: 48, image: None }, &[][..]),
        ("color_match_average", rust, Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [0.05, 0.22, 0.3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None }, &[][..]),
        ("color_match_picked_hue", clouds, Filter::ColorMatch { source_mode: MatchSource::Parameter, source: [0.9, 0.3, 0.03], target: [0.1, 0.5, 0.9], hue: 1.0, chroma: 1.0, luma: 0.25, source_image: None, target_image: None }, &[][..]),
        ("slope_noise_min_scratches", scratches, slope(SlopeSource::Noise, SlopeMode::Min), &[][..]),
        ("slope_noise_max_scratches", scratches_dark, slope(SlopeSource::Noise, SlopeMode::Max), &[][..]),
        ("slope_custom_blur", shapes, slope(SlopeSource::Custom, SlopeMode::Blur), &[(0, "swirl")][..]),
        ("slope_custom_max", shapes, slope(SlopeSource::Custom, SlopeMode::Max), &[(0, "swirl")][..]),
        ("non_uniform_image", cells, Filter::NonUniformBlur { radius: 24.0 * px, map: BlurMap::Image, scale: 4, seed: 1, samples: 48, image: None }, &[(0, "spot")][..]),
        ("color_match_target_image", rust, Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target: [0.05, 0.22, 0.3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None }, &[(1, "sunset")][..]),
        ("color_match_source_image", rust, Filter::ColorMatch { source_mode: MatchSource::Image, source: [0.5; 3], target: [0.05, 0.22, 0.3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None }, &[(0, "sunset")][..]),
    ];
    for (name, (kind, scale, low, high), mut filter, inputs) in cases {
        if !filter.valid() {
            return Err(format!("{name}: invalid at this size"));
        }
        let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), size);
        let ts = s.document().texture_sets[0].id;
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        for (slot, input) in inputs {
            let path = &images.iter().find(|(n, _)| n == input).ok_or("unknown input image")?.1;
            let id = s.import_image(std::path::Path::new(path))?;
            if !filter.set_image(*slot, Some(id)) {
                return Err(format!("{name}: no image slot {slot}"));
            }
        }
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        let layer = s.selection().layer.unwrap();
        let source = FillSource::Procedural { params: ProceduralParams { scale, ..ProceduralParams::new(kind) }, low: ChannelValue::rgb(low[0], low[1], low[2]), high: ChannelValue::rgb(high[0], high[1], high[2]) };
        s.execute(Command::SetFillChannel { layer, channel: C::BaseColor, fill: FillChannel::new(source) }).map_err(err)?;
        s.update_composites();
        let before = s.composite(ts).unwrap().channel(C::BaseColor).cloned().unwrap();
        s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Filter { channel: C::BaseColor, filter } }).map_err(err)?;
        let t = std::time::Instant::now();
        s.update_composites();
        println!("{name}: composite {:.2}s at {size}", t.elapsed().as_secs_f32());
        let after = s.composite(ts).unwrap().channel(C::BaseColor).cloned().unwrap();
        let mut data = vec![0.0f32; (size * 2 * size * 3) as usize];
        for y in 0..size {
            for x in 0..size {
                for (col, img) in [(0, &before), (1, &after)] {
                    let p = img.get(x, y);
                    let o = ((y * size * 2 + col * size + x) * 3) as usize;
                    data[o..o + 3].copy_from_slice(&linear3_to_srgb([p[0], p[1], p[2]]));
                }
            }
        }
        save_image(std::path::Path::new(&format!("{prefix}_{name}.png")), size * 2, size, 3, &data, FileFormat::Png8).map_err(|e| e.to_string())?;
    }
    Ok(())
}
