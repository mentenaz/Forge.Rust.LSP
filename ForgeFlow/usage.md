# ForgeFlow Language Server — Usage & Implementation

`forge-lsp-forgeflow` is the [ForgeFlow DSL](./SPEC.md) language server: a
hand-rolled Rust binary speaking LSP 3.16 over stdio (the same `Content-Length`
framing the Forge editor client uses). It is **not** a proxy wrapper — unlike
most packages in this repo, the language intelligence (parser, diagnostics,
highlighting) is implemented in Rust here (`ForgeFlow/src/main.rs`).

It covers `BuildOrder.md` phases 1–3 and 5:

| Phase | Status | What it does |
| --- | --- | --- |
| 1 — Grammar + Parser | ✅ | Lexer + recursive-descent parser for `flow`/`step`/`param` |
| 2 — LSP Scaffolding | ✅ | `initialize`/`shutdown`/`didOpen`/`didChange` over stdio |
| 3 — Editor Features | ✅ | Diagnostics, semantic tokens, hover docs, completion |
| 4 — Runtime Integration | ⏸️ | Deferred — this server never executes workflows |
| 5 — File Extensions | ✅ | `.fwrk`/`.fdgn`/`.fmeta`/`.forge` handling |
| 6 — Packaging | ⚠️ | Published via this repo's registry/CI (no VSCode extension yet) |

---

## 1. How to use it (as a Forge user)

1. Open any ForgeFlow file in Forge:
   - `.fwrk` — workflow chain DSL (primary target)
   - `.fdgn` — graph/workflow file (JSON *or* DSL)
   - `.fmeta` — project metadata (JSON; flags `ENC:` secret fields)
   - `.forge` — binary project container (surfaced as an info note — open the
     `.fwrk`/`.fdgn`/`.fmeta` *sources* instead)
2. Forge auto-discovers the server through the remote registry
   (`forge-registry.json`) and downloads `forge-lsp-forgeflow-<platform>.zip`
   on first open. No app update is required.
3. You get:
   - **Diagnostics** — red squiggles on syntax errors (e.g. missing `)`),
     amber warnings on `ENC:` secret fields in `.fmeta`.
   - **Hover** — built-in action docs (e.g. hovering `http` shows its params).
   - **Completion** — `flow`/`step` keywords and built-in action names.
   - **Semantic highlighting** — keywords, flow/step declarations, params, and
     literals are tokenized so the theme can color them (see §4).

> **Known limitation:** the Forge editor does not yet *render* LSP semantic
> tokens, so the highlighting above is parsed correctly by the server but will
> appear as plain text until the app-side rendering lands (see §5).

### Example `.fwrk`

```text
flow Build {
  step http(method = "GET", url = "https://api.forge.dev/v1/build")
  step delay(ms = 100)
  step transform(expr = "$.status")
}
```

---

## 2. How it is implemented

### Package layout

```text
ForgeFlow/
  Cargo.toml            # crate `forge-lsp-forgeflow`, binary of the same name
  forge-extension.toml  # id=forgeflow, language_id=forgeflow, exts fwrk/fdgn/fmeta/forge
  src/main.rs            # entire server (framing + lexer + parser + LSP handlers)
  README.md  SPEC.md  BuildOrder.md  usage.md
```

Only one runtime dependency: `serde_json`. No external LSP crate — framing is
~40 lines mirroring `Json/src/main.rs`.

### Pipeline

1. **Framing** (`read_message`/`write_message`) — read `Content-Length`-framed
   JSON-RPC; every response is flushed immediately (stdout is line-buffered when
   piped, so LSP frames must be flushed per message).
2. **Lexer** (`lex`) — turns source into tokens with 0-based `(line, col)`,
   recording lexer errors.
3. **Parser** (`parse`) — recursive descent over the grammar
   `flow = "flow" id "{" step* "}"`,
   `step = "step" id "(" param_list? ")"`,
   `param = id "=" value`, `value = STRING|NUMBER|BOOLEAN`.
   Produces `(diagnostics, semantic_tokens)`.
4. **Dispatch by extension** (`analyze`) — picks DSL vs JSON strategy.
5. **LSP handlers** — `publishDiagnostics`, completion, hover,
   `semanticTokens/full`.

### Capabilities advertised

```json
{
  "textDocumentSync": 1,
  "hoverProvider": true,
  "completionProvider": { "triggerCharacters": ["{", "(", ",", "="] },
  "semanticTokensProvider": { "legend": { "tokenTypes": ["keyword","function","title","property","string","number","boolean"], "tokenModifiers": [] }, "full": true }
}
```

