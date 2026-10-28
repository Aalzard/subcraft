//! Substance Painter `.spp` reading, against a synthetic project built here (a small HDF5 file
//! written by `hdf5_file`, a hand-made `document.bin`, and two tiny OpenJPEG-written tiles).
//! No real project file is needed or stored.

use std::io::Cursor;

#[path = "support/spp_fixture.rs"]
mod fixture;

use fixture::{GRAY16, H, RGB8, W, document, pattern, project_file};
use subcraft_mesh::spp::{self, BakedKind, CacheFormat, LayerKind, SppFile, channel_bits, jp2};

// ----- tests ---------------------------------------------------------------------------------------------

#[test]
fn jp2_gray16_is_exact() {
    let d = jp2::decode(GRAY16).unwrap();
    assert_eq!((d.width, d.height, d.components, d.precision), (W, H, 1, 16));
    for y in 0..H {
        for x in 0..W {
            assert_eq!(d.samples[y * W + x], pattern(x, y, 0), "at {x},{y}");
        }
    }
}

#[test]
fn jp2_rgb8_uses_the_reversible_color_transform() {
    let d = jp2::decode(RGB8).unwrap();
    assert_eq!((d.width, d.height, d.components, d.precision), (W, H, 3, 8));
    for y in 0..H {
        for x in 0..W {
            for c in 0..3 {
                assert_eq!(d.samples[(y * W + x) * 3 + c], pattern(x, y, c) >> 8, "at {x},{y},{c}");
            }
        }
    }
}

#[test]
fn jp2_reduced_decode_is_a_smaller_picture_of_the_same_image() {
    let full = jp2::decode(GRAY16).unwrap();
    let half = jp2::decode_reduced(GRAY16, 1).unwrap();
    assert_eq!((half.width, half.height, half.reduced), (W / 2, H / 2, 1));
    let mean = |s: &[u16]| s.iter().map(|v| *v as f64).sum::<f64>() / s.len() as f64;
    assert!((mean(&full.samples) - mean(&half.samples)).abs() < 2500.0, "{} vs {}", mean(&full.samples), mean(&half.samples));
}

#[test]
fn jp2_rejects_garbage_and_truncation() {
    assert!(jp2::decode(b"not a jpeg 2000 file").is_err());
    for cut in [10, 60, 200, GRAY16.len() / 2, GRAY16.len() - 1] {
        // Must not panic; a truncated stream is an error or a degraded picture.
        let _ = jp2::decode(&GRAY16[..cut]);
    }
}

#[test]
fn hdf5_fixture_is_navigable() {
    let mut h5 = spp::hdf5::Hdf5File::open(Cursor::new(project_file())).unwrap();
    let mut top = h5.list("").unwrap();
    top.sort();
    assert_eq!(top, ["paint", "resources", "resources.toc", "texture-cache"]);
    assert_eq!(h5.list("texture-cache/4242").unwrap(), ["1"]);
    let info = h5.dataset("texture-cache/header").unwrap();
    assert_eq!(h5.read(&info).unwrap(), [0, 1, 2]);
    assert!(matches!(h5.dataset("paint/missing"), Err(spp::hdf5::Hdf5Error::NotFound(_))));
}

#[test]
fn hdf5_rejects_other_files() {
    assert!(matches!(spp::hdf5::Hdf5File::open(Cursor::new(vec![0u8; 200])), Err(spp::hdf5::Hdf5Error::NotHdf5)));
    let mut truncated = project_file();
    truncated.truncate(100);
    let r = spp::hdf5::Hdf5File::open(Cursor::new(truncated)).and_then(|mut h| h.list("paint"));
    assert!(r.is_err());
}

