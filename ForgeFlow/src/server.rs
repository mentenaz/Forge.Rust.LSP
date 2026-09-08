// ── ForgeFlow LSP server (Component 2 transport). ──
// JSON-RPC framing + editor-facing request handling. Depends on
// grammar (parse tree) and semantic (completions/hover).
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use serde_json::{Value, json};
use crate::grammar::{lex, parse_program, analyze, file_mode_for, encode_semantic_tokens, SEM_LEGEND, Diag, Tok, diag_to_json};
use crate::semantic::{Decl, DefinitionResult, Import, collect_declarations, collect_imports, completions_for, definition_at, hover_at};

fn read_message<R: BufRead>(reader: &mut R) -> Option<Value> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
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
// ── Server loop ─────────────────────────────────────────────────────────────

pub fn run() {
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
                            "completionProvider": { "triggerCharacters": ["{", "(", ",", "=", ":", "."] },
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "semanticTokensProvider": {
                                "legend": { "tokenTypes": SEM_LEGEND, "tokenModifiers": [] },
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
                    let (mut diags, _sems) = analyze(uri, text);
                    let (toks, _, _) = lex(text);
                    diags.extend(check_imports(&toks, uri, &documents));
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
                let (toks, _, _) = lex(text);
                let items = completions_for(line, character, &toks, file_mode_for(uri));
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": { "isIncomplete": false, "items": items } });
                write_message(&mut writer, &resp);
            }
            "textDocument/hover" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let line = msg.pointer("/params/position/line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;
                let character = msg.pointer("/params/position/character").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, lex_diags, _) = lex(text);
                let (_diags, sems) = parse_program(toks.clone(), lex_diags, file_mode_for(uri));
                let result = match hover_at(&toks, &sems, line, character, file_mode_for(uri)) {
                    Some(md) => json!({ "contents": { "kind": "markdown", "value": md } }),
                    None => Value::Null,
                };
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                write_message(&mut writer, &resp);
            }
            "textDocument/definition" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let line = msg.pointer("/params/position/line").and_then(|l| l.as_u64()).unwrap_or(0) as usize;
                let character = msg.pointer("/params/position/character").and_then(|c| c.as_u64()).unwrap_or(0) as usize;
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, _, _) = lex(text);
                let found = match definition_at(&toks, line, character) {
                    DefinitionResult::Local(decl) => Some((uri.to_string(), decl)),
                    DefinitionResult::Unresolved(name) => {
                        resolve_imported_definition(&toks, uri, &name, &documents)
                    }
                    DefinitionResult::None => None,
                };
                let result = match found {
                    Some((target_uri, decl)) => json!({
                        "uri": target_uri,
                        "range": {
                            "start": { "line": decl.line, "character": decl.col },
                            "end": { "line": decl.line, "character": decl.col + decl.len }
                        }
                    }),
                    None => Value::Null,
                };
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                write_message(&mut writer, &resp);
            }
            "textDocument/semanticTokens/full" => {
                let uri = msg.pointer("/params/textDocument/uri").and_then(|u| u.as_str()).unwrap_or("");
                let text = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let (toks, lex_diags, mut lex_sems) = lex(text);
                let (_diags, mut sems) = parse_program(toks, lex_diags, file_mode_for(uri));
                lex_sems.append(&mut sems);
                lex_sems.sort_by_key(|s| (s.line, s.col));
                let data = encode_semantic_tokens(&lex_sems);
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

/// Cross-file fallback for `textDocument/definition`: `name` wasn't declared
/// in the current document, so check whether it's a `gbk`-imported name and,
/// if so, resolve the import's path relative to the current file, load that
/// file's text (from the open-document cache if the client has it open too,
/// disk otherwise — imported files are routinely *not* open as their own
/// tab), and look for `name`'s declaration there.
///
/// Only one hop: an import chain (file A imports from B, which itself
/// imports the real declaration from C) isn't followed — the common case
/// (SPEC.md's own examples) is a flat "actions live in one shared file"
/// layout, and chasing an arbitrary-depth chain is a bigger feature than
/// this fallback is trying to be.
fn resolve_imported_definition(
    toks: &[Tok],
    current_uri: &str,
    name: &str,
    documents: &HashMap<String, String>,
) -> Option<(String, Decl)> {
    let import = collect_imports(toks).into_iter().find(|imp| imp.name == name)?;
    let (target_uri, target_text) = resolve_import_file(&import, current_uri, documents)?;
    let (target_toks, _, _) = lex(&target_text);
    let decl = *collect_declarations(&target_toks).get(name)?;
    Some((target_uri, decl))
}

/// Resolves one `gbk` import's path relative to `current_uri` and returns
/// the target file's `(uri, text)` — from the open-document cache if the
/// client has it open too (it routinely won't; an imported file is rarely
/// open as its own tab), disk otherwise. Shared by `resolve_imported_definition`
/// (go-to-definition's cross-file fallback) and `check_imports` (the
/// unresolved-import diagnostic below) so the path-resolution rules — the
/// extension default and `.` normalization — can't drift between the two.
fn resolve_import_file(
    import: &Import,
    current_uri: &str,
    documents: &HashMap<String, String>,
) -> Option<(String, String)> {
    let current_path = uri_to_path(current_uri)?;
    // An extension-less import path (`vannaf "./Found"`) always means
    // `.fwrk` — imports exist to pull in behavior (`flow`/`soort`
    // declarations), and behavior only ever lives in `.fwrk`; `.fdgn` is
    // structural-only (SPEC.md §10: no `flow`/`step` grammar there) and was
    // never a legal import target to begin with. A path that already names
    // an extension (`"./actions.fwrk"`) is left exactly as written.
    let import_path = if std::path::Path::new(&import.path).extension().is_none() {
        format!("{}.fwrk", import.path)
    } else {
        import.path.clone()
    };
    let joined = std::path::Path::new(&current_path).parent()?.join(&import_path);
    // `.components()` normalizes away `.` segments (e.g. the `./` in
    // `"./actions.fwrk"`) without needing the file to actually exist yet —
    // unlike `canonicalize()`, which would also resolve symlinks and `..`
    // but requires the path to exist and touches the filesystem again.
    let target_path: std::path::PathBuf = joined.components().collect();
    let target_uri = path_to_uri(&target_path.to_string_lossy());

    let target_text = match documents.get(&target_uri) {
        Some(text) => text.clone(),
        None => std::fs::read_to_string(&target_path).ok()?,
    };
    Some((target_uri, target_text))
}

/// Diagnostics for every `gbk` import in the file: an error at the path
/// string if it doesn't resolve to a readable file at all, or an error at
/// the imported name if the file resolves but doesn't declare that name
/// (the `actions.fwrk`-exists-but-has-no-`flow http`-case). Both were
/// previously silent — the language had no diagnostic for either, only a
/// go-to-definition that quietly did nothing (see `resolve_imported_definition`).
fn check_imports(toks: &[Tok], current_uri: &str, documents: &HashMap<String, String>) -> Vec<Diag> {
    let mut diags = Vec::new();
    for import in collect_imports(toks) {
        match resolve_import_file(&import, current_uri, documents) {
            None => {
                let (line, col, len) = import.path_pos;
                diags.push(Diag::new(
                    line,
                    col,
                    len,
                    1,
                    format!("Import path not found: \"{}\"", import.path),
                ));
            }
            Some((_, target_text)) => {
                let (target_toks, _, _) = lex(&target_text);
                if !collect_declarations(&target_toks).contains_key(&import.name) {
                    let (line, col, len) = import.name_pos;
                    diags.push(Diag::new(
                        line,
                        col,
                        len,
                        1,
                        format!("`{}` is not declared in \"{}\"", import.name, import.path),
                    ));
                }
            }
        }
    }
    diags
}

/// Minimal `file://` URI → native filesystem path, matching this server's
/// only actual need (reading an imported file off disk) — no percent-
/// decoding, no drive-letter case normalization. Forge_GPUI's own client
/// already compares returned definition URIs case-insensitively against its
/// open tabs (its `path_to_uri` follows the WHATWG standard and lowercases
/// the drive letter), so a case mismatch here doesn't break navigation.
fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:///")?;
    Some(rest.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// The inverse of `uri_to_path`, for the `uri` field of a cross-file
/// definition response.
fn path_to_uri(path: &str) -> String {
    format!("file:///{}", path.replace('\\', "/"))
}

fn publish_diagnostics(writer: &mut impl Write, uri: &str, diags: &[Diag]) {
    let diagnostics: Vec<Value> = diags.iter().map(|d| diag_to_json(d)).collect();
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    });
    write_message(writer, &msg);
}
