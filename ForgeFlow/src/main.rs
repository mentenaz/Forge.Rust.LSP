//! ForgeFlow language server.
//!
//! A hand-rolled LSP server (JSON-RPC 2.0 over stdio with `Content-Length`
//! framing, mirroring Forge's LSP client and the `Json/` reference package)
//! for the ForgeFlow DSL. Implements the `BuildOrder.md` phases 1–3 and 5
//! (text file types):
//!   - Phase 1: lexer + recursive-descent parser for the `flow`/`step`/`param`
//!     grammar, producing an AST and positioned diagnostics.
//!   - Phase 2: LSP scaffolding (initialize/shutdown/didOpen/didChange).
//!   - Phase 3: diagnostics, semantic tokens, hover docs, completion.
//!   - Phase 5: per-extension handling for `.fwrk` (DSL), `.fdgn` (JSON or
//!     DSL), `.fmeta` (JSON config with `ENC:` secret fields), `.forge`
//!     (binary container — surfaced as an informational note only).
//!
//! Phases 4 (runtime execution integration) and 6 (VSCode extension,
//! crate publishing) are intentionally deferred — this server is a pure
//! editor aid and never executes workflows.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use serde_json::{Value, json};

// ── JSON-RPC framing ────────────────────────────────────────────────────────

fn read_message<R: BufRead>(reader: &mut R) -> Option<Value> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None; // EOF
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(rest.trim().parse().ok()?);
        }
    }
    let length = length?;
    let mut buf = vec![0u8; length];
    reader.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn write_message(writer: &mut impl Write, msg: &Value) {
    let payload = serde_json::to_vec(msg).expect("serialize");
    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
    let _ = writer.write_all(header.as_bytes());
    let _ = writer.write_all(&payload);
    let _ = writer.flush();
}

// ── Lexer ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Ident,
    Str,
    Num,
    Bool,
    Punct,
    Unknown,
}

struct Tok {
    kind: Kind,
    text: String,
    line: usize,
    col: usize,
    len: usize,
}

/// Lex the source into tokens, recording (0-based) line/col per token.
/// Lexer-level errors (stray characters) are returned alongside tokens.
fn lex(src: &str) -> (Vec<Tok>, Vec<Diag>) {
    let mut toks = Vec::new();
    let mut diags = Vec::new();
    let mut chars = src.char_indices().peekable();
    let mut line: usize = 0;
    let mut col: usize = 0;

    while let Some((_, c)) = chars.peek().copied() {
        if c == '\n' {
            line += 1;
            col = 0;
            chars.next();
            continue;
        }
        if c == '\r' || c.is_whitespace() {
            col += 1;
            chars.next();
            continue;
        }

        if c == '"' {
            let start_line = line;
            let start_col = col;
            col += 1;
            let _ = chars.next(); // opening quote
            let mut s = String::new();
            let mut closed = false;
            while let Some((_, k)) = chars.peek().copied() {
                col += 1;
                let _ = chars.next();
                if k == '\n' {
                    line += 1;
                    // unterminated string: stop at newline.
                    break;
                }
                if k == '"' {
                    closed = true;
                    break;
                }
                if k == '\\' {
                    if let Some((_, n)) = chars.peek().copied() {
                        col += 1;
                        chars.next();
                        s.push(n);
                    }
                    continue;
                }
                s.push(k);
            }
            let len = col - start_col;
            if !closed {
                diags.push(Diag {
                    line: start_line,
                    col: start_col,
                    len: len.max(1),
                    severity: 1,
                    message: "Unterminated string literal".into(),
                });
            }
            toks.push(Tok { kind: Kind::Str, text: s, line: start_line, col: start_col, len });
            continue;
        }

        if c.is_ascii_alphabetic() {
            let start_line = line;
            let start_col = col;
            let mut s = String::new();
            while let Some((_, k)) = chars.peek().copied() {
                if k.is_ascii_alphanumeric() {
                    col += 1;
                    s.push(chars.next().unwrap().1);
                } else {
                    break;
                }
            }
            let lower = s.to_ascii_lowercase();
            let kind = if lower == "true" || lower == "false" {
                Kind::Bool
            } else {
                Kind::Ident
            };
            let len = s.chars().count();
            toks.push(Tok { kind, text: s, line: start_line, col: start_col, len });
            continue;
        }

        if c.is_ascii_digit() || c == '-' {
            let start_line = line;
            let start_col = col;
            let mut s = String::new();
            while let Some((_, k)) = chars.peek().copied() {
                if k.is_ascii_digit() || k == '.' || k == '-' || k == '+' || k == 'e' || k == 'E' {
                    col += 1;
                    s.push(chars.next().unwrap().1);
                } else {
                    break;
                }
            }
            let len = s.chars().count();
            toks.push(Tok { kind: Kind::Num, text: s, line: start_line, col: start_col, len });
            continue;
        }

        // Punctuation / operators.
        let start_line = line;
        let start_col = col;
        let ch = chars.next().unwrap().1;
        col += 1;
        if "{}()=,;".contains(ch) {
            toks.push(Tok { kind: Kind::Punct, text: ch.to_string(), line: start_line, col: start_col, len: 1 });
        } else {
            diags.push(Diag {
                line: start_line,
                col: start_col,
                len: 1,
                severity: 1,
                message: format!("Unexpected character `{ch}`"),
            });
            toks.push(Tok { kind: Kind::Unknown, text: ch.to_string(), line: start_line, col: start_col, len: 1 });
        }
    }

    (toks, diags)
}

