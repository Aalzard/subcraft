use std::path::PathBuf;

use subcraft_spp::read_container;

#[test]
fn read_drehgestell_container_when_scratch_copy_exists() {
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/project.spp");
    if !scratch.is_file() {
        return;
    }
    let c = read_container(&scratch).expect("read_container");
    assert!(c.scene_assbin.len() > 1_000_000);
    assert!(c.document_bin.len() > 100_000);
}