---

## 3. Extending the server

### Add a built-in action (hover + completion)

Edit `builtin_actions()` in `src/main.rs`:

```rust
("deploy", "Publishes the build artifact. Params: `target`, `channel`."),
```

That single entry feeds hover docs, completion, and (optionally) future
validation.

### Add a grammar rule

Extend the lexer (token kinds in `Kind`) and the `parse` function. Semantic
tokens are emitted by role (`Role::{Keyword,Action,FlowName,Param,Str,Num,Bool}`)
— add a variant + a legend entry if you introduce a new visual category.

### Publishing a change

```sh
cargo build --release -p forge-lsp-forgeflow   # verify it compiles
cargo run --release -p forge-registry-gen       # regen index (run from repo root)
git add -A && git commit -m "forgeflow: ..."
git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z   # CI builds all 6 platforms
```

> Bump `version` in **both** `Cargo.toml` and `forge-extension.toml` whenever the
> binary behavior changes — it is the download-cache key on user machines.

---

## 4. Theming / semantic token legend

Token-type names are chosen to match the **Forge theme's `syntax` keys** so the
editor can color them directly with **no theme change**:

| ForgeFlow element | Token type | Theme `syntax` key | Color |
| --- | --- | --- | --- |
| `flow`, `step` | `keyword` | `syntax.keyword` | `#c4b5fd` bold |
| Flow declaration name (e.g. `Build`) | `title` | `syntax.title` | `#c4b5fd` bold |
| Step action name (e.g. `http`) | `function` | `syntax.function` | `#d8b4fe` |
| Parameter name (e.g. `method`) | `property` | `syntax.property` | `#9333ea` |
| `"..."` | `string` | `syntax.string` | `#a3e635` |
| `100` | `number` | `syntax.number` | `#fbbf24` |
| `true`/`false` | `boolean` | `syntax.boolean` | `#fb923c` |

The encode scheme is the standard LSP relative-position delta
(`[deltaLine, deltaChar, length, tokenType, 0]` per token).

---

## 5. Wiring it into the Forge app (consumer side)

Everything below is about `E:\Forge_GPUI` (`src/backend/lsp/`).

### Already wired — no code change needed

- **Language detection** is registry-driven. `registry.rs::language_id_for_path`
  looks up the extension in the merged registry. Because
  `forge-registry.json` lists `forgeflow` with
  `file_extensions: ["fwrk","fdgn","fmeta","forge"]`, opening any of those
  files resolves to language id `forgeflow` automatically once the app has
  refreshed its remote registry (fetched from `mentenaz/Forge.Rust.LSP`
  releases).
- **Server launch + download** works through the existing path:
  `server_manager::start_server("forgeflow", root_uri, None)` reads the spec,
  downloads/caches the binary keyed by `version`, spawns it, and performs the
  `initialize` → `initialized` handshake (`server_manager.rs`).
- **Theme**: as shown in §4, no theme edit is required.

### Missing — must be implemented for highlighting to show

The app currently does **not** request or render LSP semantic tokens. To make
ForgeFlow (and tsgo, etc.) colorize, add a step after `didOpen`:

```rust
// in editor_panel.rs, after the document is opened against a started server:
let tokens = server
    .request("textDocument/semanticTokens/full", json!({
        "textDocument": { "uri": uri }
    }))
    .await?;
// `tokens.result.data` is the flat [dl, dc, len, type, mods]* array.
// Map each `type` index -> legend type name -> theme `syntax` color and
// paint ranges over the buffer.
```

Concretely:

1. The `initialize` result already advertises `semanticTokensProvider`, so the
   client knows the server supports it.
2. After `didOpen`, issue `textDocument/semanticTokens/full`.
3. Resolve each token's `type` index against the server's `legend.tokenTypes`
   (the names in §4) and look up `theme.syntax[<name>].color`
   (`forge_ui::ActiveTheme`).
4. Apply the color to the buffer range `[line, char] … [line, char+len]`.

Until that lands, ForgeFlow diagnostics/hover/completion work, but the text is
uncolored.

### Smoke-test the server directly

```sh
printf 'Content-Length: 75\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' |
  target/release/forge-lsp-forgeflow
```

Expect a `Content-Length`-framed `capabilities` reply. A bare JSON line (no
`Content-Length` header) makes the server exit silently.
