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
                            "completionProvider": { "triggerCharacters": ["{", ",", ":"] }
                        },
                        "serverInfo": { "name": "forge-lsp-json", "version": "0.1.0" }
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