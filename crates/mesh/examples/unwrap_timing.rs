//! Time the automatic UV unwrap on a dense sphere cut into many charts.
//! `cargo run --release -p subcraft-mesh --example unwrap_timing -- [segments] [angle_deg]`
use subcraft_mesh::unwrap::{UnwrapSettings, unwrap};

fn main() {
    let mut args = std::env::args().skip(1);
    let segments: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(256);
    let angle: f32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(8.0);
    let mesh = subcraft_mesh::primitives::uv_sphere(1.0, segments, segments / 2);
    let settings = UnwrapSettings { angle_threshold_deg: angle, ..Default::default() };
    let start = std::time::Instant::now();
    let (_, stats) = unwrap(&mesh, &settings).expect("unwrap");
    println!("{} triangles, {} charts ({} split): {:.3} s", stats.triangles, stats.charts, stats.split_charts, start.elapsed().as_secs_f64());
}
