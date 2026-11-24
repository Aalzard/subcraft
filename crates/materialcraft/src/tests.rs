//! Real MaterialCraft files, written by MaterialCraft's own writers, opened and cooked.

use super::*;
use materialcraft_engine::samples::template;
use materialcraft_formats::{material::write_material, project::ProjectFiles, sbs::write_sbs};

/// MaterialCraft's "Material outputs" template: six uniform outputs.
fn pbr_package() -> Package {
    let mut package = Package::new("PBR test");
    package.graphs.push(template("pbr", "Surface", 6).expect("template"));
    package
}

fn mean(output: &Output) -> [f32; 4] {
    let mut sum = [0.0f64; 4];
    for texel in output.texels.chunks(output.channels) {
        for (c, v) in texel.iter().enumerate() {
            sum[c] += f64::from(*v);
        }
    }
    let n = (output.width * output.height) as f64;
    sum.map(|s| (s / n) as f32)
}

fn by_usage<'a>(outputs: &'a [Output], usage: &str) -> &'a Output {
    outputs.iter().find(|o| o.usage == usage).unwrap_or_else(|| panic!("no {usage} output in {:?}", outputs.iter().map(|o| &o.usage).collect::<Vec<_>>()))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn mcmat_opens_and_cooks_every_pbr_output() {
    let bytes = write_material(&pbr_package(), 0).expect("write .mcmat");
    assert!(is_materialcraft(&bytes, "no-extension"));
    let material = Material::open(&bytes, "surface.mcmat").expect("open");
    let graphs = material.graphs();
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].usages, ["baseColor", "normal", "roughness", "metallic", "height", "ambientOcclusion"]);

    assert_eq!(material.graph_index("Surface"), Some(material.entry));
    assert_eq!(material.graph_index(""), Some(material.entry));
    assert_eq!(material.graph_index("Missing"), None);

    let outputs = material.cook(material.entry, &BTreeMap::new(), 64).expect("cook");
    assert_eq!(outputs.len(), 6);
    for output in &outputs {
        assert_eq!((output.width, output.height), (64, 64), "{}", output.usage);
        assert_eq!(output.texels.len(), 64 * 64 * output.channels);
    }
    let base = mean(by_usage(&outputs, "baseColor"));
    assert!(close(base[0], 0.5) && close(base[1], 0.5) && close(base[2], 0.5), "linear base color kept as authored: {base:?}");
    let normal = mean(by_usage(&outputs, "normal"));
    assert!(close(normal[0], 0.5) && close(normal[1], 0.5) && close(normal[2], 1.0), "{normal:?}");
    assert!(!by_usage(&outputs, "normal").normal_y_down);
    assert!(close(mean(by_usage(&outputs, "roughness"))[0], 0.5));
    assert!(close(mean(by_usage(&outputs, "metallic"))[0], 0.0));
    assert!(close(mean(by_usage(&outputs, "ambientOcclusion"))[0], 1.0));
}

