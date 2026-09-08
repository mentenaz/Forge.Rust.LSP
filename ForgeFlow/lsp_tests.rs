//! Integration tests for `forge-lsp-forgeflow`.
//!
//! Spawns the real compiled binary and drives it over stdio with actual
//! `Content-Length`-framed JSON-RPC — the same way Forge (or any LSP
//! client) does. This replaces the ad-hoc Python scripts used to verify
//! `tak`/`been`, completion-context fixes, `documentHighlight`, import
//! validation, and `.fdgn` cross-reference validation during development:
//! those checks now run on every `cargo test` instead of needing to be
//! manually re-run and re-typed by whoever touches this code next.
//!
//! Run with: `cargo test -p forge-lsp-forgeflow`

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use serde_json::{json, Value};

struct LspClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl LspClient {
    fn start() -> Self {
        let exe = env!("CARGO_BIN_EXE_forge-lsp-forgeflow");
        let mut child = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn forge-lsp-forgeflow");
        let stdin = child.stdin.take().expect("no stdin");
        let stdout = child.stdout.take().expect("no stdout");
        let mut c = LspClient { child, stdin, reader: BufReader::new(stdout), next_id: 1 };
        c.request("initialize", json!({ "capabilities": {} }));
        c
    }

    fn write_message(&mut self, msg: &Value) {
        let body = serde_json::to_vec(msg).expect("serialize request");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        self.stdin.write_all(&body).expect("write body");
        self.stdin.flush().expect("flush");
    }

