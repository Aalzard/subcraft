//! `subcraft-mcp`: the MCP server over stdio (default), or `subcraft-mcp --script file.rhai
//! [project.subcraft]` to run a SubCraft script headlessly and exit (batch jobs, CI).

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--script") => std::process::exit(subcraft_mcp::script::run_cli(&args[1..])),
        Some("--help" | "-h") => {
            println!("subcraft-mcp            MCP server on stdin/stdout\nsubcraft-mcp --script <file.rhai> [project.subcraft]   run a script and exit");
        }
        _ => subcraft_mcp::run_stdio(),
    }
}
