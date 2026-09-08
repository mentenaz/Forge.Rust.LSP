# ForgeFlow LSP — Integration Guide for Forge (GPUI)

How `forge-lsp-forgeflow` actually behaves, verified against the real
binary and the live source — not inferred from memory of old docs. This is
a reconstruction: the original version of this guide (written 28 Aug) was
lost, along with its companion `forgeflow-lsp-architecture-reference.md`.
Everything below has been re-verified against the current source rather
than recalled, and §0 explains exactly what's changed since the original.

---

## 0. What's changed since the original guide (28 Aug → now)

Read this section first — it's the difference between this document and
the one that was lost, and it matters for anything you're about to wire up.

- **The crate split.** The original guide described one file,
  `src/main.rs`, holding framing + lexer + parser + every LSP handler. That
  file now only wires three components together:
  `src/grammar.rs` (lexer, parser, token model — pure, no I/O),
  `src/semantic.rs` (completions, hover, document highlight — pure, depends
  only on `grammar`), `src/server.rs` (LSP transport, and the crate's only
  filesystem access). Any reference you find to "everything is in
  `main.rs`" — in old docs, in your own memory of this — is stale.
- **`tak`/`been` shipped, but not the design you may be remembering.** The
  28 Aug session designed a considerably larger grammar delta: `tak` with
  optional `lewer` result-binding, `probeer`/`vang` (try/catch),
  `huidige` (an engine-injected `.fout` context namespace), `saam`
  (explicit `.fdgn` fan-in join), `gaan` (fire-and-forget branch modifier).
  That delta was fully resolved on paper (`SPEC-delta-error-handling.md`)
  but **never merged into the live grammar** — as of 28 Aug's own
  architecture reference, only the `&&`/`||` precedence fix had actually
  landed. What shipped just now is a fresh, simpler `tak`/`been`: named
  parallel branches, run in `.fwrk` as a real control-flow statement, run
  in `.fdgn` as a fork-point node reusing plain `edge` for branches — no
  `lewer`, no `probeer`/`vang`, no `huidige`, no `saam`/`gaan`. **This is an
  open decision, not a resolved one**: fold the error-handling delta in as
  a future version, or treat it as superseded by the simpler design. Worth
  deciding deliberately rather than by default.
- **New since 28 Aug:** `textDocument/documentHighlight` (§6), filesystem-
  backed import validation (§8), and `.fdgn` edge/`tak` cross-reference
  validation (§9) — none of these existed in the original guide. The
  cross-reference gap in particular was flagged in the 28 Aug session
  itself as "the biggest real gap, ranked above runtime integration" — it
  took a while, but it's closed now.
- **Two small correctness fixes, also new tonight:** `node`/`edge` are no
  longer globally reserved keywords — they were blocking a `.fwrk` author
  from ever naming a variable `node`, exactly as flagged (and left
  unaddressed) in the 28 Aug session; now they're ordinary identifiers
  outside `.fdgn`. And top-level completion in both file kinds no longer
  offers keywords that aren't actually valid there (`.fwrk` was offering
  `laat`/`anders`; `.fdgn` was offering `anders` and missing `edge`
  entirely).
- **A real test suite now exists** (`tests/lsp_tests.rs`) — spawns the
  actual compiled binary and drives it over real stdio JSON-RPC, covering
  everything above. This replaces the ad-hoc manual verification scripts
  the 28 Aug session flagged the *absence* of a test suite as the biggest
  long-term risk for a hand-rolled parser; `cargo test` now catches
  regressions in this instead of relying on someone re-running one-off
  scripts by hand.
- **Protocol fundamentals are unchanged and re-verified**: framing,
  severity codes, the semantic token legend and encoding, and the `.fdgn`
  dual-format detection all still behave exactly as the original guide
  described. Those sections below are re-confirmed against the current
  source, not just carried over from memory.

---

## 1. What this server is

