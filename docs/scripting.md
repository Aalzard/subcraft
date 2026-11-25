# Scripting SubCraft

SubCraft scripts are small [Rhai](https://rhai.rs/book/) programs. They drive the engine through
the same tools the MCP server offers to AI agents, so one API serves scripts, plugins and agents.

## Running scripts

* **In the app:** put `.rhai` files in the plugins folder (`Documents/SubCraft/Plugins`, or the folder
  named by the `SUBCRAFT_PLUGINS` environment variable). They appear in the **Plugins** menu and run
  on the open project. **Plugins > Run script file...** runs any file, and **Plugins > Examples** has
  a few to start from. To share scripts together with assets, make a plugin: see
  [Plugins](plugins.md). A whole run is one undo step. `print` output goes to the log, and the last
  line also to the status bar.
* **Headless (batch jobs):** `subcraft-mcp --script my_script.rhai [project.subcraft]`. The exit code is 0
  when the script finished and 1 when it failed. `print` output goes to stdout.

## The API

| Function | What it does |
| --- | --- |
| `tool(name, #{ ... })` | Runs one tool with named arguments and returns its result as a map. |
| `tool(name)` | Runs a tool that needs no arguments, such as `project_info`. |
| `tools()` | The names of every tool. |
| `print(x)` | Writes a line to the output. |

A failing tool throws an error. Catch it with `try { ... } catch (e) { ... }`. Otherwise the script
stops and reports the line. The tool names, arguments and results are the MCP server's: see
`apps/subcraft-mcp` and the tool descriptions returned by `tools/list`. Layers and texture sets are
named by id or by exact name.

Inside the app, `project_new`, `project_open` and `open_in_app` are blocked. Use the File menu for
those. A script that runs longer than 5 minutes is stopped.

## Example

```rust
// Add a dark, rough grime layer rising from the bottom of every texture set.
let info = tool("project_info");
for ts in info.texture_sets {
    let layer = tool("layer_add", #{
        type: "fill", name: "Grime", texture_set: ts.name,
        fill: #{ base_color: "#3a2e22", roughness: 0.9 },
    });
    tool("mask_add_effect", #{
        layer: layer.added.id, texture_set: ts.name,
        effect: #{ type: "generator", kind: "position_gradient", axis: "y", level: 0.25, invert: true },
    });
    print(`grime added to ${ts.name}`);
}
```

More in [`docs/scripting/examples`](scripting/examples).
