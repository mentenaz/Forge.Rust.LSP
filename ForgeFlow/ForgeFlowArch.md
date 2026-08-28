# ForgeFlow LSP — Architecture & Extension Reference

How `forge-lsp-forgeflow` is actually built internally, and how to extend
it. Companion to `forgeflow-lsp-integration-guide.md` (which covers wiring
this server into Forge as a consumer) — this document is about the server's
own code, for whoever is modifying `main.rs` itself.

Everything below reflects the current state of the file as of the `&&`/`||`
precedence fix (the most recent change). It already incorporates the
earlier constructor-literal / condition-block ambiguity fix (`no_brace_ctor`,
see §4) and the detailed keyword/type/action hover-doc tables (see §6). It
does not yet include the `tak`/`probeer`/`vang`/`huidige`/`lewer`/`saam`/`gaan`
grammar — §7 below is written specifically so that work has a clear pattern
to follow when it happens.

---

## 1. Shape of the crate

One binary, one file. `src/main.rs`, one runtime dependency
(`serde_json`), no external LSP crate — framing, lexer, parser, and every
LSP handler live in this one file, in this order:

```
JSON-RPC framing        read_message / write_message
Lexer                    Kind, Tok, lex()
Parser core              Diag, Role, SemToken, FileMode, Parser, parse_program()
Built-in actions          builtin_actions(), builtin_doc(), keyword_doc(), property_doc()
Analysis dispatch         analyze(), json_diags(), diag_to_json()
Completion                completions_for()
Hover                     hover_at()
Semantic token encoding   encode_semantic_tokens()
Server loop                main(), publish_diagnostics()
```

This ordering is also the dependency order — later sections consume types
and functions defined earlier, nothing forward-references. Worth preserving
when adding new sections (e.g. `tak`/`probeer`/`vang` parsing logic belongs
inside the existing "Parser core" section, not bolted on afterward).

---

## 2. Request-handling architecture — no caching, re-derive every time

The server holds exactly one piece of state across requests: `documents:
HashMap<String, String>`, mapping URI to raw text. That's it — **no cached
token list, no cached AST, no cached diagnostics.**

Every single request — `didOpen`, `didChange`, `hover`, `completion`,
`semanticTokens/full` — independently calls `lex()` (and, for everything
except plain completion, `parse_program()`) fresh, from the document's
current raw text. The hover handler, for instance, calls `lex()` and
`parse_program()` from scratch on every hover request and passes the
resulting `toks`/`sems` into `hover_at()` — it doesn't reuse whatever
diagnostics were computed a moment earlier during the same document's last
`didChange`. (Note: `hover_at()` itself only consumes the `toks`/`sems` it
is given; the re-derivation happens in the handler that calls it, not inside
`hover_at`.)

This is a real, deliberate-looking simplicity trade-off, not an oversight —
worth understanding before extending the server, because it has two
consequences:

- **Correctness is easy to reason about** — there's no cache invalidation
  bug to worry about, ever, because nothing is ever cached. Any change to
  the lexer or parser takes effect on the very next request with no state
  to reset.
- **It doesn't scale gracefully to expensive analysis.** A future symbol
  table (needed for real completion/hover — see the integration guide's
  §6/§7) is exactly the kind of thing this pattern makes awkward: either it
  gets recomputed from scratch on every keystroke's `didChange` (fine for
  small files, a real cost on large ones), or the server needs to grow
  actual caching for the first time, which is a bigger architectural
  change than anything done so far. Worth deciding explicitly when that
  work starts, rather than discovering it mid-implementation.

---

## 3. Lexer (`lex`)

Single pass over `char_indices()`, tracking `(line, col)` manually (not
byte offsets) — every `Tok` carries `line`, `col`, `len` in characters, used
directly for LSP `Range` positions later with no offset translation needed.

**Token categories (`Kind`):** `Kw` (reserved word), `Type` (primitive type
name), `Lit` (`waar`/`onwaar`/`niks`/`true`/`false`), `Ident`, `Str`, `Num`,
`Op`, `Punct`, `Comment` (unused as a token kind — comments are emitted only
as `Role::Comment` _semantic_ tokens, never into the parser's stream — see
§3/§8), `Unknown`
(anything unrecognized; always pushes a diagnostic).

