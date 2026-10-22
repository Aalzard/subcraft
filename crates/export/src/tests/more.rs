//! Map selection, Alt+click solo, normal-format override and UDIM file layout.
use super::*;

fn files_in(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out)
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"))
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn keys(sets: &[&str], maps: &[&str]) -> Vec<MapKey> {
    sets.iter().flat_map(|s| maps.iter().map(move |m| (s.to_string(), m.to_string()))).collect()
}

fn all_keys(sets: &[ExportTextureSet], settings: &ExportSettings) -> Vec<MapKey> {
    plan_all("M", sets, settings).into_iter().map(|f| (f.set, f.suffix)).collect()
}

fn plain(ch: &BTreeMap<ChannelKind, TiledImage>) -> [ExportTextureSet<'_>; 1] {
    [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: ch, mesh_maps: None }]
}

#[test]
fn only_checked_maps_are_planned_and_written() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let sets = plain(&ch);
    let mut settings = ExportSettings::default();
    let all = all_keys(&sets, &settings);
    assert!(all.len() >= 4 && plan_all("M", &sets, &settings).iter().all(|f| f.enabled));
    settings.selection.select_none(&all);
    assert!(plan("M", &sets, &settings).is_empty());
    assert_eq!(plan_all("M", &sets, &settings).len(), all.len(), "unchecked maps stay listed");
    settings.selection.set_on("Body", "Roughness", true);
    settings.selection.set_on("Body", "Normal", true);
    let dir = tmp("selected");
    let written = export(&mesh, "M", &sets, &settings, &dir).unwrap();
    assert_eq!(files_in(&dir), ["M_Body_Normal.png", "M_Body_Roughness.png"]);
    assert_eq!(written.len(), 2);
    assert_eq!(archive(&mesh, "M", &sets, &settings).unwrap().files, 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn packed_maps_are_written_only_when_selected() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let sets = plain(&ch);
    let mut settings = ExportSettings { template: ExportTemplate::unreal(), ..Default::default() };
    let all = all_keys(&sets, &settings);
    assert!(all.iter().any(|(_, m)| m == "OcclusionRoughnessMetallic"));
    settings.selection.select_none(&all);
    let dir = tmp("packed-off");
    assert!(export(&mesh, "M", &sets, &settings, &dir).unwrap().is_empty());
    settings.selection.set_on("Body", "OcclusionRoughnessMetallic", true);
    export(&mesh, "M", &sets, &settings, &dir).unwrap();
    assert_eq!(files_in(&dir), ["M_Body_OcclusionRoughnessMetallic.png"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn select_all_none_invert_and_per_set_rules() {
    let all = keys(&["A", "B"], &["Color", "Rough", "Normal"]);
    let mut sel = MapSelection::default();
    sel.select_none(&all);
    assert!(all.iter().all(|(s, m)| !sel.is_on(s, m)));
    sel.set_on("A", "Color", true);
    sel.invert(&all);
    assert!(!sel.is_on("A", "Color") && sel.is_on("A", "Rough") && sel.is_on("B", "Color"));
    sel.set_set_on("B", &all, false);
    assert!(!sel.is_on("B", "Normal") && sel.is_on("A", "Normal"));
    sel.select_all();
    assert!(all.iter().all(|(s, m)| sel.is_on(s, m)));
    let stored = MapSelection::from_stored(&["A/Color".to_string()], None);
    assert!(!stored.is_on("A", "Color") && stored.is_on("A", "Rough") && stored.stored_off() == ["A/Color"]);
}

#[test]
fn alt_click_solo_keeps_one_map_and_restores_the_previous_selection() {
    let all = keys(&["A", "B"], &["Color", "Rough"]);
    let mut sel = MapSelection::default();
    sel.set_on("A", "Rough", false);
    sel.set_on("B", "Color", false);
    let before = sel.clone();
    sel.solo("B", "Rough", &all);
    let on: Vec<_> = all.iter().filter(|(s, m)| sel.is_on(s, m)).collect();
    assert_eq!(on, [&("B".to_string(), "Rough".to_string())]);
    assert!(sel.is_solo());
    // another map while soloed moves the solo but keeps the original selection to come back to
    sel.solo("A", "Color", &all);
    assert!(sel.is_on("A", "Color") && !sel.is_on("B", "Rough"));
    sel.solo("A", "Color", &all);
    assert_eq!(sel, before, "restored exactly");
    assert!(!sel.is_solo());
    // a manual change drops the remembered selection
    sel.solo("A", "Color", &all);
    sel.set_on("B", "Color", true);
    assert!(!sel.is_solo());
}

#[test]
fn normal_format_override_flips_the_green_channel() {
    let mesh = primitives::plane(2.0, 1);
    let mut ch = channels();
    ch.insert(ChannelKind::Normal, TiledImage::new(RES, RES, 4, [0.5, 0.75, 1.0, 1.0]));
    ch.remove(&ChannelKind::Height);
    let sets = plain(&ch);
    let mut settings = ExportSettings { height_to_normal: 0.0, padding: Padding::None, ..Default::default() };
    let mut green = |directx: Option<bool>| {
        settings.normal_directx = directx;
        let dir = tmp("normal-format");
        export(&mesh, "M", &sets, &settings, &dir).unwrap();
        let g = px(&dir.join("M_Body_Normal.png"), 3, 3)[1];
        std::fs::remove_dir_all(dir).unwrap();
        g
    };
    let (default, gl, dx) = (green(None), green(Some(false)), green(Some(true)));
    assert!((default - gl).abs() < 1e-3 && (gl - 0.75).abs() < 0.01, "{default} {gl}");
    assert!((dx - 0.25).abs() < 0.01, "{dx}");
}

fn tiles(ch: &BTreeMap<ChannelKind, TiledImage>) -> Vec<ExportTextureSet<'_>> {
    vec![
        ExportTextureSet { name: "Body.1001", material_slot: 0, udim: Some(1001), channels: ch, mesh_maps: None },
        ExportTextureSet { name: "Body.1002", material_slot: 1, udim: Some(1002), channels: ch, mesh_maps: None },
    ]
}

#[test]
fn udim_sets_get_a_tile_suffix_or_one_folder_per_tile() {
    let mesh = primitives::sample_crate();
    let ch = channels();
    let sets = tiles(&ch);
    let mut settings = ExportSettings::default();
    let all = all_keys(&sets, &settings);
    settings.selection.select_none(&all);
    settings.selection.set_on("Body.1001", "BaseColor", true);
    settings.selection.set_on("Body.1002", "BaseColor", true);
    let dir = tmp("udim-flat");
    export(&mesh, "Prop", &sets, &settings, &dir).unwrap();
    assert_eq!(files_in(&dir), ["Prop_Body_BaseColor.1001.png", "Prop_Body_BaseColor.1002.png"]);
    std::fs::remove_dir_all(&dir).unwrap();
    settings.udim_folders = true;
    let dir = tmp("udim-folders");
    export(&mesh, "Prop", &sets, &settings, &dir).unwrap();
    assert_eq!(files_in(&dir), ["1001/Prop_Body_BaseColor.png", "1002/Prop_Body_BaseColor.png"]);
    assert_eq!(plan("Prop", &sets, &settings).len(), 2);
    let zip = archive(&mesh, "Prop", &sets, &settings).unwrap();
    let z = zip::ZipArchive::new(std::io::Cursor::new(zip.bytes)).unwrap();
    let mut names: Vec<_> = z.file_names().filter_map(|n| n.ok().map(|n| n.to_string())).collect();
    names.sort();
    assert_eq!(names, ["1001/Prop_Body_BaseColor.png", "1002/Prop_Body_BaseColor.png"]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plain_sets_ignore_the_udim_folder_option() {
    let ch = channels();
    let sets = plain(&ch);
    let settings = ExportSettings { udim_folders: true, ..Default::default() };
    assert!(plan("M", &sets, &settings).iter().all(|(_, f)| !f.contains('/') && f.starts_with("M_Body_")));
}
