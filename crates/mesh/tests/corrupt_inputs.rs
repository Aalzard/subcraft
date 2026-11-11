//! QA sweep: damaged OBJ, glTF, GLB and USD files are import errors (or partial imports), never
//! a panic, a hang or a huge allocation. PLY / STL have their own tests; `.spp` and Alembic are
//! covered next to their parsers.

use std::path::PathBuf;

use subcraft_mesh::{import_mesh, import_mesh_for_baking};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-corrupt-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Import `bytes` as `fixture.<ext>` both ways; the result does not matter, only that it returns.
fn try_import(d: &std::path::Path, ext: &str, bytes: &[u8]) {
    let p = d.join(format!("fixture.{ext}"));
    std::fs::write(&p, bytes).unwrap();
    let _ = import_mesh(&p);
    let _ = import_mesh_for_baking(&p);
}

/// Every truncation on a grid plus deterministic single-byte damage.
fn damage(name: &str, ext: &str, good: &[u8], step: usize) {
    let d = dir(name);
    let p = d.join(format!("good.{ext}"));
    std::fs::write(&p, good).unwrap();
    if let Err(e) = import_mesh(&p) {
        panic!("the undamaged {ext} fixture must import: {e}");
    }
    for cut in (0..good.len()).step_by(step.max(1)) {
        try_import(&d, ext, &good[..cut]);
    }
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..300 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let mut bad = good.to_vec();
        let at = (seed % good.len() as u64) as usize;
        bad[at] = (seed >> 32) as u8;
        try_import(&d, ext, &bad);
        // And a run of identical bytes (zeros, 0xFF), which is what a bad disk sector looks like.
        let fill = if seed & 1 == 0 { 0x00 } else { 0xFF };
        let end = (at + 16).min(bad.len());
        bad[at..end].fill(fill);
        try_import(&d, ext, &bad);
    }
    std::fs::remove_dir_all(d).ok();
}

const OBJ: &str = "# cube face\no Quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 0 1\nusemtl Body\ns 1\nf 1/1/1 2/2/1 3/3/1 4/4/1\nf 1/1/1 3/3/1 4/4/1\n";

#[test]
fn damaged_obj_files_never_panic() {
    damage("obj", "obj", OBJ.as_bytes(), 1);
    let d = dir("obj-odd");
    for text in [
        "f 1 2 3\n",
        "v 0 0 0\nf 1 1 1\n",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 99999999999\n",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -1 -2 -3\nf -9 -9 -9\nf 0 0 0\n",
        "v nan nan nan\nv inf 0 0\nv 0 1e39 0\nf 1 2 3\n",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 1e30 -1e30\nf 1/1 2/1 3/1\n",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/ 2/ 3/\nf 1// 2// 3//\nf / / /\n",
        "v\nvt\nvn\nf\no\ng\nusemtl\nmtllib\n",
        "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3 1 2 3 1 2 3 1 2 3 1 2 3 1 2 3\n",
    ] {
        try_import(&d, "obj", text.as_bytes());
    }
    std::fs::remove_dir_all(d).ok();
}

/// One triangle: positions, normals, uvs and u16 indices in an embedded buffer.
fn gltf_json() -> String {
    let mut bin: Vec<u8> = Vec::new();
    for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        v.iter().for_each(|f| bin.extend_from_slice(&f.to_le_bytes()));
    }
    for _ in 0..3 {
        [0.0f32, 0.0, 1.0].iter().for_each(|f| bin.extend_from_slice(&f.to_le_bytes()));
    }
    for v in [[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]] {
        v.iter().for_each(|f| bin.extend_from_slice(&f.to_le_bytes()));
    }
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    bin.extend_from_slice(&[0, 0]);
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for c in bin.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        b64.push(B64[(n >> 18) as usize & 63] as char);
        b64.push(B64[(n >> 12) as usize & 63] as char);
        b64.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        b64.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"name":"Tri"}}],
