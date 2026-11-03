//! Serves one library over MCP without the app, for trying a real MCP client.
//!
//! `FALCON_SANDBOX=<dir> cargo run -p falcon-server --example mcp_serve -- <dir>/Copy.library [--writes] [--token=<token>]`
//!
//! Like the app, it opens only libraries inside the sandbox folder, so give it
//! a throwaway copy. It takes a free port, prints the client configuration and
//! runs until killed.

use std::sync::{Arc, Mutex};

use falcon_library::{Library, sandbox};
use falcon_server::mcp::{self, Options};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: mcp_serve <library> [--writes] [--token=<token>]");
        std::process::exit(2);
    };
    sandbox::enforce(sandbox::default_root());
    let lib = Library::open(path).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let token = args.iter().find_map(|a| a.strip_prefix("--token=")).map(String::from);
    let options = Options { allow_writes: args.iter().any(|a| a == "--writes"), token: token.clone() };
    let server = mcp::start(Arc::new(Mutex::new(lib)), [0], options, || {}).expect("could not start the server");
    println!("{}", mcp::client_config(server.port, token.as_deref()));
    loop {
        std::thread::park();
    }
}
