//! USD import: usda fixtures written here, converted to usdc / usdz with the library's own writers.

use std::path::{Path, PathBuf};

use glam::Vec3;
use openusd::sdf;
use subcraft_mesh::{BuildError, ImportError, import_mesh};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-usd-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A unit quad in the XY plane (z = 0), faceVarying UVs spanning 0..1, authored y-up in meters.
const QUAD: &str = r#"#usda 1.0
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

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn usda_quad_triangulates_with_uvs_normals_and_names() {
    let dir = tmp("quad");
    let r = import_mesh(&write(&dir, "quad.usda", QUAD)).unwrap();
    let m = &r.mesh;
    assert_eq!(m.triangle_count(), 2);
    assert_eq!(m.positions.len(), 4);
    assert_eq!(m.objects.len(), 1);
    assert_eq!(m.objects[0].name, "Quad");
    assert!(m.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
    // v is flipped to SubCraft's top-left origin: the point at y = 0 has v = 1.
    let i = m.positions.iter().position(|p| *p == Vec3::ZERO).unwrap();
    assert!((m.uvs[i].y - 1.0).abs() < 1e-6 && m.uvs[i].x.abs() < 1e-6);
    assert!(m.normals.iter().all(|n| (*n - Vec3::Z).length() < 1e-5));
    assert!(m.validate().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn z_up_units_and_transform_hierarchy_are_applied() {
    let dir = tmp("xform");
    let text = r#"#usda 1.0
(
    upAxis = "Z"
    metersPerUnit = 0.01
)
def Xform "Parent"
{
    double3 xformOp:translate = (100, 0, 0)
    uniform token[] xformOpOrder = ["xformOp:translate"]
    def Xform "Child"
    {
        double3 xformOp:scale = (2, 2, 2)
        uniform token[] xformOpOrder = ["xformOp:scale"]
        def Mesh "Tri"
        {
            int[] faceVertexCounts = [3]
            int[] faceVertexIndices = [0, 1, 2]
            point3f[] points = [(0, 0, 0), (50, 0, 0), (0, 0, 50)]
            texCoord2f[] primvars:st = [(0, 0), (1, 0), (0, 1)] (interpolation = "vertex")
        }
    }
}
"#;
    let r = import_mesh(&write(&dir, "x.usda", text)).unwrap();
    let m = &r.mesh;
    // Local (50,0,0) -> scale 2 -> +translate 100 -> 200 cm = 2 m in x. Local z 50 -> 100 cm -> 1 m, which is +Y after the Z-up fix.
    let (mn, mx) = m.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    assert!((mn - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-4, "{mn:?}");
    assert!((mx - Vec3::new(2.0, 1.0, 0.0)).length() < 1e-4, "{mx:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn indexed_uvs_subset_materials_and_multiple_objects() {
    let dir = tmp("multi");
    let text = r#"#usda 1.0
def Scope "Looks"
{
    def Material "Red" {}
    def Material "Blue" {}
}
def Mesh "A" (prepend apiSchemas = ["MaterialBindingAPI"])
{
    int[] faceVertexCounts = [3, 3]
    int[] faceVertexIndices = [0, 1, 2, 0, 2, 3]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
    rel material:binding = </Looks/Red>
    texCoord2f[] primvars:st = [(0, 0), (0.5, 0), (0.5, 0.5), (0, 0.5)] (interpolation = "vertex")
    int[] primvars:st:indices = [0, 1, 2, 0, 2, 3]
    def GeomSubset "Second" (prepend apiSchemas = ["MaterialBindingAPI"])
    {
        uniform token elementType = "face"
        int[] indices = [1]
        rel material:binding = </Looks/Blue>
    }
}
def Mesh "B"
{
    int[] faceVertexCounts = [5]
    int[] faceVertexIndices = [0, 1, 2, 3, 4]
    point3f[] points = [(0, 0, 1), (1, 0, 1), (2, 1, 1), (1, 2, 1), (0, 1, 1)]
    texCoord2f[] primvars:st = [(0.2, 0.2)] (interpolation = "constant")
}
"#;
    let r = import_mesh(&write(&dir, "multi.usda", text)).unwrap();
    let m = &r.mesh;
    assert_eq!(m.triangle_count(), 2 + 3, "two triangles plus a pentagon fan");
    let names: Vec<_> = m.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["A", "B"]);
    let mats: Vec<_> = m.materials.iter().map(|x| x.name.as_str()).collect();
    assert!(mats.contains(&"Red") && mats.contains(&"Blue") && mats.contains(&"DefaultMaterial"), "{mats:?}");
    assert!(m.uvs.iter().all(|uv| uv.x >= 0.0 && uv.x <= 0.5 + 1e-6 || (uv.x - 0.2).abs() < 1e-6));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn references_and_sublayers_are_composed() {
    let dir = tmp("compose");
    write(&dir, "asset.usda", QUAD);
    let text = r#"#usda 1.0
def Xform "Placed" (references = @./asset.usda@</Root>)
{
    double3 xformOp:translate = (0, 5, 0)
    uniform token[] xformOpOrder = ["xformOp:translate"]
}
"#;
    let r = import_mesh(&write(&dir, "scene.usda", text)).unwrap();
    assert_eq!(r.mesh.triangle_count(), 2);
    let min_y = r.mesh.positions.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    assert!((min_y - 5.0).abs() < 1e-5, "min_y {min_y}");
    write(&dir, "layered.usda", "#usda 1.0\n(\n    subLayers = [@./asset.usda@]\n)\n");
    assert_eq!(import_mesh(&dir.join("layered.usda")).unwrap().mesh.triangle_count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn usdc_and_usdz_import_matches_usda() {
    let dir = tmp("binary");
    let usda = write(&dir, "quad.usda", QUAD);
    let layer = sdf::Layer::open(usda.to_str().unwrap()).unwrap();
    let fmt = sdf::LayerRegistry::find_by_extension("usdc").unwrap();
    let mut cursor = std::io::Cursor::new(Vec::new());
    fmt.write(layer.data(), &mut cursor).unwrap();
    let usdc_bytes = cursor.into_inner();
    let usdc = dir.join("quad.usdc");
    std::fs::write(&usdc, &usdc_bytes).unwrap();

    let usdz = dir.join("quad.usdz");
    let mut z = openusd::usdz::ArchiveWriter::create(&usdz).unwrap();
    z.add_layer("quad.usdc", &usdc_bytes).unwrap();
    z.finish().unwrap();

    let a = import_mesh(&usda).unwrap().mesh;
    for p in [&usdc, &usdz] {
        let b = import_mesh(p).unwrap().mesh;
        assert_eq!(b.triangle_count(), a.triangle_count(), "{}", p.display());
        assert_eq!(b.positions, a.positions, "{}", p.display());
        assert_eq!(b.uvs, a.uvs, "{}", p.display());
        assert_eq!(b.objects[0].name, "Quad");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn uv_less_mesh_reports_the_usual_error() {
    let dir = tmp("nouv");
    let text = QUAD.replace("texCoord2f[] primvars:st = [(0, 0), (1, 0), (1, 1), (0, 1)] (interpolation = \"faceVarying\")", "");
    let p = write(&dir, "nouv.usda", &text);
    assert!(matches!(import_mesh(&p), Err(ImportError::Build(BuildError::NoUvs))));
    let high = subcraft_mesh::import_mesh_for_baking(&p).unwrap();
    assert_eq!(high.mesh.triangle_count(), 2);
    assert!(high.warnings.iter().any(|w| w.contains("has no UVs")), "{:?}", high.warnings);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn broken_and_empty_files_fail_cleanly() {
    let dir = tmp("broken");
    for (name, body) in [("garbage.usda", "this is not usd"), ("trunc.usda", "#usda 1.0\ndef Mesh \"X\" {"), ("empty.usdc", ""), ("bad.usdc", "PXR-USDC\0\0\0garbage")] {
        let r = import_mesh(&write(&dir, name, body));
        assert!(r.is_err(), "{name} should fail");
    }
    // A valid file without meshes is an error too, not a panic.
    let r = import_mesh(&write(&dir, "nomesh.usda", "#usda 1.0\ndef Xform \"A\" {}\n"));
    assert!(r.is_err());
    // Inconsistent topology (index out of range) is skipped, leaving nothing to import.
    let bad = QUAD.replace("[0, 1, 2, 3]", "[0, 1, 2, 9]");
    assert!(import_mesh(&write(&dir, "oob.usda", &bad)).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invisible_prims_are_skipped_and_reported() {
    let dir = tmp("skip");
    let hidden = QUAD.replace("def Mesh \"Quad\"", "def Mesh \"Hidden\" (visibility = \"invisible\") {}\n    def Mesh \"Quad\"").replace("(visibility = \"invisible\") {}", "{ token visibility = \"invisible\" }");
    let r = import_mesh(&write(&dir, "h.usda", &hidden)).unwrap();
    assert_eq!(r.mesh.objects.len(), 1);
    assert!(r.warnings.iter().any(|w| w.contains("invisible")), "{:?}", r.warnings);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The prim walk recurses per nesting level; a file nested thousands of levels deep must be
/// refused as an error, not overflow the stack and take the whole process down.
#[test]
fn absurdly_nested_prims_are_an_error_not_a_crash() {
    let dir = tmp("deep");
    let depth = 3000;
    let mut text = String::from("#usda 1.0
");
    for i in 0..depth {
        text += &format!("def Xform \"X{i}\"
{{
");
    }
    text += &"}
".repeat(depth);
    let r = std::thread::Builder::new().stack_size(8 << 20).spawn({
        let p = write(&dir, "deep.usda", &text);
        move || import_mesh(&p)
    }).unwrap().join().expect("import thread must not crash");
    assert!(matches!(&r, Err(ImportError::Usd(m)) if m.contains("nested")), "{r:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// QA 2026-11: the nesting limit was only applied after parsing, and the text parser itself
/// recurses per nested prim. On an ordinary thread (the app opens models on the UI thread) a
/// deeply nested file overflowed the stack, which ends the process. No big-stack thread here.
#[test]
fn deeply_nested_text_files_are_refused_before_they_are_parsed() {
    let dir = tmp("deeper");
    let depth = 200_000;
    let mut text = String::from("#usda 1.0\n");
    for i in 0..depth {
        text += &format!("def Xform \"X{i}\"\n{{\n");
    }
    text += &"}\n".repeat(depth);
    let r = import_mesh(&write(&dir, "deeper.usda", &text));
    assert!(matches!(&r, Err(ImportError::Usd(m)) if m.contains("nested")), "{r:?}");
    // Braces inside comments do not count; an ordinary file still imports.
    let commented = QUAD.replace("def Xform \"Root\"", &format!("# {}\ndef Xform \"Root\"", "{".repeat(2000)));
    assert_eq!(import_mesh(&write(&dir, "commented.usda", &commented)).unwrap().mesh.triangle_count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}
