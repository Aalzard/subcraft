//! `subcraft-mcp`: a Model Context Protocol server over stdio that drives the SubCraft engine
//! headlessly (no window). Each request is one JSON-RPC message per line on stdin; responses go
//! to stdout. Nothing else may write to stdout, so diagnostics go to stderr.
//!
//! The server holds one open project (a [`subcraft_app::Session`]). Every edit goes through the
//! same undoable commands the desktop app uses, and the saved `.subcraft` file opens in the app.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod describe;
mod paths;
mod params;
mod render;
mod server;
mod tools;

const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "SubCraft is a 3D texture painting app (Substance Painter style). This server runs its engine \
headlessly on one open project. Typical flow: project_new (from a model) or project_open -> bake_mesh_maps (needed by \
smart materials, edge/cavity masks) -> add layers / apply shelf materials / masks / paint -> render_preview to look at the \
result -> project_save -> export_textures. Layers are listed top first. Layer and texture-set arguments accept the id or \
the exact name. Colors for base_color/emissive accept '#rrggbb' or [r,g,b] in sRGB 0..1. Paths may be Windows paths \
(D:\\\\models\\\\a.fbx). Every edit is undoable (undo / redo).";

/// Write one message to stdout (the only writer: replies and progress notifications).
fn send(msg: &Value) -> bool {
    let text = serde_json::to_string(msg).unwrap_or_else(|_| "{}".into());
    let mut out = std::io::stdout().lock();
    writeln!(out, "{text}").and_then(|_| out.flush()).is_ok()
}

/// MCP progress notifications for the running tool call, when the client asked for them
/// (`params._meta.progressToken`).
pub mod progress {
    use serde_json::{Value, json};
    use std::sync::Mutex;

    static TOKEN: Mutex<Option<Value>> = Mutex::new(None);

    pub fn set(token: Option<Value>) {
        *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = token;
    }

    /// `progress` out of `total` (e.g. 0.42 of 1.0), with a short status message.
    pub fn report(progress: f64, total: f64, message: &str) {
        let token = TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(token) = token {
            super::send(&json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": { "progressToken": token, "progress": progress, "total": total, "message": message },
            }));
        }
    }
}

fn main() {
    let mut server = server::Server::new();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.into_iter().filter_map(|m| handle(&mut server, m)).collect();
                (!out.is_empty()).then_some(Value::Array(out))
            }
            Ok(msg) => handle(&mut server, msg),
            Err(e) => Some(error_reply(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply
            && !send(&reply) {
                break;
            }
    }
}

/// Answer one JSON-RPC message; `None` for notifications and responses.
fn handle(server: &mut server::Server, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let id = id?; // notifications (initialized, cancelled, ...) need no reply
    if method.is_empty() {
        return None; // a response to something we never send
    }
    let result = match method.as_str() {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSIONS[1]);
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(PROTOCOL_VERSIONS[1]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "subcraft", "title": "SubCraft", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            progress::set(params.get("_meta").and_then(|m| m.get("progressToken")).cloned());
            let result = server.call(&name, args);
            progress::set(None);
            Ok(result)
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error_reply(id, code, &message),
    })
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
