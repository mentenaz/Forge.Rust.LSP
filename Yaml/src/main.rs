//! Forge YAML language server proxy.
//!
//! yaml-language-server is a Node program with no standalone upstream
//! binaries, so this proxy resolves an already-installed server first, then
//! falls back to installing the pinned npm package into the engine cache.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use forge_lsp_proxy::{configure_no_window, engine_cache_dir, spawn_and_pump, which};

/// Pinned yaml-language-server npm version.
const PACKAGE: &str = "yaml-language-server@1.24.0";

const ENTRY: &str = "yaml-language-server/bin/yaml-language-server";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some(path) = which("yaml-language-server") {
        eprintln!("forge-lsp-yaml: using yaml-language-server from PATH");
        return spawn_and_pump(&path, &["--stdio"], None);
    }

    if install() {
        let entry = node_modules().join(ENTRY);
        if entry.exists() {
            return spawn_via_node(&entry, "--stdio");
        }
    }

    eprintln!(
        "forge-lsp-yaml: no YAML language server available. Tried:\n\
         - yaml-language-server on PATH\n\
         - npm install of {PACKAGE} (needs Node.js on PATH)\n\n\
         Install manually with: npm install -g yaml-language-server"
    );
    1
}

fn node_modules() -> PathBuf {
    engine_cache_dir("yaml").join("npm").join("node_modules")
}

fn install() -> bool {
    let Some(npm) = which("npm") else {
        return false;
    };
    let dir = engine_cache_dir("yaml").join("npm");
    if dir.join("node_modules/yaml-language-server/package.json").exists() {
        return true;
    }

    eprintln!("forge-lsp-yaml: installing {PACKAGE} via npm...");
    let mut cmd = Command::new(npm);
    cmd.args(["install", "--prefix"])
        .arg(&dir)
        .arg(PACKAGE)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_no_window(&mut cmd);

    matches!(cmd.status(), Ok(status) if status.success())
}

fn spawn_via_node(entry: &std::path::Path, arg: &'static str) -> i32 {
    let Some(node) = which("node") else {
        eprintln!("forge-lsp-yaml: node not found on PATH");
        return 1;
    };
    spawn_and_pump(&node, &[&entry.to_string_lossy(), arg], None)
}
