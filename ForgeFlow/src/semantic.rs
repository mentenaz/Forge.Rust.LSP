// ── ForgeFlow semantic analysis (Component 2). ──
// Sits on top of the grammar's token stream and adds meaning:
// in-scope symbols, context-aware completions, hover docs.
// Depends only on grammar for the token model.
use std::collections::{HashMap, HashSet};
use serde_json::{Value, json};
use crate::grammar::{Tok, Kind, Role, SemToken, FileMode, TYPE_KW};

// ── Built-in actions ────────────────────────────────────────────────────────

/// Sample registry of built-in Forge runtime actions. Extensible; hover and
/// completion read from it. (Phase 4 would wire these to real execution.)
fn builtin_actions() -> &'static [(&'static str, &'static str)] {
    &[
        ("http", "Sends an HTTP request. Params: `method`, `url`, `headers`, `body`."),
        ("file", "Reads or writes a file. Params: `path`, `content`, `mode`."),
        ("delay", "Pauses the flow for a duration. Params: `ms`."),
        ("condition", "Branches the flow on a predicate. Params: `when`."),
        ("loop", "Repeats the contained steps. Params: `times`, `while`."),
        ("email", "Sends an email. Params: `to`, `subject`, `body`."),
        ("db", "Runs a database query. Params: `query`, `connection`."),
        ("script", "Executes a script. Params: `lang`, `code`."),
        ("transform", "Transforms data with an expression. Params: `expr`."),
        ("notify", "Raises a notification. Params: `channel`, `message`."),
        ("log", "Writes a log line. Params: `level`, `text`."),
        ("process", "Processes an item. Params: `id`, `url`."),
    ]
}

fn builtin_doc(name: &str) -> Option<&'static str> {
    builtin_actions().iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

/// Detailed hover docs for ForgeFlow keywords, control-flow words, type
/// primitives and literals. Mirrors the lexer's `KEYWORDS` / `TYPE_KW` /
/// `LIT_KW` sets (lines ~74-79).
fn keyword_doc(name: &str) -> Option<&'static str> {
    let m: &[(&str, &str)] = &[
        ("gbk", "Imports a flow or function module so its steps can be reused. Form: `gbk <naam>` or `gbk vannaf \"<pad>\"`."),
        ("vannaf", "Means \"from\". Supplies the source module/path for an import: `gbk vannaf \"<pad>\"`."),
        ("flow", "Defines a flow/function — the top-level unit of work. `flow <naam> { ... }`. A flow contains steps (`step`) and control flow (`as` / `terwyl` / `elk`)."),
        ("soort", "Defines a type/interface (struct). `soort <naam> { veld lyn }`."),
        ("laat", "Declares a variable. `laat <naam> <tipe> = <uitdrukking>`."),
        ("step", "A single action step inside a flow. `step <action> <param>=<waarde> ...` (e.g. `step http url=...`). Hover the action name for its parameters."),
        ("gee", "Returns a value and exits the current flow. `gee <uitdrukking>`."),
        ("as", "If. Branches the flow: `as <voorwaarde> { ... } anders { ... }`."),
        ("anders", "Else. The alternative branch of an `as` (if) statement."),
        ("terwyl", "While. Repeats the block while a condition holds: `terwyl <voorwaarde> { ... }`."),
        ("elk", "Each / for-each. Iterates over a list: `elk <item> in <lys> { ... }`."),
        ("node", "Declares a node in a flow graph (.fdgn). `node <naam> action=<step> <param>=<waarde> ...`."),
        ("edge", "Connects two nodes in a flow graph (.fdgn). `edge <a> -> <b>`."),
        ("lyn", "String type — a sequence of characters."),
        ("nmr", "Number type — an integer or floating-point value."),
        ("vraag", "Boolean type — `waar` (true) or `onwaar` (false)."),
        ("objk", "Object type — a key/value map."),
        ("lys", "List type — an ordered collection."),
        ("enige", "Any type — accepts a value of any type."),
        ("leeg", "Null/empty type — the absence of a value."),
        ("waar", "Boolean literal — true."),
        ("onwaar", "Boolean literal — false."),
        ("niks", "Null literal — no value."),
    ];
    m.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

