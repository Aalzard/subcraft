//! Scripts: Rhai programs calling the tool catalog (headless and on an app's open session).

use serde_json::json;
use subcraft_app::Session;

use crate::script::{self, APP_BLOCKED};
use crate::server::Server;

fn sample() -> Server {
    let mut server = Server::new();
    let r = server.call("project_new", json!({ "model_path": "sample", "resolution": 256 }));
    assert_eq!(r["isError"], json!(false), "{r}");
    server
}

fn layer_names(session: &Session) -> Vec<String> {
    session.selected_texture_set().layers.iter().rev().map(|l| l.name.clone()).collect()
}

#[test]
fn a_script_adds_layers_reads_results_and_counts_as_one_undo_step() {
    let server = sample();
    let before = layer_names(server.session.as_ref().unwrap());
    let source = r##"
        let info = tool("project_info");
        let ts = info.texture_sets[0].name;
        print(`texture set: ${ts}`);
        for i in 1..=3 {
            tool("layer_add", #{ type: "fill", name: `Grime ${i}`, fill: #{ base_color: "#402010", roughness: 0.8 } });
        }
        let layers = tool("layers_list", #{ texture_set: ts });
        print(`layers: ${layers.layers.len()}`);
    "##;
    let (mut server, outcome) = script::run(server, source, &[]);
    assert!(outcome.ok(), "{:?}", outcome.error);
    assert_eq!(outcome.calls.len(), 5);
    assert!(outcome.output[0].starts_with("texture set: "), "{:?}", outcome.output);
    assert_eq!(outcome.output[1], format!("layers: {}", before.len() + 3));
    let s = server.session.as_mut().unwrap();
    assert_eq!(layer_names(s)[..3], ["Grime 3", "Grime 2", "Grime 1"], "top first");
    // One undo removes everything the script did.
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert_eq!(layer_names(s), before);
}

#[test]
fn tool_errors_throw_and_can_be_caught_and_uncaught_ones_stop_with_the_line() {
    let server = sample();
    let source = r#"
        let msg = "";
        try { tool("layer_update", #{ layer: "No such layer", opacity: 0.5 }); } catch (e) { msg = e; }
        print(msg);
        tool("layer_add", #{ type: "paint", name: "Kept" });
        tool("no_such_tool");
        print("never");
    "#;
    let (server, outcome) = script::run(server, source, &[]);
    assert!(outcome.output[0].contains("layer_update"), "{:?}", outcome.output);
    let err = outcome.error.expect("the unknown tool stops the script");
    assert!(err.contains("unknown tool `no_such_tool`") && err.contains("line 6"), "{err}");
    assert!(!outcome.output.iter().any(|l| l == "never"));
    assert_eq!(layer_names(server.session.as_ref().unwrap())[0], "Kept", "work before the error stays");
}

#[test]
fn plugins_in_the_app_edit_the_open_session_and_cannot_replace_it() {
    let session = Session::new(subcraft_mesh::primitives::cube(1.0), 64);
    let ts = session.selected_texture_set().id;
    let (session, outcome) = script::run_on_session(session, r#"tool("layer_add", #{ type: "folder", name: "From plugin" });"#);
    assert!(outcome.ok(), "{:?}", outcome.error);
    assert_eq!(session.selected_texture_set().id, ts, "the same project");
    assert_eq!(layer_names(&session)[0], "From plugin");
    for tool in APP_BLOCKED {
        let (_, outcome) = script::run_on_session(Session::new(subcraft_mesh::primitives::cube(1.0), 64), &format!(r#"tool("{tool}", #{{ model_path: "sample" }});"#));
        assert!(outcome.error.as_deref().is_some_and(|e| e.contains("not available to plugins")), "{tool}: {:?}", outcome.error);
    }
}

#[test]
fn an_endless_loop_is_limited_and_syntax_errors_are_reported() {
    let (_, outcome) = script::run(Server::new(), "let x = ; ", &[]);
    assert!(outcome.error.unwrap().contains("line 1"));
    // The time limit is long for real use; the operation hook is what stops it, so check that
    // an expensive loop finishes or reports instead of hanging forever (bounded here by depth).
    let (_, outcome) = script::run(Server::new(), "fn f(n) { f(n + 1) } f(0);", &[]);
    assert!(outcome.error.is_some(), "runaway recursion is stopped");
    assert_eq!(script::run(Server::new(), "print(tools().len() >= 49);", &[]).1.output, ["true"]);
}

#[test]
fn the_bundled_example_plugins_run_on_a_project() {
    for (name, source) in subcraft_ui::plugins::EXAMPLES {
        let server = sample();
        let (_, outcome) = script::run(server, source, APP_BLOCKED);
        assert!(outcome.ok(), "{name}: {:?}", outcome.error);
        assert!(!outcome.output.is_empty(), "{name} reports what it did");
    }
    let (server, outcome) = script::run(sample(), subcraft_ui::plugins::EXAMPLES[0].1, APP_BLOCKED);
    let s = server.session.as_ref().unwrap();
    assert!(s.document().texture_sets.iter().all(|ts| ts.layers.last().is_some_and(|l| l.name == "Grime" && l.mask.is_some())), "{:?}", outcome.output);
}