#[test]
fn project_structure_is_read() {
    let mut f = SppFile::from_reader(Cursor::new(project_file())).unwrap();
    let project = f.project().unwrap();
    assert_eq!(project.texture_sets.len(), 1);
    let set = &project.texture_sets[0];
    assert_eq!((set.name.as_str(), set.uid, set.width_log2, set.height_log2), ("Cube", 4242, 8, 8));
    assert_eq!(set.channels.iter().map(|c| c.bits).collect::<Vec<_>>(), [channel_bits::BASE_COLOR, channel_bits::ROUGHNESS]);
    assert_eq!(set.mesh_maps.len(), 2);
    assert_eq!(set.layers.len(), 2);
    let fill = &set.layers[0];
    assert_eq!((fill.name.as_str(), fill.kind, fill.visible, fill.has_mask), ("Rust", LayerKind::Fill, true, false));
    assert_eq!(fill.sources, [spp::SourceInfo::Uniform { channels: 1, color: [0.25, 0.5, 0.75] }]);
    let group = &set.layers[1];
    assert_eq!((group.name.as_str(), group.kind, group.visible, group.children.len()), ("Dirt", LayerKind::Group, false, 1));
    assert_eq!(group.children[0].name, "Scratches");
}

#[test]
fn truncated_documents_are_errors_not_panics() {
    let doc = document(0);
    assert!(spp::document::parse(&doc).is_ok());
    for cut in (0..doc.len()).step_by(3) {
        assert!(spp::document::parse(&doc[..cut]).is_err(), "prefix of {cut} bytes parsed");
    }
    let mut bad = doc.clone();
    bad[0] ^= 0xff;
    assert!(matches!(spp::document::parse(&bad), Err(spp::document::DocumentError::BadMagic)));
}

#[test]
fn baked_maps_are_decoded_and_matched_by_texture_set_name() {
    let mut f = SppFile::from_reader(Cursor::new(project_file())).unwrap();
    assert_eq!(f.baked_map_kinds("Cube").unwrap(), [BakedKind::AmbientOcclusion, BakedKind::Position]);
    assert!(f.baked_map_kinds("Other").unwrap().is_empty());
    let ao = f.baked_map("Cube", BakedKind::AmbientOcclusion, 4096).unwrap().unwrap();
    assert_eq!((ao.width as usize, ao.height as usize, ao.components), (W, H, 1));
    assert!((ao.pixel(7, 9)[0] - pattern(7, 9, 0) as f32 / 65535.0).abs() < 1e-6);
    let pos = f.baked_map("Cube", BakedKind::Position, 4096).unwrap().unwrap();
    assert_eq!(pos.components, 4);
    assert!((pos.pixel(3, 5)[1] - (pattern(3, 5, 1) >> 8) as f32 / 255.0).abs() < 1e-6);
    assert_eq!(pos.pixel(3, 5)[3], 1.0, "alpha is opaque when the tiles have none");
    assert!(f.baked_map("Cube", BakedKind::Thickness, 4096).unwrap().is_none());
    // Asking for a smaller size halves the picture.
    let small = f.baked_map("Cube", BakedKind::AmbientOcclusion, 68).unwrap().unwrap();
    assert_eq!((small.width, small.height), (68, 36));
}

#[test]
fn texture_cache_channel_is_decoded() {
    let mut f = SppFile::from_reader(Cursor::new(project_file())).unwrap();
    let project = f.project().unwrap();
    let set = &project.texture_sets[0];
    let img = f.channel_texture(set, channel_bits::ROUGHNESS, 4096).unwrap().unwrap();
    assert_eq!(img.format, CacheFormat::Rg8);
    assert_eq!((img.plane.width, img.plane.height, img.plane.components), (4, 2, 2));
    assert!((img.plane.pixel(3, 1)[0] - 70.0 / 255.0).abs() < 1e-6);
    assert_eq!(img.plane.pixel(3, 1)[1], 1.0);
    assert!(f.channel_texture(set, channel_bits::METALLIC, 4096).unwrap().is_none());
}

