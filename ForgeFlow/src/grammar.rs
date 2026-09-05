// ── ForgeFlow grammar: lexer + parser + token model (Component 1). ──
// Parses raw .fwrk/.fdgn text into tokens + a diagnostic/semantic-token
// stream. No type checking, no completions, no execution. See
// DOCS/forgeflow-architecture-split.md.
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Kw,     // keyword: gbk vannaf flow soort laat step gee as anders terwyl elk
    Type,   // type primitive: lyn nmr vraag objk lys enige leeg
    Lit,    // literal keyword: waar onwaar niks
    Ident,
    Str,
    Num,
    Op,     // + - * / == != < > <= >= ! (unary)
    Punct,  // {}()[]<>=?.,;: and =>
    Comment,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Tok {
    pub kind: Kind,
    pub text: String,
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

const KEYWORDS: &[&str] = &[
    "gbk", "vannaf", "flow", "soort", "laat", "step", "gee", "as", "anders", "terwyl", "elk",
    "node", "edge",
];
pub const TYPE_KW: &[&str] = &["lyn", "nmr", "vraag", "objk", "lys", "enige", "leeg"];
/// Well-known `.fdgn` node fields (SPEC.md §10). Free-form in the parser but
/// offered as completions inside a `node { ... }` block.
const NODE_FIELDS: &[&str] = &["title", "action", "pos"];
const LIT_KW: &[&str] = &["waar", "onwaar", "niks"];

pub fn lex(src: &str) -> (Vec<Tok>, Vec<Diag>, Vec<SemToken>) {
    let mut toks = Vec::new();
    let mut diags = Vec::new();
    let mut comment_sems = Vec::new();
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut i = 0usize;
    let mut line: usize = 0;
    let mut col: usize = 0;

    let at = |i: usize| chars.get(i).copied();

    while let Some((_, c)) = at(i) {
        if c == '\n' {
            line += 1;
            col = 0;
            i += 1;
            continue;
        }
        if c == '\r' || c.is_whitespace() {
            col += 1;
            i += 1;
            continue;
        }

        // Comments (line + block) — tokenized as a `Comment` semantic token
        // but kept OUT of the parser's token stream (the parser never sees
        // them, so structure/diagnostics are unchanged).
        if c == '/' && at(i + 1).map(|(_, n)| n) == Some('/') {
            let start_line = line;
            let start_col = col;
            while let Some((_, n)) = at(i) {
                if n == '\n' {
                    break;
                }
                col += 1;
                i += 1;
            }
            let len = col - start_col;
            comment_sems.push(SemToken { role: Role::Comment, line: start_line, col: start_col, len });
            continue;
        }
        if c == '/' && at(i + 1).map(|(_, n)| n) == Some('*') {
            let start_line = line;
            let start_col = col;
            let mut clen = 2usize;
            col += 2;
            i += 2;
            while let Some((_, n)) = at(i) {
                if n == '\n' {
                    line += 1;
                    col = 0;
                    i += 1;
                    clen += 1;
                } else if n == '*' && at(i + 1).map(|(_, n)| n) == Some('/') {
                    col += 2;
                    i += 2;
                    clen += 2;
                    break;
                } else {
                    col += 1;
                    i += 1;
                    clen += 1;
                }
            }
            comment_sems.push(SemToken { role: Role::Comment, line: start_line, col: start_col, len: clen });
            continue;
        }

        if c == '"' {
            let start_line = line;
            let start_col = col;
            col += 1;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while let Some((_, k)) = at(i) {
                col += 1;
                i += 1;
                if k == '\n' {
                    line += 1;
                    break;
                }
                if k == '"' {
                    closed = true;
                    break;
                }
                if k == '\\' {
                    if let Some((_, n)) = at(i) {
                        col += 1;
                        i += 1;
                        s.push(n);
                    }
                    continue;
                }
                s.push(k);
            }
            let len = col - start_col;
            if !closed {
                diags.push(Diag { line: start_line, col: start_col, len: len.max(1), severity: 1, message: "Unterminated string literal".into() });
            }
            toks.push(Tok { kind: Kind::Str, text: s, line: start_line, col: start_col, len });
            continue;
        }

        if c.is_ascii_alphabetic() {
            let start_line = line;
            let start_col = col;
            let mut s = String::new();
            while let Some((_, k)) = at(i) {
                if k.is_ascii_alphanumeric() {
                    col += 1;
                    s.push(chars[i].1);
                    i += 1;
                } else {
                    break;
                }
            }
            let kind = if KEYWORDS.contains(&s.as_str()) {
                Kind::Kw
            } else if TYPE_KW.contains(&s.as_str()) {
                Kind::Type
            } else if LIT_KW.contains(&s.as_str()) || s == "true" || s == "false" {
                Kind::Lit
            } else {
                Kind::Ident
            };
            let len = s.chars().count();
            toks.push(Tok { kind, text: s, line: start_line, col: start_col, len });
            continue;
        }

        if c.is_ascii_digit() {
            let start_line = line;
            let start_col = col;
            let mut s = String::new();
            let mut seen_dot = false;
            let mut seen_exp = false;
            while let Some((_, k)) = at(i) {
                let is_digit = k.is_ascii_digit();
                let is_dot = k == '.' && !seen_dot && !seen_exp;
                let is_exp = (k == 'e' || k == 'E') && !seen_exp;
                let is_sign = (k == '-' || k == '+')
                    && seen_exp
                    && s.ends_with(|c| c == 'e' || c == 'E');
                if is_digit || is_dot || is_exp || is_sign {
                    if is_dot {
                        seen_dot = true;
                    }
                    if is_exp {
                        seen_exp = true;
                    }
                    col += 1;
                    s.push(k);
                    i += 1;
                } else {
                    break;
                }
            }
            let len = s.chars().count();
            toks.push(Tok { kind: Kind::Num, text: s, line: start_line, col: start_col, len });
            continue;
        }

        // Operators (incl. multi-char) and punctuation.
        let start_line = line;
        let start_col = col;
        let next = at(i + 1).map(|(_, n)| n);
        let (text, kind) = match c {
            '=' => {
                if next == Some('=') { (text2("=="), Kind::Op) }
                else if next == Some('>') { (text2("=>"), Kind::Punct) }
                else { (text1('='), Kind::Punct) }
            }
            '!' => {
                if next == Some('=') { (text2("!="), Kind::Op) }
                else { (text1('!'), Kind::Op) }
            }
            '&' => {
                if next == Some('&') { (text2("&&"), Kind::Op) }
                else { (text1('&'), Kind::Unknown) }
            }
            '|' => {
                if next == Some('|') { (text2("||"), Kind::Op) }
                else { (text1('|'), Kind::Unknown) }
            }
            '<' => {
                if next == Some('=') { (text2("<="), Kind::Op) }
                else { (text1('<'), Kind::Punct) }
            }
            '>' => {
                if next == Some('=') { (text2(">="), Kind::Op) }
                else { (text1('>'), Kind::Punct) }
            }
            '-' => {
                if next == Some('>') { (text2("->"), Kind::Punct) }
                else { (text1('-'), Kind::Op) }
            }
            '+' | '*' | '/' => (text1(c), Kind::Op),
            '{' | '}' | '(' | ')' | '[' | ']' | '?' | '.' | ',' | ';' | ':' => (text1(c), Kind::Punct),
            _ => (text1(c), Kind::Unknown),
        };
        let _ = chars[i].1;
        i += text.chars().count();
        col += text.chars().count();
        if kind == Kind::Unknown {
            diags.push(Diag { line: start_line, col: start_col, len: text.chars().count(), severity: 1, message: format!("Unexpected character `{text}`") });
        }
        let len = text.chars().count();
        toks.push(Tok { kind, text, line: start_line, col: start_col, len });
    }

    (toks, diags, comment_sems)
}

fn text1(c: char) -> String { c.to_string() }
fn text2(s: &str) -> String { s.to_string() }

// ── Parser core ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Diag {
    pub line: usize,
    pub col: usize,
    pub len: usize,
    severity: u64,
    message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Keyword,
    Control,
    Type,
    Function,
    Variable,
    Property,
    Operator,
    Str,
    Num,
    Bool,
    Comment,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemToken {
    pub role: Role,
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

pub const SEM_LEGEND: &[&str] = &[
    "keyword", "control", "type", "function", "variable", "property", "operator", "string",
    "number", "boolean", "comment",
];

fn role_to_index(role: Role) -> u32 {
    match role {
        Role::Keyword => 0,
        Role::Control => 1,
        Role::Type => 2,
        Role::Function => 3,
        Role::Variable => 4,
        Role::Property => 5,
        Role::Operator => 6,
        Role::Str => 7,
        Role::Num => 8,
        Role::Bool => 9,
        Role::Comment => 10,
    }
}

fn kw_role(s: &str) -> Role {
    match s {
        "as" | "anders" | "terwyl" => Role::Control,
        _ => Role::Keyword,
    }
}

/// Which top-level grammar a file is parsed against (SPEC.md §10). `.fdgn`
/// is a deliberately narrower grammar than `.fwrk` — imports/node/edge only,
/// no flow/step/soort/laat/control-flow — not a second general-purpose
/// language.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileMode {
    Fwrk,
    Fdgn,
}

pub fn file_mode_for(uri: &str) -> FileMode {
    let ext = uri.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if ext == "fdgn" { FileMode::Fdgn } else { FileMode::Fwrk }
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    diags: Vec<Diag>,
    sems: Vec<SemToken>,
    mode: FileMode,
    /// Suppresses `Ident { ... }` constructor-literal parsing while set.
    /// Set while parsing a `terwyl`/`as` condition, whose trailing `{`
    /// would otherwise be ambiguous with a constructor literal ending the
    /// condition (`terwyl x < retries { ... }` — is `retries {` a
    /// constructor, or is `{` the loop body?). Cleared whenever we
    /// descend into a delimited sub-expression (parens, brackets, call
    /// arguments, object-literal field values) where the ambiguity can't
    /// occur, so constructors still work there — same rule Go and Rust use
    /// for composite/struct literals in `if`/`for` conditions.
    no_brace_ctor: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }
    fn sem(&mut self, role: Role, t: &Tok) {
        self.sems.push(SemToken { role, line: t.line, col: t.col, len: t.len });
    }
    fn bump_as(&mut self, role: Role) -> Option<Tok> {
        let t = self.bump()?;
        self.sem(role, &t);
        Some(t)
    }
    fn err_at(&mut self, t: &Tok, msg: &str) {
        self.diags.push(Diag { line: t.line, col: t.col, len: t.len.max(1), severity: 1, message: msg.into() });
    }
    fn err_here(&mut self, msg: &str) {
        if let Some(t) = self.peek().cloned() {
            self.err_at(&t, msg);
        } else {
            self.diags.push(Diag { line: 0, col: 0, len: 1, severity: 1, message: msg.into() });
        }
    }
    fn cur_text(&self) -> String {
        self.toks.get(self.pos).map(|t| t.text.clone()).unwrap_or_default()
    }
    fn is_kw(&self, word: &str) -> bool {
        matches!(self.peek(), Some(t) if t.kind == Kind::Kw && t.text == word)
    }
    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(t) if t.kind == Kind::Punct && t.text == p)
    }
    fn bump_kw(&mut self, word: &str) -> bool {
        if self.is_kw(word) {
            self.bump_as(kw_role(word));
            true
        } else {
            self.err_here(&format!("expected `{word}`"));
            false
        }
    }
    fn bump_punct(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.bump();
            true
        } else {
            self.err_here(&format!("expected `{p}`"));
            false
        }
    }

    // ── top level ──
    fn run(&mut self) {
        match self.mode {
            FileMode::Fwrk => self.run_fwrk(),
            FileMode::Fdgn => self.run_fdgn(),
        }
    }

    fn run_fwrk(&mut self) {
        while !self.at_end() {
            if self.is_kw("gbk") {
                self.parse_import();
            } else if self.is_kw("soort") {
                self.parse_type_decl();
            } else if self.is_kw("flow") {
                self.parse_flow();
            } else if self.is_punct("}") {
                self.err_here("unexpected `}`");
                self.bump();
            } else {
                self.err_here("expected import, type, or flow at top level");
                self.bump();
            }
        }
    }

    /// `.fdgn` top level (SPEC.md §10): imports, `node`, `edge` only.
    fn run_fdgn(&mut self) {
        while !self.at_end() {
            if self.is_kw("gbk") {
                self.parse_import();
            } else if self.is_kw("node") {
                self.parse_node();
            } else if self.is_kw("edge") {
                self.parse_edge();
            } else if self.is_punct("}") {
                self.err_here("unexpected `}`");
                self.bump();
            } else if self.is_kw("flow") || self.is_kw("soort") || self.is_kw("step") || self.is_kw("laat") {
                self.err_here(&format!(
                    "`{}` is not valid in a .fdgn file — .fdgn only supports imports, node, and edge (see SPEC.md \u{a7}10)",
                    self.cur_text()
                ));
                self.bump();
            } else {
                self.err_here("expected import, node, or edge at top level");
                self.bump();
            }
        }
    }

    /// `gbk <name> vannaf "<path>"` or `gbk { <name>, <name>, ... } vannaf
    /// "<path>"` (SPEC.md §2) — both forms valid in `.fwrk` and `.fdgn`.
    fn parse_import(&mut self) {
        self.bump_kw("gbk"); // gbk
        if self.is_punct("{") {
            self.bump_punct("{");
            loop {
                if let Some(t) = self.peek() {
                    if t.kind == Kind::Ident {
                        self.bump_as(Role::Variable);
                    } else {
                        self.err_here("expected imported name");
                        self.bump();
                        continue;
                    }
                }
                if self.is_punct(",") {
                    self.bump();
                    continue;
                }
                break;
            }
            self.bump_punct("}");
        } else if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Variable);
            } else {
                self.err_here("expected imported name");
            }
        }
        self.bump_kw("vannaf"); // vannaf
        if self.is_punct("\"") || (self.peek().map(|t| t.kind == Kind::Str).unwrap_or(false)) {
            self.bump_as(Role::Str);
        } else {
            self.err_here("expected import path string");
        }
    }

    fn parse_type_decl(&mut self) {
        self.bump_kw("soort"); // soort
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident || t.kind == Kind::Type {
                self.bump_as(Role::Type);
            } else {
                self.err_here("expected type name");
            }
        }
        self.bump_punct("=");
        if self.is_punct("{") {
            self.bump_punct("{");
            while !self.at_end() && !self.is_punct("}") {
                if let Some(t) = self.peek() {
                    if t.kind == Kind::Ident {
                        self.bump_as(Role::Property);
                    } else {
                        self.err_here("expected field name");
                        self.bump();
                        continue;
                    }
                }
                self.bump_punct(":");
                self.parse_type();
                if self.is_punct("?") {
                    self.bump_as(Role::Operator);
                }
                if self.is_punct(",") {
                    self.bump();
                }
            }
            self.bump_punct("}");
        } else {
            self.parse_type();
        }
    }

    fn parse_flow(&mut self) {
        self.bump_kw("flow"); // flow
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Function);
            } else {
                self.err_here("expected flow name");
            }
        }
        self.bump_punct("(");
        if !self.is_punct(")") {
            loop {
                if let Some(t) = self.peek() {
                    if t.kind == Kind::Ident {
                        self.bump_as(Role::Property);
                    } else {
                        self.err_here("expected parameter name");
                        self.bump();
                        continue;
                    }
                }
                self.bump_punct(":");
                self.parse_type();
                if self.is_punct("=") {
                    self.bump_as(Role::Operator);
                    self.parse_expr();
                }
                if self.is_punct(",") {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.bump_punct(")");
        if self.is_punct(":") {
            self.bump();
            self.parse_type();
        }
        self.bump_punct("{");
        self.parse_block();
    }

    // ── .fdgn: node / edge (SPEC.md §10) ──

    /// `node <name> { <field> = <expr> (newline|`,`)* }`. Field names
    /// (`title`, `action`, `pos`, ...) are ordinary identifiers, not
    /// reserved — the parser doesn't special-case which fields appear.
    fn parse_node(&mut self) {
        self.bump_kw("node"); // node
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Function);
            } else {
                self.err_here("expected node name");
            }
        }
        self.bump_punct("{");
        loop {
            while self.is_punct(";") {
                self.bump();
            }
            if self.at_end() || self.is_punct("}") {
                break;
            }
            let start_line = self.peek().map(|t| t.line).unwrap_or(0);
            if let Some(t) = self.peek() {
                if t.kind == Kind::Ident {
                    self.bump_as(Role::Property);
                } else {
                    self.err_here("expected field name (e.g. `title`, `action`, `pos`)");
                    self.bump();
                    continue;
                }
            }
            self.bump_punct("=");
            self.parse_expr();
            if self.is_punct(",") {
                self.bump();
                continue;
            }
            if self.at_end() || self.is_punct("}") {
                break;
            }
            if self.peek().map(|t| t.line).unwrap_or(0) > start_line {
                continue;
            }
            self.err_here("expected newline or `,` between node fields");
        }
        self.bump_punct("}");
    }

    /// `edge <name> -> <name>`. Endpoint names are not cross-referenced
    /// against declared `node`s (structure only, matching this server's
    /// policy everywhere else).
    fn parse_edge(&mut self) {
        self.bump_kw("edge"); // edge
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Function);
            } else {
                self.err_here("expected source node name");
            }
        }
        self.bump_punct("->");
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Function);
            } else {
                self.err_here("expected target node name");
            }
        }
    }

    // ── blocks / statements ──
    fn parse_block(&mut self) {
        loop {
            while self.is_punct(";") {
                self.bump();
            }
            if self.at_end() || self.is_punct("}") {
                break;
            }
            let start_line = self.peek().map(|t| t.line).unwrap_or(0);
            let before = self.pos;
            self.parse_stmt();
            if self.pos == before {
                if let Some(t) = self.bump() {
                    self.err_at(&t, "unexpected token");
                }
                continue;
            }
            if self.at_end() || self.is_punct("}") {
                break;
            }
            if self.is_punct(";") {
                self.bump();
                continue;
            }
            if self.peek().map(|t| t.line).unwrap_or(0) > start_line {
                continue;
            }
            self.err_here("expected newline or `;` between statements");
            if let Some(t) = self.bump() {
                self.err_at(&t, "unexpected token");
            }
        }
        if self.is_punct("}") {
            self.bump();
        }
    }

    fn parse_stmt(&mut self) {
        if self.is_kw("step") {
            self.parse_step();
        } else if self.is_kw("laat") {
            self.parse_laat();
        } else if self.is_kw("as") {
            self.parse_if();
        } else if self.is_kw("terwyl") {
            self.parse_while();
        } else if self.is_kw("gee") {
            self.parse_return();
        } else if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                let next = self.peek_at(1);
                if matches!(next, Some(n) if n.kind == Kind::Punct && n.text == "=") {
                    self.bump_as(Role::Variable);
                    self.bump_as(Role::Operator); // =
                    self.parse_expr();
                } else if matches!(next, Some(n) if n.kind == Kind::Punct && n.text == "(") {
                    self.parse_expr(); // expression-statement call
                } else {
                    self.parse_expr(); // expression-statement (e.g. .elk map)
                }
            } else {
                self.parse_expr(); // expression-statement
            }
        }
    }

    fn parse_step(&mut self) {
        self.bump_kw("step"); // step
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Function);
            } else {
                self.err_here("expected action name");
            }
        }
        self.bump_punct("(");
        if !self.is_punct(")") {
            loop {
                if let Some(t) = self.peek() {
                    if t.kind == Kind::Ident {
                        self.bump_as(Role::Property);
                    } else {
                        self.err_here("expected argument name");
                        self.bump();
                        continue;
                    }
                }
                self.bump_punct("=");
                self.parse_expr();
                if self.is_punct(",") {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.bump_punct(")");
    }

    fn parse_laat(&mut self) {
        self.bump_kw("laat"); // laat
        if let Some(t) = self.peek() {
            if t.kind == Kind::Ident {
                self.bump_as(Role::Variable);
            } else {
                self.err_here("expected variable name");
            }
        }
        if self.is_punct(":") {
            self.bump();
            self.parse_type();
        }
        self.bump_punct("=");
        self.parse_expr();
    }

    fn parse_if(&mut self) {
        self.bump_kw("as");
        self.parse_condition();
        self.bump_punct("{");
        self.parse_block();
        if self.is_kw("anders") {
            self.bump_kw("anders");
            if self.is_kw("as") {
                self.parse_if(); // else-if: `anders as <cond> { ... } [anders ...]`
            } else {
                self.bump_punct("{");
                self.parse_block();
            }
        }
    }

    fn parse_while(&mut self) {
        self.bump_kw("terwyl");
        self.parse_condition();
        self.bump_punct("{");
        self.parse_block();
    }

    /// Parses a `terwyl`/`as` condition with constructor-literal parsing
    /// suppressed at the top level (see `no_brace_ctor`), so a trailing
    /// bare identifier isn't misread as the start of `Ident { ... }` when
    /// what actually follows is the condition's own block-opening `{`.
    fn parse_condition(&mut self) {
        let prev = self.no_brace_ctor;
        self.no_brace_ctor = true;
        self.parse_expr();
        self.no_brace_ctor = prev;
    }

    fn parse_return(&mut self) {
        let gee_line = self.peek().map(|t| t.line).unwrap_or(0);
        self.bump_kw("gee");
        if self.at_end() || self.is_punct("}") || self.is_punct(";") {
            return;
        }
        if self.peek().map(|t| t.line).unwrap_or(0) == gee_line {
            self.parse_expr();
        }
    }

    // ── expressions ──
    fn parse_expr(&mut self) {
        self.parse_logical();
    }

    /// Lowest-precedence binary level (SPEC.md §7: "Logical: && ||"). Was
    /// previously missing entirely — `parse_expr` called `parse_comparison`
    /// directly, so `&&`/`||` (already lexed as `Kind::Op`, see the `'&'`/
    /// `'|'` arms in `lex`) were never consumed by the expression parser.
    /// Any expression using them (e.g. `c > 20 && !false` from
    /// examples.md's `reken()`) parsed only up to the comparison, then
    /// cascaded into spurious diagnostics on the unconsumed `&&`/`||` and
    /// everything after it.
    fn parse_logical(&mut self) {
        self.parse_comparison();
        while let Some(t) = self.peek() {
            if t.kind == Kind::Op && matches!(t.text.as_str(), "&&" | "||") {
                self.bump_as(Role::Operator);
                self.parse_comparison();
            } else {
                break;
            }
        }
    }

    /// Parses an expression with constructor-literal parsing re-enabled,
    /// even if called from inside a `terwyl`/`as` condition — for
    /// positions delimited by their own closing token (`)`, `]`, a call
    /// argument before `,`/`)`, an object-literal field before `,`/`}`),
    /// where a trailing `Ident {` can only mean a constructor, never the
    /// condition's block-opening brace. See `no_brace_ctor`.
    fn parse_expr_allowing_ctor(&mut self) {
        let prev = self.no_brace_ctor;
        self.no_brace_ctor = false;
        self.parse_expr();
        self.no_brace_ctor = prev;
    }

    fn parse_comparison(&mut self) {
        self.parse_add();
        while let Some(t) = self.peek() {
            if (t.kind == Kind::Op || t.kind == Kind::Punct) && matches!(t.text.as_str(), "==" | "!=" | "<" | ">" | "<=" | ">=") {
                self.bump_as(Role::Operator);
                self.parse_add();
            } else {
                break;
            }
        }
    }

    fn parse_add(&mut self) {
        self.parse_mul();
        while let Some(t) = self.peek() {
            if t.kind == Kind::Op && matches!(t.text.as_str(), "+" | "-") {
                self.bump_as(Role::Operator);
                self.parse_mul();
            } else {
                break;
            }
        }
    }

    fn parse_mul(&mut self) {
        self.parse_unary();
        while let Some(t) = self.peek() {
            if t.kind == Kind::Op && matches!(t.text.as_str(), "*" | "/") {
                self.bump_as(Role::Operator);
                self.parse_unary();
            } else {
                break;
            }
        }
    }

    fn parse_unary(&mut self) {
        if let Some(t) = self.peek() {
            if t.kind == Kind::Op && matches!(t.text.as_str(), "-" | "!") {
                self.bump_as(Role::Operator);
                self.parse_unary();
                return;
            }
        }
        self.parse_postfix();
    }

    fn parse_postfix(&mut self) {
        self.parse_primary();
        loop {
            if self.is_punct(".") {
                // method/field
                if self.peek_at(1).map(|t| t.kind == Kind::Kw && t.text == "elk").unwrap_or(false) {
                    self.bump(); // .
                    self.bump_as(Role::Function); // elk
                    self.bump_punct("(");
                    self.bump_punct("(");
                    if let Some(t) = self.peek() {
                        if t.kind == Kind::Ident {
                            self.bump_as(Role::Variable);
                        } else {
                            self.err_here("expected loop variable");
                        }
                    }
                    self.bump_punct(")");
                    self.bump_punct(")");
                    if self.is_punct("{") {
                        self.bump_punct("{");
                        self.parse_block();
                    } else if self.is_punct("=>") {
                        self.bump();
                        self.parse_expr();
                    } else {
                        self.err_here("expected `{` block or `=>` for .elk");
                    }
                } else {
                    self.bump(); // .
                    if let Some(t) = self.peek() {
                        if t.kind == Kind::Ident {
                            self.bump_as(Role::Property);
                        } else {
                            self.err_here("expected member name");
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    fn parse_primary(&mut self) {
        let t = match self.peek().cloned() {
            Some(t) => t,
            None => {
                self.err_here("unexpected end of input");
                return;
            }
        };
        match t.kind {
            Kind::Num => {
                self.bump_as(Role::Num);
            }
            Kind::Str => {
                self.bump_as(Role::Str);
            }
            Kind::Lit => {
                self.bump_as(Role::Bool);
            }
            Kind::Kw if t.text == "waar" || t.text == "onwaar" || t.text == "niks" || t.text == "true" || t.text == "false" => {
                self.bump_as(Role::Bool);
            }
            Kind::Ident => {
                let next = self.peek_at(1);
                if matches!(next, Some(n) if n.kind == Kind::Punct && n.text == "(") {
                    self.bump_as(Role::Function); // call
                    self.bump_punct("(");
                    if !self.is_punct(")") {
                        loop {
                            if let Some(t) = self.peek() {
                                if t.kind == Kind::Ident {
                                    self.bump_as(Role::Property);
                                } else {
                                    self.err_here("expected argument name");
                                    self.bump();
                                    continue;
                                }
                            }
                            self.bump_punct("=");
                            self.parse_expr_allowing_ctor();
                            if self.is_punct(",") {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.bump_punct(")");
                } else if !self.no_brace_ctor && matches!(next, Some(n) if n.kind == Kind::Punct && n.text == "{") {
                    self.bump_as(Role::Type); // constructor
                    self.parse_object_literal();
                } else {
                    self.bump_as(Role::Variable);
                }
            }
            Kind::Punct if t.text == "(" => {
                self.bump();
                self.parse_expr_allowing_ctor();
                self.bump_punct(")");
            }
            Kind::Punct if t.text == "[" => {
                self.bump();
                while !self.at_end() && !self.is_punct("]") {
                    self.parse_expr_allowing_ctor();
                    if self.is_punct(",") {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.bump_punct("]");
            }
            Kind::Punct if t.text == "{" => {
                self.parse_object_literal();
            }
            _ => {
                self.err_here("unexpected token in expression");
                self.bump();
            }
        }
    }

    fn parse_object_literal(&mut self) {
        self.bump_punct("{");
        while !self.at_end() && !self.is_punct("}") {
            if let Some(t) = self.peek() {
                if t.kind == Kind::Ident {
                    self.bump_as(Role::Property);
                } else {
                    self.err_here("expected field name");
                    self.bump();
                    continue;
                }
            }
            self.bump_punct("=");
            self.parse_expr_allowing_ctor();
            if self.is_punct(",") {
                self.bump();
            } else {
                break;
            }
        }
        self.bump_punct("}");
    }

    fn parse_type(&mut self) {
        let t = match self.peek().cloned() {
            Some(t) => t,
            None => {
                self.err_here("expected type");
                return;
            }
        };
        if t.kind == Kind::Type || t.kind == Kind::Ident {
            self.bump_as(Role::Type);
            if self.is_punct("<") {
                self.bump();
                self.parse_type();
                while self.is_punct(",") {
                    self.bump();
                    self.parse_type();
                }
                self.bump_punct(">");
            }
        } else {
            self.err_here("expected type");
        }
        if self.is_punct("?") {
            self.bump_as(Role::Operator);
        }
    }
}

pub fn parse_program(toks: Vec<Tok>, lex_diags: Vec<Diag>, mode: FileMode) -> (Vec<Diag>, Vec<SemToken>) {
    let mut p = Parser { toks, pos: 0, diags: lex_diags, sems: Vec::new(), mode, no_brace_ctor: false };
    p.run();
    (p.diags, p.sems)
}

// ── Analysis dispatch by extension ───────────────────────────────────────────

pub fn analyze(uri: &str, text: &str) -> (Vec<Diag>, Vec<SemToken>) {
    let ext = uri.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "fwrk" | "fdgn" => {
            if ext == "fdgn" {
                if let Ok(serde_json::Value::Object(_)) = serde_json::from_str::<serde_json::Value>(text) {
                    return json_diags(text);
                }
            }
            let (toks, lex_diags, _lex_sems) = lex(text);
            parse_program(toks, lex_diags, file_mode_for(uri))
        }
        "fmeta" => json_diags(text),
        "forge" => (
            vec![Diag {
                line: 0,
                col: 0,
                len: 1,
                severity: 3,
                message: "`.forge` is a binary project container - not editable as text. Open the `.fwrk`/`.fdgn`/`.fmeta` sources instead.".into(),
            }],
            Vec::new(),
        ),
        _ => (Vec::new(), Vec::new()),
    }
}

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
                        severity: 2,
                        message: "Encrypted secret field (`ENC:`). Stays encrypted at rest; resolved only by the Forge runtime.".into(),
                    });
                }
            }
        }
    }
    (diags, Vec::new())
}

pub fn diag_to_json(d: &Diag) -> Value {
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

// ── Semantic token encoding ──────────────────────────────────────────────────

pub fn encode_semantic_tokens(sems: &[SemToken]) -> Vec<u32> {
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
        data.push(0);
        prev_line = line;
        prev_char = col;
    }
    data
}

