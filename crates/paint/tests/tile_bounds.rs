//! Conservative stamp bounds must produce exactly the original full-triangle coverage.
use glam::{Vec2, Vec3};
use std::{collections::BTreeMap, sync::Arc};
use subcraft_image::{Bitmap, TILE_TEXELS};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::{Bvh, primitives};
use subcraft_paint::{
    Alignment, Camera, DepthCache,
    kernel::{KernelInput, TileWork},
    stamp::{Dab, resolve_overlap},
};

#[test]
fn bounded_tiles_are_bit_exact_to_full_triangle_kernel_for_seams_alpha_and_mirrors() {
    let camera = Camera::perspective(
        Vec3::new(3.0, 0.4, 4.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    );
    let cameras = [camera, camera.mirrored(0, 0.0)];
    let alpha = Bitmap::new_u8(2, 2, 1, vec![255, 120, 64, 255]);
    let mut saved_tiles = 0;
    for mesh in [primitives::cube(2.0), primitives::uv_sphere(1.0, 16, 12)] {
        let mesh = Arc::new(mesh);
        let bvh = Bvh::build(&mesh);
        let geometry = TextureSetGeometry::new(mesh.clone(), 0, 512);
        let cache = DepthCache::new();
        let depth: Vec<_> = cameras.iter().map(|c| cache.get(&mesh, c)).collect();
        for alignment in [
            Alignment::TangentWrap,
            Alignment::TangentPlanar,
            Alignment::Uv,
            Alignment::Camera,
        ] {
            for tip in [None, Some(&alpha)] {
                let input = KernelInput {
                    mesh: &mesh,
                    bvh: &bvh,
                    geometry: &geometry,
                    alignment,
                    cameras: &cameras,
                    depth: &depth,
                    backface_cos: Some(0.0),
                    alpha: tip,
                    stencil: None,
                };
                let mut stamps = Vec::new();
                for x in [150.0, 210.0, 256.0, 300.0, 370.0] {
                    for (ci, cam) in cameras.iter().enumerate() {
                        let dab = Dab {
                            screen: Vec2::new(x, 256.0),
                            radius: 0.15,
                            flow: 0.6,
                            angle: 0.73,
                            roundness: 0.6,
                            hardness: 0.4,
                        };
                        if let Some(st) = resolve_overlap(
                            &dab,
                            ci,
                            cam,
                            &mesh,
                            &bvh,
                            alignment,
                            tip.is_some(),
                            |_| true,
                        ) {
                            stamps.push(st);
                        }
                    }
                }
                assert!(stamps.len() > 2);
                let mut full = BTreeMap::<usize, TileWork>::new();
                for (i, stamp) in stamps.iter().enumerate() {
                    for tri in input.candidates(stamp) {
                        for tile in input.triangle_tiles(tri) {
                            let w = full.entry(tile).or_default();
                            w.tris.insert(tri);
                            w.stamps.insert(i as u32);
                        }
                    }
                }
                let bounded = input.bin(&stamps);
                saved_tiles += full.len() - bounded.len();
                for (tile, work) in full {
                    let samples = input.rasterize_tile(tile, work.tris);
                    let mut reference = vec![0.0; TILE_TEXELS];
                    input.accumulate(&samples, &stamps, &work.stamps, &mut reference);
                    let mut actual = vec![0.0; TILE_TEXELS];
                    if let Some(work) = bounded.get(&tile) {
                        let samples = input.rasterize_tile(tile, work.tris.iter().copied());
                        input.accumulate(&samples, &stamps, &work.stamps, &mut actual);
                    }
                    assert_eq!(
                        actual,
                        reference,
                        "{alignment:?}, alpha={}, tile={tile}",
                        tip.is_some()
                    );
                }
            }
        }
    }
    assert!(
        saved_tiles > 30,
        "must remove real work, not only exercise the old path"
    );
}