A single Rust binary (`forge-lsp-forgeflow`) speaking LSP 3.16 over stdio.
No external LSP crate — framing (`server.rs`), lexing/parsing
(`grammar.rs`), and completions/hover/highlight (`semantic.rs`) are all
hand-rolled. It never executes ForgeFlow code — it's a pure editor aid
(diagnostics, hover, completion, semantic tokens, document highlight, and
import validation) for `.fwrk`/`.fdgn`/`.fmeta`/`.forge` files. Phase 4
(runtime integration — actually running a `.fwrk` workflow) remains
deferred; nothing in this server maps ForgeFlow to executable engine
actions.

---

## 2. Transport: `Content-Length` framing

Every message in both directions is:

```
Content-Length: <byte length of the JSON body>\r\n
\r\n
<JSON body, exactly <byte length> bytes>
```

The server reads headers line-by-line until a blank line, parses
`Content-Length`, then reads exactly that many bytes as the body. It
writes responses the same way, flushing after every message — this
matters because stdout is line-buffered when piped, so a client that
doesn't flush per-message will appear to hang. A client sending a bare
JSON line with no `Content-Length` header makes the server exit silently
(no error message) rather than attempting to recover.

### Capabilities advertised (`initialize` response)

```json
{
  "textDocumentSync": 1,
  "hoverProvider": true,
  "documentHighlightProvider": true,
  "completionProvider": { "triggerCharacters": ["{", "(", ",", "=", ":", "."] },
  "semanticTokensProvider": {
    "legend": {
      "tokenTypes": ["keyword", "control", "type", "function", "variable", "property", "operator", "string", "number", "boolean", "comment"],
      "tokenModifiers": []
    },
    "full": true
  },
  "serverInfo": { "name": "forge-lsp-forgeflow", "version": "<current>" }
}
```

`textDocumentSync: 1` means full-document sync — the client resends the
entire file text on every change, not incremental diffs.

---

## 3. Diagnostics (`textDocument/publishDiagnostics`)

Sent after every `didOpen`/`didChange`. Shape, per diagnostic:

```json
{
  "range": { "start": { "line": N, "character": N }, "end": { "line": N, "character": N } },
  "severity": 1,
  "source": "forgeflow",
  "message": "..."
}
```

`severity` values in actual use: `1` = Error (syntax errors, unresolvable
import paths), `2` = Warning (`ENC:` secret fields in `.fmeta`, imported
names not found in an otherwise-readable target file), `3` = Information
(the `.forge` binary-container note — see §10's file-kind table). There is
no caching: every request that needs diagnostics re-lexes and re-parses
the current document text from scratch. Nothing is memoized between
`didChange` events and the next request that needs the result.

---

## 4. Completion (`textDocument/completion`)

Context-aware, not a flat keyword dump — the response depends on exactly
where the cursor is:

- Directly inside a `tak { }` block → only `been` (the one thing valid
  there).
- Statement position inside a `flow`/`as`/`anders`/`terwyl`/`been` body →
  `step`/`laat`/`as`/`terwyl`/`tak`/`gee`/`elk`, plus in-scope
  variables/parameters/flow names.
- True top level → `gbk`/`soort`/`flow`.
- `.fdgn` top level → `gbk`/`node`/`edge`/`tak`.
- Inside a `.fdgn` node/`tak` field block → field names (`title`/`action`/
  `pos`).

Each completion item is `{ "label": ..., "kind": <number>, "detail": ...
}`. `kind` follows the standard LSP `CompletionItemKind` enum; the values
actually emitted are `3` (Function — flows, call targets), `6` (Variable —
variables, parameters, node names), `7` (Class — types/interfaces), `10`
(Property — node/`tak` field names), `12` (Value — the `waar`/`onwaar`/
`niks` literals), `14` (Keyword). No other kind values are used.

---

## 5. Hover (`textDocument/hover`)

`{ "contents": { "kind": "markdown", "value": "<text>" } }`, or `null` if
the cursor isn't over anything hoverable. **No `range` field is ever
included** in the response — this is legal per the LSP spec (range is
optional), but it means the client controls what visually highlights on
hover, since the server gives it nothing to go on. This is directly
relevant to §6.

---

## 6. Document highlight (`textDocument/documentHighlight`) — new since 28 Aug

`[{ "range": { start, end } }, ...]` — every range that should be
highlighted together with whatever's under the cursor. Two matching
strategies, chosen automatically based on what's under the cursor:

- **Whole-file, by name**: `flow`/`soort`/`node`/`tak` declarations,
  `soort` fields, `gbk` imports, call/edge-reference sites.
- **Scoped to the enclosing `flow`**: parameters, `laat` variables, `.elk`
  loop variables — bounded to that flow's own token range (its `flow`
  keyword through the matching closing `}`), so two different flows each
  declaring e.g. `laat x` never cross-highlight.

