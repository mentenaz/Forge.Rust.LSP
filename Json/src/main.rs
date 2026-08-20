//! Forge JSON language server.
//!
//! A minimal, hand-rolled LSP server speaking JSON-RPC 2.0 over stdio with
//! `Content-Length` framing — the mirror image of Forge's LSP client. It
//! provides real parse-error diagnostics (via `serde_json`) and simple
//! key/value completion. This is the reference package proving the full
//! Forge pipeline: download -> spawn -> didOpen -> diagnostics.

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

// ── Completion ──────────────────────────────────────────────────────────────

fn completions_for(document: &str, line: usize, character: usize) -> Vec<Value> {
    let lines: Vec<&str> = document.lines().collect();
    let Some(line_text) = lines.get(line) else {
        return Vec::new();
    };
    let up_to = &line_text[..line_text.len().min(character)];
    let prefix = up_to.trim_end();
    let last = prefix.chars().last();

    let items = match last {
        Some('{') | Some(',') => vec!["\"name\"", "\"value\"", "\"enabled\"", "\"id\""],
        Some(':') => vec!["true", "false", "null", "\"\""],
        _ => Vec::new(),
    };

    items
        .into_iter()
        .map(|label| json!({ "label": label, "kind": 6 }))
        .collect()
}

// ── Hover ───────────────────────────────────────────────────────────────────

/// Describes whatever JSON token sits under `(line, character)`: a quoted
/// string (key or value, inferred from what follows the closing quote) or a
/// bare literal (`true`/`false`/`null`/a number). Text/line based, same
/// heuristic style as `completions_for` above — no real JSON parser needed
/// for this level of detail.
fn hover_at(document: &str, line: usize, character: usize) -> Option<String> {
    let lines: Vec<&str> = document.lines().collect();
    let line_text = *lines.get(line)?;
    let chars: Vec<char> = line_text.chars().collect();
    let idx = character.min(chars.len());

    if let Some((start, end)) = quoted_span_at(&chars, idx) {
        let text: String = chars[start + 1..end].iter().collect();
        let after: String = chars[end + 1..].iter().collect();
        let kind = if after.trim_start().starts_with(':') { "key" } else { "string value" };
        return Some(format!("\"{text}\" — {kind}"));
    }

    let (start, end) = word_span_at(&chars, idx)?;
    let text: String = chars[start..end].iter().collect();
    let kind = match text.as_str() {
        "true" | "false" => "boolean value",
        "null" => "null value",
        _ if text.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-') => "number value",
        _ => return None,
    };
    Some(format!("`{text}` — {kind}"))
}

/// If `idx` falls strictly inside an unescaped `"..."` pair on the line,
/// returns the (start, end) char indices of the quotes.
fn quoted_span_at(chars: &[char], idx: usize) -> Option<(usize, usize)> {
    let mut in_string = false;
    let mut start = 0;
    for i in 0..chars.len() {
        if chars[i] != '"' || (i > 0 && chars[i - 1] == '\\') {
            continue;
        }
        if !in_string {
            in_string = true;
            start = i;
        } else {
            if idx > start && idx <= i {
                return Some((start, i));
            }
            in_string = false;
        }
    }
    None
}

/// Finds the contiguous run of "bare literal" characters (alphanumeric plus
/// `-`/`.`/`+`, covering numbers and `true`/`false`/`null`) touching `idx`.
fn word_span_at(chars: &[char], idx: usize) -> Option<(usize, usize)> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+';
    let at = |i: usize| chars.get(i).copied().is_some_and(is_word);
    if !at(idx) && !(idx > 0 && at(idx - 1)) {
        return None;
    }
    let mut start = idx.min(chars.len().saturating_sub(1));
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = idx;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end { None } else { Some((start, end)) }
}

// ── Server loop ─────────────────────────────────────────────────────────────

fn main() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    // uri -> document text, plus the base path for position math.
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
                            "completionProvider": { "triggerCharacters": ["{", ",", ":"] },
                            "hoverProvider": true
                        },
                        "serverInfo": { "name": "forge-lsp-json", "version": env!("CARGO_PKG_VERSION") }
                    }
                });
                write_message(&mut writer, &resp);
            }
            "initialized" => { /* no-op */ }
            "shutdown" => {
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": null });
                write_message(&mut writer, &resp);
            }
            "exit" => break,
            "textDocument/didOpen" => {
                if let Some(doc) = msg.pointer("/params/textDocument") {
                    if let (Some(uri), Some(text)) = (
                        doc.get("uri").and_then(|u| u.as_str()),
                        doc.get("text").and_then(|t| t.as_str()),
                    ) {
                        documents.insert(uri.to_string(), text.to_string());
                        publish_diagnostics(&mut writer, uri, text);
                    }
                }
            }
            "textDocument/didChange" => {
                if let Some(doc) = msg.pointer("/params/textDocument") {
                    if let Some(uri) = doc.get("uri").and_then(|u| u.as_str()) {
                        if let Some(text) = msg
                            .pointer("/params/contentChanges/0/text")
                            .and_then(|t| t.as_str())
                        {
                            documents.insert(uri.to_string(), text.to_string());
                            publish_diagnostics(&mut writer, uri, text);
                        }
                    }
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = msg
                    .pointer("/params/textDocument/uri")
                    .and_then(|u| u.as_str())
                {
                    documents.remove(uri);
                }
            }
            "textDocument/completion" => {
                let uri = msg
                    .pointer("/params/textDocument/uri")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                let line = msg
                    .pointer("/params/position/line")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0) as usize;
                let character = msg
                    .pointer("/params/position/character")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0) as usize;
                let doc = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let items = completions_for(doc, line, character);
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": { "isIncomplete": false, "items": items } });
                write_message(&mut writer, &resp);
            }
            "textDocument/hover" => {
                let uri = msg
                    .pointer("/params/textDocument/uri")
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                let line = msg
                    .pointer("/params/position/line")
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0) as usize;
                let character = msg
                    .pointer("/params/position/character")
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0) as usize;
                let doc = documents.get(uri).map(|s| s.as_str()).unwrap_or("");
                let result = match hover_at(doc, line, character) {
                    Some(text) => json!({ "contents": { "kind": "plaintext", "value": text } }),
                    None => Value::Null,
                };
                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                write_message(&mut writer, &resp);
            }
            _ => {
                // Unknown request: respond method-not-found so the client isn't left hanging.
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

/// Parse `text` and push `textDocument/publishDiagnostics` for any errors.
fn publish_diagnostics(writer: &mut impl Write, uri: &str, text: &str) {
    let diagnostics: Vec<Value> = match serde_json::from_str::<Value>(text) {
        Ok(_) => Vec::new(),
        Err(e) if e.is_io() => Vec::new(),
        Err(e) => vec![json!({
            "range": {
                "start": { "line": (e.line().saturating_sub(1)) as u64, "character": e.column().saturating_sub(1) as u64 },
                "end": { "line": (e.line().saturating_sub(1)) as u64, "character": e.column() as u64 }
            },
            "severity": 1,
            "message": e.to_string()
        })],
    };

    let msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    });
    write_message(writer, &msg);
}