**Keyword classification is three flat `&[&str]` lists** — `KEYWORDS`,
`TYPE_KW`, `LIT_KW` — checked in that order against each identifier-shaped
token. Currently one shared list regardless of file kind; the reserved-word
file-scoping design (delta doc) will need this lexer function to accept a
`FileMode` and check the right list per mode, which it doesn't do today —
`file_mode_for(uri)` already exists and is computed at every call site, it's
just never passed into `lex()` itself.

**Comments are tokenized as a `Comment` semantic token (since v0.6.5) but
kept out of the parser's token stream** — `//` and `/* */` handling records
the span, pushes a `SemToken { role: Comment }` into a dedicated
`comment_sems` vec, and `continue`s without pushing a `Tok`. `lex()` returns
that vec as its third element; the `semanticTokens/full` handler merges it
with the parser's `sems` (sorted by position) before encoding. This is why
`Kind::Comment` is no longer dead code and why comments are now
highlightable — but they never affect parsing or diagnostics, since the
parser never sees them.

**Fixed (v0.6.4):** the number-literal loop now only accepts `.` once and
only while no exponent has been seen, `e`/`E` once, and `-`/`+` only
immediately after a consumed `e`/`E`. So `10-5` (no spaces) lexes as three
tokens (`10`, `-`, `5`) and parses as subtraction, `1e-5` stays one number,
and `3.14` / `1.2.3` (the latter stops at the second dot) behave sanely.

---

## 4. Parser core

### Structure

Straightforward recursive descent over a `Vec<Tok>` with a `pos` cursor.
The `Parser` struct is the entire mutable state during a parse — no
separate AST is built; the parser's job is entirely to walk tokens,
validate structure, push `Diag`s, and push `SemToken`s (see §5). There is
no tree returned from `parse_program` — just `(Vec<Diag>, Vec<SemToken>)`.
This is consistent with SPEC.md's "structure only" stance: nothing downstream
needs a real AST because nothing does semantic analysis on one yet.

### Dispatch by file kind

`Parser::run` branches on `self.mode: FileMode` (`Fwrk` | `Fdgn`) into
`run_fwrk` / `run_fdgn` — two genuinely separate top-level grammars, per
SPEC.md §10's "deliberately narrower" design for `.fdgn`.