#[test]
fn the_model_comes_from_the_dump() {
    let dir = std::env::temp_dir().join(format!("subcraft-spp-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Cube.spp");
    std::fs::write(&path, project_file()).unwrap();
    let meshes = spp::read_meshes(&path).unwrap();
    assert_eq!((meshes.len(), meshes[0].name.as_str(), meshes[0].triangles.len(), meshes[0].positions.len()), (1, "Cube", 2, 4));
    let report = subcraft_mesh::import_mesh(&path).unwrap();
    assert_eq!(report.mesh.triangle_count(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sevenz_reads_lzma_and_copy_archives() {
    let expected_b = {
        // b.bin = 900 pseudo-random bytes + their first 300 + 100 bytes of text; compare structurally.
        let t: Vec<u8> = b"The quick brown fox jumps over the lazy dog. ".iter().copied().cycle().take(100).collect();
        t
    };
    for archive in [&include_bytes!("data/lzma.7z")[..], &include_bytes!("data/copy.7z")[..]] {
        let files = spp::sevenz::read_all(archive).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a/hello.txt", "b.bin"]);
        assert_eq!(files[0].data, b"The quick brown fox jumps over the lazy dog. ".repeat(40));
        assert_eq!(files[1].data.len(), 1300);
        assert_eq!(&files[1].data[900..1200], &files[1].data[..300], "the repeated part decoded identically");
        assert_eq!(&files[1].data[1200..], &expected_b[..]);
    }
    assert!(matches!(spp::sevenz::read_all(b"PK not 7z at all, just long enough to pass the size check"), Err(spp::sevenz::SevenZipError::NotSevenZip)));
    let mut cut = include_bytes!("data/lzma.7z").to_vec();
    cut.truncate(cut.len() - 40);
    assert!(spp::sevenz::read_all(&cut).is_err());
}

#[test]
fn embedded_jpegs_are_found_inside_a_substance_archive() {
    let mut f = SppFile::from_reader(Cursor::new(project_file())).unwrap();
    let url = "resource://starter_assets/g/g?version=dddd.sbsar";
    assert_eq!(spp::sbsar_key(url), Some("dddd.sbsar"));
    let jpegs = f.embedded_jpegs(url).unwrap();
    assert_eq!(jpegs.len(), 1);
    assert!(jpegs[0].starts_with(&[0xff, 0xd8, 0xff]) && jpegs[0].ends_with(&[0xff, 0xd9]));
    assert_eq!(jpegs[0].len(), 458);
    // Unknown resources and URLs without a version are simply absent.
    assert!(f.embedded_jpegs("resource://starter_assets/x/x?version=zzzz.sbsar").unwrap().is_empty());
    assert!(f.embedded_jpegs("resource://tool/shape/shape").unwrap().is_empty());
}

#[test]
fn imported_image_resources_are_decoded_top_row_first() {
    // 2x2 BGRA8 with a padded pitch; the first stored row is the top one.
    let rows = [
        200, 100, 0, 128, 0, 0, 255, 255, 0, 0, // top row, two padding bytes
        10, 20, 30, 255, 40, 50, 60, 255, 0, 0, // bottom row
    ];
    let blob = fixture::image_blob("BGRA8", 2, 2, 10, &rows);
    let p = subcraft_mesh::spp::images::decode_image_resource(&blob, 64).unwrap();
    assert_eq!((p.width, p.height, p.components), (2, 2, 4));
    let px = |x, y| p.pixel(x, y).iter().map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>();
    assert_eq!(px(0, 0), vec![0, 100, 200, 128], "top-left, BGRA swizzled to RGBA");
    assert_eq!(px(1, 0), vec![255, 0, 0, 255]);
    assert_eq!(px(0, 1), vec![30, 20, 10, 255], "the second stored row is the bottom one");
    assert!(subcraft_mesh::spp::images::decode_image_resource(&blob[..blob.len() - 12], 64).is_err(), "truncated data is an error");
}
