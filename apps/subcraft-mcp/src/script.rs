//! SubCraft scripts: small Rhai programs (<https://rhai.rs>) that drive the engine through the
//! same tool catalog the MCP server offers, so one documented API serves scripts, plugins and AI
//! agents. A script calls `tool("layer_add", #{ type: "fill", name: "Dirt" })` and gets the
//! tool's JSON result back as a map; a failing tool throws, which a script can `try`/`catch`.
//!
//! Runs headless (`subcraft-mcp --script file.rhai [project]`) or inside the desktop app on the
//! open project (Plugins menu). A whole run is one undo step.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rhai::{Dynamic, Engine, EvalAltResult, Map};
use serde_json::Value;
use subcraft_app::Session;

use crate::server::Server;

/// Tools a plugin running inside the desktop app may not call: they replace the open project or
/// start another program, which the app does through its own menus.
pub const APP_BLOCKED: &[&str] = &["project_new", "project_open", "open_in_app"];

/// The file extension of scripts and plugins.
pub const EXTENSION: &str = "rhai";

/// Longest a script may run before it is stopped (an endless loop must not hang the app).
pub const TIME_LIMIT: Duration = Duration::from_secs(300);

/// What a script run printed and how it ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// `print` / `debug` lines, in order.
    pub output: Vec<String>,
    /// Tool calls made, in order.
    pub calls: Vec<String>,
    /// Why the script stopped early, with its line when known.
    pub error: Option<String>,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }

    /// One line for a status bar.
    pub fn summary(&self) -> String {
        match &self.error {
            Some(e) => format!("Script failed: {e}"),
            None => format!("Script finished ({} tool call{})", self.calls.len(), if self.calls.len() == 1 { "" } else { "s" }),
        }
    }
}

struct Run {
    server: Server,
    outcome: Outcome,
    blocked: &'static [&'static str],
}

/// Run `source` on `server`. `blocked` names tools the script may not call.
pub fn run(server: Server, source: &str, blocked: &'static [&'static str]) -> (Server, Outcome) {
    let pushes = server.session.as_ref().map(|s| s.history().pushes());
    let state = Rc::new(RefCell::new(Run { server, outcome: Outcome::default(), blocked }));
    let error = {
        let engine = engine(&state);
        engine.run(source).err().map(|e| describe(&e))
    };
    let Ok(cell) = Rc::try_unwrap(state) else { unreachable!("the engine, the only other owner, is dropped") };
    let mut run = cell.into_inner();
    run.outcome.error = error;
    // The whole run is one undo step (each tool call already is one).
    if let (Some(s), Some(before)) = (run.server.session.as_mut(), pushes) {
        let n = s.history().pushes().saturating_sub(before);
        s.squash_history(usize::try_from(n).unwrap_or(usize::MAX));
    }
    (run.server, run.outcome)
}

/// Run `source` on an open project (the desktop app's Plugins menu): the session goes into a
/// server for the run and comes back out, edited.
pub fn run_on_session(session: Session, source: &str) -> (Session, Outcome) {
    let mut server = Server::new();
    server.session = Some(session);
    let (mut server, outcome) = run(server, source, APP_BLOCKED);
    let session = server.session.take().expect("blocked tools cannot close the project");
    (session, outcome)
}

fn engine(state: &Rc<RefCell<Run>>) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_expr_depths(128, 64);
    engine.set_max_call_levels(64);
    engine.set_max_string_size(16 << 20);
    engine.set_max_array_size(1 << 20);
    engine.set_max_map_size(1 << 16);
    let started = Instant::now();
    engine.on_progress(move |_| (started.elapsed() > TIME_LIMIT).then(|| Dynamic::from(format!("stopped after {} s", TIME_LIMIT.as_secs()))));
    let out = Rc::clone(state);
    engine.on_print(move |text| out.borrow_mut().outcome.output.push(text.to_string()));
    let out = Rc::clone(state);
    engine.on_debug(move |text, _, pos| out.borrow_mut().outcome.output.push(format!("[debug {pos}] {text}")));
    let s = Rc::clone(state);
    engine.register_fn("tool", move |name: &str, args: Map| call(&s, name, args));
    let s = Rc::clone(state);
    engine.register_fn("tool", move |name: &str| call(&s, name, Map::new()));
    engine.register_fn("tools", || -> rhai::Array { crate::tools::definitions().iter().filter_map(|d| d["name"].as_str().map(|n| Dynamic::from(n.to_string()))).collect() });
    engine
}

fn call(state: &Rc<RefCell<Run>>, name: &str, args: Map) -> Result<Dynamic, Box<EvalAltResult>> {
    let mut run = state.borrow_mut();
    if run.blocked.contains(&name) {
        return Err(format!("`{name}` is not available to plugins inside the app: use the File menu").into());
    }
    if !crate::tools::definitions().iter().any(|d| d["name"] == Value::from(name)) {
        return Err(format!("unknown tool `{name}` (tools() lists them)").into());
    }
    let args: Value = rhai::serde::from_dynamic(&Dynamic::from_map(args))?;
    run.outcome.calls.push(name.to_string());
    let result = run.server.call(name, args);
    drop(run);
    let text = result["content"].as_array().and_then(|c| c.iter().find_map(|item| item["text"].as_str())).unwrap_or_default().to_string();
    if result["isError"].as_bool() == Some(true) {
        return Err(format!("{name}: {text}").into());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => rhai::serde::to_dynamic(v),
        Err(_) => Ok(Dynamic::from(text)),
    }
}

/// The error message with its line, without Rhai's internal wrapping.
fn describe(e: &EvalAltResult) -> String {
    let pos = e.position();
    let msg = match e {
        EvalAltResult::ErrorRuntime(v, _) => v.to_string(),
        EvalAltResult::ErrorTerminated(v, _) => v.to_string(),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => return describe(inner),
        other => other.to_string(),
    };
    if pos.is_none() || msg.contains("(line ") { msg } else { format!("{msg} (line {})", pos.line().unwrap_or(0)) }
}

/// `subcraft-mcp --script <file> [project]`: run headless, print the script's output, return the
/// process exit code (0 = finished, 1 = the script failed, 2 = bad arguments).
pub fn run_cli(args: &[String]) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("usage: subcraft-mcp --script <file.rhai> [project.subcraft]");
        return 2;
    };
    let source = match std::fs::read_to_string(crate::paths::input(file)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {file}: {e}");
            return 2;
        }
    };
    let mut server = Server::new();
    if let Some(project) = args.get(1) {
        let opened = server.call("project_open", serde_json::json!({ "path": project }));
        if opened["isError"].as_bool() == Some(true) {
            eprintln!("cannot open {project}: {}", opened["content"][0]["text"].as_str().unwrap_or_default());
            return 2;
        }
    }
    let (_, outcome) = run(server, &source, &[]);
    for line in &outcome.output {
        println!("{line}");
    }
    match outcome.error {
        Some(e) => {
            eprintln!("script failed: {e}");
            1
        }
        None => 0,
    }
}
