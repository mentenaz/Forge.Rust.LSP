//! Forge Bash language server proxy.
//!
//! bash-language-server is a Node program with no standalone upstream
//! binaries, so this proxy resolves an already-installed server first, then
//! falls back to installing the pinned npm package into the engine cache.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use forge_lsp_proxy::{configure_no_window, engine_cache_dir, spawn_and_pump, which};

/// Pinned bash-language-server npm version.
const PACKAGE: &str = "bash-language-server@5.6.0";

const CANDIDATES: &[(&str, &[&str])] = &[("bash-language-server", &["start"])];

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some((program, args)) = find_installed() {
        return spawn_and_pump(&program, args, None);
    }

    if install() {
        let entry = node_modules().join("bash-language-server/out/cli.js");
        if entry.exists() {
            return spawn_via_node(&entry, &["start"]);
        }
    }

    eprintln!(
        "forge-lsp-bash: no Bash language server available. Tried:\n\
         - bash-language-server on PATH\n\
         - npm install of {PACKAGE} (needs Node.js on PATH)\n\n\
         Install manually with: npm install -g bash-language-server"
    );
    1
}

fn find_installed() -> Option<(std::path::PathBuf, &'static [&'static str])> {
    for (program, args) in CANDIDATES {
        if let Some(path) = which(program) {
            eprintln!("forge-lsp-bash: using {program} from PATH");
            return Some((path, args));
        }
    }
    None
}

fn node_modules() -> PathBuf {
    engine_cache_dir("bash").join("npm").join("node_modules")
}

fn install() -> bool {
    let Some(npm) = which("npm") else {
        return false;
    };
    let dir = engine_cache_dir("bash").join("npm");
    if dir.join("node_modules/bash-language-server/package.json").exists() {
        return true;
    }

    eprintln!("forge-lsp-bash: installing {PACKAGE} via npm...");
    let mut cmd = Command::new(npm);
    cmd.args(["install", "--prefix"])
        .arg(&dir)
        .arg(PACKAGE)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_no_window(&mut cmd);

    matches!(cmd.status(), Ok(status) if status.success())
}

fn spawn_via_node(entry: &std::path::Path, args: &[&str]) -> i32 {
    let Some(node) = which("node") else {
        eprintln!("forge-lsp-bash: node not found on PATH");
        return 1;
    };
    spawn_and_pump(&node, &[&entry.to_string_lossy(), args[0]], None)
}