// ── Parser + AST ────────────────────────────────────────────────────────────

struct Diag {
    line: usize,
    col: usize,
    len: usize,
    severity: u64,
    message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Keyword,   // flow / step
    FlowName,  // the identifier after `flow`
    Action,    // the identifier after `step` (the built-in action)
    Param,     // the identifier before `=`
    Str,
    Num,
    Bool,
}

struct SemToken {
    role: Role,
    line: usize,
    col: usize,
    len: usize,
}

/// Parse the ForgeFlow DSL grammar:
///   flow = "flow" identifier "{" step* "}"
///   step = "step" identifier "(" param_list? ")"
///   param_list = param ("," param)*
///   param = identifier "=" value
///   value = STRING | NUMBER | BOOLEAN
///
/// Tolerant: reports the first structural error positioned at the offending
/// token and stops, while still emitting semantic tokens for everything lexed.
fn parse(toks: &[Tok], lex_diags: Vec<Diag>) -> (Vec<Diag>, Vec<SemToken>) {
    let mut diags = lex_diags;
    let mut sems: Vec<SemToken> = Vec::new();
    let mut i = 0usize;

    let push_sem = |sems: &mut Vec<SemToken>, role: Role, t: &Tok| {
        sems.push(SemToken { role, line: t.line, col: t.col, len: t.len });
    };

    while i < toks.len() {
        let t = &toks[i];
        if t.kind == Kind::Ident && t.text == "flow" {
            push_sem(&mut sems, Role::Keyword, t);
            i += 1;
            if let Some(name) = toks.get(i) {
                if name.kind == Kind::Ident {
                    push_sem(&mut sems, Role::FlowName, name);
                    i += 1;
                } else {
                    diags.push(err(name, "expected flow name after `flow`"));
                    i += 1;
                    continue;
                }
            } else {
                diags.push(err(t, "expected flow name and `{` after `flow`"));
                break;
            }
            if !expect_punct(&toks, &mut i, '{', &mut diags) {
                break;
            }
            // step*
            loop {
                if let Some(peek) = toks.get(i) {
                    if peek.kind == Kind::Punct && peek.text == "}" {
                        i += 1;
                        break;
                    }
                    if peek.kind == Kind::Ident && peek.text == "step" {
                        push_sem(&mut sems, Role::Keyword, peek);
                        i += 1;
                        if let Some(act) = toks.get(i) {
                            if act.kind == Kind::Ident {
                                push_sem(&mut sems, Role::Action, act);
                                i += 1;
                            } else {
                                diags.push(err(act, "expected action name after `step`"));
                                i += 1;
                                break;
                            }
                        } else {
                            diags.push(err(peek, "expected action name and `(` after `step`"));
                            break;
                        }
                        if !expect_punct(&toks, &mut i, '(', &mut diags) {
                            break;
                        }
                        // param_list?
                        if let Some(peek) = toks.get(i) {
                            if peek.kind == Kind::Ident {
                                // params
                                loop {
                                    let p = toks.get(i).unwrap();
                                    if p.kind != Kind::Ident {
                                        diags.push(err(p, "expected parameter name"));
                                        break;
                                    }
                                    push_sem(&mut sems, Role::Param, p);
                                    i += 1;
                                    if !expect_punct(&toks, &mut i, '=', &mut diags) {
                                        break;
                                    }
                                    if let Some(v) = toks.get(i) {
                                        match v.kind {
                                            Kind::Str => push_sem(&mut sems, Role::Str, v),
                                            Kind::Num => push_sem(&mut sems, Role::Num, v),
                                            Kind::Bool => push_sem(&mut sems, Role::Bool, v),
                                            _ => {
                                                diags.push(err(v, "expected value (string, number, or boolean)"));
                                            }
                                        }
                                        i += 1;
                                    } else {
                                        diags.push(err(p, "expected value after `=`"));
                                        break;
                                    }
                                    if let Some(c) = toks.get(i) {
                                        if c.kind == Kind::Punct && c.text == "," {
                                            i += 1;
                                            continue;
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        if !expect_punct(&toks, &mut i, ')', &mut diags) {
                            break;
                        }
                        continue;
                    }
                    // something unexpected inside the flow body
                    diags.push(err(peek, "expected `step` or `}` inside flow"));
                    i += 1;
                    continue;
                } else {
                    diags.push(err(t, "unterminated flow: missing `}`"));
                    break;
                }
            }
        } else if t.kind == Kind::Punct {
            i += 1; // skip stray punctuation
        } else {
            diags.push(err(t, &format!("unexpected token `{}` at top level", t.text)));
            i += 1;
        }
    }

    (diags, sems)
}

fn expect_punct(toks: &[Tok], i: &mut usize, want: char, diags: &mut Vec<Diag>) -> bool {
    match toks.get(*i) {
        Some(t) if t.kind == Kind::Punct && t.text == want.to_string() => {
            *i += 1;
            true
        }
        Some(t) => {
            diags.push(err(t, &format!("expected `{want}`")));
            false
        }
        None => {
            diags.push(Diag {
                line: 0,
                col: 0,
                len: 1,
                severity: 1,
                message: format!("expected `{want}` at end of file"),
            });
            false
        }
    }
}

fn err(t: &Tok, message: &str) -> Diag {
    Diag { line: t.line, col: t.col, len: t.len.max(1), severity: 1, message: message.into() }
}

// ── Built-in actions ────────────────────────────────────────────────────────

/// Sample registry of built-in Forge runtime actions. This is an extensible
/// catalogue; hover and completion read from it. (Phase 4 would wire these to
/// the actual Forge runtime execution model.)
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
    ]
}

fn builtin_doc(name: &str) -> Option<&'static str> {
    builtin_actions().iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
}

// ── Semantic tokens ─────────────────────────────────────────────────────────

const SEM_LEGEND: &[&str] = &["keyword", "function", "variable", "string", "number", "boolean"];

fn role_to_index(role: Role) -> u32 {
    match role {
        Role::Keyword => 0,
        Role::Action => 1,
        Role::FlowName | Role::Param => 2,
        Role::Str => 3,
        Role::Num => 4,
        Role::Bool => 5,
    }
}

/// Encode semantic tokens using the LSP relative-position delta scheme.
fn encode_semantic_tokens(sems: &[SemToken]) -> Vec<u32> {
    let mut data = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for s in sems {
        let line = s.line as u32;
        let col = s.col as u32;
        let delta_line = line - prev_line;
        let delta_char = if delta_line == 0 { col - prev_char } else { col };
        data.push(delta_line);
        data.push(delta_char);
        data.push(s.len as u32);
        data.push(role_to_index(s.role));
        data.push(0); // no modifiers
        prev_line = line;
        prev_char = col;
    }
    data
}

// ── Diagnostics / analysis dispatch by extension ─────────────────────────────

/// Returns (diagnostics, semantic_tokens) for a document, choosing the
/// analysis strategy from its extension.
fn analyze(uri: &str, text: &str) -> (Vec<Diag>, Vec<SemToken>) {
    let ext = uri.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "fwrk" | "fdgn" => {
            // DSL text. `.fdgn` permits JSON too, so try JSON first.
            if ext == "fdgn" {
                if let Ok(serde_json::Value::Object(_)) = serde_json::from_str::<serde_json::Value>(text) {
                    return json_diags(text);
                }
            }
            let (toks, lex_diags) = lex(text);
            parse(&toks, lex_diags)
        }
        "fmeta" => json_diags(text),
        "forge" => (
            vec![Diag {
                line: 0,
                col: 0,
                len: 1,
                severity: 3, // info
                message: "`.forge` is a binary project container — not editable as text. Open the `.fwrk`/`.fdgn`/`.fmeta` sources instead.".into(),
            }],
            Vec::new(),
        ),
        _ => (Vec::new(), Vec::new()),
    }
}

/// JSON-based analysis (`.fdgn`/`fmeta`): parse and report JSON errors, plus
/// flag `ENC:`-prefixed secret fields in `.fmeta`.
fn json_diags(text: &str) -> (Vec<Diag>, Vec<SemToken>) {
    let mut diags = Vec::new();
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => {}
        Err(e) if e.is_io() => {}
        Err(e) => {
            diags.push(Diag {
                line: e.line().saturating_sub(1) as usize,
                col: e.column().saturating_sub(1) as usize,
                len: 1,
                severity: 1,
                message: e.to_string(),
            });
        }
    }

    let is_fmeta = text.contains("ENC:");
    if is_fmeta {
        for (lineno, line) in text.lines().enumerate() {
            if line.contains("ENC:") {
                if let Some(start) = line.find("ENC:") {
                    diags.push(Diag {
                        line: lineno,
                        col: line[..start].chars().count(),
                        len: line[start..].chars().count().min(40),
                        severity: 2, // warning
                        message: "Encrypted secret field (`ENC:`). Stays encrypted at rest; resolved only by the Forge runtime.".into(),
                    });
                }
            }
        }
    }
    (diags, Vec::new())
}

