//! Forge SQL language server proxy.
//!
//! Wraps sqls. Upstream only publishes x86_64 builds (no ARM assets), so
//! unsupported platforms fall through to the standard manual-install error.

use forge_lsp_proxy::{EngineSpec, PathCandidate};

/// Pinned sqls release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`.
const TAG: &str = "v0.2.48";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("sqls-windows-0.2.48.zip"),
        "macos-x86_64" => Some("sqls-darwin-0.2.48.zip"),
        "linux-x86_64" => Some("sqls-linux-0.2.48.zip"),
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "sql",
        repo: "sqls-server/sqls",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "sqls",
        path_candidates: &[PathCandidate { program: "sqls", args: &[] }],
        engine_args: &[],
        cwd_engine_root: false,
        asset_url: None,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
