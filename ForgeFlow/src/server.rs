// ── ForgeFlow LSP server (Component 2 transport). ──
// JSON-RPC framing + editor-facing request handling. Depends on
// grammar (parse tree) and semantic (completions/hover).
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use serde_json::{Value, json};
use crate::grammar::{lex, parse_program, analyze, file_mode_for, encode_semantic_tokens, SEM_LEGEND, Diag, diag_to_json};
use crate::semantic::{completions_for, hover_at};

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

fn publish_diagnostics(writer: &mut impl Write, uri: &str, diags: &[Diag]) {
    let diagnostics: Vec<Value> = diags.iter().map(|d| diag_to_json(d)).collect();
    let msg = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": uri, "diagnostics": diagnostics }
    });
    write_message(writer, &msg);
}