fn diag_to_json(_uri: &str, d: &Diag) -> Value {
    json!({
        "range": {
            "start": { "line": d.line as u64, "character": d.col as u64 },
            "end": { "line": d.line as u64, "character": (d.col + d.len) as u64 }
        },
        "severity": d.severity,
        "source": "forgeflow",
        "message": d.message
    })
}

// ── Completion ───────────────────────────────────────────────────────────────

fn completions_for(toks: &[Tok], line: usize, character: usize) -> Vec<Value> {
    // Token immediately before the cursor on its line (if any).
    let mut prev: Option<&Tok> = None;
    for t in toks {
        if t.line > line || (t.line == line && t.col + t.len > character) {
            break;
        }
        if t.line == line && t.col + t.len <= character {
            prev = Some(t);
        }
    }

    let mut items: Vec<Value> = Vec::new();
    match prev {
        None => {
            // start of line: suggest top-level keywords
            for (label, doc) in [("flow", "Define a workflow flow"), ("step", "Add a step to a flow")] {
                items.push(item(label, 14, doc));
            }
        }
        Some(t) if t.text == "step" => {
            for (name, doc) in builtin_actions() {
                items.push(item(name, 3, doc));
            }
        }
        Some(t) if t.text == "=" => {
            for (label, doc) in [("\"\"", "string value"), ("0", "number value"), ("true", "boolean value"), ("false", "boolean value")] {
                items.push(item(label, 1, doc));
            }
        }
        Some(_) => {
            // generic: keyword + actions
            items.push(item("flow", 14, "Define a workflow flow"));
            items.push(item("step", 14, "Add a step to a flow"));
            for (name, doc) in builtin_actions() {
                items.push(item(name, 3, doc));
            }
        }
    }
    items
}

