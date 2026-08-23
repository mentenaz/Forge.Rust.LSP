//! Forge HTML language server proxy.
//!
//! Wraps vscode-html-language-server (from vscode-langservers-extracted).
//! Resolves an already-installed server first, then falls back to installing
//! the pinned npm package into the engine cache.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use forge_lsp_proxy::{configure_no_window, engine_cache_dir, spawn_and_pump, which};

/// Pinned vscode-langservers-extracted npm version.
const PACKAGE: &str = "vscode-langservers-extracted@4.10.0";

const SERVER: &str = "vscode-html-language-server";
const ENTRY: &str = "vscode-langservers-extracted/bin/vscode-html-language-server";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some(path) = which(SERVER) {
        eprintln!("forge-lsp-html: using {SERVER} from PATH");
        return spawn_and_pump(&path, &["--stdio"], None);
    }

    if install() {
        let entry = node_modules().join(ENTRY);
        if entry.exists() {
            return spawn_via_node(&entry);
        }
    }

    eprintln!(
        "forge-lsp-html: no HTML language server available. Tried:\n\
         - {SERVER} on PATH\n\
         - npm install of {PACKAGE} (needs Node.js on PATH)\n\n\
         Install manually with: npm install -g vscode-langservers-extracted"
    );
    1
}

fn node_modules() -> PathBuf {
    engine_cache_dir("html").join("npm").join("node_modules")
}

fn install() -> bool {
    let Some(npm) = which("npm") else {
        return false;
    };
    let dir = engine_cache_dir("html").join("npm");
    if dir.join(format!("node_modules/{SERVER}/package.json")).exists() {
        return true;
    }

    eprintln!("forge-lsp-html: installing {PACKAGE} via npm...");
    let mut cmd = Command::new(npm);
    cmd.args(["install", "--prefix"])
        .arg(&dir)
        .arg(PACKAGE)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_no_window(&mut cmd);

    matches!(cmd.status(), Ok(status) if status.success())
}

fn spawn_via_node(entry: &std::path::Path) -> i32 {
    let Some(node) = which("node") else {
        eprintln!("forge-lsp-html: node not found on PATH");
        return 1;
    };
    spawn_and_pump(&node, &[&entry.to_string_lossy(), "--stdio"], None)
}
