// Bake curvature, then add a worn bare-metal layer on the edges of every texture set.
// The bake runs on the processor and can take a minute on a big model.
print("baking curvature...");
tool("bake_mesh_maps", #{ maps: ["curvature"] });
let info = tool("project_info");
for ts in info.texture_sets {
    let name = ts.name;
    let layer = tool("layer_add", #{
        type: "fill", name: "Edge Wear", texture_set: name,
        fill: #{ base_color: "#9a9894", metallic: 1.0, roughness: 0.35 },
    });
    tool("mask_add_effect", #{
        layer: layer.added.id, texture_set: name,
        effect: #{ type: "generator", kind: "edges", level: 0.6, softness: 0.15, noise_amount: 0.5 },
    });
    print(`edge wear added to ${name}`);
}