fn item(label: &str, kind: u64, detail: &str) -> Value {
    json!({ "label": label, "kind": kind, "detail": detail })
}

// ── Hover ────────────────────────────────────────────────────────────────────

fn hover_at(toks: &[Tok], sems: &[SemToken], line: usize, character: usize) -> Option<String> {
    // Prefer action role under cursor for built-in docs.
    for s in sems {
        if s.line == line && character >= s.col && character <= s.col + s.len {
            match s.role {
                Role::Action => {
                    let name: String = toks
                        .iter()
                        .find(|t| t.line == s.line && t.col == s.col && t.kind == Kind::Ident)
                        .map(|t| t.text.clone())
                        .unwrap_or_default();
                    if let Some(doc) = builtin_doc(&name) {
                        return Some(format!("**{name}** — built-in action\n\n{doc}"));
                    }
                }
                Role::Keyword => {
                    return Some("ForgeFlow keyword. `flow` opens a workflow; `step` adds an action.".into());
                }
                Role::Param => {
                    return Some("Parameter name. Syntax: `name = value`.".into());
                }
                _ => {}
            }
        }
    }
    // Fallback: string/number/boolean hover
    for t in toks {
        if t.line == line && character >= t.col && character <= t.col + t.len {
            return match t.kind {
                Kind::Str => Some("String value".into()),
                Kind::Num => Some("Number value".into()),
                Kind::Bool => Some("Boolean value".into()),
                _ => None,
            };
        }
    }
    None
}

