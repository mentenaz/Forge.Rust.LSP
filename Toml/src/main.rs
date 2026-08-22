//! Forge TOML language server proxy.
//!
//! Wraps taplo. A locally installed binary wins; otherwise a pinned release is
//! downloaded (gzip'd single-file asset on every platform). taplo serves LSP
//! via `taplo lsp stdio`.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned taplo release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`.
const TAG: &str = "0.10.0";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("taplo-windows-x86_64.gz"),
        "windows-aarch64" => Some("taplo-windows-aarch64.gz"),
        "macos-x86_64" => Some("taplo-darwin-x86_64.gz"),
        "macos-aarch64" => Some("taplo-darwin-aarch64.gz"),
        "linux-x86_64" => Some("taplo-linux-x86_64.gz"),
        "linux-aarch64" => Some("taplo-linux-aarch64.gz"),
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "toml",
        repo: "tamasfe/taplo",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "taplo",
        path_candidates: &[PathCandidate {
            program: "taplo",
            args: &["lsp", "stdio"],
        }],
        engine_args: &["lsp", "stdio"],
        cwd_engine_root: false,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
