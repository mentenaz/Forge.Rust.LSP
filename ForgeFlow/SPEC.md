# ForgeFlow Language Server Specification

## Overview

ForgeFlow is a domain-specific language designed to describe workflows, node graphs, and backend task chains. The ForgeFlow Language Server provides editor support via the Language Server Protocol (LSP).

## Goals

- Syntax parsing for `.fdgn` and `.forge` files
- Semantic highlighting of ForgeFlow constructs (flows, steps, params)
- Autocompletion for actions and parameters
- Diagnostics for invalid syntax or unsupported actions
- Hover documentation for built-in actions
- Execution model integration with Forge runtime

## Grammar (PEG-style)

flow = "flow" identifier "{" step* "}"
step = "step" identifier "(" param_list? ")"
param_list = param ("," param)*
param = identifier "=" value
identifier = ASCII_ALPHA+
value = STRING | NUMBER | BOOLEAN

## File Extensions

| Extension | Purpose                                               | Format                                          | Security                      |
| --------- | ----------------------------------------------------- | ----------------------------------------------- | ----------------------------- |
| `.forge`  | Project container (binary archive with graphs/assets) | Binary (header + chunks)                        | Optional encryption per chunk |
| `.fdgn`   | Graph/workflow file                                   | JSON or binary schema                           | Plain (debug‑friendly)        |
| `.fmeta`  | Metadata/config                                       | JSON (plaintext) with optional encrypted fields | Encrypt only secrets          |
| `.fwrk`   | Workflow chain                                        | ForgeFlow DSL text                              | Plain (safe declarative)      |

## `.fmeta` Schema

Example plaintext JSON with optional encrypted fields:

```json
{
  "projectName": "Mentenaz Forge",
  "version": "0.1.0",
  "author": "Francois Huyzers",
  "lastModified": "2026-08-28T08:44:00",
  "properties": {
    "theme": "dark",
    "layout": "grid"
  },
  "secrets": {
    "dbPassword": "ENC:base64:aGVsbG9Xb3JsZA=="
  }
}
```
