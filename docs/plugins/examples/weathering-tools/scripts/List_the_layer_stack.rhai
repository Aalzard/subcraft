// Print every texture set's layer stack (top first) to the log.
fn show(layers, depth) {
    for l in layers {
        let pad = "";
        for i in 0..depth { pad += "  "; }
        let hidden = if l.visible { "" } else { " (hidden)" };
        print(`${pad}${l.name} [${l.type}]${hidden}`);
        if "children" in l { show(l.children, depth + 1); }
    }
}
let all = tool("layers_list");
let count = 0;
for ts in all.texture_sets {
    print(`${ts.texture_set}:`);
    show(ts.layers, 1);
    count += ts.layers.len();
}
print(`${count} top-level layers`);
