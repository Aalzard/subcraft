//! Painter 11 `scene.assbin` header (optional `Drehgestell.spp` fixture).

use std::path::PathBuf;

use subcraft_mesh::assbin::assbin_header;
use subcraft_spp::open_container;

fn drehgestell_spp() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUBCRAFT_SPP_FIXTURE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let local = PathBuf::from(r"C:\Users\princ\Desktop\Substance Klon\Drehgestell.spp");
    local.is_file().then_some(local)
}

#[test]
fn painter_11_assbin_header() {
    let Some(spp) = drehgestell_spp() else {
        eprintln!("skip assbin_header: set SUBCRAFT_SPP_FIXTURE or place Drehgestell.spp");
        return;
    };
    let container = open_container(&spp).expect("hdf5");
    assert!(container.scene_assbin.len() > 1000);
    let h = assbin_header(&container.scene_assbin).expect("header");
    assert_eq!(h.format_major, 1);
    assert!(h.format_minor >= 7);
    assert!(!h.compressed);
    let tail = subcraft_mesh::assbin::painter_v17_tail_offset(&container.scene_assbin).expect("graph");
    assert_eq!(tail, 4257, "Drehgestell scene-graph block ends at 528+3729");
}