**Why this exists**: without it, a client with its own generic "highlight
everything sharing the same semantic token type" fallback (Zed has this)
will use that instead — and since node names and edge references all share
the `function` token type, hovering any one node lights up every node in
the file. That's a real behavior observed in testing, not a hypothetical.
If Forge's own editor has (or grows) a similar client-side fallback,
wiring this request is what fixes it — same as §7 semantic tokens, this is
implemented and correct server-side today but does nothing until the
client actually requests it.

---

## 7. Semantic tokens (`textDocument/semanticTokens/full`)

Response: `{ "data": [...] }`, a flat `u32` array, 5 integers per token:

```
[deltaLine, deltaChar, length, tokenTypeIndex, tokenModifiers]
```

Standard LSP relative-position delta encoding — `deltaLine`/`deltaChar`
are relative to the *previous* token's start, not absolute (reset to
absolute `col` when `deltaLine != 0`). `tokenModifiers` is always `0`.
`tokenTypeIndex` indexes into the legend from §2:

```
0=keyword  1=control  2=type  3=function  4=variable
5=property 6=operator 7=string 8=number 9=boolean 10=comment
```

**Index `10` (`comment`) is correctly emitted** — `lex()` tokenizes both
line (`//`) and block (`/* */`) comments as `Role::Comment` semantic
tokens (kept separate from the main token stream so the parser never sees
them), and `server.rs`'s `semanticTokens/full` handler merges them back in
with the parser's own tokens before encoding. Verified directly: a file
starting with `// a real comment` returns `[0, 0, 17, 10, 0, ...]` as its
first token — length 17 matches the comment text exactly. (An earlier
version of this note, carried over from the original 28 Aug guide without
re-checking it, claimed this was dead/unreachable — that was wrong as of
the current source and has been corrected here.)

**This is the concrete remaining integration work, still true as of
tonight**: the server computes and returns this data correctly, but
nothing on the Forge side currently requests it after `didOpen` or paints
the returned ranges. Concrete steps: after opening a document against a
started `forgeflow` server, issue `textDocument/semanticTokens/full`, walk
the flat array five values at a time reconstructing absolute
`(line, char, length)` from the deltas, map each `tokenTypeIndex` to its
legend name and then to `theme.syntax[<name>].color`, and paint that
range. Until wired, ForgeFlow files render with no syntax coloring —
diagnostics/hover/completion/highlight all still work.

**Theme mapping gap**: colors are confirmed for `keyword`, `function`,
`property`, `string`, `number`, `boolean` (see `usage.md` §4). No sourced
theme color exists yet for `control`, `type`, `variable`, `operator`, or
`comment` — assign these before relying on the full legend being colored.

---

## 8. Import validation — new since 28 Aug

Not part of the original guide at all. `gbk <name> vannaf "<path>"` (and
its `{ }`-list form) is now validated against the real filesystem — one of
two deliberate exceptions to this server's otherwise "structure only"
philosophy. The other is `.fdgn` edge/`tak` cross-reference checking (§9)
— the gap flagged back in the 28 Aug session as "the biggest real gap,
ranked above runtime integration" has since been closed; both this and §9
are now handled, and nothing else in the grammar performs cross-referencing.

