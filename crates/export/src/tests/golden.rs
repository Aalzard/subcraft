//! Byte-identity guard: every built-in template, rendered through every padding mode and a few
//! format / normal overrides, must keep producing exactly the bytes it produced before the
//! preset editor existed. The table was recorded on the v0.3.0 exporter (commit f739604).
use super::*;

/// FNV-1a 64: stable across platforms and Rust versions (unlike `DefaultHasher`).
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Channels with gradients (so padding, packing and encoding all show up in the bytes).
fn gradient_channels() -> BTreeMap<ChannelKind, TiledImage> {
    let mut m = BTreeMap::new();
    let r = 64u32;
    let mut img = |kind: ChannelKind, comps: usize, f: &dyn Fn(f32, f32) -> [f32; 4]| {
        let mut t = TiledImage::new(r, r, comps, [0.0; 4]);
        for y in 0..r {
            for x in 0..r {
                t.set(x, y, f(x as f32 / r as f32, y as f32 / r as f32));
            }
        }
        m.insert(kind, t);
    };
    img(ChannelKind::BaseColor, 4, &|u, v| [u, v, 0.3, 1.0]);
    img(ChannelKind::Roughness, 1, &|u, v| [(u + v) * 0.5; 4]);
    img(ChannelKind::Metallic, 1, &|u, _| [if u > 0.5 { 1.0 } else { 0.0 }; 4]);
    img(ChannelKind::AmbientOcclusion, 1, &|_, v| [1.0 - v * 0.5; 4]);
    img(ChannelKind::Emissive, 4, &|u, v| [u * v, 0.1, 0.9 - u * 0.5, 1.0]);
    img(ChannelKind::Height, 1, &|u, v| [((u * 9.0).sin() * (v * 7.0).cos()) * 0.3; 4]);
    img(ChannelKind::Normal, 4, &|u, v| {
        let n = glam::Vec3::new(u - 0.5, v - 0.5, 1.0).normalize();
        [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0]
    });
    m
}

/// `(case, file, fnv)` for every file written by every built-in template in each setting.
pub(super) fn golden_hashes() -> Vec<(String, String, u64)> {
    let mesh = primitives::sample_crate();
    let ch = gradient_channels();
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(32, 32, 1, [0.4; 4]));
    let mut no_ao = ch.clone();
    no_ao.remove(&ChannelKind::AmbientOcclusion);
    let sets = [
        ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None },
        ExportTextureSet { name: "Trim.1002", material_slot: 1, udim: Some(1002), channels: &no_ao, mesh_maps: Some(&baked) },
    ];
    let cases: Vec<(&str, ExportSettings)> = vec![
        ("default", ExportSettings::default()),
        ("pad_none", ExportSettings { padding: Padding::None, ..Default::default() }),
        ("pad_4", ExportSettings { padding: Padding::Texels(4), ..Default::default() }),
        ("png16_dx", ExportSettings { format_override: Some(FileFormat::Png16), normal_directx: Some(true), ..Default::default() }),
        ("exr_gl", ExportSettings { format_override: Some(FileFormat::Exr32), normal_directx: Some(false), udim_folders: true, ..Default::default() }),
        ("tga_noh2n", ExportSettings { format_override: Some(FileFormat::Tga8), height_to_normal: 0.0, ..Default::default() }),
    ];
    let mut out = Vec::new();
    // The nine templates of v0.3.0; the alpha variants (#145) come after them.
    for t in ExportTemplate::builtin().into_iter().take(9).filter(|t| t.name != "2D View") {
        for (case, base) in &cases {
            let s = ExportSettings { template: t.clone(), ..base.clone() };
            render_maps(&mesh, "Golden Mesh", &sets, &s, |name, bytes| {
                out.push((format!("{}/{case}", t.name), name.to_string(), fnv(bytes)));
                Ok(())
            })
            .unwrap();
        }
    }
    out
}

#[test]
#[ignore = "prints the table; run on the reference exporter to re-record"]
fn print_golden_hashes() {
    for (case, file, h) in golden_hashes() {
        println!("(\"{case}\", \"{file}\", 0x{h:016x}),");
    }
}

fn recorded() -> Vec<(String, String, u64)> {
    include_str!("golden_v030.tsv")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut f = l.trim_end().split('\t');
            let (case, file, h) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
            (case.to_string(), file.to_string(), u64::from_str_radix(h, 16).unwrap())
        })
        .collect()
}

#[test]
fn builtin_templates_write_the_same_bytes_as_v0_3_0() {
    let now = golden_hashes();
    let then = recorded();
    assert_eq!(then.len(), 492, "recorded table");
    assert_eq!(now.len(), then.len(), "same files written");
    for (a, b) in now.iter().zip(&then) {
        assert_eq!(a, b, "{} {} changed", b.0, b.1);
    }
}

#[test]
fn builtins_saved_as_user_presets_still_write_the_same_bytes() {
    // Duplicate every built-in through the JSON file format; the copy must export identically.
    let mesh = primitives::sample_crate();
    let ch = gradient_channels();
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let then = recorded();
    // The nine templates of v0.3.0; the alpha variants (#145) come after them.
    for t in ExportTemplate::builtin().into_iter().take(9).filter(|t| t.name != "2D View") {
        let copy = preset::duplicate(&t, "Copy");
        let back = preset::from_json(&preset::to_json(&copy), "x").unwrap().presets.remove(0);
        assert_eq!(back, copy);
        let s = ExportSettings { template: back, ..Default::default() };
        let mut n = 0;
        render_maps(&mesh, "Golden Mesh", &sets, &s, |name, bytes| {
            let rec = then.iter().find(|(c, f, _)| *c == format!("{}/default", t.name) && f == name).unwrap_or_else(|| panic!("{name} not recorded"));
            assert_eq!(rec.2, fnv(bytes), "{} {name}", t.name);
            n += 1;
            Ok(())
        })
        .unwrap();
        assert!(n > 0);
    }
}
