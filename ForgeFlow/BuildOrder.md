---

## 📑 BuildOrder.md

```markdown
# Build Order for Forge.Rust.LSP

## Phase 1: Grammar + Parser

- Define ForgeFlow DSL grammar (PEG/LALRPOP)
- Implement parser → AST

## Phase 2: LSP Scaffolding

- Setup `tower-lsp` or `lsp-server` crate
- Implement initialize, shutdown, textDocument handlers

## Phase 3: Editor Features

- Diagnostics (syntax errors)
- Semantic tokens (highlighting)
- Hover docs
- Autocompletion

## Phase 4: Runtime Integration

- Map AST → Forge runtime actions
- Safe execution model (declarative workflows)

## Phase 5: File Extension Support

- `.forge` binary container
- `.fdgn` graph/workflow files
- `.fmeta` metadata/config (plaintext + optional encrypted fields)
- `.fwrk` workflow chain DSL

## Phase 6: Packaging

- Publish crate
- VSCode extension integration
- Documentation + examples
```
