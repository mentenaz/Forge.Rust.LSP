//! Forge Python language server proxy.
//!
//! basedpyright/pyright are Node programs with no standalone upstream binaries,
//! so this proxy resolves an already-installed server first, then falls back to
//! auto-installing basedpyright through npm — mirroring how the C# proxy
//! bootstraps the roslyn dotnet tool.

use std::process::{Command, Stdio};

use forge_lsp_proxy::{configure_no_window, spawn_and_pump, which};

/// `(program, args)` probes in preference order. pylsp and jedi-language-server
/// speak stdio by default; the pyright-family servers need `--stdio`.
const CANDIDATES: &[(&str, &[&str])] = &[
    ("basedpyright-langserver", &["--stdio"]),
    ("pyright-langserver", &["--stdio"]),
    ("pylsp", &[]),
    ("jedi-language-server", &[]),
];

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some((program, args)) = find_installed() {
        return spawn_and_pump(&program, args, None);
    }

    if install_basedpyright() {
        if let Some(path) = which("basedpyright-langserver") {
            return spawn_and_pump(&path, &["--stdio"], None);
        }
    }

    eprintln!(
        "forge-lsp-python: no Python language server available. Tried:\n\
         - basedpyright / pyright on PATH\n\
         - pylsp (python-lsp-server) on PATH\n\
         - jedi-language-server on PATH\n\n\
         Install manually with: npm install -g basedpyright"
    );
    1
}

fn find_installed() -> Option<(std::path::PathBuf, &'static [&'static str])> {
    for (program, args) in CANDIDATES {
        if let Some(path) = which(program) {
            eprintln!("forge-lsp-python: using {program} from PATH");
            return Some((path, args));
        }
    }
    None
}

fn install_basedpyright() -> bool {
    let Some(npm) = which("npm") else {
        return false;
    };

    eprintln!("forge-lsp-python: installing basedpyright via npm...");
    let mut cmd = Command::new(npm);
    cmd.args(["install", "-g", "basedpyright"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_no_window(&mut cmd);

    matches!(cmd.status(), Ok(status) if status.success())
}
