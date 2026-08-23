//! Forge Go language server proxy.
//!
//! Wraps gopls. The Go project publishes no prebuilt release binaries —
//! gopls is installed via `go install` into the user's toolchain, so this
//! proxy is PATH-only and prints install instructions when it is missing.

use forge_lsp_proxy::{spawn_and_pump, which};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some(path) = which("gopls") {
        return spawn_and_pump(&path, &[], None);
    }

    eprintln!(
        "forge-lsp-go: gopls not found on PATH.\n\
         Install it with: go install golang.org/x/tools/gopls@latest\n\
         (requires a Go toolchain; https://go.dev/dl/)"
    );
    1
}
