# ForgeFlow Examples

Practical, copy-pasteable ForgeFlow snippets. All examples use the `.fwrk`
extension and follow the grammar locked in `SPEC.md`. Keep in mind the language
server validates **structure only** — types and scopes are not checked yet, so the
examples below are illustrative of syntax, not guaranteed to "run" against the
engine.

---

## 1. Minimal flow

The first `flow` in a file is the entry point.

```
flow hoofd() {
    gee "hello"
}
```

---

## 2. Imports

Named import of another flow file, resolved relative to the current file.

```
gbk Math vannaf "./math.fwrk"

flow hoofd() : nmr {
    gee Math.add(2, 3)
}
```

---

## 3. Types and interfaces (`soort`)

Alias a primitive:

```
soort Naam = lyn
```

Declare a structured type:

```
soort User = {
    naam: lyn
    ouderdom: nmr
    aktief: vraag
}
```

Nullable field with `?`:

```
soort Account = {
    id: nmr
    email: lyn?
}
```

Generic list type:

```
soort Roster = lys<User>
```

---

## 4. Variables and assignment

```
flow hoofd() {
    laat naam = "Jan"
    laat ouderdom: nmr = 30
    laat aktief: vraag = waar

    ouderdom = ouderdom + 1
    gee ouderdom
}
```

`laat` declares; a bare `name = expr` reassigns.

---

## 5. Object and constructor literals

```
flow hoofd() : User {
    laat u = User {
        naam = "Jan",
        ouderdom = 30,
        aktief = waar
    }
    gee u
}
```

Standalone object (untyped):

```
laat config = {
    retries = 3,
    timeout = 1000
}
```

---

## 6. Flows with parameters and defaults

```
flow groet(naam: lyn, punct: lyn = "!") : lyn {
    gee "Hello, " + naam + punct
}
```

Call it:

```
flow hoofd() : lyn {
    gee groet("World")
}
```

---

## 7. Lists and iteration with `.elk`

Map form — transform each element into a new list:

```
flow hoofd() : lys<nmr> {
    laat nums = [1, 2, 3, 4]
    gee nums.elk((x)) => x * 2
    // produces [2, 4, 6, 8]
}
```

Loop form — run a block for each element:

```
flow hoofd() {
    laat nums = [1, 2, 3]
    nums.elk((n)) {
        laat doubled = n * 2
        // doubled is scoped to this iteration
    }
}
```

---

## 8. Control flow

`if` / `else` (`as` / `anders`):

```
flow classify(n: nmr) : lyn {
    as n > 0 {
        gee "positive"
    } anders {
        gee "non-positive"
    }
}
```

`while` (`terwyl`):

```
flow telOp() : nmr {
    laat totaal: nmr = 0
    terwyl totaal < 5 {
        totaal = totaal + 1
    }
    gee totaal
}
```

Early return (`gee`):

```
flow eerste(n: nmr) : nmr {
    as n <= 0 {
        gee 0
    }
    gee n
}
```

---

## 9. Step actions (`step`)

Invoke an engine action. Arguments are `name = value` pairs:

```
flow hoofd() {
    step sendEmail(
        to = "team@forge.dev",
        subject = "Daily report",
        body = "All systems nominal"
    )
}
```

---

## 10. Expressions and operators

```
flow reken() : nmr {
    laat a = 10
    laat b = 3
    laat c = (a + b) * 2          // 26
    as c > 20 && !false {
        gee c
    } anders {
        gee 0
    }
}
```

Boolean and null literals:

```
laat ok: vraag = waar
laat niksWaarde = niks
```

`true` / `false` are accepted aliases for `waar` / `onwaar`.

---

## 11. A more complete example

```
gbk Utils vannaf "./utils.fwrk"

soort Product = {
    naam: lyn
    prys: nmr
    uitgestock: vraag
}

flow hoofd() : nmr {
    laat items = [
        Product { naam = "Pen", prys = 12, uitgestock = onwaar },
        Product { naam = "Boek", prys = 45, uitgestock = waar },
        Product { naam = "Map", prys = 8, uitgestock = onwaar }
    ]

    laat totaal: nmr = 0
    items.elk((p)) {
        as !p.uitgestock {
            totaal = totaal + p.prys
        }
    }

    gee totaal
}
```

---

## 12. Comments

```
// line comment
/* block comment
   spanning lines */
flow hoofd() {
    gee 1   // trailing comment
}
```

---

## 13. `.fmeta` metadata lint

`.fmeta` files are validated as JSON. Encrypted fields are flagged:

```
{
    "name": "credentials",
    "apiKey": "ENC:a1b2c3...",
    "region": "eu-west"
}
```

The language server surfaces an info diagnostic on any `ENC:` value reminding you
it stays encrypted at rest and is resolved only by the Forge runtime.

---

## 14. `.fdgn` graph files

`.fdgn` is a narrower, declarative grammar (locked in `SPEC.md` §10): imports,
`node` blocks, and standalone `edge` declarations only — no `flow`, `step`,
`soort`, or control flow. This is what the Forge Designer panel reads and
writes; hand-editing one directly works the same way editing a `.fwrk` file
does (diagnostics, hover, completion, semantic tokens).

A two-node pipeline, matching the `.fwrk` example in §1:

```
gbk { http, delay } vannaf "./actions.fwrk"

node fetchData {
    title = "Fetch Data"
    action = http(method = "GET", url = "https://api.forge.dev/v1/build")
    pos = [80, 80]
}

node wait {
    title = "Wait"
    action = delay(ms = 500)
    pos = [300, 80]
}

edge fetchData -> wait
```

A node with no outgoing edge is a dead end (valid structurally — the graph
just doesn't continue past it). A node referenced by an `edge` that has no
matching `node` declaration is not flagged by the language server today (no
cross-reference checking, same as unresolved `gbk` import paths).

List-import (`gbk { a, b } vannaf "path"`) works here exactly as it does in
`.fwrk` (§2):

```
gbk { http, delay, transform } vannaf "./actions.fwrk"
```