"meshes":[{{"name":"Tri","primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2}},"indices":3,"material":0}}]}}],
"materials":[{{"name":"Body"}}],
"buffers":[{{"byteLength":{len},"uri":"data:application/octet-stream;base64,{b64}"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":36}},{{"buffer":0,"byteOffset":72,"byteLength":24}},{{"buffer":0,"byteOffset":96,"byteLength":6}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":2,"componentType":5126,"count":3,"type":"VEC2"}},{{"bufferView":3,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
        len = bin.len()
    )
}

#[test]
fn damaged_gltf_files_never_panic() {
    let json = gltf_json();
    damage("gltf", "gltf", json.as_bytes(), 3);
    // Accessors that point outside their buffer or lie about counts.
    let d = dir("gltf-odd");
    for (from, to) in [
        ("\"count\":3,\"type\":\"VEC3\",\"min\"", "\"count\":4000000000,\"type\":\"VEC3\",\"min\""),
        ("\"byteOffset\":96,\"byteLength\":6", "\"byteOffset\":4294967290,\"byteLength\":6"),
        ("\"componentType\":5123,\"count\":3", "\"componentType\":5125,\"count\":3"),
        ("\"indices\":3", "\"indices\":99"),
        ("\"POSITION\":0", "\"POSITION\":2"),
        ("\"nodes\":[0]", "\"nodes\":[7]"),
        ("\"mesh\":0", "\"mesh\":0,\"children\":[0]"),
    ] {
        assert!(json.contains(from), "fixture changed: {from}");
        try_import(&d, "gltf", json.replace(from, to).as_bytes());
    }
    std::fs::remove_dir_all(d).ok();
}

#[test]
fn damaged_glb_files_never_panic() {
    // The same asset as a binary container: 12-byte header, JSON chunk, no BIN chunk needed.
    let mut json = gltf_json().into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((12 + 8 + json.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    damage("glb", "glb", &glb, 5);
    let d = dir("glb-odd");
    let mut lie = glb.clone();
    lie[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    try_import(&d, "glb", &lie);
    let mut lie = glb.clone();
    lie[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    try_import(&d, "glb", &lie);
    std::fs::remove_dir_all(d).ok();
}

const USDA: &str = r#"#usda 1.0
(
    defaultPrim = "Root"
    upAxis = "Y"
)
def Xform "Root"
{
    def Mesh "Quad"
    {
        int[] faceVertexCounts = [4]
        int[] faceVertexIndices = [0, 1, 2, 3]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
        normal3f[] normals = [(0, 0, 1), (0, 0, 1), (0, 0, 1), (0, 0, 1)] (interpolation = "faceVarying")
        texCoord2f[] primvars:st = [(0, 0), (1, 0), (1, 1), (0, 1)] (interpolation = "faceVarying")
    }
}
"#;

#[test]
fn damaged_usda_files_never_panic() {
    damage("usda", "usda", USDA.as_bytes(), 2);
    let d = dir("usda-odd");
    for (from, to) in [
        ("faceVertexCounts = [4]", "faceVertexCounts = [4, 4, 4]"),
        ("faceVertexCounts = [4]", "faceVertexCounts = [-4]"),
        ("faceVertexCounts = [4]", "faceVertexCounts = [2147483647]"),
        ("faceVertexIndices = [0, 1, 2, 3]", "faceVertexIndices = [0, 1, 2, 2147483647]"),
        ("faceVertexIndices = [0, 1, 2, 3]", "faceVertexIndices = [0, 1, 2, -1]"),
        ("faceVertexIndices = [0, 1, 2, 3]", "faceVertexIndices = []"),
        ("point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]", "point3f[] points = []"),
        ("texCoord2f[] primvars:st = [(0, 0), (1, 0), (1, 1), (0, 1)]", "texCoord2f[] primvars:st = [(0, 0)]"),
        ("normal3f[] normals = [(0, 0, 1), (0, 0, 1), (0, 0, 1), (0, 0, 1)]", "normal3f[] normals = [(0, 0, 1)]"),
        ("upAxis = \"Y\"", "upAxis = \"Q\"\n    metersPerUnit = 0"),
        ("upAxis = \"Y\"", "upAxis = \"Z\"\n    metersPerUnit = -1e30"),
    ] {
        assert!(USDA.contains(from), "fixture changed: {from}");
        try_import(&d, "usda", USDA.replace(from, to).as_bytes());
    }
    std::fs::remove_dir_all(d).ok();
}
