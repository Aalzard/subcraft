// Fill every texture set with the pack's hazard yellow paint (a fill layer at the top).
let info = tool("project_info");
for ts in info.texture_sets {
    tool("layer_add", #{
        type: "fill", name: "Hazard Yellow", texture_set: ts.name,
        fill: #{ base_color: "#e6b400", metallic: 0.0, roughness: 0.45 },
    });
    print(`painted ${ts.name}`);
}
