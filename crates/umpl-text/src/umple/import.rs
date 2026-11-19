//! Reads an Umple model into a [`Document`].
//!
//! The importer works on tokens, statement by statement: Umple statements
//! end with `;` or a `{ … }` block, wherever the line breaks fall. Names are
//! resolved once the whole file is read, because Umple class names are
//! global and may be used before they are declared.

use std::collections::HashMap;

use umpl_core::*;

use super::lexer::{lex, Kind, Token};
use crate::parser::{Parsed, Problem};
use crate::scan::{clean, is_name};

/// Reads the class model in an Umple file. Never fails: anything skipped
/// is reported as a problem at its line and column. Umple positions
/// (`position x y w h;`) are not read, so the editor lays the diagram out.
pub fn import(src: &str) -> Parsed {
    let (comments, toks) = lex(src).into_iter().partition(|t| t.kind == Kind::Comment);
    let mut im = Importer {
        src,
        toks,
        comments,
        i: 0,
        ci: 0,
        doc: Document::new(),
        problems: Vec::new(),
        namespace: None,
        packages: HashMap::new(),
        by_name: HashMap::new(),
        pending_comments: Vec::new(),
        pending: Vec::new(),
        eof_reported: false,
        type_params: Vec::new(),
    };
    im.top();
    im.finish()
}

/// Languages of method bodies and derived attributes: `Java { … }`.
const CODE_LANGS: [&str; 9] = [
    "Java",
    "RTCpp",
    "SimpleCpp",
    "Cpp",
    "Php",
    "Ruby",
    "Python",
    "Alloy",
    "UmpleSelf",
];

/// Top-level statements about code generation or tooling, skipped silently.
const SILENT_TOP: [&str; 18] = [
    "generate",
    "suboption",
    "strictness",
    "glossary",
    "distributable",
    "debug",
    "top",
    "before",
    "after",
    "around",
    "filter",
    "require",
    "template",
    "tracer",
    "trace",
    "implementsReq",
    "isFeature",
    "emit",
];

/// Class-body statements about code, keys or diagram positions, skipped
/// silently.
const SILENT_BODY: [&str; 22] = [
    "key",
    "depend",
    "position",
    "position.association",
    "displayColor",
    "displayColour",
    "before",
    "after",
    "around",
    "test",
    "generic",
    "testSequence",
    "emit",
    "trace",
    "tracer",
    "distributable",
    "proxyPattern",
    "strictness",
    "implementsReq",
    "active",
    "require",
    "port",
];

/// Words that may come before an attribute or method.
const MODIFIERS: [&str; 20] = [
    "public",
    "protected",
    "private",
    "static",
    "synchronized",
    "queued",
    "abstract",
    "final",
    "unique",
    "lazy",
    "ivar",
    "immutable",
    "settable",
    "internal",
    "defaulted",
    "const",
    "constant",
    "fixml",
    "autounique",
    "transient",
];

const ARROWS: [&str; 6] = ["--", "->", "<-", "><", "<@>-", "-<@>"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Body {
    Class,
    Interface,
    AssociationClass,
}

/// An association end before names are resolved.
enum Ref {
    Id(ElementId),
    Name {
        name: String,
        line: usize,
        col: usize,
    },
}

struct PendingRelation {
    id: RelationId,
    container: Option<PackageId>,
    /// `None` for `isA`, which becomes generalization or realization
    /// depending on what the parent turns out to be.
    kind: Option<RelationKind>,
    from: (Ref, Option<String>),
    to: (Ref, Option<String>),
    comments: Vec<String>,
}

struct Importer<'a> {
    src: &'a str,
    /// Every token except comments.
    toks: Vec<Token<'a>>,
    comments: Vec<Token<'a>>,
    i: usize,
    /// The next comment not yet taken or passed.
    ci: usize,
    doc: Document,
    problems: Vec<Problem>,
    /// The package set by the last `namespace` statement.
    namespace: Option<PackageId>,
    /// Packages by full dotted name.
    packages: HashMap<String, PackageId>,
    /// Umple class names are global.
    by_name: HashMap<String, ElementId>,
    /// Top-level comments waiting for the next declaration.
    pending_comments: Vec<String>,
    pending: Vec<PendingRelation>,
    eof_reported: bool,
    /// Type parameters of the trait being read: `trait T <Observer>`.
    type_params: Vec<String>,
}

impl<'a> Importer<'a> {
    // ----- tokens -----------------------------------------------------------

