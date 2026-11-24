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