In `.fdgn`, the valid top-level items are only `gbk` (import), `node`, and
`edge` — parsed by `parse_node` / `parse_edge` (the graph-shaped structure
of a `.fdgn` design file, distinct from `.fwrk`'s flows). Any attempt to put
a `.fwrk`-only construct at `.fdgn` top level is caught explicitly: `run_fdgn`
detects `flow`/`soort`/`step`/`laat` and reports a specific, named error for
each (`"`{kw}` is not valid in a .fdgn file`") rather than a generic
"unexpected token" — worth preserving this pattern for any new
`.fdgn`-invalid keyword rather than falling back to the generic message,
since the specific error is meaningfully more useful.

### Expression precedence chain

Lowest to highest binding, each level a function that calls the next level
tighter, matching the standard recursive-descent precedence-climbing shape:

```
parse_expr           →  parse_logical      (&&, ||)      [added in the && / || fix]
parse_logical         →  parse_comparison   (== != < > <= >=)
parse_comparison      →  parse_add          (+ -)
parse_add              →  parse_mul          (* /)
parse_mul               →  parse_unary        (- ! prefix)
parse_unary             →  parse_postfix      (.field, .elk(...))
parse_postfix           →  parse_primary      (literals, idents, calls, ( ), [ ], { })
```

Every binary level follows the identical shape — parse the tighter level
once, then loop consuming a matching operator + another tighter-level
parse. **Any new binary operator added later should follow this exact
pattern** (a new `while let Some(t) = self.peek() { if ... } else { break }`
loop at the appropriate precedence tier) rather than a one-off special case,
for the same reason `&&`/`||` broke: a level that doesn't follow this shape
is a level that's easy to accidentally omit.

### The `no_brace_ctor` disambiguation

Worth understanding before touching `as`/`terwyl`/conditions — this is the
one genuinely subtle piece of the parser. `terwyl x < retries { ... }` is
ambiguous on its face: is `retries {` the start of a `retries { field = ... }`
constructor-literal expression, or is `{` the loop's own body opening? The
parser resolves this with a boolean flag (`no_brace_ctor`) set to `true`
while parsing a condition's top-level expression, suppressing
constructor-literal parsing at that level — then explicitly cleared again
(`parse_expr_allowing_ctor`) whenever descending into any _delimited_
sub-expression (inside `()`, `[]`, a call argument, an object-literal
field value) where the ambiguity structurally can't occur, since a `)`/`]`/
`,`/`}` will end the sub-expression before a bare `{` could be
misinterpreted. Same technique Go and Rust use for composite/struct
literals inside `if`/`for` conditions. Any new construct that opens a block
directly after a condition-like expression (this will matter for `probeer`,
whose body is also a `{ }` block) needs to use `parse_condition`/this same
flag discipline if it can ever be preceded by a bare identifier.

### Error recovery discipline

Two consistent patterns worth replicating in any new parsing code:

- **`err_here` + `bump()` on unexpected tokens** — report the diagnostic,
  then still consume the bad token, so the parser makes forward progress
  and doesn't spin. `parse_block`'s loop explicitly checks `if self.pos ==
before` after `parse_stmt()` and force-advances if a statement consumed
  nothing at all — a direct guard against infinite loops on unparseable
  input.
- **Bump-as-you-go semantic tagging** — `bump_as(role)` is the standard way
  to consume a token _and_ record it as a `SemToken` in one call, used
  throughout instead of separate bump-then-tag steps. New grammar should
  use `bump_as` rather than bare `bump()` for any token that should be
  colorable (i.e., almost every meaningful token except pure punctuation).

---

## 5. Semantic tokens vs. diagnostics — two separate outputs from one parse

`Parser` accumulates both `diags: Vec<Diag>` and `sems: Vec<SemToken>` in
the same pass — parsing and semantic-highlighting classification happen
together, not as separate passes. `Role` (11 variants, matching the
`SEM_LEGEND` array's order exactly — index and legend name must stay in
sync, see `role_to_index`) is assigned at the point of consumption via
`bump_as`. Encoding to the LSP wire format (`encode_semantic_tokens`) is a
separate, later step — delta-line/delta-char/length/type/modifier per
token, computed from absolute `(line, col)` stored on each `SemToken`.

Any new `Role` variant needs three things kept in sync, or the legend and
the encoded indices will silently mismatch: an entry in the `Role` enum, a
matching-position entry in `SEM_LEGEND`, and a matching arm in
`role_to_index`.

---

## 6. Built-in registries — flat tables, linear lookup

`builtin_actions()`, `keyword_doc()`, `property_doc()` are all `&'static
[(&'static str, &'static str)]` slices, searched linearly
(`.iter().find(...)`) on every hover/completion call — no `HashMap`,
rebuilt as a fresh slice reference each call (cheap; these are `'static`
data, not allocated per-call). Fine at current size; worth reconsidering
only if these tables grow into the hundreds of entries.

**To add a new built-in action:** one line in `builtin_actions()`. That
single entry automatically feeds both hover (`builtin_doc`) and completion
(`completions_for`'s `.fwrk`-after-`step` and `.fdgn`-after-`action` arms
both iterate this same table) — no second place to update.

**To add a new keyword's hover doc:** one line in `keyword_doc()`'s table.
Note this is currently _not_ auto-derived from `KEYWORDS`/`TYPE_KW`/
`LIT_KW` — adding a word to the lexer's keyword lists doesn't automatically
give it a hover entry; that's a manual, separate step, and it's exactly how
the current elk/step/gbk/etc. doc-mismatch problem happened (grammar moved,
doc strings didn't).

---

## 7. Recipe: adding a new keyword/construct

Concrete checklist, in the order these tend to matter, for `tak`/`probeer`/
`vang`/`huidige`/`lewer`/`saam`/`gaan` when that work starts:

1. **Lexer:** add the word to the appropriate keyword list (`KEYWORDS` for
   now — see §3's note on this needing to become file-mode-aware first for
   the reserved-word-scoping design to actually work).
2. **Parser — new `parse_*` function** for any construct with its own block
   shape (`tak`, `probeer`), following the existing `parse_if`/`parse_while`
   pattern: `bump_kw`, delimiters, `parse_block`/`parse_expr` as
   appropriate, using `bump_as` for every meaningful token.
3. **Wire it into dispatch** — `parse_stmt` (for a new statement kind
   inside a flow body), `run_fwrk`/`run_fdgn` (for a new top-level item),
   or `parse_primary`/`parse_postfix` (for a new expression form) —
   whichever matches where the construct is grammatically valid.
4. **`Role` + `SEM_LEGEND`** — only if the construct needs a _new_ semantic
   category; several of the new keywords likely just reuse `Role::Keyword`/
   `Role::Control`/`Role::Function` and need nothing here.
5. **`keyword_doc()`** — add the hover string, and make sure it actually
   matches what step 2 implements (the lesson from §6's caveat).
6. **`completions_for()`** — add to the relevant `(FileMode, prev-token)`
   arm if the new keyword should be suggested in some context.
7. **Verify against real input** — build, then round-trip real source
   through the running binary (§9) rather than reading the diff and
   assuming correctness. This is what caught the `&&`/`||` bug being a real
   6-diagnostic cascade rather than a theoretical gap.

---

## 8. Current dead/unused code, and why

- **`Kind::Comment` / `Role::Comment`** — previously dead: the lexer skipped
  comment text without emitting a token (§3). **As of v0.6.5 this is no
  longer dead** — `lex()` now pushes a `SemToken { role: Comment }` for each
  `//`/`/* */` span into its returned `comment_sems` vec, and the
  `semanticTokens/full` handler encodes them (§3 details the mechanics).
  `Role::Comment` is now genuinely used; `Kind::Comment` remains unused as a
  _token kind_ (comments are never put into the parser's `toks` stream — only
  their semantic tokens survive), which is intentional.
- **`Kind::Kw` arm in `parse_primary`** matching `waar`/`onwaar`/`niks`/
  `true`/`false` — unreachable. These words are always lexed as `Kind::Lit`
  (checked before `Kind::Kw` in the lexer's classification order), so the
  `Kind::Lit` arm immediately above always wins first.
- **`is_punct("\"")` check in `parse_import`** — the lexer never emits a
  standalone `"` token (string literals are consumed whole as one
  `Kind::Str` token), so this condition can never be true.

None of these affect correctness today; they're listed here so they aren't
mistaken for load-bearing logic while extending nearby code.

---

## 9. Verifying changes — recommended workflow going forward

Reading a diff against the grammar rules doesn't catch everything a real
parse would — the `&&`/`||` bug looked like a one-line gap on paper but
produced a 6-diagnostic cascade in practice, and was only actually confirmed
by building and sending real `didOpen` traffic through the binary. Recommended
loop for any future change to this file:

1. `cargo build` — catch compile errors first.
2. Send real source through the running binary — a small script implementing
   Content-Length read/write framing (a few dozen lines in any scripting
   language) is enough to `initialize`, `didOpen` a test document, and read
   back `publishDiagnostics`.
3. Test both a case that _should_ now succeed (the fix's target) and a case
   that _should_ still fail (confirm the fix didn't loosen error detection —
   this caught nothing extra passing when the `&&`/`||` fix was verified,
   but it's cheap insurance worth keeping in the habit).

Worth keeping a small `tests/` scaffold with a handful of `examples.md`
snippets fed through this loop as the grammar grows — currently there's no
test module in the crate at all (`Cargo.toml` has no `[dev-dependencies]`,
`main.rs` has no `#[cfg(test)]` block), which is the concrete reason a bug
this basic shipped invisibly in the first place.-