/// Detailed hover docs for the well-known built-in action parameters. These
/// names appear as `property` role tokens inside `step` calls.
fn property_doc(name: &str) -> Option<&'static str> {
    let m: &[(&str, &str)] = &[
        ("method", "HTTP method for the request (GET, POST, PUT, DELETE, …)."),
        ("url", "Target URL of the HTTP request."),
        ("headers", "Request headers, as a key/value object."),
        ("body", "Request body payload."),
        ("path", "File path to read or write."),
        ("content", "File contents to write."),
        ("mode", "File open mode (e.g. `write`, `append`, `read`)."),
        ("ms", "Delay duration in milliseconds."),
        ("when", "Predicate that guards a `condition` branch."),
        ("times", "Number of repetitions for a `loop`."),
        ("to", "Email recipient address."),
        ("subject", "Email subject line."),
        ("query", "SQL / query string to execute."),
        ("connection", "Database connection identifier."),
        ("lang", "Script language (e.g. `bash`, `pwsh`, `python`)."),
        ("code", "Script source to execute."),
        ("expr", "Transformation expression."),
        ("channel", "Notification channel / recipient."),
        ("message", "Notification or email body text."),
        ("level", "Log severity (e.g. `info`, `warn`, `error`)."),
        ("text", "Log message text."),
        ("id", "Item / process identifier."),
    ];
    m.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

// ── Completion ───────────────────────────────────────────────────────────────



const NODE_FIELDS: &[&str] = &["title", "action", "pos"];

// ── Completion context / symbol scope ────────────────────────────────────────

/// A small in-scope symbol table gathered from the token stream, used to make
/// completions context-aware: variables/parameters/loop-vars, declared `flow`
/// names, declared `soort` types and their fields, and the resolved type of
/// each variable so member access (`.elk`, struct fields) can be suggested.
#[derive(Default)]
struct Scope {
    vars: Vec<String>,
    flows: Vec<String>,
    soorts: Vec<String>,
    nodes: Vec<String>,
    var_types: HashMap<String, String>,
    soort_fields: HashMap<String, Vec<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Keyword,
    Type,
    Expr,
    Member,
    Action,
    Field,
}

fn push_item(items: &mut Vec<Value>, seen: &mut HashSet<String>, label: &str, kind: u64, detail: &str) {
    if seen.insert(label.to_string()) {
        items.push(json!({ "label": label, "kind": kind, "detail": detail }));
    }
}

