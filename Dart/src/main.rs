//! Forge Dart language server proxy.
//!
//! The Dart analyzer's LSP server ships inside the Dart/Flutter SDK
//! (`dart language-server`), so there is nothing to download — this proxy is
//! PATH-only and points at the SDK installer when dart is missing.

use forge_lsp_proxy::{spawn_and_pump, which};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    if let Some(path) = which("dart") {
        return spawn_and_pump(&path, &["language-server"], None);
    }

    eprintln!(
        "forge-lsp-dart: dart not found on PATH.\n\
         Install the Dart SDK (https://dart.dev/get-dart) or the Flutter SDK\n\
         (https://docs.flutter.dev/get-started/install) and put `dart` on PATH."
    );
    1
}