Behavior: the path is resolved relative to the *current file's own
directory* (not the workspace root). An unresolvable path is a severity-1
diagnostic on the path string itself, with the real OS error text
included. A resolvable path whose target file doesn't declare the
imported name as a `flow` or `soort` is a severity-2 diagnostic on the
name. If the target file itself has syntax errors, whatever top-level
names lex cleanly are still used for the check (a deliberate choice — a
broken import target doesn't cascade into every name from it being
reported missing).

---

## 9. `.fdgn` edge/`tak` cross-reference validation — new since 28 Aug

The other cross-reference exception alongside §8, and the one specifically
flagged in the 28 Aug session as the priority gap. `edge <source> ->
<target>` now requires both `<source>` and `<target>` to be a declared
`node` or `tak` name somewhere in the same file — an edge referencing an
undeclared name is a severity-1 error on that name.

Entirely in-process, no filesystem access needed (unlike §8): both the
declared names and the edge references live in the same token stream the
parser already has. The check runs as a post-pass after the whole file
parses, specifically so declaration order doesn't matter — `edge a -> b`
is valid even if `node b` is declared later in the file. `.fwrk` has no
equivalent check (it has no `edge` construct), and this is unrelated to
`tak`/`been` validation in `.fwrk`, which only checks that a `tak` block
has at least one `been` branch, not that branches reference anything.

---

## 10. File-kind dispatch

Extension is read as the substring after the last `.` in the URI,
lowercased. Dispatch:

- **`.fwrk`** → lexed and parsed as the full ForgeFlow grammar (types,
  variables, control flow, `tak`/`been`, `step`).
- **`.fdgn`** → **dual-format.** The server first tries to parse the raw
  text as JSON; if it succeeds *and* the top-level value is a JSON object,
  it's treated as JSON (same validation path as `.fmeta`, including the
  `ENC:` lint). Only if that JSON parse fails does it fall through to
  being lexed/parsed as the ForgeFlow graph DSL (`node`/`edge`/`tak`/
  imports). **This is the specific mechanism that matters for Forge's
  Designer panel**: whichever format the Designer actually writes to disk
  when a user drags nodes around needs to match one of these two paths.
  This was flagged as untested against real Designer output back in
  28 Aug (only tested against clean hand-written examples) — worth
  confirming it's actually been exercised against real Designer-panel
  writes before relying on round-tripping between hand-edited DSL text and
  Designer-written JSON.
- **`.fmeta`** → always JSON, plus the `ENC:` warning lint (severity 2).
- **`.forge`** → never parsed as text. Always exactly one severity-3
  diagnostic pointing at line 0, telling the user to open the text sources
  instead. No hover, no completion, no coloring, no highlight.

---

## 11. Wiring checklist for the GPUI app side

Already wired (per `usage.md` §5 — language detection via the registry,
server launch/download, diagnostics/hover/completion/import-validation all
work with zero additional client code):

- [x] Language detection (registry-driven, `.fwrk`/`.fdgn`/`.fmeta`/`.forge`)
- [x] Server launch + lazy download via `forge-registry.json`
- [x] Diagnostics, hover, completion — request/response shapes unchanged
      by everything added tonight, so no client code needed for these

Not yet wired — both fully correct server-side, both need a client-side
request added (§6 and §7 above have the concrete steps for each):

- [ ] `textDocument/semanticTokens/full` after `didOpen` → syntax coloring
- [ ] `textDocument/documentHighlight` on cursor move/click → correct
      scoped highlighting (fixes the "everything shares a color, so
      everything highlights together" fallback behavior if the app has
      one)

Open design decision, not a wiring task:

- [ ] Whether the `probeer`/`vang`/`huidige`/`saam`/`gaan`/`lewer` grammar
      delta from 28 Aug gets built in a future version, or is treated as
      superseded by the simpler `tak`/`been` that shipped instead (§0)
