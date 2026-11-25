// Add a dark, rough grime layer rising from the bottom of the model to every texture set.
// Works without baked mesh maps (the mask uses the position on the model).
let info = tool("project_info");
for ts in info.texture_sets {
    let name = ts.name;
    let layer = tool("layer_add", #{
        type: "fill", name: "Ground Grime", texture_set: name,
        fill: #{ base_color: "#3a2e22", roughness: 0.9 },
    });
    tool("mask_add_effect", #{
        layer: layer.added.id, texture_set: name,
        effect: #{ type: "generator", kind: "position_gradient", axis: "y", level: 0.25, softness: 0.3, invert: true },
    });
    print(`grime added to ${name}`);
}
