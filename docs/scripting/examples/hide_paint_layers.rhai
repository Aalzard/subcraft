// Hide every top-level paint layer, to look at the fills and smart materials alone.
// Run it again after an undo (Ctrl+Z puts everything back in one step).
let all = tool("layers_list");
let hidden = 0;
for ts in all.texture_sets {
    for l in ts.layers {
        if l.type == "paint" && l.visible {
            tool("layer_update", #{ layer: l.id, texture_set: ts.texture_set, visible: false });
            hidden += 1;
        }
    }
}
print(`${hidden} paint layer(s) hidden`);
