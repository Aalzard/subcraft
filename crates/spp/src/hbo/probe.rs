//! Count procedural / mask HBO nodes in a decoded `document.bin` tree.

use std::collections::BTreeSet;

use super::decode::{Value, UNode};

#[derive(Debug, Clone, Default)]
pub struct HboFeatureCounts {
    pub procedural_nodes: usize,
    pub mask_or_filter_nodes: usize,
    pub distinct_types: Vec<String>,
}

pub fn count_hbo_features(document_bin: &[u8]) -> Result<HboFeatureCounts, String> {
    let (root, _, _) = super::decode::decode(document_bin)?;
    let mut types = BTreeSet::new();
    let mut procedural = 0usize;
    let mut mask_filter = 0usize;
    walk_node(&root, &mut types, &mut procedural, &mut mask_filter);
    Ok(HboFeatureCounts {
        procedural_nodes: procedural,
        mask_or_filter_nodes: mask_filter,
        distinct_types: types.into_iter().collect(),
    })
}

fn walk_node(
    node: &UNode,
    types: &mut BTreeSet<String>,
    procedural: &mut usize,
    mask_filter: &mut usize,
) {
    if is_procedural_type(&node.type_name) {
        *procedural += 1;
        types.insert(node.type_name.clone());
    } else if is_mask_or_filter_type(&node.type_name) {
        *mask_filter += 1;
        types.insert(node.type_name.clone());
    }
    for (_, val) in &node.fields {
        walk_value(val, types, procedural, mask_filter);
    }
}

fn walk_value(
    val: &Value,
    types: &mut BTreeSet<String>,
    procedural: &mut usize,
    mask_filter: &mut usize,
) {
    match val {
        Value::Object(Some(node)) => walk_node(node, types, procedural, mask_filter),
        Value::Array(items) => {
            for el in items {
                walk_value(el, types, procedural, mask_filter);
            }
        }
        _ => {}
    }
}

fn is_procedural_type(name: &str) -> bool {
    name.contains("Procedural")
        || name.contains("Generator")
        || name == "DataGridDeformation"
        || name == "DataLevels"
}

fn is_mask_or_filter_type(name: &str) -> bool {
    name.contains("Mask")
        || name.starts_with("DataActionFilter")
        || name == "DataBlending"
}

/// Layer/mask graph is structurally decodable (not yet mapped 1:1 to SubCraft).
pub fn generators_decodable(counts: &HboFeatureCounts) -> bool {
    counts.procedural_nodes > 0 || counts.mask_or_filter_nodes > 0
}