// ── Server loop ─────────────────────────────────────────────────────────────

fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    let mut documents: HashMap<String, String> = HashMap::new();

    while let Some(msg) = read_message(&mut reader) {
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();

        match method {
            "initialize" => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "completionProvider": { "triggerCharacters": ["{", "(", ",", "="] },
                            "hoverProvider": true,
                            "semanticTokensProvider": {
                                "legend": {
                                    "tokenTypes": SEM_LEGEND,
                                    "tokenModifiers": []
                                },
                                "full": true
                            }
                        },
                        "serverInfo": { "name": "forge-lsp-forgeflow", "version": env!("CARGO_PKG_VERSION") }
                    }
                });
                write_message(&mut writer, &resp);
            }
            "initialized" => {}
            "shutdown" => {
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": null });
                write_message(&mut writer, &resp);
            }
            "exit" => break,
            "textDocument/didOpen" | "textDocument/didChange" => {
                let doc = msg.pointer("/params/textDocument");
                let uri = doc.and_then(|d| d.get("uri")).and_then(|u| u.as_str());
                let text = if method == "textDocument/didChange" {
                    msg.pointer("/params/contentChanges/0/text").and_then(|t| t.as_str())
                } else {
                    doc.and_then(|d| d.get("text")).and_then(|t| t.as_str())
                };
                if let (Some(uri), Some(text)) = (uri, text) {
                    documents.insert(uri.to_string(), text.to_string());
                    let (diags, _sems) = analyze(uri, text);
                    publish_diagnostics(&mut writer, uri, &diags);
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()) {
                    documents.remove(uri);
                }
            }
            "textDocument/completion" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let line = msg.pointer("/params/position/line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;
                let character = msg.pointer("/params/position/character").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, _) = lex(text);
                let items = completions_for(&toks, line, character);
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": { "isIncomplete": false, "items": items } });
                write_message(&mut writer, &resp);
            }
            "textDocument/hover" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let line = msg.pointer("/params/position/line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;
                let character = msg.pointer("/params/position/character").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, lex_diags) = lex(text);
                let (_diags, sems) = parse(&toks, lex_diags);
                let result = match hover_at(&toks, &sems, line, character) {
                    Some(md) => json!({ "contents": { "kind": "markdown", "value": md } }),
                    None => Value::Null,
                };
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                write_message(&mut writer, &resp);
            }
            "textDocument/semanticTokens/full" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, lex_diags) = lex(text);
                let (_diags, sems) = parse(&toks, lex_diags);
                let data = encode_semantic_tokens(&sems);
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": { "data": data } });
                write_message(&mut writer, &resp);
            }
            _ => {
                if id.is_some() {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("method not found: {method}") }
                    });
                    write_message(&mut writer, &resp);
                }
            }
        }
    }
}

fn publish_diagnostics(writer: &mut impl Write, uri: &str, diags: &[Diag]) {
    let diagnostics: Vec<Value> = diags.iter().map(|d| diag_to_json(uri, d)).collect();
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    });
    write_message(writer, &msg);
}