/// Walks the token stream building a `Scope`. Top-level `flow`/`soort`
/// declarations are collected from the whole file (order-independent), while
/// variables/parameters/loop-vars are only collected up to `character` on
/// `line` — i.e. things actually in scope at the cursor.
fn collect_scope(toks: &[Tok], line: usize, character: usize) -> Scope {
    let mut scope = Scope::default();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        match t.text.as_str() {
            "flow" => {
                if let Some(n) = toks.get(i + 1) {
                    if n.kind == Kind::Ident {
                        scope.flows.push(n.text.clone());
                    }
                }
                // Capture this flow's parameters (name + type), but only when
                // the `flow` keyword itself is before the cursor.
                if t.line < line || (t.line == line && t.col + t.len <= character) {
                    let mut j = i + 2;
                    while j < toks.len() && toks[j].text != "(" {
                        j += 1;
                    }
                    if j < toks.len() {
                        let mut depth = 0usize;
                        let mut k = j;
                        while k < toks.len() {
                            if toks[k].text == "(" {
                                depth += 1;
                                k += 1;
                                continue;
                            }
                            if toks[k].text == ")" {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                k += 1;
                                continue;
                            }
                            if depth == 1
                                && toks[k].kind == Kind::Ident
                                && toks.get(k + 1).map(|x| x.text == ":").unwrap_or(false)
                            {
                                let pname = toks[k].text.clone();
                                let mut ty = String::new();
                                let mut m = k + 2;
                                while m < toks.len() {
                                    let tk = &toks[m];
                                    if tk.text == "," && depth == 1 {
                                        break;
                                    }
                                    if tk.text == ")" && depth == 1 {
                                        break;
                                    }
                                    if tk.text != "(" && tk.text != ")" && tk.text != "," {
                                        if !ty.is_empty() && tk.text != "<" && tk.text != ">" {
                                            ty.push(' ');
                                        }
                                        ty.push_str(&tk.text);
                                    }
                                    m += 1;
                                }
                                scope.vars.push(pname.clone());
                                scope.var_types.insert(pname, ty.trim().to_string());
                            }
                            k += 1;
                        }
                    }
                }
            }
            "soort" => {
                if let Some(n) = toks.get(i + 1) {
                    if n.kind == Kind::Ident {
                        let sname = n.text.clone();
                        scope.soorts.push(sname.clone());
                        let mut j = i + 2;
                        while j < toks.len() && toks[j].text != "{" {
                            j += 1;
                        }
                        if j < toks.len() {
                            let mut fields = Vec::new();
                            let mut depth = 0usize;
                            let mut k = j;
                            while k < toks.len() {
                                if toks[k].text == "{" {
                                    depth += 1;
                                    k += 1;
                                    continue;
                                }
                                if toks[k].text == "}" {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                    k += 1;
                                    continue;
                                }
                                if depth == 1
                                    && toks[k].kind == Kind::Ident
                                    && toks.get(k + 1).map(|x| x.text == ":").unwrap_or(false)
                                {
                                    fields.push(toks[k].text.clone());
                                }
                                k += 1;
                            }
                            scope.soort_fields.insert(sname, fields);
                        }
                    }
                }
            }
            "node" => {
                if let Some(n) = toks.get(i + 1) {
                    if n.kind == Kind::Ident {
                        scope.nodes.push(n.text.clone());
                    }
                }
            }
            "laat" => {
                if t.line < line || (t.line == line && t.col + t.len <= character) {
                if let Some(n) = toks.get(i + 1) {
                        if n.kind == Kind::Ident {
                            let vname = n.text.clone();
                            scope.vars.push(vname.clone());
                            if toks.get(i + 2).map(|x| x.text == ":").unwrap_or(false) {
                                let mut ty = String::new();
                                let mut m = i + 3;
                                while m < toks.len() {
                                    let tk = &toks[m];
                                    if tk.text == "=" || tk.text == ";" || tk.text == "{" || tk.text == "}" {
                                        break;
                                    }
                                    if tk.text != "(" && tk.text != ")" && tk.text != "," && tk.text != "?" {
                                        if !ty.is_empty() && tk.text != "<" && tk.text != ">" {
                                            ty.push(' ');
                                        }
                                        ty.push_str(&tk.text);
                                    }
                                    m += 1;
                                }
                                scope.var_types.insert(vname, ty.trim().to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        // `.elk((v))` loop variable declaration.
        if t.text == "."
            && toks.get(i + 1).map(|x| x.text == "elk").unwrap_or(false)
            && toks.get(i + 2).map(|x| x.text == "(").unwrap_or(false)
            && toks.get(i + 3).map(|x| x.text == "(").unwrap_or(false)
            && toks.get(i + 4).map(|x| x.kind == Kind::Ident).unwrap_or(false)
            && (t.line < line || (t.line == line && t.col + t.len <= character))
        {
            scope.vars.push(toks[i + 4].text.clone());
        }
        i += 1;
    }
    scope
}

pub fn completions_for(line: usize, character: usize, toks: &[Tok], mode: FileMode) -> Vec<Value> {
    // Last three tokens ending at or before the cursor — drives the context.
    let mut prev: Option<&Tok> = None;
    let mut prev2: Option<&Tok> = None;
    let mut prev3: Option<&Tok> = None;
    for t in toks {
        if t.line > line || (t.line == line && t.col + t.len > character) {
            break;
        }
        if t.line == line && t.col + t.len <= character {
            prev3 = prev2;
            prev2 = prev;
            prev = Some(t);
        }
    }

    let scope = collect_scope(toks, line, character);

    let mut items: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Classify the cursor position into a completion context.
    let mut ctx = Ctx::Expr;
    if let Some(p) = prev {
        match p.text.as_str() {
            "." => ctx = Ctx::Member,
            ":" => {
                let type_pos = prev2
                    .map(|p2| {
                        p2.text == ")"
                            || (p2.kind == Kind::Ident
                                && prev3
                                    .map(|p3| matches!(p3.text.as_str(), "(" | "," | "laat" | "soort"))
                                    .unwrap_or(false))
                    })
                    .unwrap_or(false);
                ctx = if type_pos { Ctx::Type } else { Ctx::Expr };
            }
            "<" => ctx = Ctx::Type,
            "=" => {
                ctx = if mode == FileMode::Fdgn
                    && prev2.map(|t| t.text == "action").unwrap_or(false)
                {
                    Ctx::Action
                } else {
                    Ctx::Expr
                };
            }
            "step" => ctx = Ctx::Action,
            "action" if mode == FileMode::Fdgn => ctx = Ctx::Action,
            "{" if mode == FileMode::Fdgn => ctx = Ctx::Field,
            ";" | "{" | "}" | ")" => ctx = Ctx::Keyword,
            _ => ctx = Ctx::Expr,
        }
    } else {
        ctx = Ctx::Keyword;
    }

    match ctx {
        Ctx::Member => {
            let recv = prev2.filter(|t| t.kind == Kind::Ident).map(|t| t.text.clone());
            let mut offered: Vec<(String, u64, String)> = Vec::new();
            if let Some(r) = recv {
                if let Some(ty) = scope.var_types.get(&r) {
                    if ty.starts_with("lys") {
                        offered.push((
                            "elk".into(),
                            2,
                            "Iterate the list: `.elk((item)) => ...`".into(),
                        ));
                    } else if let Some(fields) = scope.soort_fields.get(ty) {
                        for f in fields {
                            offered.push((f.clone(), 10, format!("field of {ty}")));
                        }
                    }
                }
            }
            if offered.is_empty() {
                offered.push((
                    "elk".into(),
                    2,
                    "Iterate a list: `.elk((item)) => ...`".into(),
                ));
            }
            for (l, k, d) in offered {
                push_item(&mut items, &mut seen, &l, k, &d);
            }
        }
        Ctx::Type => {
            for ty in TYPE_KW {
                push_item(&mut items, &mut seen, ty, 7, "type");
            }
            for s in &scope.soorts {
                push_item(&mut items, &mut seen, s, 7, "type / interface");
            }
        }
        Ctx::Action => {
            for (name, doc) in builtin_actions() {
                push_item(&mut items, &mut seen, name, 3, doc);
            }
        }
        Ctx::Field => {
            for f in NODE_FIELDS {
                push_item(&mut items, &mut seen, f, 10, "node field");
            }
        }
        Ctx::Expr => {
            for v in &scope.vars {
                push_item(&mut items, &mut seen, v, 6, "variable / parameter");
            }
            for f in &scope.flows {
                push_item(&mut items, &mut seen, f, 3, "flow / function");
            }
            for n in &scope.nodes {
                push_item(&mut items, &mut seen, n, 6, "node");
            }
            for lit in ["waar", "onwaar", "niks"] {
                push_item(&mut items, &mut seen, lit, 12, "literal");
            }
            let kw: &[&str] = if mode == FileMode::Fdgn {
                &["gbk", "node", "edge", "as", "terwyl", "gee", "laat"]
            } else {
                &["gbk", "soort", "flow", "laat", "as", "terwyl", "gee", "elk"]
            };
            for k in kw {
                push_item(&mut items, &mut seen, k, 14, "keyword");
            }
        }
        Ctx::Keyword => {
            let kw: &[&str] = if mode == FileMode::Fdgn {
                &["gbk", "node", "edge", "anders"]
            } else {
                &["gbk", "soort", "flow", "laat", "anders"]
            };
            for k in kw {
                push_item(&mut items, &mut seen, k, 14, "keyword");
            }
            for f in &scope.flows {
                push_item(&mut items, &mut seen, f, 3, "flow / function");
            }
            for s in &scope.soorts {
                push_item(&mut items, &mut seen, s, 7, "type / interface");
            }
        }
    }
    items
}

// ── Hover ────────────────────────────────────────────────────────────────────

pub fn hover_at(toks: &[Tok], sems: &[SemToken], line: usize, character: usize, mode: FileMode) -> Option<String> {
    for s in sems {
        if s.line == line && character >= s.col && character <= s.col + s.len {
            let name = toks
                .iter()
                .find(|t| t.line == s.line && t.col == s.col)
                .map(|t| t.text.clone())
                .unwrap_or_default();
            match s.role {
                Role::Function => {
                    if let Some(doc) = builtin_doc(&name) {
                        return Some(format!("**{name}** — built-in action\n\n{doc}"));
                    }
                    return Some(match mode {
                        FileMode::Fdgn => format!("**{name}** — node"),
                        FileMode::Fwrk => format!("**{name}** — flow/function"),
                    });
                }
                // Non-function roles: resolve docs from the combined registry.
                // The parser reports action names (e.g. `http` after `step`)
                // under the property role and action params (e.g. `url`)
                // under the type role, so try every source regardless of role.
                Role::Control | Role::Keyword | Role::Type | Role::Bool | Role::Property => {
                    if let Some(doc) = builtin_doc(&name) {
                        return Some(format!("**{name}** — built-in action\n\n{doc}"));
                    }
                    if let Some(doc) = keyword_doc(&name) {
                        let label = match s.role {
                            Role::Control => "control-flow keyword",
                            Role::Type => "type",
                            Role::Bool => "literal",
                            _ => "keyword",
                        };
                        return Some(format!("**{name}** — {label}\n\n{doc}"));
                    }
                    if let Some(doc) = property_doc(&name) {
                        return Some(format!("**{name}** — parameter\n\n{doc}"));
                    }
                    return Some(match s.role {
                        Role::Control => "Control-flow keyword".into(),
                        Role::Type => "Type".into(),
                        Role::Bool => "Boolean / null literal".into(),
                        Role::Property => "Parameter / field name".into(),
                        _ => "ForgeFlow keyword".into(),
                    });
                }
                _ => {}
            }
        }
    }
    for t in toks {
        if t.line == line && character >= t.col && character <= t.col + t.len {
            return match t.kind {
                Kind::Str => Some("String value".into()),
                Kind::Num => Some("Number value".into()),
                Kind::Lit => Some("Boolean / null value".into()),
                _ => None,
            };
        }
    }
    None
}

