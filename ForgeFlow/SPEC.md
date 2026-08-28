# ForgeFlow Language Specification (locked)

ForgeFlow is the declarative workflow / dataflow DSL for the Forge engine. Files use
the `.fwrk` (flow), `.fdgn` (design), `.fmeta` (metadata), or `.forge` (binary
container) extensions. This document is the authoritative grammar as implemented by
`forge-lsp-forgeflow`.

Lexical and syntactic rules below are what the language server parses today. Type
checking, scope resolution, and runtime semantics are **not** enforced by the
language server — it validates structure only.

---

## 1. Lexical structure

### Comments
- Line comment: `//` to end of line.
- Block comment: `/* ... */` (may span lines).

### Identifiers
`[A-Za-z][A-Za-z0-9_]*`. Identifiers are case-sensitive.

### String literals
Double-quoted only: `"..."`. No string interpolation in v1.

### Number literals
`[0-9]` sequences, optionally containing a single `.` decimal point and an
optional `e`/`E` exponent (`123`, `3.14`, `1e10`).

### Reserved words (keywords)
The following are **reserved** and may not be used as identifiers (variable names,
field names, type names):

```
gbk      vannaf     soort      flow       step
laat     as         anders     terwyl     gee
lyn      nmr        vraag      objk       lys
enige    leeg       elk        waar       onwaar    niks
```

> Design note: `flow` and `step` are intentionally English (stable engine API
> names). All other keywords are Afrikaans-rooted. Type keywords (`lyn`, `nmr`,
> `vraag`, `objk`, `lys`, `enige`, `leeg`) double as the built-in type names and
> are therefore reserved everywhere.

`true` / `false` are accepted as aliases for `waar` / `onwaar`.

### Operators & punctuation
Operators: `+ - * / == != < > <= >= && || ! =>`
Punctuation: `{ } ( ) [ ] . , ; : ? =`

`<` and `>` are punctuation tokens (also used for generics), but participate in
comparison expressions.

---

## 2. Top-level structure

A ForgeFlow file is an ordered list of top-level items, each separated by a
newline (or `;`):

1. **Import** — `gbk <name> vannaf "<path>"`
2. **Type declaration** — `soort ...`
3. **Flow** — `flow ...`

The first `flow` declared in a file is the **entry point** executed by the engine.

### Import
```
gbk Tools vannaf "./tools.fwrk"
```
Imports a named binding from a path relative to the current file. Only named
imports are supported (no glob/wildcard).

### Type / interface declaration
Alias form:
```
soort Id = lyn
```
Interface form (field list, newline- or comma-separated):
```
soort User = {
    naam: lyn
    ouderdom: nmr
    aktief: vraag
}
```
A trailing `?` marks a field/type nullable: `ouderdom: nmr?`.

---

## 3. Types

| Keyword | Meaning            | Notes                          |
|---------|--------------------|--------------------------------|
| `lyn`   | string             |                                |
| `nmr`   | number             |                                |
| `vraag` | boolean            |                                |
| `objk`  | object             | `objk<K,V>` for map types      |
| `lys<T>`| list of `T`        | e.g. `lys<User>`               |
| `enige` | any                |                                |
| `leeg`  | void / no value    |                                |
| `T?`    | nullable `T`       |                                |

User-declared types (via `soort`) are referenced by their name and may carry
generics, e.g. `lys<User>`.

---

## 4. Flows (functions)

```
flow naam(param: type = default, other: type) : retType {
    // statements
}
```

- Parameters are `name: type`, optionally with a default value `name: type = expr`.
- The return type after `:` is optional; omit for `leeg`.
- A flow is invoked as `naam(args)` in expression position, or as a step action
  `step naam(args)` (see §6).

---

## 5. Statements

Statements inside a `{ ... }` block are separated by a newline or `;`.

### Variable declaration
```
laat x = 10
laat y: nmr = 10
laat user: User = User { naam = "Jan", ouderdom = 3 }
```
`laat <name> [: type] = <expr>`.

### Assignment
```
x = x + 1
```
Plain reassignment of an existing variable (no `laat`).

### Expression statement
Any expression may stand alone as a statement — notably `.elk` map expressions:
```
lyste.elk((x)) => x * 2
```

### Control flow
```
as <cond> {
    // ...
} anders {
    // ...
}

terwyl <cond> {
    // ...
}

gee <expr>          // return (optional value)
gee                // return with no value
```
`as` = if, `anders` = else, `terwyl` = while, `gee` = return.

---

## 6. Step actions

Inside a flow, engine actions are invoked with `step`:
```
step sendEmail(to = "a@b.c", body = "hi")
```
`step <name>(arg = value, ...)` — arguments are `name = value` pairs.

---

## 7. Expressions

Precedence (highest to lowest):

1. Primary: literals, identifiers, `( expr )`, `[ elems ]`, `{ fields }`,
   `Type { fields }` (constructor), `name( args )` (call).
2. Postfix: `.field` (member access), `.elk((v)) { ... }` (loop),
   `.elk((v)) => expr` (map).
3. Unary: `-`, `!`.
4. Multiplicative: `*`, `/`.
5. Additive: `+`, `-`.
6. Comparison: `== != < > <= >=`.
7. Logical: `&&`, `||`.

### Object / constructor literal
```
{ naam = "Jan", ouderdom = 3 }
User { naam = "Jan", ouderdom = 3 }
```
Fields are `name = value`, comma- or newline-separated.

### List literal
```
[1, 2, 3]
[]
```

### `.elk` — iterate / map
Loop form (runs the block for each element):
```
lyste.elk((n)) {
    laat doubled = n * 2
}
```
Map form (produces a new list by transforming each element):
```
lyste.elk((x)) => x * 2
```
The binding name inside `(( ))` is the per-element variable.

---

## 8. Semantic token roles

The language server emits the following semantic-token types (matching the Forge
theme `syntax` keys, so no theme change is required):

`keyword · control · type · function · variable · property · operator · string · number · boolean · comment`

---

## 9. Language server capabilities

- `textDocument/publishDiagnostics` — structural/parse errors.
- `textDocument/semanticTokens/full` — highlighting.
- `textDocument/hover` — keyword/role documentation for the token under the cursor.
- `textDocument/completion` — keyword/type/identifier completions based on the
  preceding token.

`.fmeta` files are validated as JSON (with an `ENC:` encrypted-field lint).
`.forge` files are treated as binary containers and are not parsed as text.