    fn read_message(&mut self) -> Value {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).expect("read header line");
            assert!(n > 0, "server closed stdout unexpectedly while reading headers");
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(idx) = line.to_ascii_lowercase().find("content-length:") {
                let val = line[idx + "content-length:".len()..].trim();
                content_length = val.parse().ok();
            }
        }
        let len = content_length.expect("response missing Content-Length header");
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf).expect("read body");
        serde_json::from_slice(&buf).expect("parse response JSON")
    }

    /// Sends a request and returns its `result`, skipping over any
    /// notifications that happen to arrive first (none should, in these
    /// tests, since every `did_open` drains its own `publishDiagnostics`
    /// before returning — this is just defensive).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let msg = self.read_message();
            if msg.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return msg.get("result").cloned().unwrap_or(Value::Null);
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write_message(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Opens a document (inferring `languageId` as `"forgeflow"`) and
    /// returns the `diagnostics` array from the `publishDiagnostics`
    /// notification the server pushes in response. Assumes exactly one
    /// such notification follows `didOpen` — matches the server's actual,
    /// verified behavior throughout this file.
    fn did_open(&mut self, uri: &str, text: &str) -> Vec<Value> {
        self.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": uri, "languageId": "forgeflow", "version": 1, "text": text } }),
        );
        loop {
            let msg = self.read_message();
            if msg.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                return msg["params"]["diagnostics"].as_array().cloned().unwrap_or_default();
            }
        }
    }

    fn completion(&mut self, uri: &str, line: u64, character: u64) -> Vec<Value> {
        let result = self.request(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        );
        result["items"].as_array().cloned().unwrap_or_default()
    }

    fn hover_text(&mut self, uri: &str, line: u64, character: u64) -> Option<String> {
        let result = self.request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        );
        result["contents"]["value"].as_str().map(|s| s.to_string())
    }

    fn document_highlight(&mut self, uri: &str, line: u64, character: u64) -> Vec<Value> {
        let result = self.request(
            "textDocument/documentHighlight",
            json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": character } }),
        );
        result.as_array().cloned().unwrap_or_default()
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn completion_labels(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(|i| i["label"].as_str().map(|s| s.to_string())).collect()
}

fn diag_messages(diags: &[Value]) -> Vec<String> {
    diags.iter().filter_map(|d| d["message"].as_str().map(|s| s.to_string())).collect()
}

// ── tak/been (SPEC.md §5, §10) ──────────────────────────────────────────────

#[test]
fn tak_been_valid_parses_clean() {
    let mut c = LspClient::start();
    let src = "flow Ingest() {\n    tak {\n        been FetchUser {\n            step http(method = \"GET\", url = \"https://api.forge.dev/user\")\n        }\n        been FetchOrders {\n            step http(method = \"GET\", url = \"https://api.forge.dev/orders\")\n        }\n    }\n}\n";
    let diags = c.did_open("file:///tak_valid.fwrk", src);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn tak_with_no_been_branches_errors() {
    let mut c = LspClient::start();
    let src = "flow Bad() {\n    tak {\n    }\n}\n";
    let diags = c.did_open("file:///tak_empty.fwrk", src);
    assert!(
        diag_messages(&diags).iter().any(|m| m.contains("at least one `been` branch")),
        "expected the empty-tak diagnostic, got {diags:?}"
    );
}

#[test]
fn fdgn_tak_fork_point_parses_clean() {
    let mut c = LspClient::start();
    let src = "tak fanOut {\n    title = \"Fan Out\"\n}\n\nnode a {\n    title = \"A\"\n}\n\nnode b {\n    title = \"B\"\n}\n\nedge fanOut -> a\nedge fanOut -> b\n";
    let diags = c.did_open("file:///tak_fork.fdgn", src);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

// ── completion context (semantic.rs Ctx resolution) ─────────────────────────

#[test]
fn completion_inside_tak_offers_only_been() {
    let mut c = LspClient::start();
    let src = "flow Ingest() {\n    tak {\n    }\n}\n";
    c.did_open("file:///comp_tak.fwrk", src);
    // cursor right after "tak {" on line 1 ("    tak {" -- brace ends at col 9)
    let items = c.completion("file:///comp_tak.fwrk", 1, 9);
    let labels = completion_labels(&items);
    assert!(labels.contains(&"been".to_string()), "expected `been` in {labels:?}");
    assert!(!labels.contains(&"step".to_string()), "`step` should not appear inside a bare `tak {{}}`, got {labels:?}");
}

#[test]
fn completion_on_blank_line_in_flow_body_offers_statements() {
    // The cross-line prev-token bug fix: a blank line after `flow X() {`
    // (pressing Enter) must still resolve statement-position completions,
    // not fall back to the wrong context.
    let mut c = LspClient::start();
    let src = "flow X() {\n    \n}\n";
    c.did_open("file:///comp_blank.fwrk", src);
    let items = c.completion("file:///comp_blank.fwrk", 1, 4);
    let labels = completion_labels(&items);
    for expected in ["step", "laat", "as", "terwyl", "tak", "gee", "elk"] {
        assert!(labels.contains(&expected.to_string()), "expected `{expected}` in {labels:?}");
    }
}

#[test]
fn completion_true_top_level_fwrk_is_exactly_three_keywords() {
    let mut c = LspClient::start();
    let src = "flow X() {\n    step log(level = \"info\", text = \"hi\")\n}\n\n";
    c.did_open("file:///comp_top.fwrk", src);
    let items = c.completion("file:///comp_top.fwrk", 3, 0);
    let labels = completion_labels(&items);
    for expected in ["gbk", "soort", "flow"] {
        assert!(labels.contains(&expected.to_string()), "expected `{expected}` in {labels:?}");
    }
    for forbidden in ["laat", "anders"] {
        assert!(!labels.contains(&forbidden.to_string()), "`{forbidden}` is not valid at .fwrk top level, got {labels:?}");
    }
}

#[test]
fn completion_true_top_level_fdgn_includes_edge() {
    let mut c = LspClient::start();
    let src = "node a {\n    title = \"A\"\n}\n\n";
    c.did_open("file:///comp_top.fdgn", src);
    let items = c.completion("file:///comp_top.fdgn", 3, 0);
    let labels = completion_labels(&items);
    for expected in ["gbk", "node", "tak", "edge"] {
        assert!(labels.contains(&expected.to_string()), "expected `{expected}` in {labels:?}");
    }
    assert!(!labels.contains(&"anders".to_string()), "`anders` is not valid at .fdgn top level, got {labels:?}");
}

// ── hover (semantic.rs hover_at) ─────────────────────────────────────────────

#[test]
fn hover_on_tak_and_been_are_distinct() {
    let mut c = LspClient::start();
    let src = "flow Ingest() {\n    tak {\n        been FetchUser {\n        }\n    }\n}\n";
    c.did_open("file:///hover.fwrk", src);
    let tak_doc = c.hover_text("file:///hover.fwrk", 1, 5).unwrap_or_default();
    let been_doc = c.hover_text("file:///hover.fwrk", 2, 10).unwrap_or_default();
    assert!(tak_doc.contains("Parallel fork"), "unexpected tak hover: {tak_doc}");
    assert!(been_doc.contains("named branch"), "unexpected been hover: {been_doc}");
}

// ── document highlight (semantic.rs document_highlight_at) ──────────────────

#[test]
fn document_highlight_node_excludes_unrelated_node() {
    let mut c = LspClient::start();
    let src = "tak fanOut {\n    title = \"Fan Out\"\n}\n\nnode a {\n    title = \"A\"\n}\n\nnode b {\n    title = \"B\"\n}\n\nedge fanOut -> a\nedge fanOut -> b\n";
    c.did_open("file:///dh.fdgn", src);
    // "a" in `node a {` -- line 4, col 5
    let ranges = c.document_highlight("file:///dh.fdgn", 4, 5);
    assert_eq!(ranges.len(), 2, "expected exactly 2 ranges (declaration + its own edge), got {ranges:?}");
    for r in &ranges {
        let line = r["range"]["start"]["line"].as_u64().unwrap();
        assert_ne!(line, 8, "node `b`'s declaration must not be highlighted, got {ranges:?}");
    }
}

#[test]
fn document_highlight_scopes_variable_to_its_own_flow() {
    let mut c = LspClient::start();
    let src = "flow First(x: nmr) {\n    laat y = x + 1\n}\n\nflow Second(x: lyn) {\n    step log(level = \"info\", text = x)\n}\n";
    c.did_open("file:///dh_scope.fwrk", src);
    // "x" in First's parameter list -- line 0, col 11
    let ranges = c.document_highlight("file:///dh_scope.fwrk", 0, 11);
    assert_eq!(ranges.len(), 2, "First's `x` should have exactly 2 ranges (its own decl + its own use), got {ranges:?}");
    for r in &ranges {
        let line = r["range"]["start"]["line"].as_u64().unwrap();
        assert!(line <= 2, "First's `x` must not reach into Second's flow, got {ranges:?}");
    }
}

// ── import validation (server.rs validate_imports) ───────────────────────────

#[test]
fn import_validation_flags_missing_name_and_missing_file() {
    let dir = std::env::temp_dir().join(format!("ffgo_import_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    std::fs::write(dir.join("actions.fwrk"), "flow Notifier(msg: lyn) {\n    step log(level = \"info\", text = msg)\n}\n").expect("write actions.fwrk");

    let main_src = "gbk Notifier vannaf \"./actions.fwrk\"\ngbk MissingThing vannaf \"./actions.fwrk\"\ngbk Ghost vannaf \"./does-not-exist.fwrk\"\n\nflow Main() {\n    step log(level = \"info\", text = \"hi\")\n}\n";
    let uri = format!("file://{}/main.fwrk", dir.display());

    let mut c = LspClient::start();
    let diags = c.did_open(&uri, main_src);
    let messages = diag_messages(&diags);

    assert!(
        messages.iter().any(|m| m.contains("MissingThing") && m.contains("not declared")),
        "expected a MissingThing diagnostic, got {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("does-not-exist.fwrk") && m.contains("cannot resolve")),
        "expected a cannot-resolve diagnostic for Ghost's path, got {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("Notifier") && !m.contains("MissingThing")),
        "the valid `Notifier` import must not be flagged, got {messages:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ── .fdgn edge/tak cross-reference validation ────────────────────────────────

#[test]
fn fdgn_edge_forward_reference_is_valid_but_undeclared_target_errors() {
    let mut c = LspClient::start();
    let src = "node a {\n    title = \"A\"\n}\n\n// forward reference: b is declared after this edge\nedge a -> b\n\nnode b {\n    title = \"B\"\n}\n\nedge b -> ghost\n";
    let diags = c.did_open("file:///refs.fdgn", src);
    let messages = diag_messages(&diags);
    assert_eq!(messages.len(), 1, "expected exactly 1 diagnostic (the undeclared `ghost`), got {messages:?}");
    assert!(messages[0].contains("ghost") && messages[0].contains("not declared"), "unexpected diagnostic: {}", messages[0]);
}

// ── node/edge scoped to .fdgn only (grammar.rs FDGN_ONLY_KEYWORDS) ───────────

#[test]
fn node_and_edge_are_usable_as_fwrk_identifiers() {
    let mut c = LspClient::start();
    let src = "flow X() {\n    laat node = 5\n    laat edge = 10\n    step log(level = \"info\", text = node)\n}\n";
    let diags = c.did_open("file:///idents.fwrk", src);
    assert!(diags.is_empty(), "`node`/`edge` should be usable as .fwrk variable names, got {diags:?}");
}

#[test]
fn node_still_a_keyword_in_fdgn() {
    let mut c = LspClient::start();
    let src = "node a {\n    title = \"A\"\n}\n";
    let diags = c.did_open("file:///idents.fdgn", src);
    assert!(diags.is_empty(), "`node` must still work as the .fdgn keyword, got {diags:?}");
}

// ── comment semantic tokens (grammar.rs Role::Comment) ───────────────────────

#[test]
fn comment_semantic_token_is_emitted() {
    let mut c = LspClient::start();
    let src = "// a real comment\nflow X() {\n    step log(level = \"info\", text = \"hi\")\n}\n";
    c.did_open("file:///comment.fwrk", src);
    let result = c.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": "file:///comment.fwrk" } }),
    );
    let data = result["data"].as_array().expect("data array");
    // First token: [deltaLine=0, deltaChar=0, length=17, tokenType=10 (comment), modifiers=0]
    assert_eq!(data[0].as_u64(), Some(0));
    assert_eq!(data[1].as_u64(), Some(0));
    assert_eq!(data[2].as_u64(), Some(17), "comment token length should match `// a real comment`");
    assert_eq!(data[3].as_u64(), Some(10), "tokenType 10 is `comment` in the legend");
}
