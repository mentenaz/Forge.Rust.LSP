//! Forge TypeScript language server proxy.
//!
//! Wraps the native TypeScript 7 language server (typescript-go). The release
//! assets are npm-style `.tgz` archives extracting to `package/lib/tsc(.exe)`
//! plus the standard-library declarations; launched in LSP mode. No PATH
//! probe on purpose — a `tsc` on PATH is almost always npm's TS6 compiler,
//! which has no LSP mode.

use forge_lsp_proxy::EngineSpec;

/// Pinned typescript-go release. Bump together with `version` in
/// `Cargo.toml` and `forge-extension.toml`. The tag contains a slash;
/// GitHub resolves it fine in plain download URLs.
const TAG: &str = "typescript/v7.0.2";

fn asset_for(platform: &str) -> Option<&'static str> {
    match platform {
        "windows-x86_64" => Some("typescript-win32-x64.tgz"),
        "windows-aarch64" => Some("typescript-win32-arm64.tgz"),
        "macos-x86_64" => Some("typescript-darwin-x64.tgz"),
        "macos-aarch64" => Some("typescript-darwin-arm64.tgz"),
        "linux-x86_64" => Some("typescript-linux-x64.tgz"),
        "linux-aarch64" => Some("typescript-linux-arm64.tgz"),
        _ => None,
    }
}

fn main() {
    let spec = EngineSpec {
        lang: "typescript",
        repo: "microsoft/typescript-go",
        tag: TAG,
        platform_asset: asset_for,
        binary_base: "tsc",
        path_candidates: &[],
        engine_args: &["--lsp", "--stdio"],
        cwd_engine_root: false,
    };
    std::process::exit(forge_lsp_proxy::run(&spec));
}