    fn peek(&self, k: usize) -> Option<&Token<'a>> {
        self.toks.get(self.i + k)
    }

    fn text(&self, k: usize) -> &'a str {
        self.peek(k).map(|t| t.text).unwrap_or("")
    }

    /// Is token `k` the punctuation or word `s` (not inside a string)?
    fn is(&self, k: usize, s: &str) -> bool {
        self.peek(k)
            .is_some_and(|t| t.text == s && t.kind != Kind::Str)
    }

    fn is_ident(&self, k: usize) -> bool {
        self.peek(k).is_some_and(|t| t.kind == Kind::Ident)
    }

    fn is_arrow(&self, k: usize) -> bool {
        self.peek(k)
            .is_some_and(|t| t.kind == Kind::Punct && ARROWS.contains(&t.text))
    }

    fn eat(&mut self, s: &str) -> bool {
        let yes = self.is(0, s);
        if yes {
            self.i += 1;
        }
        yes
    }

    fn ident(&mut self) -> Option<&'a str> {
        let t = self.peek(0).filter(|t| t.kind == Kind::Ident)?.text;
        self.i += 1;
        Some(t)
    }

    /// Is token `k` a code language, or a list of them, before a `{`?
    /// Returns the number of tokens up to the `{`.
    fn code_langs(&self, k: usize) -> Option<usize> {
        let mut n = 0;
        while self.is_ident(k + n) && CODE_LANGS.contains(&self.text(k + n)) {
            n += 1;
            if self.is(k + n, ",") {
                n += 1;
            }
        }
        (n > 0 && self.is(k + n, "{")).then_some(n)
    }

    /// Source text from token `from` up to the token before the current one.
    fn source_since(&self, from: usize) -> &'a str {
        match (self.toks.get(from), self.i.checked_sub(1)) {
            (Some(a), Some(last)) if last >= from => &self.src[a.start..self.toks[last].end],
            _ => "",
        }
    }

    // ----- problems and comments ----------------------------------------------

    fn problem_at(&mut self, k: usize, message: impl Into<String>) {
        let (line, col) = match self.toks.get(k) {
            Some(t) => (t.line, t.col),
            None => self.toks.last().map(|t| (t.line, t.col)).unwrap_or((1, 1)),
        };
        self.problems.push(Problem {
            line,
            col,
            message: message.into(),
        });
    }

    fn missing_brace(&mut self) {
        if !self.eof_reported {
            self.eof_reported = true;
            self.problem_at(self.toks.len(), "missing `}` at end of file");
        }
    }

    /// The comments between the previous token and the current one, as
    /// lines. Comments inside skipped code are passed over.
    fn comments_here(&mut self) -> Vec<String> {
        let prev_end = self.i.checked_sub(1).map(|p| self.toks[p].end).unwrap_or(0);
        let cur_start = self.peek(0).map(|t| t.start).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        while let Some(c) = self.comments.get(self.ci) {
            if c.start >= cur_start {
                break;
            }
            if c.start >= prev_end {
                out.extend(comment_lines(c.text));
            }
            self.ci += 1;
        }
        out
    }

    // ----- skipping -----------------------------------------------------------

    /// Skips a `{ … }` block starting at the current `{`.
    fn skip_block(&mut self) {
        let mut depth = 0usize;
        while let Some(t) = self.peek(0) {
            let (open, close) = (t.text == "{", t.text == "}");
            let punct = t.kind == Kind::Punct;
            self.i += 1;
            if punct && open {
                depth += 1;
            } else if punct && close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return;
                }
            }
        }
        self.missing_brace();
    }

    /// Skips further method bodies in other languages: `Python { … }`.
    fn skip_more_code(&mut self) {
        while let Some(n) = self.code_langs(0) {
            self.i += n;
            self.skip_block();
        }
    }

    /// Skips one statement: through a `;` outside brackets, or through a
    /// `{ … }` block (and its other-language variants and a `;` after it).
    /// Stops before a `}` that closes the enclosing body.
    fn skip_statement(&mut self) {
        let start = self.i;
        let mut depth = 0usize;
        while let Some(t) = self.peek(0) {
            if t.kind == Kind::Punct {
                match t.text {
                    "(" | "[" => depth += 1,
                    ")" | "]" => depth = depth.saturating_sub(1),
                    ";" if depth == 0 => {
                        self.i += 1;
                        return;
                    }
                    "{" => {
                        self.skip_block();
                        if depth == 0 {
                            self.skip_more_code();
                            self.eat(";");
                            return;
                        }
                        continue;
                    }
                    "}" => {
                        if self.i == start {
                            self.i += 1;
                        }
                        return;
                    }
                    _ => {}
                }
            }
            self.i += 1;
        }
    }

    /// Skips a statement the importer does not understand, with a problem
    /// quoting its first line.
    fn skip_unknown(&mut self) {
        let start = self.i;
        self.skip_statement();
        let text = self.source_since(start);
        let first = text.lines().next().unwrap_or("").trim();
        let mut snippet: String = first.chars().take(40).collect();
        if snippet.len() < first.len() {
            snippet.push('…');
        }
        self.problem_at(start, format!("`{snippet}` is not understood; skipped"));
    }

    /// Skips a statement that declares something fastumpl does not draw,
    /// with a problem naming it.
    fn skip_reported(&mut self, message: String) {
        let start = self.i;
        self.skip_statement();
        self.problem_at(start, message);
    }

    // ----- declarations --------------------------------------------------------

    fn push_decl(&mut self, pkg: Option<PackageId>, d: Decl) {
        match pkg {
            Some(p) => self.doc.packages.get_mut(&p).unwrap().body.push(d),
            None => self.doc.root.push(d),
        }
    }

    /// The package for a dotted namespace, created as needed. Pending
    /// comments go to the outermost package created here.
    fn ensure_package(&mut self, path: &str) -> Option<PackageId> {
        let mut cur: Option<PackageId> = None;
        let mut full = String::new();
        for part in path.split('.').filter(|p| !p.is_empty()) {
            if !full.is_empty() {
                full.push('.');
            }
            full.push_str(&clean(part));
            cur = Some(match self.packages.get(&full) {
                Some(p) => *p,
                None => {
                    let id = self.doc.new_package_id();
                    let mut pkg = Package::new(clean(part));
                    pkg.comments = std::mem::take(&mut self.pending_comments);
                    self.doc.packages.insert(id, pkg);
                    self.push_decl(cur, Decl::Package(id));
                    self.packages.insert(full.clone(), id);
                    id
                }
            });
        }
        cur
    }

    /// The element named `name`, declared in `pkg` if new. A class opened
    /// again (Umple mixins, such as UmpleOnline's position blocks) is the
    /// same element; comments before it are dropped.
    fn declare(&mut self, name: &str, kind: ElementKind, pkg: Option<PackageId>) -> ElementId {
        let name = if is_name(name) {
            name.to_string()
        } else {
            clean(name)
        };
        if let Some(id) = self.by_name.get(&name) {
            self.pending_comments.clear();
            return *id;
        }
        let id = self.doc.new_element_id();
        let mut el = Element::new(kind, name.clone());
        el.comments = std::mem::take(&mut self.pending_comments);
        self.doc.elements.insert(id, el);
        self.push_decl(pkg, Decl::Element(id));
        self.by_name.insert(name, id);
        id
    }

    fn add_stereotype(&mut self, id: ElementId, s: &str) {
        let el = self.doc.elements.get_mut(&id).unwrap();
        match &mut el.stereotype {
            None => el.stereotype = Some(s.to_string()),
            Some(cur) if !cur.split(", ").any(|x| x == s) => {
                cur.push_str(", ");
                cur.push_str(s);
            }
            _ => {}
        }
    }

    fn add_relation(
        &mut self,
        container: Option<PackageId>,
        kind: Option<RelationKind>,
        from: (Ref, Option<String>),
        to: (Ref, Option<String>),
        comments: Vec<String>,
    ) {
        let id = self.doc.new_relation_id();
        self.push_decl(container, Decl::Relation(id));
        self.pending.push(PendingRelation {
            id,
            container,
            kind,
            from,
            to,
            comments,
        });
    }

    /// An association written `left arrow right`, with Umple's arrows.
    fn add_association(
        &mut self,
        container: Option<PackageId>,
        arrow: &str,
        left: (Ref, Option<String>),
        right: (Ref, Option<String>),
        comments: Vec<String>,
    ) {
        let (kind, from, to) = match arrow {
            "->" => (RelationKind::Directed, left, right),
            "<-" => (RelationKind::Directed, right, left),
            "<@>-" => (RelationKind::Composition, left, right),
            "-<@>" => (RelationKind::Composition, right, left),
            _ => (RelationKind::Association, left, right),
        };
        self.add_relation(container, Some(kind), from, to, comments);
    }

    fn name_ref(&self, k: usize) -> Ref {
        let t = &self.toks[k];
        Ref::Name {
            name: t.text.to_string(),
            line: t.line,
            col: t.col,
        }
    }

    // ----- top level -------------------------------------------------------------

    fn top(&mut self) {
        while let Some(t) = self.peek(0).copied() {
            let comments = self.comments_here();
            self.pending_comments.extend(comments);
            if t.kind != Kind::Ident {
                match t.text {
                    ";" => self.i += 1,
                    "}" => {
                        self.problem_at(self.i, "`}` without a matching `{`");
                        self.i += 1;
                    }
                    "@" => self.skip_annotation(),
                    _ => self.skip_unknown(),
                }
                continue;
            }
            let name = self.text(1);
            match t.text {
                "namespace" => self.namespace(),
                "class" => self.class_like(ElementKind::Class, None, Body::Class),
                "interface" => self.class_like(ElementKind::Interface, None, Body::Interface),
                "trait" => self.class_like(ElementKind::Class, Some("trait"), Body::Class),
                "external" => {
                    if self.is(1, "interface") {
                        self.i += 1;
                        self.class_like(ElementKind::Interface, Some("external"), Body::Interface)
                    } else {
                        self.class_like(ElementKind::Class, Some("external"), Body::Class)
                    }
                }
                "associationClass" => {
                    self.class_like(ElementKind::Class, None, Body::AssociationClass)
                }
                "enum" => {
                    let pkg = self.namespace;
                    self.enum_def(pkg);
                }
                "association" => self.association_block(),
                "use" => {
                    let start = self.i;
                    self.skip_statement();
                    let what = self.source_since(start).trim_end_matches(';').trim();
                    self.problem_at(start, format!("`{what}`: other files are not read"));
                }
                "req" => self.skip_reported(format!("requirement `{name}` is not shown")),
                "statemachine" => {
                    let name = if matches!(name, "queued" | "pooled") {
                        self.text(2)
                    } else {
                        name
                    };
                    self.skip_reported(format!("state machine `{name}` is not shown"))
                }
                "mixset" => self.skip_reported(format!("mixset `{name}` is not read")),
                // Aspects for several classes: `after {A, B} set* { … }`.
                "before" | "after" | "around" if name == "{" => {
                    self.i += 1;
                    self.skip_block();
                    self.skip_statement();
                }
                w if SILENT_TOP.contains(&w) => self.skip_statement(),
                _ => {
                    self.pending_comments.clear();
                    self.skip_unknown();
                }
            }
        }
        let comments = self.comments_here();
        self.pending_comments.extend(comments);
    }

    /// `@Override` or `@SuppressWarnings("x")` before a method.
    fn skip_annotation(&mut self) {
        self.i += 1;
        if self.ident().is_some() && self.is(0, "(") {
            let mut depth = 0usize;
            while let Some(t) = self.peek(0) {
                let text = t.text;
                self.i += 1;
                match text {
                    "(" => depth += 1,
                    ")" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn namespace(&mut self) {
        let start = self.i;
        self.i += 1;
        let path = if self.is_ident(0) { self.text(0) } else { "" };
        self.skip_statement();
        if path.is_empty() && !self.source_since(start).contains('-') {
            self.problem_at(start, "`namespace` without a name; skipped");
        }
        self.namespace = self.ensure_package(path);
        self.pending_comments.clear();
    }

    /// `class`, `interface`, `trait`, `external` or `associationClass`.
    fn class_like(&mut self, kind: ElementKind, stereotype: Option<&str>, body: Body) {
        let start = self.i;
        self.i += 1;
        let Some(name) = self.ident() else {
            self.i = start;
            return self.skip_unknown();
        };
        let params = if self.is(0, "<") {
            self.type_parameters()
        } else {
            Vec::new()
        };
        if !self.is(0, "{") {
            self.i = start;
            return self.skip_unknown();
        }
        let pkg = self.namespace;
        let id = self.declare(name, kind, pkg);
        if let Some(s) = stereotype {
            self.add_stereotype(id, s);
        }
        if !params.is_empty() {
            self.doc.elements.get_mut(&id).unwrap().type_params = Some(params.join(", "));
        }
        let outer = std::mem::replace(&mut self.type_params, params);
        self.body(id, body);
        self.type_params = outer;
    }

    /// The names in `<A, B isA I = Default>`: the names after `<` or `,`.
    fn type_parameters(&mut self) -> Vec<String> {
        let start = self.i;
        self.skip_angles();
        let mut names = Vec::new();
        for k in start + 1..self.i {
            let prev = self.toks[k - 1].text;
            if self.toks[k].kind == Kind::Ident && (k == start + 1 || prev == ",") {
                names.push(self.toks[k].text.to_string());
            }
        }
        names
    }

    /// Skips `<…>` type parameters, nested.
    fn skip_angles(&mut self) -> String {
        let mut depth = 0usize;
        let mut out = String::new();
        while let Some(t) = self.peek(0) {
            if matches!(t.text, ";" | "{" | "}") && t.kind == Kind::Punct {
                break;
            }
            let text = t.text;
            self.i += 1;
            push_type_token(&mut out, text);
            match text {
                "<" => depth += 1,
                ">" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn enum_def(&mut self, pkg: Option<PackageId>) {
        let start = self.i;
        self.i += 1;
        let Some(name) = self.ident() else {
            self.i = start;
            return self.skip_unknown();
        };
        if !self.is(0, "{") {
            self.i = start;
            return self.skip_unknown();
        }
        self.i += 1;
        let id = self.declare(name, ElementKind::Enum, pkg);
        loop {
            let Some(t) = self.peek(0) else {
                return self.missing_brace();
            };
            let text = t.text;
            let is_ident = t.kind == Kind::Ident;
            self.i += 1;
            if text == "}" {
                return;
            }
            if is_ident {
                let el = self.doc.elements.get_mut(&id).unwrap();
                el.members.push(Member::plain(text));
            }
        }
    }

    fn association_block(&mut self) {
        let start = self.i;
        self.i += 1;
        self.ident();
        if !self.is(0, "{") {
            self.i = start;
            return self.skip_unknown();
        }
        self.i += 1;
        let pkg = self.namespace;
        let mut comments = std::mem::take(&mut self.pending_comments);
        loop {
            comments.extend(self.comments_here());
            let Some(t) = self.peek(0) else {
                return self.missing_brace();
            };
            match t.text {
                "}" => {
                    self.i += 1;
                    return;
                }
                ";" => self.i += 1,
                _ if t.kind == Kind::Number || t.text == "*" || t.text == "immutable" => {
                    let s = self.i;
                    match self.standalone_association() {
                        Some((arrow, left, right)) => {
                            let c = std::mem::take(&mut comments);
                            self.add_association(pkg, arrow, left, right, c);
                        }
                        None => {
                            self.i = s;
                            self.skip_unknown();
                        }
                    }
                }
                "implementsReq" | "mixset" => self.skip_statement(),
                _ => self.skip_unknown(),
            }
        }
    }

    /// `[immutable] 1 A role -- * B role;` in an `association` block.
    #[allow(clippy::type_complexity)]
    fn standalone_association(
        &mut self,
    ) -> Option<(&'a str, (Ref, Option<String>), (Ref, Option<String>))> {
        self.eat("immutable");
        let left = self.association_end()?;
        if !self.is_arrow(0) {
            return None;
        }
        let arrow = self.text(0);
        self.i += 1;
        let right = self.association_end()?;
        self.eat(";").then_some((arrow, left, right))
    }

    /// `mult Type [role] [sorted {key}]`.
    fn association_end(&mut self) -> Option<(Ref, Option<String>)> {
        let mult = self.multiplicity()?;
        self.ident()?;
        let ty = self.name_ref(self.i - 1);
        let role = self.role();
        Some((ty, label(&mult, role)))
    }

    /// An optional role name, and an optional `sorted {key}` after it.
    fn role(&mut self) -> Option<&'a str> {
        let role = if self.is_ident(0) && !self.is(0, "sorted") {
            self.ident()
        } else {
            None
        };
        self.skip_sorted();
        role
    }

    fn skip_sorted(&mut self) {
        if self.is(0, "sorted") && self.is(1, "{") {
            self.i += 1;
            self.skip_block();
        }
    }

    /// `*`, `3`, `0..1`, `1..*`.
    fn multiplicity(&mut self) -> Option<String> {
        let bound = |k: usize| -> Option<&'a str> {
            let t = self.peek(k)?;
            let ok = (t.kind == Kind::Number && t.text.bytes().all(|b| b.is_ascii_digit()))
                || (t.kind == Kind::Punct && t.text == "*");
            ok.then_some(t.text)
        };
        let lower = bound(0)?;
        if self.is(1, "..") {
            let upper = bound(2)?;
            self.i += 3;
            Some(format!("{lower}..{upper}"))
        } else {
            self.i += 1;
            Some(lower.to_string())
        }
    }

    // ----- class bodies ------------------------------------------------------------

    fn body(&mut self, id: ElementId, body: Body) {
        self.i += 1;
        let mut comments = Vec::new();
        loop {
            comments.extend(self.comments_here());
            let Some(t) = self.peek(0) else {
                return self.missing_brace();
            };
            match t.text {
                "}" if t.kind == Kind::Punct => {
                    self.i += 1;
                    return;
                }
                ";" if t.kind == Kind::Punct => self.i += 1,
                _ => {
                    let c = std::mem::take(&mut comments);
                    self.statement(id, body, c);
                }
            }
        }
    }

    fn element_name(&self, id: ElementId) -> String {
        self.doc.elements[&id].name.clone()
    }

    /// One statement in a class, interface or association class body.
    fn statement(&mut self, id: ElementId, body: Body, comments: Vec<String>) {
        let t = self.toks[self.i];
        let next = self.text(1);
        let pkg = self.doc.element_package(id);
        if t.kind == Kind::Number || (t.kind == Kind::Punct && t.text == "*") {
            return self.association(id, body, comments);
        }
        if t.kind == Kind::Punct {
            return match t.text {
                "@" => self.skip_annotation(),
                "[" => self.constraint(id),
                _ => self.skip_unknown(),
            };
        }
        if t.kind != Kind::Ident {
            return self.skip_unknown();
        }
        if self.is(1, "<") && self.is(2, "<") && self.is(3, "!") {
            return self.skip_template();
        }
        if let Some(port) = self.port_name() {
            let class = self.element_name(id);
            return self.skip_reported(format!("port `{port}` of `{class}` is not shown"));
        }
        // Apart from `abstract;`, `singleton;` and `immutable;`, a name
        // followed by `;` or `=` is an attribute, whatever the name.
        let is_attribute = matches!(next, ";" | "=");
        match t.text {
            // Port connectors (`a.out -> b.in;`) and active methods
            // (`void active run() { … }`) belong to UmpleRT.
            _ if next == "->" || (next == "active" && self.is_ident(2)) => self.skip_statement(),
            "abstract" if next == ";" => {
                self.i += 2;
                let el = self.doc.elements.get_mut(&id).unwrap();
                if el.kind == ElementKind::Class {
                    el.kind = ElementKind::Abstract;
                }
            }
            "singleton" | "immutable" if next == ";" => {
                self.i += 2;
                self.add_stereotype(id, t.text);
            }
            _ if is_attribute => self.member(id),
            "isA" => self.is_a(id, pkg),
            "immutable" if self.peek(1).is_some_and(|n| n.kind == Kind::Number) || next == "*" => {
                self.i += 1;
                self.association(id, body, comments);
            }
            "class" => {
                self.pending_comments = comments;
                self.nested_class(pkg, Some(id));
            }
            "inner" | "static" if next == "class" => {
                self.i += 1;
                self.pending_comments = comments;
                self.nested_class(pkg, None);
            }
            "enum" if self.is_ident(1) && self.is(2, "{") => {
                self.pending_comments = comments;
                self.enum_def(pkg);
            }
            "mixset" => self.skip_reported(format!("mixset `{next}` is not read")),
            w if SILENT_BODY.contains(&w) => self.skip_statement(),
            "queued" | "pooled" if self.is_ident(1) && self.is(2, "{") => {
                let class = self.element_name(id);
                let sm = self.text(1);
                self.skip_reported(format!("state machine `{sm}` of `{class}` is not shown"));
            }
            _ if next == "{" || next == "as" => {
                let class = self.element_name(id);
                self.skip_reported(format!(
                    "state machine `{}` of `{class}` is not shown",
                    t.text
                ));
            }
            _ => self.member(id),
        }
    }

    /// A code template, `name <<! text !>>`: skipped by searching the
    /// source for `!>>`, since template text is not Umple and may hold
    /// quotes, braces or `//` that would mislead the lexer.
    fn skip_template(&mut self) {
        let from = self.toks[self.i + 3].end;
        let end = match self.src[from..].find("!>>") {
            Some(k) => from + k + 3,
            None => self.src.len(),
        };
        while self.peek(0).is_some_and(|t| t.start < end) {
            self.i += 1;
        }
    }

    /// The name of a UmpleRT port, `[public] [conjugated] in|out|port Type name;`.
    fn port_name(&self) -> Option<&'a str> {
        let mut k = 0;
        while matches!(
            self.text(k),
            "public" | "protected" | "private" | "conjugated"
        ) {
            k += 1;
        }
        let port = matches!(self.text(k), "in" | "out" | "port")
            && self.is_ident(k + 1)
            && self.is_ident(k + 2);
        port.then(|| self.text(k + 2))
    }

    /// A class declared inside another: in Umple, a subclass of it.
    /// `inner class` and `static class` are plain nested classes.
    fn nested_class(&mut self, pkg: Option<PackageId>, parent: Option<ElementId>) {
        let start = self.i;
        self.i += 1;
        let Some(name) = self.ident() else {
            self.i = start;
            return self.skip_unknown();
        };
        if self.is(0, "<") {
            self.skip_angles();
        }
        if !self.is(0, "{") {
            self.i = start;
            return self.skip_unknown();
        }
        let id = self.declare(name, ElementKind::Class, pkg);
        if let Some(parent) = parent {
            self.add_relation(
                pkg,
                None,
                (Ref::Id(id), None),
                (Ref::Id(parent), None),
                Vec::new(),
            );
        }
        self.body(id, Body::Class);
    }

    /// `isA A, B;`, `isA A, isA B;`, `isA T<…>;`.
    fn is_a(&mut self, id: ElementId, pkg: Option<PackageId>) {
        self.i += 1;
        loop {
            self.eat("isA");
            if self.is_ident(0) {
                let parent = self.name_ref(self.i);
                self.i += 1;
                if self.is(0, "<") {
                    self.skip_angles();
                }
                self.add_relation(pkg, None, (Ref::Id(id), None), (parent, None), Vec::new());
            }
            if !self.eat(",") {
                break;
            }
        }
        if !self.is(0, "}") {
            self.skip_statement();
        }
    }

    /// `[condition]`: a constraint, reported and skipped.
    fn constraint(&mut self, id: ElementId) {
        let start = self.i;
        let mut depth = 0usize;
        while let Some(t) = self.peek(0) {
            if matches!(t.text, ";" | "{" | "}") && depth == 0 {
                break;
            }
            let text = t.text;
            self.i += 1;
            match text {
                "[" => depth += 1,
                "]" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        let text = self.source_since(start).to_string();
        self.eat(";");
        let class = self.element_name(id);
        self.problem_at(
            start,
            format!("constraint `{text}` of `{class}` is not shown"),
        );
    }

    /// Inline association `1 role -- * Other role;`, reflexive
    /// `0..1 self role;`, or in an association class an end `* Other;`.
    fn association(&mut self, id: ElementId, body: Body, comments: Vec<String>) {
        let start = self.i;
        let pkg = self.doc.element_package(id);
        let Some(mult) = self.multiplicity() else {
            return self.skip_unknown();
        };
        if self.eat("self") {
            let role = self.role();
            if self.eat(";") {
                let l = label(&mult, role);
                return self.add_relation(
                    pkg,
                    Some(RelationKind::Association),
                    (Ref::Id(id), l.clone()),
                    (Ref::Id(id), l),
                    comments,
                );
            }
            self.i = start;
            return self.skip_unknown();
        }
        let mut names = Vec::new();
        while self.is_ident(0) && !self.is(0, "sorted") && names.len() < 3 {
            names.push(self.i);
            self.i += 1;
        }
        self.skip_sorted();
        if self.is_arrow(0) && names.len() <= 1 {
            let arrow = self.text(0);
            self.i += 1;
            let role = names.first().map(|k| self.toks[*k].text);
            if let Some(right) = self.association_end() {
                if self.eat(";") {
                    if let Ref::Name { name, .. } = &right.0 {
                        if self.type_params.contains(name) {
                            let class = self.element_name(id);
                            let message = format!(
                                "association to type parameter `{name}` of `{class}` is not shown"
                            );
                            return self.problem_at(start, message);
                        }
                    }
                    let left = (Ref::Id(id), label(&mult, role));
                    return self.add_association(pkg, arrow, left, right, comments);
                }
            }
        } else if body == Body::AssociationClass && !names.is_empty() && self.eat(";") {
            // `* Type;`, `* Type role;` or `* otherRole Type role;`: each
            // instance of the association class links one `Type`.
            let text = |k: usize| self.toks[k].text;
            let (other_role, ty, role) = match names[..] {
                [t] => (None, t, None),
                [t, r] => (None, t, Some(text(r))),
                [o, t, r] => (Some(text(o)), t, Some(text(r))),
                _ => unreachable!(),
            };
            let from = (self.name_ref(ty), label("1", role));
            let to = (Ref::Id(id), label(&mult, other_role));
            return self.add_relation(pkg, Some(RelationKind::Association), from, to, comments);
        }
        self.i = start;
        self.skip_unknown();
    }

    /// An attribute or a method.
    fn member(&mut self, id: ElementId) {
        let start = self.i;
        match self.try_member() {
            Some(m) => self.doc.elements.get_mut(&id).unwrap().members.push(m),
            None => {
                self.i = start;
                self.skip_unknown();
            }
        }
    }

    fn try_member(&mut self) -> Option<Member> {
        let mut vis = None;
        let (mut is_static, mut is_abstract, mut autounique) = (false, false, false);
        while self.is_ident(0) && MODIFIERS.contains(&self.text(0)) && self.is_ident(1) {
            match self.text(0) {
                "public" => vis = Some(Visibility::Public),
                "protected" => vis = Some(Visibility::Protected),
                "private" => vis = Some(Visibility::Private),
                "static" | "const" | "constant" => is_static = true,
                "abstract" => is_abstract = true,
                "autounique" => autounique = true,
                _ => {}
            }
            self.i += 1;
        }
        let first = self.type_text()?;
        let plain = is_name(&first);
        let (name, ty) = if self.is(0, "(") || self.is(0, ";") || self.is(0, "=") {
            if !plain {
                return None;
            }
            (first, None)
        } else {
            (self.ident()?.to_string(), Some(first))
        };
        if self.is(0, "(") {
            let params = self.parameters()?;
            if self.eat("throws") {
                while self.ident().is_some() && self.eat(",") {}
            }
            if !self.eat(";") {
                // One body per language, and a default body without one:
                // `Java,Php { … } { … }`.
                if !self.is(0, "{") && self.code_langs(0).is_none() {
                    return None;
                }
                loop {
                    if self.is(0, "{") {
                        self.skip_block();
                    } else if self.code_langs(0).is_some() {
                        self.skip_more_code();
                    } else {
                        break;
                    }
                }
                self.eat(";");
            }
            let ret = ty
                .filter(|t| t != "void")
                .map(|t| format!(": {t}"))
                .unwrap_or_default();
            return Some(Member {
                vis,
                text: format!("{name}({params}){ret}"),
                is_static,
                is_abstract,
            });
        }
        // An attribute. Untyped attributes are strings in Umple.
        let ty = ty.unwrap_or_else(|| if autounique { "Integer" } else { "String" }.into());
        let mut derived = false;
        let mut value = None;
        if self.eat("=") {
            if self.is(0, "{") || self.code_langs(0).is_some() {
                // `= { code }` derives the value; `T[] x = { … };` sets it.
                derived = !ty.ends_with("[]");
                if self.is(0, "{") {
                    self.skip_block();
                }
                self.skip_more_code();
            } else {
                let from = self.i;
                let mut depth = 0usize;
                while let Some(t) = self.peek(0) {
                    match t.text {
                        "(" | "[" | "{" => depth += 1,
                        ")" | "]" | "}" if depth > 0 => depth -= 1,
                        ";" | "}" if depth == 0 => break,
                        _ => {}
                    }
                    self.i += 1;
                }
                let v = self.source_since(from).trim();
                // Kept when short and plain; a `(` would make the line read
                // as an operation.
                if !v.is_empty() && !v.contains(['\n', '(']) {
                    value = Some(v.to_string());
                }
            }
        }
        if !self.eat(";") && !derived && !self.is(0, "}") {
            return None;
        }
        let mut text = format!("{}{name}: {ty}", if derived { "/" } else { "" });
        if let Some(v) = value {
            text.push_str(" = ");
            text.push_str(&v);
        }
        Some(Member {
            vis,
            text,
            is_static,
            is_abstract: false,
        })
    }

    /// `Name`, `List<String>`, `String[]`, `Object...`.
    fn type_text(&mut self) -> Option<String> {
        let mut out = self.ident()?.to_string();
        if self.is(0, "<") {
            out.push_str(&self.skip_angles());
        }
        while self.is(0, "[") && self.is(1, "]") {
            out.push_str("[]");
            self.i += 2;
        }
        if self.eat("...") {
            out.push_str("...");
        }
        Some(out)
    }

    /// `(Integer a, List<String> b)` as `a: Integer, b: List<String>`.
    fn parameters(&mut self) -> Option<String> {
        self.i += 1;
        let mut params = Vec::new();
        let mut cur: Vec<&str> = Vec::new();
        let mut depth = 0usize;
        loop {
            let t = self.peek(0)?;
            let text = t.text;
            if t.kind == Kind::Punct && matches!(text, ";" | "{" | "}") {
                return None;
            }
            self.i += 1;
            match text {
                ")" if depth == 0 => break,
                "," if depth == 0 => {
                    params.push(parameter(&cur));
                    cur.clear();
                    continue;
                }
                "<" | "(" => depth += 1,
                ">" | ")" => depth = depth.saturating_sub(1),
                _ => {}
            }
            cur.push(text);
        }
        if !cur.is_empty() {
            params.push(parameter(&cur));
        }
        Some(params.join(", "))
    }

    // ----- names ---------------------------------------------------------------------

    fn finish(mut self) -> Parsed {
        for p in std::mem::take(&mut self.pending) {
            let from = self.resolve(p.from.0, p.container);
            let to = self.resolve(p.to.0, p.container);
            let kind = p.kind.unwrap_or_else(|| self.is_a_kind(from, to));
            let relation = Relation {
                kind,
                from: End {
                    target: Endpoint::Element(from),
                    label: p.from.1,
                },
                to: End {
                    target: Endpoint::Element(to),
                    label: p.to.1,
                },
                name: None,
                comments: p.comments,
            };
            self.doc.relations.insert(p.id, relation);
        }
        self.doc.trailing_comments = std::mem::take(&mut self.pending_comments);
        self.problems.sort_by_key(|p| (p.line, p.col));
        Parsed {
            doc: self.doc,
            problems: self.problems,
            has_layout: false,
            spans: Vec::new(),
        }
    }

    /// The element a name refers to. A name declared nowhere in the file
    /// becomes a class where it is used.
    fn resolve(&mut self, r: Ref, container: Option<PackageId>) -> ElementId {
        let (name, line, col) = match r {
            Ref::Id(id) => return id,
            Ref::Name { name, line, col } => (name, line, col),
        };
        let name = if is_name(&name) { name } else { clean(&name) };
        if let Some(id) = self.by_name.get(&name) {
            return *id;
        }
        self.problems.push(Problem {
            line,
            col,
            message: format!("`{name}` is not declared in this file; added as a class"),
        });
        let comments = std::mem::take(&mut self.pending_comments);
        let id = self.declare(&name, ElementKind::Class, container);
        self.pending_comments = comments;
        id
    }

    /// `isA` names a superclass, or an interface or trait to implement.
    fn is_a_kind(&self, child: ElementId, parent: ElementId) -> RelationKind {
        let (c, p) = (&self.doc.elements[&child], &self.doc.elements[&parent]);
        let trait_parent = p
            .stereotype
            .as_deref()
            .is_some_and(|s| s.split(", ").any(|x| x == "trait"));
        if (p.kind == ElementKind::Interface && c.kind != ElementKind::Interface) || trait_parent {
            RelationKind::Realization
        } else {
            RelationKind::Generalization
        }
    }
}

/// `"mult role"`, the `.umpl` end label.
fn label(mult: &str, role: Option<&str>) -> Option<String> {
    Some(match role {
        Some(r) => format!("{mult} {r}"),
        None => mult.to_string(),
    })
}

/// Appends a token to a type, with a space after commas only.
fn push_type_token(out: &mut String, token: &str) {
    out.push_str(token);
    if token == "," {
        out.push(' ');
    }
}

/// `Integer a` as `a: Integer`; a lone name stays as it is.
fn parameter(tokens: &[&str]) -> String {
    let tokens: Vec<&str> = tokens.iter().copied().filter(|t| *t != "final").collect();
    match tokens.split_last() {
        None => String::new(),
        Some((name, [])) => name.to_string(),
        Some((name, ty)) => {
            let mut t = String::new();
            for tok in ty {
                push_type_token(&mut t, tok);
            }
            format!("{name}: {t}")
        }
    }
}

/// The lines of a comment, without `*` margins or UmpleOnline markers
/// such as `$?[End_of_model]$?`.
fn comment_lines(text: &str) -> Vec<String> {
    if text.trim_start().starts_with("$?") {
        return Vec::new();
    }
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            let l = l.trim();
            let l = l.strip_prefix('*').map(str::trim_start).unwrap_or(l);
            l.to_string()
        })
        .collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print;

    /// The imported model as `.umpl` text, with no problems allowed.
    fn umpl(src: &str) -> String {
        let p = import(src);
        assert_eq!(p.problems, vec![], "{src}");
        assert!(!p.has_layout);
        print(&p.doc)
    }

    fn problems(src: &str) -> Vec<(usize, String)> {
        import(src)
            .problems
            .into_iter()
            .map(|p| (p.line, p.message))
            .collect()
    }

    #[test]
    fn attributes_and_methods() {
        let src = r#"
class Person {
  name;
  Integer id;
  autounique code;
  const Integer MAX_LOANS = 5;
  lazy immutable Date joined;
  String[] emails;
  Boolean active = true;
  Date created = new Date();
  Double area = { return 3; }
  public String fullName() { return name + " }"; }
  private static void reset(Integer level, Map<String, Integer> names) throws IOException Java { } Python { pass }
  protected abstract Integer count();
  @Override
  public String toString() { return "{"; }
  lazy;
}
"#;
        assert_eq!(
            umpl(src),
            "\
class Person {
  name: String
  id: Integer
  code: Integer
  MAX_LOANS: Integer = 5 {static}
  joined: Date
  emails: String[]
  active: Boolean = true
  created: Date
  /area: Double
  +fullName(): String
  -reset(level: Integer, names: Map<String, Integer>) {static}
  #count(): Integer {abstract}
  +toString(): String
  lazy: String
}
"
        );
    }

    #[test]
    fn associations_inline_and_standalone() {
        let src = "
class A {
  1 owner -- * B items;
  * -> 0..1 C;
  0..1 <- * D;
  1 <@>- 1..* E;
  * -<@> 1 F;
  0..1 >< * G;
  // kept with the line below
  immutable 1 -- * H sorted {id};
  0..1 self buddy;
}
class B {} class C {} class D {} class E {} class F {} class G {} class H {}
association Links {
  * B -- 1 C;
  0..1 C original -> * C copies;
}
";
        assert_eq!(
            umpl(src),
            "\
class A

A \"1 owner\" -- \"* items\" B
A \"*\" --> \"0..1\" C
D \"*\" --> \"0..1\" A
A \"1\" *-- \"1..*\" E
F \"1\" *-- \"*\" A
A \"0..1\" -- \"*\" G
# kept with the line below
A \"1\" -- \"*\" H
A \"0..1 buddy\" -- \"0..1 buddy\" A

class B

class C

class D

class E

class F

class G

class H

B \"*\" -- \"1\" C
C \"0..1 original\" --> \"* copies\" C
"
        );
    }

    #[test]
    fn generalization_interfaces_enums_and_kinds() {
        let src = r#"
class Shape { abstract; }
interface Drawable {
  const String PREFIX = "!";
  void draw(Integer scale);
  isA Base;
}
interface Base {}
class Circle { isA Shape, Drawable; singleton; enum Unit { Cm, Mm } }
class Square { isA Shape; isA Drawable; class Tiny {} }
enum Colour { Red, Green, Blue }
trait Named { name; }
external Widget {}
class Label { isA Named, isA Widget<T>; immutable; singleton; }
"#;
        assert_eq!(
            umpl(src),
            "\
abstract class Shape

interface Drawable {
  PREFIX: String = \"!\" {static}
  draw(scale: Integer)
}

Drawable --|> Base

interface Base

class Circle <<singleton>>

Circle --|> Shape
Circle ..|> Drawable

enum Unit { Cm, Mm }

class Square

Square --|> Shape
Square ..|> Drawable

class Tiny

Tiny --|> Square

enum Colour { Red, Green, Blue }

class Named <<trait>> {
  name: String
}

class Widget <<external>>

class Label <<immutable, singleton>>

Label ..|> Named
Label --|> Widget
"
        );
    }

    #[test]
    fn association_class_becomes_a_class_with_two_associations() {
        let src = "
associationClass Reservation {
  * Member;
  * waiting Book wanted;
  Date placed;
}
class Member {}
class Book {}
";
        assert_eq!(
            umpl(src),
            "\
class Reservation {
  placed: Date
}

Member \"1\" -- \"*\" Reservation
Book \"1 wanted\" -- \"* waiting\" Reservation

class Member

class Book
"
        );
    }

    #[test]
    fn namespaces_become_packages_and_reopened_classes_merge() {
        let src = "
// The school system
namespace school.core;

/* A student,
 * enrolled in courses */
class Student { name; }

namespace school;
class Course { 1 -- * Student; }

//$?[End_of_model]$?
// Positioning
class Student {
  position 10 20 109 45;
  position.association Course__Student 1,2 3,4;
  displayColor #ff0000;
  Integer id;
}
";
        assert_eq!(
            umpl(src),
            "\
# The school system
package school {
  package core {
    # A student,
    # enrolled in courses
    class Student {
      name: String
      id: Integer
    }
  }

  class Course

  school.Course \"1\" -- \"*\" school.core.Student
}
"
        );
    }

    #[test]
    fn skipped_parts_are_reported() {
        let src = r#"generate Java;
use "other.ump";
req R1 { Members can't borrow { more } books. }
statemachine Global { A { go -> B; } B {} }
mixset Extra { class Ghost {} }
class Loan {
  status { Open { close -> Closed; } Closed {} }
  [amount >= 0]
  key { id }
  depend java.util.*;
  before getId { log(); }
  whatever this is;
  1 -- * Fine;
}
something odd;
strictness allow 1006;
"#;
        assert_eq!(
            problems(src),
            [
                (
                    2,
                    "`use \"other.ump\"`: other files are not read".to_string()
                ),
                (3, "requirement `R1` is not shown".to_string()),
                (4, "state machine `Global` is not shown".to_string()),
                (5, "mixset `Extra` is not read".to_string()),
                (
                    7,
                    "state machine `status` of `Loan` is not shown".to_string()
                ),
                (
                    8,
                    "constraint `[amount >= 0]` of `Loan` is not shown".to_string()
                ),
                (
                    12,
                    "`whatever this is;` is not understood; skipped".to_string()
                ),
                (
                    13,
                    "`Fine` is not declared in this file; added as a class".to_string()
                ),
                (
                    15,
                    "`something odd;` is not understood; skipped".to_string()
                ),
            ]
        );
        let doc = import(src).doc;
        let names: Vec<&str> = doc.elements.values().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Loan", "Fine"]);
        assert_eq!(doc.relations.len(), 1);
    }

    #[test]
    fn code_templates_ports_and_aspects_are_skipped() {
        let src = r#"
after {*} set* { doSomething(); }
around {Page} custom Label1-Label2:render() { around_proceed: }
class Page {
  // `//` and braces inside a template are template text
  cr <<!
!>>
  html <<!<a href="http://x">{<<=name>>}</a> // }!>>
  int m1() Java,Php { return 0; } { return 1; }
  void active refresh() { }
  void active poll { }
  public in Integer clicks;
  out Boolean ready;
  clicks -> view.input;
  this.ready -> other.go;
  name;
}
"#;
        let p = import(src);
        let problems: Vec<(usize, &str)> = p
            .problems
            .iter()
            .map(|p| (p.line, p.message.as_str()))
            .collect();
        assert_eq!(
            problems,
            [
                (12, "port `clicks` of `Page` is not shown"),
                (13, "port `ready` of `Page` is not shown"),
            ]
        );
        assert_eq!(
            print(&p.doc),
            "class Page {\n  m1(): int\n  name: String\n}\n"
        );
    }

    #[test]
    fn trait_type_parameters_are_not_classes() {
        let src = "
trait Subject <Observer, Item isA Comparable = Thing> {
  0..1 -> * Observer;
  1 -- * Item;
  1 -- * Log;
}
class Log {}
class Dashboard { isA Subject<Observer = Dashboard>; }
";
        let p = import(src);
        let messages: Vec<&str> = p.problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "association to type parameter `Observer` of `Subject` is not shown",
                "association to type parameter `Item` of `Subject` is not shown",
            ]
        );
        let names: Vec<&str> = p.doc.elements.values().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Subject", "Log", "Dashboard"]);
        assert_eq!(p.doc.relations.len(), 2);
    }

    #[test]
    fn unclosed_body_is_reported() {
        let p = import("class A {\n  name;\n");
        assert_eq!(p.problems.len(), 1);
        assert_eq!(p.problems[0].message, "missing `}` at end of file");
        assert_eq!(p.doc.elements.len(), 1);
    }

    #[test]
    fn stray_closing_brace_is_reported() {
        assert_eq!(
            problems("class A {}\n}\nclass B {}"),
            [(2, "`}` without a matching `{`".to_string())]
        );
    }

    #[test]
    fn garbage_never_panics() {
        let src = r#"class A { 1 -- * B; isA C; x; void f(int a) { "}" } s { a -> b; } [x] }
association { 0..1 A -> * B r; } associationClass R { * A; 2..3 x B y; }
enum E { a, b } namespace p.q; interface I { void m(); } @ ; ) ] < > <@>- -<@> ..."#;
        // Every prefix, so truncated statements and blocks are covered.
        for end in (0..=src.len()).filter(|i| src.is_char_boundary(*i)) {
            let p = import(&src[..end]);
            let _ = print(&p.doc);
        }
        for junk in [
            "{{{{", "}}}}", "class", "class {", "isA", "1 --", "* <@>- ;", "enum {", "\"", "'",
        ] {
            import(junk);
        }
    }
}
