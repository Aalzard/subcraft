//! Reproducible artwork, using SubCraft's real mesh, bake, layer, project and export APIs.
use glam::{Mat4, Vec2, Vec3};
use std::{path::Path, sync::Arc, time::Instant};
use subcraft_app::{Command, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress};
use subcraft_core::{ChannelKind as C, ChannelValue as V, MeshMapKind as M};
use subcraft_doc::*;
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder};
use subcraft_render::{SHADED_CHANNELS, ViewParams, ViewportRenderer, headless};

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
const RES: u32 = 4096;
const FACES: [(Vec3, Vec3, Vec3); 6] = [
    (Vec3::Z, Vec3::X, Vec3::NEG_Y),
    (Vec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
    (Vec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
    (Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
    (Vec3::Y, Vec3::X, Vec3::Z),
    (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
];
fn smooth(a: f32, b: f32, v: f32) -> f32 {
    let t = ((v - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn inside(d: f32) -> f32 {
    1. - smooth(-0.003, 0.003, d)
}
fn oct(x: f32, y: f32, hx: f32, hy: f32, cut: f32) -> f32 {
    (x.abs() - hx)
        .max(y.abs() - hy)
        .max((x.abs() + y.abs() - hx - hy + cut) * std::f32::consts::FRAC_1_SQRT_2)
}
fn circle(x: f32, y: f32, r: f32) -> f32 {
    x.hypot(y) - r
}
fn glyph(c: char) -> [u8; 7] {
    match c {
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        _ => [0; 7],
    }
}
fn text(x: f32, y: f32, x0: f32, y0: f32, pitch: f32, label: &str) -> f32 {
    let xx = (x - x0) / pitch;
    let yy = (y - y0) / (pitch * 1.45);
    if xx < 0. || !(0. ..1.).contains(&yy) {
        return 0.;
    }
    let ci = xx.floor() as usize;
    let Some(c) = label.chars().nth(ci) else {
        return 0.;
    };
    let u = xx.fract() * 6.;
    if u >= 5. {
        return 0.;
    }
    let row = (yy * 7.) as usize;
    let col = u as usize;
    if glyph(c)[row] & (1 << (4 - col)) != 0 {
        1.
    } else {
        0.
    }
}
#[derive(Clone, Copy, Default)]
struct Features {
    armor: f32,
    cover: f32,
    gasket: f32,
    bolt: f32,
    yellow: f32,
    ink: f32,
    white: f32,
    led: f32,
    height: f32,
}
fn features(fi: usize, x: f32, y: f32) -> Features {
    let mut f = Features::default();
    let dp = oct(x, y, 0.71, 0.71, 0.16);
    f.armor = inside(dp + 0.016);
    f.height += 0.012 * inside(dp + 0.018) - 0.018 * inside(dp.abs() - 0.012);
    f.gasket = inside(dp.abs() - 0.013);
    let cover = oct(x - 0.13, y + 0.015, 0.385, 0.285, 0.07);
    if fi != 4 {
        f.cover = inside(cover + 0.014);
        let seam = inside(cover.abs() - 0.009);
        f.gasket = f.gasket.max(seam);
        f.height += 0.012 * inside(cover + 0.012) - 0.018 * seam;
        if fi == 2 || fi == 3 {
            for i in 0..7 {
                let d = oct(
                    x - 0.13,
                    y - (-0.205 + i as f32 * 0.065),
                    0.285,
                    0.012,
                    0.01,
                );
                let v = inside(d);
                f.gasket = f.gasket.max(v);
                f.height -= 0.018 * v;
            }
        } else {
            let r = circle(x - 0.23, y + 0.015, 0.118);
            let ring = inside(r.abs() - 0.008);
            f.gasket = f.gasket.max(ring);
            f.height -= 0.014 * ring;
            f.bolt = f.bolt.max(inside(r + 0.014));
            f.height += 0.008 * inside(r + 0.015);
            let slot = inside(oct(x - 0.23, y + 0.015, 0.064, 0.008, 0.003));
            f.gasket = f.gasket.max(slot);
            f.height -= 0.013 * slot;
        }
    } else {
        let r = x.hypot(y + 0.03);
        let ring = inside((r - 0.46).abs() - 0.013);
        f.gasket = f.gasket.max(ring);
        f.height -= 0.022 * ring;
        let disk = inside(r - 0.425);
        f.cover = disk;
        let a = (y + 0.03).atan2(x);
        let slots = smooth(0.1, 0.35, (a * 12.).sin()) * inside(r - 0.375) * smooth(0.16, 0.19, r);
        f.gasket = f.gasket.max(slots);
        f.height -= 0.025 * slots;
        f.bolt = f.bolt.max(inside(r - 0.115));
        f.height += 0.009 * inside(r - 0.115);
    }
    for sx in [-0.79, 0.79] {
        for sy in [-0.79, 0.79] {
            let r = (x - sx).hypot(y - sy);
            let well = inside(r - 0.047);
            let bolt = inside(r - 0.029);
            f.gasket = f.gasket.max(well);
            f.bolt = f.bolt.max(bolt);
            f.height -= 0.013 * well;
            f.height += 0.017 * bolt;
            let slot = inside(oct(x - sx, y - sy, 0.021, 0.004, 0.001));
            f.gasket = f.gasket.max(slot);
            f.bolt *= 1. - slot;
            f.height -= 0.012 * slot;
        }
    }
    let control = inside(oct(x + 0.53, y - 0.015, 0.075, 0.22, 0.022));
    if fi != 4 {
        f.gasket = f.gasket.max(control);
        f.height -= 0.012 * control;
        for yy in [-0.10, 0.015, 0.13] {
            let led = inside(oct(x + 0.53, y - yy, 0.034, 0.017, 0.006));
            f.led = f.led.max(led);
            f.height += 0.007 * led;
        }
    }
    let band = inside(oct(x, y - 0.566, 0.51, 0.045, 0.012));
    f.yellow = band;
    let stripe = if ((x + y) * 18.).rem_euclid(2.) < 0.82 {
        1.
    } else {
        0.
    };
    f.ink = band * stripe;
    let label = inside(oct(x + 0.385, y - 0.34, 0.145, 0.050, 0.004));
    if fi != 4 {
        f.cover = f.cover.max(label);
        let bar = if ((x + 0.52) * 190.).floor() as i32 % 5 < 2 {
            1.
        } else {
            0.
        };
        f.ink = f.ink.max(label * bar);
    }
    let title = if fi == 4 { "NOVA-07" } else { "CORE-07" };
    f.white =
        text(x, y, -0.52, -0.59, 0.115, title).max(text(x, y, 0.02, 0.375, 0.057, "NOVA SYS"));
    if fi == 0 || fi == 1 || fi == 5 {
        f.ink = f.ink.max(text(x, y, -0.15, -0.11, 0.072, "07"));
    }
    let edge_fade = 1. - smooth(0.86, 0.92, x.abs().max(y.abs()));
    // Stronger real high-poly relief, baked once into the tangent normal.
    f.height *= edge_fade * 1.35;
    f
}
// Continuous deterministic breakup: no per-texel white noise or UV-dependent RNG.
fn patina(x: f32, y: f32, face: usize) -> f32 {
    let z = face as f32 * 1.731;
    (0.5 + 0.23 * (x * 29. + y * 17. + z).sin()
        + 0.16 * (x * 67. - y * 43. + z * 2.).sin()
        + 0.11 * (x * 173. + y * 137. - z).sin())
    .clamp(0., 1.)
}
fn weathering(fi: usize, x: f32, y: f32, f: Features) -> [f32; 3] {
    let p = patina(x, y, fi);
    let protected = 1. - f.white.max(f.ink).max(f.yellow).max(f.led).max(f.gasket);
    let rim = inside(oct(x, y, 0.71, 0.71, 0.16).abs() - 0.036);
    let scuff = rim * smooth(0.60, 0.84, p) * protected * 0.52;
    // Dust catches on panel lips and lower surfaces; clean broad panels remain readable.
    let lower = smooth(0.25, 0.70, y) * f.armor * 0.20;
    let dust = (rim * 0.23 + f.gasket * 0.25 + lower) * smooth(0.32, 0.78, p) * (1. - f.led);
    // Oxidation belongs only on exposed steel, never the ceramic or titanium.
    let rust = f.bolt * smooth(0.56, 0.82, p) * 0.78;
    [scuff, dust.min(0.42), rust]
}
fn point(fi: usize, x: f32, y: f32, detail: bool) -> Vec3 {
    let (n, u, v) = FACES[fi];
    let p = n + u * x + v * y;
    let q = p.clamp(Vec3::splat(-0.94), Vec3::splat(0.94));
    let rounded = q + (p - q).normalize_or_zero() * 0.06;
    rounded
        + n * if detail {
            features(fi, x, y).height
        } else {
            0.
        }
}
fn mesh(detail: bool) -> Mesh {
    let mut b = MeshBuilder::new(if detail {
        "CORE-07 HIGH · recessed hardware"
    } else {
        "CORE-07"
    });
    let m = b.material("CORE_07_Assembly");
    let axis: Vec<f32> = if detail {
        (0..=192).map(|i| i as f32 / 96. - 1.).collect()
    } else {
        vec![
            -1., -0.985, -0.96, -0.94, -0.90, -0.82, -0.65, -0.35, 0., 0.35, 0.65, 0.82, 0.90,
            0.94, 0.96, 0.985, 1.,
        ]
    };
    for fi in 0..6 {
        let corner = |x: f32, y: f32| {
            let eps = if detail { 0.0075 } else { 0.0005 };
            let du = point(fi, x + eps, y, detail) - point(fi, x - eps, y, detail);
            let dv = point(fi, x, y + eps, detail) - point(fi, x, y - eps, detail);
            let mut normal = du.cross(dv).normalize_or_zero();
            if normal.dot(FACES[fi].0) < 0. {
                normal = -normal;
            }
            let cell = Vec2::new((fi % 3) as f32 / 3., (fi / 3) as f32 / 2.);
            let uv = cell + Vec2::new(1. / 60., 0.1) + Vec2::new(x + 1., y + 1.) * 0.15;
            Corner {
                position: point(fi, x, y, detail),
                normal: Some(normal),
                uv: Some(uv),
            }
        };
        for yy in axis.windows(2) {
            for xx in axis.windows(2) {
                let q = [
                    corner(xx[0], yy[0]),
                    corner(xx[1], yy[0]),
                    corner(xx[1], yy[1]),
                    corner(xx[0], yy[1]),
                ];
                for idx in [[0, 1, 2], [0, 2, 3]] {
                    let mut tri = [q[idx[0]], q[idx[1]], q[idx[2]]];
                    if (tri[1].position - tri[0].position)
                        .cross(tri[2].position - tri[0].position)
                        .dot(FACES[fi].0)
                        < 0.
                    {
                        tri.swap(1, 2);
                    }
                    b.push_triangle(m, tri);
                }
            }
        }
    }
    b.build().unwrap()
}
fn sample_uv(u: f32, v: f32) -> Option<(usize, f32, f32)> {
    let cx = (u * 3.).floor() as usize;
    let cy = (v * 2.).floor() as usize;
    let x = (u - cx as f32 / 3. - 1. / 60.) / 0.15 - 1.;
    let y = (v - cy as f32 / 2. - 0.1) / 0.15 - 1.;
    (cx < 3 && cy < 2 && x.abs() <= 1. && y.abs() <= 1.).then_some((cy * 3 + cx, x, y))
}
fn masks() -> Vec<TiledImage> {
    let mut images: Vec<_> = (0..11)
        .map(|_| TiledImage::new(RES, RES, 1, [0.; 4]))
        .collect();
    for t in 0..images[0].tile_count() {
        let rect = images[0].tile_rect(t);
        let mut buffers = vec![vec![0.; 4096]; 11];
        for i in 0..4096 {
            let x = rect.x0 as u32 + i as u32 % 64;
            let y = rect.y0 as u32 + i as u32 / 64;
            if let Some((fi, a, b)) =
                sample_uv((x as f32 + 0.5) / RES as f32, (y as f32 + 0.5) / RES as f32)
            {
                let f = features(fi, a, b);
                let [scuff, dust, rust] = weathering(fi, a, b, f);
                let values = [
                    f.armor, f.cover, f.gasket, f.bolt, f.yellow, f.ink, f.white, f.led, scuff,
                    dust, rust,
                ];
                for (buffer, value) in buffers.iter_mut().zip(values) {
                    buffer[i] = value;
                }
            }
        }
        for k in 0..11 {
            if buffers[k].iter().any(|&v| v > 0.) {
                images[k].write_tile(t, &buffers[k]);
            }
        }
    }
    images
}
fn color(rgb: [f32; 3]) -> V {
    V::rgb(
        subcraft_core::color::srgb_to_linear(rgb[0]),
        subcraft_core::color::srgb_to_linear(rgb[1]),
        subcraft_core::color::srgb_to_linear(rgb[2]),
    )
}
fn material(
    s: &Session,
    name: &str,
    rgb: [f32; 3],
    metal: f32,
    rough: f32,
    mask: Option<TiledImage>,
) -> Layer {
    let mut l = Layer::new_fill(name, s.selected_texture_set());
    let LayerContent::Fill(f) = &mut l.content else {
        unreachable!()
    };
    for ch in f.channels.values_mut() {
        ch.enabled = false;
    }
    for (k, v) in [
        (C::BaseColor, color(rgb)),
        (C::Metallic, V::scalar(metal)),
        (C::Roughness, V::scalar(rough)),
    ] {
        f.channels
            .insert(k, FillChannel::new(FillSource::uniform(v)));
    }
    if let Some(base) = mask {
        l.mask = Some(Mask {
            enabled: true,
            base,
            effects: vec![],
        });
    }
    l
}
fn insert(s: &mut Session, l: Layer) {
    s.execute(Command::InsertLayer {
        label: l.name.clone(),
        layer: Box::new(l),
    })
    .unwrap();
}
fn roughness(layer: &mut Layer, kind: ProceduralKind, scale: f32, low: f32, high: f32) {
    if let LayerContent::Fill(f) = &mut layer.content {
        let mut params = ProceduralParams::new(kind);
        params.scale = scale;
        f.channels.insert(
            C::Roughness,
            FillChannel::new(FillSource::Procedural {
                params,
                low: V::scalar(low),
                high: V::scalar(high),
            }),
        );
    }
}
fn main() {
    let out = std::env::args().nth(1).expect("output folder");
    let out = Path::new(&out);
    for dir in [
        "models",
        "bakes",
        "textures/PBR",
        "textures/Unreal",
        "renders",
    ] {
        std::fs::create_dir_all(out.join(dir)).unwrap();
    }
    let started = Instant::now();
    let low = mesh(false);
    let high = mesh(true);
    println!(
        "Meshes: low {} triangles, high {} triangles",
        low.indices.len() / 3,
        high.indices.len() / 3
    );
    subcraft_mesh::import::write_obj(&low, &out.join("models/CORE-07-low.obj")).unwrap();
    subcraft_mesh::import::write_obj(&high, &out.join("models/CORE-07-high.obj")).unwrap();
    let low_bvh = Bvh::build(&low);
    let high_bvh = Bvh::build(&high);
    println!("Baking real high-to-low maps at {RES}, AO 64 rays/texel...");
    let settings = BakeSettings {
        size: RES,
        ao_samples: 64,
        ao_distance: 0.07,
        thickness_samples: 16,
        curvature_radius: 0.007,
        cage_front: 0.060,
        cage_rear: 0.060,
        ..Default::default()
    };
    let maps = subcraft_bake::bake(
        &BakeInput {
            low: Geometry {
                mesh: &low,
                bvh: &low_bvh,
            },
            high: Some(Geometry {
                mesh: &high,
                bvh: &high_bvh,
            }),
            material_slot: 0,
        },
        &settings,
        &Progress::new(),
    )
    .unwrap();
    println!(
        "Baked {} maps in {:.1}s",
        maps.len(),
        started.elapsed().as_secs_f32()
    );
    for (k, img) in &maps {
        save_image(
            &out.join(format!("bakes/{k:?}.png")),
            RES,
            RES,
            img.comps(),
            &img.to_dense(),
            FileFormat::Png16,
        )
        .unwrap();
    }
    let mut s = Session::new(low.clone(), RES);
    let ts = s.selected_texture_set().id;
    let old = s.selection().layer.unwrap();
    s.execute(Command::DeleteLayer(old)).unwrap();
    s.set_mesh_maps(ts, maps.clone());
    let mut base = material(
        &s,
        "01  |  Titanium chassis · brushed",
        [0.28, 0.30, 0.33],
        0.95,
        0.37,
        None,
    );
    if let LayerContent::Fill(f) = &mut base.content {
        f.projection.mode = ProjectionMode::Triplanar;
        f.channels.insert(
            C::Normal,
            FillChannel::new(FillSource::MeshMap { map: M::Normal }),
        );
        let mut p = ProceduralParams::new(ProceduralKind::Brushed);
        p.scale = 240.;
        p.contrast = 0.18;
        f.channels.insert(
            C::Roughness,
            FillChannel::new(FillSource::Procedural {
                params: p,
                low: V::scalar(0.36),
                high: V::scalar(0.42),
            }),
        );
    }
    // Mesh maps are UV sources; preserve UV projection on the baked normal layer.
    if let LayerContent::Fill(f) = &mut base.content {
        f.projection.mode = ProjectionMode::Uv;
    }
    insert(&mut s, base);
    println!("Building editable material masks...");
    let mut masks = masks().into_iter();
    for (name, rgb, metal, rough) in [
        (
            "02  |  Petrol ceramic armor",
            [0.23, 0.34, 0.39],
            0.02,
            0.48,
        ),
        (
            "03  |  Ivory service covers",
            [0.67, 0.67, 0.61],
            0.02,
            0.46,
        ),
        (
            "04  |  Recesses · graphite gaskets",
            [0.055, 0.064, 0.070],
            0.03,
            0.72,
        ),
        (
            "05  |  Fasteners · machined steel",
            [0.50, 0.52, 0.55],
            0.95,
            0.26,
        ),
        ("06  |  Hazard enamel · ochre", [0.82, 0.58, 0.17], 0., 0.47),
        (
            "07  |  Black safety stencils",
            [0.045, 0.052, 0.056],
            0.,
            0.61,
        ),
        ("08  |  White identification", [0.78, 0.80, 0.77], 0., 0.58),
        ("09  |  Cyan status lenses", [0.09, 0.77, 0.82], 0., 0.21),
    ] {
        let mut l = material(&s, name, rgb, metal, rough, Some(masks.next().unwrap()));
        if name.starts_with("02") {
            roughness(&mut l, ProceduralKind::Clouds, 24., 0.34, 0.56);
        } else if name.starts_with("03") {
            roughness(&mut l, ProceduralKind::Clouds, 32., 0.32, 0.51);
        } else if name.starts_with("05") {
            roughness(&mut l, ProceduralKind::Brushed, 180., 0.19, 0.34);
        } else if name.starts_with("06") {
            roughness(&mut l, ProceduralKind::Clouds, 38., 0.37, 0.53);
        }
        if name.starts_with("09") {
            if let LayerContent::Fill(f) = &mut l.content {
                f.channels.insert(
                    C::Emissive,
                    FillChannel::new(FillSource::uniform(V::rgb(0.025, 0.70, 0.90))),
                );
            }
            l.blend
                .insert(C::Emissive, ChannelBlend::new(BlendMode::Normal));
            s.execute(Command::InsertLayerWithChannels {
                label: name.into(),
                layer: Box::new(l),
                channels: vec![C::Emissive],
            })
            .unwrap();
        } else {
            insert(&mut s, l);
        }
    }
    let mut wear = material(
        &s,
        "10  |  Edge wear · baked curvature",
        [0.39, 0.41, 0.43],
        0.93,
        0.40,
        None,
    );
    let mut g = Generator::new(GeneratorKind::Edges);
    g.level = 0.70;
    g.softness = 0.10;
    g.noise_amount = 0.13;
    let mut mask = Mask::new(RES, false);
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(Levels {
            out_high: 0.16,
            ..Default::default()
        }))));
    wear.mask = Some(mask);
    insert(&mut s, wear);
    let mut dirt = material(
        &s,
        "11  |  Crevice grime · baked AO",
        [0.09, 0.075, 0.056],
        0.,
        0.83,
        None,
    );
    let mut g = Generator::new(GeneratorKind::Cavity);
    g.level = 0.24;
    g.softness = 0.16;
    g.noise_amount = 0.04;
    let mut mask = Mask::new(RES, false);
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(Levels {
            out_high: 0.30,
            ..Default::default()
        }))));
    dirt.mask = Some(mask);
    insert(&mut s, dirt);
    for (name, rgb, metal, low, high) in [
        (
            "12  |  Broken panel-edge scuffs",
            [0.36, 0.39, 0.40],
            0.70,
            0.28,
            0.43,
        ),
        (
            "13  |  Settled dust · localized",
            [0.20, 0.17, 0.13],
            0.,
            0.73,
            0.88,
        ),
        (
            "14  |  Steel oxidation · restrained",
            [0.29, 0.115, 0.045],
            0.,
            0.74,
            0.90,
        ),
    ] {
        let mut l = material(&s, name, rgb, metal, high, Some(masks.next().unwrap()));
        roughness(&mut l, ProceduralKind::Grunge, 62., low, high);
        insert(&mut s, l);
    }
    // Leave the armor selected so the project opens on useful material controls.
    let armor = s
        .selected_texture_set()
        .layers
        .iter()
        .find(|l| l.name.starts_with("02"))
        .unwrap()
        .id;
    s.execute(Command::SelectLayer(Some(armor))).unwrap();
    s.update_composites();
    let composite = s.composite(ts).unwrap().clone();
    for img in composite.channels.values() {
        assert!(
            img.to_dense().iter().all(|x| x.is_finite()),
            "finite PBR data"
        );
    }
    let normal = composite.channel(C::Normal).unwrap();
    let mut varied = 0;
    let mut covered = 0;
    let mut max_len_error = 0f32;
    for y in (0..RES).step_by(2) {
        for x in (0..RES).step_by(2) {
            if sample_uv((x as f32 + 0.5) / RES as f32, (y as f32 + 0.5) / RES as f32).is_some() {
                let p = normal.get(x, y);
                let n = Vec3::new(p[0], p[1], p[2]) * 2. - Vec3::ONE;
                max_len_error = max_len_error.max((n.length() - 1.).abs());
                covered += 1;
                if n.x.abs() + n.y.abs() > 0.07 {
                    varied += 1;
                }
            }
        }
    }
    assert!(
        varied > covered / 50,
        "normal bake must contain real relief"
    );
    assert!(
        max_len_error < 0.012,
        "normalized normals, error {max_len_error}"
    );
    let project = out.join("CORE-07.subcraft");
    s.save(Some(&project)).unwrap();
    let mut reopened = Session::open_project(&project).unwrap();
    reopened.update_composites();
    assert_eq!(
        s.document(),
        reopened.document(),
        "authored layers and masks round trip exactly"
    );
    assert_eq!(
        &composite,
        reopened.composite(ts).unwrap(),
        "save/reopen is texel-identical"
    );
    s.export_textures(
        &out.join("textures/PBR"),
        &subcraft_export::ExportSettings::default(),
    )
    .unwrap();
    s.export_textures(
        &out.join("textures/Unreal"),
        &subcraft_export::ExportSettings {
            template: subcraft_export::ExportTemplate::unreal(),
            ..Default::default()
        },
    )
    .unwrap();
    let prefix = "../textures/PBR/CORE-07_CORE_07_Assembly_";
    let mtl = format!(
        "# PBR OBJ extensions; tangent normals are OpenGL +Y\nnewmtl CORE_07_Assembly\nKd 1 1 1\nKs 0.04 0.04 0.04\nPr 0.45\nPm 0\nmap_Kd {prefix}BaseColor.png\nmap_Pr {prefix}Roughness.png\nmap_Pm {prefix}Metallic.png\nnorm {prefix}Normal.png\nKe 1 1 1\nmap_Ke {prefix}Emissive.png\n"
    );
    std::fs::write(out.join("models/CORE-07-low.mtl"), mtl).unwrap();
    println!("Rendering with SubCraft's own PBR renderer...");
    let (adapter, dev, queue) = headless::device().expect("real GPU for preview");
    println!("GPU: {}", adapter.get_info().name);
    let mut renderer = ViewportRenderer::new(&dev, &queue);
    renderer.set_mesh(&dev, &low);
    for k in SHADED_CHANNELS {
        if let Some(img) = composite.channel(k) {
            renderer.upload_channel(&dev, &queue, 0, k, img, None);
        }
    }
    renderer.upload_channel(
        &dev,
        &queue,
        0,
        C::AmbientOcclusion,
        &maps[&M::AmbientOcclusion],
        None,
    );
    for (name, eye) in [
        ("CORE-07-beauty", Vec3::new(3.6, 2.7, 4.6)),
        ("CORE-07-reverse", Vec3::new(-3.6, 2.6, -4.6)),
    ] {
        let size = [1600, 1400];
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let proj = Mat4::perspective_rh(
            34f32.to_radians(),
            size[0] as f32 / size[1] as f32,
            0.05,
            100.,
        );
        let mut p = ViewParams::new(view, proj);
        p.background = [0.012, 0.016, 0.022];
        p.light_dir = Vec3::new(-0.5, 0.8, 0.6).normalize();
        p.light_intensity = 2.1;
        p.environment_intensity = 0.85;
        let rgba = renderer
            .render(&dev, &queue, size, &p)
            .read_rgba8(&dev, &queue);
        let data: Vec<f32> = rgba.into_iter().map(|v| v as f32 / 255.).collect();
        save_image(
            &out.join(format!("renders/{name}.png")),
            size[0],
            size[1],
            4,
            &data,
            FileFormat::Png8,
        )
        .unwrap();
    }
    let stats = serde_json::json!({"resolution":RES,"low_triangles":low.indices.len()/3,"high_triangles":high.indices.len()/3,"baked_maps":maps.len(),"ao_samples":64,"layers":s.selected_texture_set().layers.len(),"normal_detail_samples":varied,"surface_samples":covered,"max_normal_length_error":max_len_error,"save_reopen":"exact document and texel equality","all_channels_finite":true,"elapsed_seconds":started.elapsed().as_secs_f32()});
    std::fs::write(
        out.join("validation.json"),
        serde_json::to_string_pretty(&stats).unwrap(),
    )
    .unwrap();
    println!("DONE {}", stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn six_square_islands_are_separate_and_map_back_to_their_faces() {
        for fi in 0..6 {
            let u = (fi % 3) as f32 / 3. + 1. / 6.;
            let v = (fi / 3) as f32 / 2. + 0.25;
            let (found, x, y) = sample_uv(u, v).unwrap();
            assert_eq!(found, fi);
            assert!(x.abs() < 1e-5 && y.abs() < 1e-5);
        }
        assert!(sample_uv(0., 0.).is_none());
        assert!(
            sample_uv(1. / 3., 0.25).is_none(),
            "gutter separates adjacent islands"
        );
    }
    #[test]
    fn material_mask_landmarks_match_the_mechanical_design() {
        let middle = features(0, 0., 0.);
        assert!(middle.armor > 0.99 && middle.cover > 0.99);
        let edge = features(0, 0.9, 0.9);
        assert_eq!(edge.armor, 0.);
        assert!(features(0, 0.805, 0.802).bolt > 0.99);
        assert!(features(0, -0.53, -0.10).led > 0.99);
        assert!(features(0, 0., 0.566).yellow > 0.99);
        for fi in 0..6 {
            for iy in 0..41 {
                for ix in 0..41 {
                    let f = features(fi, ix as f32 / 20. - 1., iy as f32 / 20. - 1.);
                    for m in [
                        f.armor, f.cover, f.gasket, f.bolt, f.yellow, f.ink, f.white, f.led,
                    ] {
                        assert!((0.0..=1.0).contains(&m));
                    }
                    assert!(f.height.is_finite() && f.height.abs() < 0.09);
                }
            }
        }
    }
    #[test]
    fn the_display_mesh_is_beveled_with_finite_tangent_frames() {
        let m = mesh(false);
        assert_eq!(m.indices.len() / 3, 3072);
        assert!(
            m.positions
                .iter()
                .all(|p| p.is_finite() && p.abs().max_element() <= 1.00001)
        );
        assert!(
            m.normals
                .iter()
                .all(|n| n.is_finite() && (n.length() - 1.).abs() < 0.001)
        );
        assert!(m.tangents.iter().all(|t| t.is_finite()));
        let p = point(0, 1., 1., false);
        assert!(p.x < 1. && p.y < 1. && p.z < 1.);
    }
    #[test]
    fn weathering_is_restrained_protects_graphics_and_limits_rust_to_steel() {
        let mut rust_sum = 0.;
        let mut scuff_sum = 0.;
        let mut samples = 0;
        for fi in 0..6 {
            for iy in 0..201 {
                for ix in 0..201 {
                    let x = ix as f32 / 100. - 1.;
                    let y = iy as f32 / 100. - 1.;
                    let f = features(fi, x, y);
                    let w = weathering(fi, x, y, f);
                    assert_eq!(w, weathering(fi, x, y, f));
                    assert!(w.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
                    if f.bolt == 0. {
                        assert_eq!(w[2], 0.);
                    }
                    if f.white.max(f.ink).max(f.yellow).max(f.led) == 1. {
                        assert_eq!(w[0], 0.);
                    }
                    rust_sum += w[2];
                    scuff_sum += w[0];
                    samples += 1;
                }
            }
        }
        let rust_fraction = rust_sum / samples as f32;
        let scuff_fraction = scuff_sum / samples as f32;
        assert!(
            (0.0001..0.012).contains(&rust_fraction),
            "rust fraction {rust_fraction}"
        );
        assert!(
            (0.0001..0.025).contains(&scuff_fraction),
            "scuff fraction {scuff_fraction}"
        );
    }
    #[test]
    fn high_poly_panel_depth_has_numeric_landmarks() {
        assert!((features(0, 0., 0.).height - 0.0324).abs() < 0.00001);
        assert!((features(0, 0.71, 0.4).height + 0.0243).abs() < 0.00001);
        // Projection cages include every authored displacement across all six faces.
        for fi in 0..6 {
            for iy in 0..201 {
                for ix in 0..201 {
                    let h = features(fi, ix as f32 / 100. - 1., iy as f32 / 100. - 1.).height;
                    assert!((-0.060..0.060).contains(&h), "cage misses height {h}");
                }
            }
        }
    }
}