#[test]
fn sbs_and_mcraft_open_the_same_graph() {
    let package = pbr_package();
    let sbs = write_sbs(&package).expect("write .sbs");
    let mcraft = ProjectFiles::build(&package, "package.sbs", true).expect("project").zip().expect("zip");
    for (bytes, name) in [(sbs, "surface.sbs"), (mcraft, "surface.mcraft")] {
        let material = Material::open(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let outputs = material.cook(material.entry, &BTreeMap::new(), 32).expect("cook");
        assert!(close(mean(by_usage(&outputs, "roughness"))[0], 0.5), "{name}");
        assert_eq!(by_usage(&outputs, "baseColor").width, 32, "{name}");
    }
}

/// MaterialCraft's atomic sample (a pixel processor driven by an input value), with that value
/// exposed as a parameter the way the "Expose parameter" command does.
fn atomic_package() -> Package {
    let mut package = materialcraft_engine::atomic_sample().expect("sample");
    package.graphs[0].exposed.push(materialcraft_core::ExposedParameter {
        id: "brightness".into(),
        label: "Brightness".into(),
        group: "Look".into(),
        default: Value::Float(0.35),
        description: String::new(),
        editor: None,
        clamp: true,
        step: None,
        min: 0.0,
        max: 1.0,
        options: Vec::new(),
    });
    package
}

#[test]
fn exposed_parameter_is_a_live_slider() {
    let package = atomic_package();
    let bytes = write_material(&package, 0).expect("write .mcmat");
    let material = Material::open(&bytes, "atomic.mcmat").expect("open");
    let params = material.params(material.entry).expect("params");
    let brightness = params.iter().find(|p| p.id == "brightness").expect("brightness is exposed");
    assert_eq!((brightness.label.as_str(), brightness.group.as_str(), brightness.min, brightness.max), ("Brightness", "Look", 0.0, 1.0));
    assert_eq!(brightness.default, ParamValue::Float(0.35));

    let height = |value: f32| {
        let values = BTreeMap::from([("brightness".to_string(), ParamValue::Float(value))]);
        let outputs = material.cook(material.entry, &values, 64).expect("cook");
        mean(by_usage(&outputs, "height"))[0]
    };
    let (low, high) = (height(0.1), height(0.2));
    assert!(low > 0.0 && high > low * 1.5, "height follows the slider: {low} -> {high}");
    let default = material.cook(material.entry, &BTreeMap::new(), 64).expect("cook");
    assert!(close(mean(by_usage(&default, "height"))[0], height(0.35)), "no override means the default");
    assert!(close(height(5.0), height(1.0)), "clamped parameters stay in range");
}

#[test]
fn sizes_round_down_to_powers_of_two_and_clamp() {
    let bytes = write_material(&pbr_package(), 0).expect("write");
    let material = Material::open(&bytes, "surface.mcmat").expect("open");
    for (asked, got) in [(100, 64), (1, MIN_SIZE), (512, 512)] {
        let outputs = material.cook(material.entry, &BTreeMap::new(), asked).expect("cook");
        assert_eq!(outputs[0].width, got, "asked {asked}");
    }
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(Material::open(b"not a material", "x.mcmat").is_err());
    assert!(Material::open(b"<package/>", "x.sbs").is_err());
    let mut empty = Package::new("Empty");
    empty.graphs.push(template("empty", "Nothing", 6).expect("template"));
    assert!(matches!(Material::open(&write_sbs(&empty).expect("write"), "empty.sbs"), Err(Error::NoMaterial)));

    let material = Material::open(&write_material(&atomic_package(), 0).expect("write"), "a.mcmat").expect("open");
    let unknown = BTreeMap::from([("nope".to_string(), ParamValue::Float(1.0))]);
    assert!(matches!(material.cook(material.entry, &unknown, 32), Err(Error::UnknownParam(_))));
    let wrong = BTreeMap::from([("brightness".to_string(), ParamValue::Bool(true))]);
    assert!(matches!(material.cook(material.entry, &wrong, 32), Err(Error::WrongType { .. })));
    assert!(matches!(material.cook(99, &BTreeMap::new(), 32), Err(Error::NoGraph(99))));
}

/// A graph whose roughness is its `curvature` gray input and whose base color is its `ao`
/// color input, like a material that reads baked mesh maps.
fn input_package() -> Package {
    let mut graph = template("empty", "Reads_mesh_maps", 6).expect("template");
    for (index, (def, identifier, usage)) in [("input_grayscale", "curvature", 2u32), ("input_color", "ambientOcclusion", 0)].into_iter().enumerate() {
        let input = graph.add_node(def, [0.0, index as f32 * 180.0]);
        graph.node_mut(input).expect("node").params.insert("identifier".into(), Value::Text(identifier.into()));
        let output = graph.add_node("output", [220.0, index as f32 * 180.0]);
        let node = graph.node_mut(output).expect("node");
        node.params.insert("identifier".into(), Value::Text(format!("out_{identifier}")));
        node.params.insert("usage".into(), Value::Enum(usage));
        graph.connect(input, "output", output, "input").expect("connect");
    }
    let mut package = Package::new("Inputs");
    package.graphs.push(graph);
    package
}

#[test]
fn image_inputs_take_baked_maps() {
    let material = Material::open(&write_material(&input_package(), 0).expect("write"), "inputs.mcmat").expect("open");
    let inputs = material.image_inputs(material.entry).expect("inputs");
    assert_eq!(inputs.iter().map(|i| (i.identifier.as_str(), i.gray)).collect::<Vec<_>>(), [("curvature", true), ("ambientOcclusion", false)]);

    // A left-to-right ramp as curvature (RGB, as a baked map may come), a flat gray as AO.
    let (w, h) = (32u32, 8u32);
    let ramp: Vec<f32> = (0..w * h).flat_map(|i| [(i % w) as f32 / (w - 1) as f32; 3]).collect();
    let inputs = BTreeMap::from([
        ("curvature".to_string(), InputImage { width: w, height: h, channels: 3, texels: ramp }),
        ("ambientOcclusion".to_string(), InputImage { width: 4, height: 4, channels: 1, texels: vec![0.25; 16] }),
    ]);
    let outputs = material.cook_with_inputs(material.entry, &BTreeMap::new(), 64, &inputs).expect("cook");
    let rough = by_usage(&outputs, "roughness");
    assert_eq!((rough.width, rough.height), (64, 64), "inputs are resized to the cook size");
    let row = &rough.texels[..rough.width as usize * rough.channels];
    let (left, right) = (row[0], row[row.len() - rough.channels]);
    assert!(left < 0.1 && right > 0.9, "roughness follows the curvature ramp: {left} .. {right}");
    assert!((mean(rough)[0] - 0.5).abs() < 0.05);
    let base = mean(by_usage(&outputs, "baseColor"));
    assert!(close(base[0], 0.25) && close(base[2], 0.25), "gray map fills a color input: {base:?}");

    // Without inputs it still cooks; unknown inputs and bad sizes are errors.
    assert!(material.cook(material.entry, &BTreeMap::new(), 32).is_ok());
    let unknown = BTreeMap::from([("thickness".to_string(), InputImage { width: 1, height: 1, channels: 1, texels: vec![0.0] })]);
    assert!(material.cook_with_inputs(material.entry, &BTreeMap::new(), 32, &unknown).is_err());
    let short = BTreeMap::from([("curvature".to_string(), InputImage { width: 4, height: 4, channels: 1, texels: vec![0.0; 3] })]);
    assert!(material.cook_with_inputs(material.entry, &BTreeMap::new(), 32, &short).is_err());
}
