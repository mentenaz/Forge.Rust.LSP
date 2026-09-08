# ForgeFlow examples

A graded set of ForgeFlow files, simple to extreme, each one verified
against the actual parser in `ForgeFlow/src/grammar.rs` (not just against
`SPEC.md`'s prose — see the caveat at the bottom). Every file here is
structurally clean: opening it in an editor wired to `forge-lsp-forgeflow`
should produce zero diagnostics.

Read them in order:

| # | File | Introduces |
|---|------|------------|
| 01 | `01_minimal.fwrk` | The smallest valid file: one `flow`, one `gee` |
| 02 | `02_variables_and_operators.fwrk` | `laat`, reassignment, arithmetic/comparison/logical operators |
| 03 | `03_control_flow.fwrk` | `as`/`anders`, `terwyl`, early `gee`, calling one flow from another |
| 04 | `04_types_and_objects.fwrk` | `soort` alias + interface, `?` nullable, `lys<T>`/`objk<K,V>` generics, `leeg` |
| 05 | `05_lists_and_iteration.fwrk` | `.elk` map form and loop form |
| 06 | `06_step_actions.fwrk` | `step` — engine actions, always `name = value` args |
| 07 | `07_parallel_fork_tak.fwrk` | `tak { been ... }` — named parallel branches |
| 08 | `08_comments_and_literals.fwrk` | Comment styles, number-literal edge cases, `true`/`false`/`niks` |
| 09 | `09_imports/` | `gbk` single-name and `gbk { a, b }` list imports across real files |
| 10 | `10_fdgn_basic.fdgn` | Minimal `.fdgn` graph: two `node`s, one `edge` |
| 11 | `11_fmeta_secrets.fmeta` | `.fmeta` JSON validation + the `ENC:` secret-field lint |
| 12 | `12_fdgn_fork_fanout/` | `.fdgn`'s `tak` fork *point* + fan-out/fan-in via plain `edge`s, plus imports (including a `Custom`/`nuwe.fwrk` node exercising the real import-resolution diagnostics below) |
| 13 | `13_kitchen_sink_advanced/` | Everything in `.fwrk` at once: types, control flow, both `.elk` forms, `tak`/`been`, `step`, imports |
| 14 | `14_fdgn_kitchen_sink_advanced/` | Everything in `.fdgn` at once: a `tak` fan-out into three normalized pipelines merging back into one |

Folders (09, 12, 13, 14) hold multiple files because `gbk` imports resolve
against real paths on disk — a single-file snippet can't demonstrate an
import cleanly.

**Verification method**: every file here was opened against a real,
freshly-built `forge-lsp-forgeflow` (`cargo build --release -p
forge-lsp-forgeflow`) over actual `initialize`/`didOpen` JSON-RPC traffic,
and its `publishDiagnostics` response checked — not just eyeballed against
`SPEC.md`. All of them come back with zero diagnostics except
`11_fmeta_secrets.fmeta`, whose one severity-2 diagnostic on the `ENC:`
field is the example's entire point.

## One convention every example follows

**Every call uses `name = value` arguments — always, with no positional
form.** This applies not just to `step` and to `Type { field = value }`
constructors, but to plain flow-to-flow calls too: `groet(naam = "World")`,
not `groet("World")`. This was confirmed by reading the parser directly
(`parse_primary`'s identifier-call branch in `grammar.rs`): it is the exact
same code path for every kind of call, and it unconditionally requires an
argument name before each `=`. `SPEC.md` §10 says as much for `.fdgn`
action calls ("one calling convention for the whole language") but doesn't
spell it out for plain `.fwrk` flow calls — and the existing
`ForgeFlow/examples.md` doc actually gets this wrong in a few places
(e.g. its `Math.add(2, 3)` and `groet("World")`), which would produce an
`expected argument name` diagnostic against the real server today.

## A caveat about identifiers

`SPEC.md` §1 documents identifiers as `[A-Za-z][A-Za-z0-9_]*` (underscore
allowed). The real lexer's identifier-continuation check
(`src/grammar.rs`, `lex()`) only accepts `is_ascii_alphanumeric()` — no
`_` — so an underscore anywhere in an identifier is an "Unexpected
character" lex error today. All identifiers in these examples are
camelCase for this reason; `04_types_and_objects.fwrk` calls this out
directly since it originally used `log_instellings` and the verification
run below caught it immediately.

## A caveat about what's actually enforced today

**Update**: filesystem-backed `gbk` import validation (SPEC.md §2) and
`textDocument/definition` (go-to-definition, single-hop across a `gbk`
import) landed in `server.rs`/`semantic.rs`/`grammar.rs` after this folder
was first written. Verified directly: an import naming a nonexistent path
now gets a severity-1 diagnostic on the path string, and an imported name
the target file doesn't declare gets one on the name — both confirmed
against the real binary, not just read off the diff. Example 12's `Custom`
node/`nuwe.fwrk` import exercises exactly this.

One piece of documented cross-checking is still not in the binary these
examples were verified against:

- **`.fdgn` `edge`/`tak` endpoint validation** (SPEC.md §10) — the real
  `parse_edge` in `src/grammar.rs` still says endpoint names are "not
  cross-referenced against declared nodes." The cross-referencing code
  exists only in `ForgeFlow/grammar.rs` (repo root), a stale duplicate left
  over from before the `main.rs`/`grammar.rs`/`semantic.rs`/`server.rs`
  crate split — `main.rs` only ever compiles `src/grammar.rs`, so that copy
  isn't part of the build.

Don't expect a dangling `edge` (an endpoint naming no declared `node`/`tak`)
to currently produce a diagnostic — it won't. `ForgeFlow/lsp_tests.rs`,
which exercises this (and, before the update above, import validation
too), also isn't in a `tests/` directory, so `cargo test -p
forge-lsp-forgeflow` doesn't run it (verified: 0 tests execute) — it isn't
catching this drift.
