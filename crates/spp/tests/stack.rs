use std::path::PathBuf;

use subcraft_spp::load_stack;

#[test]
fn load_sample_stack_json() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/spp-stack.json");
    if !path.is_file() {
        return;
    }
    let stack = load_stack(&path).expect("stack json");
    assert!(!stack.materials.is_empty());
}
