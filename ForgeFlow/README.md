# ForgeFlow Language Server

ForgeFlow DSL support for Forge: real parser diagnostics, semantic highlighting,
hover docs, and completion for `.fwrk` / `.fdgn` / `.fmeta` / `.forge` workflow
files.

This is a **hand-rolled Rust LSP** (not a proxy wrapper): the grammar, parser,
and editor features live entirely in `src/main.rs`. It speaks LSP 3.16 over
stdio with `Content-Length` framing — the mirror image of Forge's LSP client.

## Files

- `src/main.rs` — the server (framing, lexer, parser, LSP handlers)
- `SPEC.md` — the language spec (grammar, file extensions, `.fmeta` schema)
- `BuildOrder.md` — the phased build plan (phases 1–3, 5 implemented here)
- `usage.md` — **how to use it and how to implement / wire it up**
- `forge-extension.toml` — package metadata consumed by the registry

## Features

- Syntax diagnostics for the `flow`/`step`/`param` DSL (`.fwrk`, `.fdgn`)
- JSON validation for `.fmeta`/`.fdgn`, with warnings on `ENC:` secret fields
- Hover docs for built-in actions, completion for keywords + actions
- Semantic tokens mapped to the Forge theme's `syntax` keys (see `usage.md` §4)
- `.forge` binary containers are surfaced as an informational note

## Build & test

```sh
cargo build --release -p forge-lsp-forgeflow
cargo run --release -p forge-registry-gen        # refresh the registry index
```

Forge picks the package up automatically via `forge-registry.json` — no app
update required. See [`usage.md`](./usage.md) for full usage, extension points,
and consumer-wiring details.
