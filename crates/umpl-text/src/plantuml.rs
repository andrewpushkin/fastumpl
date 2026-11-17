//! PlantUML class diagrams: import into a [`Document`] and export from one.
//!
//! Import never fails. Anything skipped or changed is reported as a
//! [`Problem`] at its line in the PlantUML file; lines that are not
//! understood are kept as raw lines, as the `.umpl` parser does. See
//! `docs/plans/2026-11-17-plantuml-mermaid.md` for the mapping.

use std::collections::HashMap;

use umpl_core::*;

use crate::parser::{Parsed, Problem};
use crate::scan::Cursor;

// ----- export ---------------------------------------------------------------

/// Writes `doc` as a PlantUML class diagram. Positions are not written:
/// PlantUML lays diagrams out itself.
pub fn export(doc: &Document) -> String {
    let mut w = Exporter {
        doc,
        refs: reference_names(doc),
        out: String::from("@startuml\n"),
    };
    if let Some(t) = &doc.title {
        w.out
            .push_str(&format!("title {}\n", t.replace('\n', "\\n")));
    }
    w.decls(&doc.root, 0, doc.title.is_some());
    if !doc.trailing_comments.is_empty() {
        w.out.push('\n');
        for c in &doc.trailing_comments {
            w.out.push_str(&comment(c));
        }
    }
    w.out.push_str("@enduml\n");
    w.out
}

struct Exporter<'a> {
    doc: &'a Document,
    refs: HashMap<ElementId, String>,
    out: String,
}

/// The name a relation uses for each element: the plain name when no other
/// element has it, otherwise the full dotted name.
fn reference_names(doc: &Document) -> HashMap<ElementId, String> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for el in doc.elements.values() {
        *count.entry(el.name.as_str()).or_default() += 1;
    }
    doc.elements
        .iter()
        .map(|(id, el)| {
            let name = if count[el.name.as_str()] == 1 {
                el.name.clone()
            } else {
                doc.full_name(*id)
            };
            (*id, name)
        })
        .collect()
}

fn comment(c: &str) -> String {
    if c.is_empty() {
        "'\n".to_string()
    } else {
        format!("' {c}\n")
    }
}

/// A PlantUML double-quoted string. PlantUML has no `\"` escape.
fn pquote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "'").replace('\n', "\\n"))
}

impl Exporter<'_> {
    fn decls(&mut self, decls: &[Decl], depth: usize, mut need_gap: bool) {
        let indent = "  ".repeat(depth);
        let mut prev: Option<&Decl> = None;
        for d in decls {
            let grouped = matches!(
                (prev, d),
                (Some(Decl::Relation(_)), Decl::Relation(_)) | (Some(Decl::Raw(_)), Decl::Raw(_))
            );
            if need_gap && !grouped {
                self.out.push('\n');
            }
            need_gap = true;
            prev = Some(d);
            match d {
                Decl::Element(id) => self.element(*id, &indent),
                Decl::Relation(id) => self.relation(*id, &indent),
                Decl::Package(id) => self.package(*id, depth),
                Decl::Raw(text) => {
                    self.out.push_str(&indent);
                    self.out.push_str(&comment(text));
                }
            }
        }
    }

    fn comments(&mut self, comments: &[String], indent: &str) {
        for c in comments {
            self.out.push_str(indent);
            self.out.push_str(&comment(c));
        }
    }

    fn package(&mut self, id: PackageId, depth: usize) {
        let Some(p) = self.doc.packages.get(&id) else {
            return;
        };
        let indent = "  ".repeat(depth);
        self.comments(&p.comments, &indent);
        self.out
            .push_str(&format!("{indent}package {} {{\n", p.name));
        self.decls(&p.body, depth + 1, false);
        self.out.push_str(&format!("{indent}}}\n"));
    }

    fn element(&mut self, id: ElementId, indent: &str) {
        let Some(el) = self.doc.elements.get(&id) else {
            return;
        };
        self.comments(&el.comments, indent);
        let out = &mut self.out;
        if el.kind == ElementKind::Note {
            let text: Vec<&str> = el.members.iter().map(|m| m.text.as_str()).collect();
            out.push_str(&format!(
                "{indent}note {} as {}\n",
                pquote(&text.join("\n")),
                el.name
            ));
            return;
        }
        out.push_str(&format!("{indent}{} {}", el.kind.keyword(), el.name));
        if let Some(s) = &el.stereotype {
            out.push_str(&format!(" <<{s}>>"));
        }
        if el.members.is_empty() {
            out.push('\n');
            return;
        }
        out.push_str(" {\n");
        for m in &el.members {
            out.push_str(&format!("{indent}  {}\n", m.to_line()));
        }
        out.push_str(&format!("{indent}}}\n"));
    }

    fn relation(&mut self, id: RelationId, indent: &str) {
        let Some(r) = self.doc.relations.get(&id) else {
            return;
        };
        self.comments(&r.comments, indent);
        let name = |e: ElementId| self.refs.get(&e).cloned().unwrap_or_default();
        let mut line = format!("{indent}{}", name(r.from.element));
        if let Some(l) = &r.from.label {
            line.push_str(&format!(" {}", pquote(l)));
        }
        line.push_str(&format!(" {}", r.kind.arrow()));
        if let Some(l) = &r.to.label {
            line.push_str(&format!(" {}", pquote(l)));
        }
        line.push_str(&format!(" {}", name(r.to.element)));
        if let Some(n) = &r.name {
            line.push_str(&format!(" : {n}"));
        }
        line.push('\n');
        self.out.push_str(&line);
    }
}

// ----- import ---------------------------------------------------------------

/// Reads a PlantUML class diagram. Never fails; see [`Parsed::problems`].
/// The result has no positions, so the editor lays it out on open.
pub fn import(src: &str) -> Parsed {
    let lines: Vec<&str> = src.lines().collect();
    let is_start = |l: &str| l.trim_start().starts_with("@start");
    let mut started = !lines.iter().any(|l| is_start(l));
    let mut im = Importer::default();
    for (i, raw) in lines.iter().enumerate() {
        let n = i + 1;
        if !started {
            started = is_start(raw);
            continue;
        }
        if raw.trim_start().starts_with("@end") {
            if let Some(j) = lines[i + 1..].iter().position(|l| is_start(l)) {
                im.problem(n + 1 + j, "only the first diagram in the file is imported");
            }
            break;
        }
        im.last_line = n;
        im.line(n, raw.trim());
    }
    im.finish()
}

/// Element keywords that become a class with the keyword as stereotype.
const CLASS_LIKE: [&str; 9] = [
    "annotation",
    "entity",
    "struct",
    "record",
    "exception",
    "metaclass",
    "protocol",
    "dataclass",
    "object",
];

/// Commands that only change how PlantUML draws; skipped with a problem.
const STYLING: [&str; 13] = [
    "skinparam",
    "hide",
    "show",
    "remove",
    "restore",
    "scale",
    "set",
    "allowmixing",
    "allow_mixing",
    "mainframe",
    "caption",
    "newpage",
    "sprite",
];

enum Mode {
    Normal,
    Body {
        id: ElementId,
        is_enum: bool,
    },
    BlockComment,
    /// A `note … end note` block.
    Note {
        id: Option<ElementId>,
        lines: Vec<String>,
    },
    /// A block to skip until a closing line.
    Skip(SkipEnd),
    Title(Vec<String>),
}

enum SkipEnd {
    /// `}` that brings the brace depth to zero.
    Brace(usize),
    /// `end <word>` or `end<word>`.
    End(&'static str),
    /// An exact closing line such as `</style>`.
    Line(&'static str),
}

/// Something to resolve once every declaration has been read.
enum Pending {
    Relation {
        id: RelationId,
        container: Option<PackageId>,
        kind: RelationKind,
        from: (String, Option<String>),
        to: (String, Option<String>),
        name: Option<String>,
        comments: Vec<String>,
    },
    Member {
        container: Option<PackageId>,
        target: String,
        member: Member,
    },
    /// A note attached to an element: `note left of X`.
    NoteLink {
        container: Option<PackageId>,
        note: ElementId,
        target: String,
    },
}

#[derive(Default)]
struct Importer {
    doc: Document,
    problems: Vec<Problem>,
    mode: Option<Mode>,
    /// The container of each open `{`; the current container is the last.
    stack: Vec<Option<PackageId>>,
    comments: Vec<String>,
    pending: Vec<Pending>,
    /// PlantUML names (aliases and quoted display names) for elements.
    aliases: HashMap<String, ElementId>,
    /// An element declared on the previous line without a body, which a
    /// `{` on the next line opens.
    last_decl: Option<(ElementId, bool)>,
    last_element: Option<ElementId>,
    last_line: usize,
}

impl Importer {
    fn problem(&mut self, line: usize, message: impl Into<String>) {
        self.problems.push(Problem {
            line,
            col: 1,
            message: message.into(),
        });
    }

    fn container(&self) -> Option<PackageId> {
        self.stack.last().copied().flatten()
    }

    fn push_decl_in(&mut self, pkg: Option<PackageId>, d: Decl) {
        match pkg {
            Some(p) => self.doc.packages.get_mut(&p).unwrap().body.push(d),
            None => self.doc.root.push(d),
        }
    }

    fn line(&mut self, n: usize, t: &str) {
        match self.mode.take().unwrap_or(Mode::Normal) {
            Mode::Normal => self.normal(n, t),
            Mode::Body { id, is_enum } => self.body_line(t, id, is_enum),
            Mode::BlockComment => self.block_comment(t),
            Mode::Note { id, mut lines } => {
                if is_end(t, "note") {
                    self.finish_note(id, lines);
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Note { id, lines });
                }
            }
            Mode::Skip(end) => self.skip_line(t, end),
            Mode::Title(mut lines) => {
                if is_end(t, "title") {
                    let title = lines.join("\n");
                    self.doc.title = Some(title).filter(|s| !s.is_empty());
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Title(lines));
                }
            }
        }
    }

    fn skip_line(&mut self, t: &str, end: SkipEnd) {
        let next = match end {
            SkipEnd::Brace(depth) => {
                let opened = t.matches('{').count();
                let closed = t.matches('}').count();
                let depth = (depth + opened).saturating_sub(closed);
                (depth > 0).then_some(SkipEnd::Brace(depth))
            }
            SkipEnd::End(word) => (!is_end(t, word)).then_some(SkipEnd::End(word)),
            SkipEnd::Line(l) => (!t.eq_ignore_ascii_case(l)).then_some(SkipEnd::Line(l)),
        };
        self.mode = next.map(Mode::Skip);
    }

    fn block_comment(&mut self, t: &str) {
        match t.find("'/") {
            Some(i) => {
                let text = t[..i].trim();
                if !text.is_empty() {
                    self.comments.push(text.to_string());
                }
            }
            None => {
                let text = t.trim();
                if !text.is_empty() {
                    self.comments.push(text.to_string());
                }
                self.mode = Some(Mode::BlockComment);
            }
        }
    }

    fn body_line(&mut self, t: &str, id: ElementId, is_enum: bool) {
        if t == "}" {
            return;
        }
        self.mode = Some(Mode::Body { id, is_enum });
        if t.is_empty() || t.starts_with('\'') || is_separator(t) {
            return;
        }
        let el = self.doc.elements.get_mut(&id).unwrap();
        if is_enum {
            el.members.extend(enum_literals(t));
        } else if let Some(m) = member(t) {
            el.members.push(m);
        }
    }

    fn normal(&mut self, n: usize, t: &str) {
        let last_decl = self.last_decl.take();
        if t.is_empty() {
            self.last_decl = last_decl;
            return;
        }
        if t == "{" {
            if let Some((id, is_enum)) = last_decl {
                self.mode = Some(Mode::Body { id, is_enum });
                return;
            }
        }
        if let Some(c) = t.strip_prefix('\'') {
            self.comments
                .push(c.strip_prefix(' ').unwrap_or(c).trim_end().to_string());
            return;
        }
        if let Some(rest) = t.strip_prefix("/'") {
            self.block_comment(rest);
            return;
        }
        if t == "}" {
            if self.stack.pop().is_none() {
                self.raw(n, t, "`}` without a matching `{`");
            }
            return;
        }
        if self.try_title(t)
            || self.try_skipped(n, t)
            || self.try_package(n, t)
            || self.try_note(n, t)
            || self.try_element(n, t)
            || self.try_relation(n, t)
            || self.try_member(t)
        {
            return;
        }
        self.raw(n, t, "not understood; kept as written");
    }

    /// Keeps a line as raw text, with any comments before it.
    fn raw(&mut self, n: usize, t: &str, message: &str) {
        let container = self.container();
        for c in std::mem::take(&mut self.comments) {
            self.push_decl_in(
                container,
                Decl::Raw(format!("# {c}").trim_end().to_string()),
            );
        }
        self.push_decl_in(container, Decl::Raw(t.to_string()));
        self.problem(n, message);
    }

    fn try_title(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("title") {
            return false;
        }
        let rest = c.rest().trim();
        if rest.is_empty() {
            self.mode = Some(Mode::Title(Vec::new()));
        } else {
            let rest = rest.strip_prefix(':').map(str::trim).unwrap_or(rest);
            self.doc.title = Some(unquote(rest).replace("\\n", "\n"));
        }
        true
    }

    fn try_skipped(&mut self, n: usize, t: &str) -> bool {
        let lower = t.to_ascii_lowercase();
        let word = lower
            .split(|c: char| c.is_whitespace() || c == '{')
            .next()
            .unwrap_or("");
        let skip = |me: &mut Self, what: &str, end: Option<SkipEnd>| {
            let block = if end.is_some() { "block" } else { "line" };
            me.problem(
                n,
                format!("PlantUML `{what}` is not supported; {block} skipped"),
            );
            me.mode = end.map(Mode::Skip);
            true
        };
        if lower.starts_with('!') {
            let word = word.trim_start_matches('!');
            return skip(self, &format!("!{word}"), None);
        }
        if lower.starts_with("<style>") {
            let end = (!lower.contains("</style>")).then_some(SkipEnd::Line("</style>"));
            return skip(self, "<style>", end);
        }
        if lower.ends_with(" direction") && lower.split_whitespace().count() == 4 {
            return skip(self, t, None);
        }
        let first = t
            .split(|c: char| c.is_whitespace() || c == '{')
            .next()
            .unwrap_or("");
        let after = t[first.len()..].trim_start();
        let looks_like_relation = after.starts_with(['-', '.', '<', '*', ':', '"']);
        if STYLING.contains(&first) && !looks_like_relation {
            let depth = t
                .matches('{')
                .count()
                .saturating_sub(t.matches('}').count());
            let end = (depth > 0).then_some(SkipEnd::Brace(depth));
            return skip(self, word, end);
        }
        if looks_like_relation {
            return false;
        }
        match first {
            "legend" => skip(self, "legend", Some(SkipEnd::End("legend"))),
            "header" | "footer" => {
                let w: &'static str = if first == "header" {
                    "header"
                } else {
                    "footer"
                };
                let block = lower.trim() == w;
                skip(self, w, block.then_some(SkipEnd::End(w)))
            }
            _ => false,
        }
    }

    fn try_package(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if c.eat_word("together") {
            if c.eat("{") && c.at_end() {
                self.stack.push(self.container());
                return true;
            }
            return false;
        }
        if !(c.eat_word("package") || c.eat_word("namespace")) {
            return false;
        }
        let Some(name) = decl_name(&mut c) else {
            return false;
        };
        skip_decorations(&mut c);
        let open = c.eat("{") && !c.eat("}");
        if !c.at_end() {
            return false;
        }
        let mut pkg = self.container();
        let parts: Vec<String> = name.split('.').map(clean).collect();
        for part in &parts {
            pkg = Some(self.ensure_package(n, pkg, part));
        }
        if open {
            self.stack.push(pkg);
        }
        true
    }

    /// The package `name` directly inside `container`, created if needed.
    fn ensure_package(&mut self, n: usize, container: Option<PackageId>, name: &str) -> PackageId {
        let existing = self.doc.body(container).iter().find_map(|d| match d {
            Decl::Package(p) if self.doc.packages[p].name == name => Some(*p),
            _ => None,
        });
        if let Some(p) = existing {
            return p;
        }
        let mut name = name.to_string();
        if !self.doc.name_is_free(&name, container) {
            let new = self.doc.unique_name(&name, container);
            self.problem(
                n,
                format!("package `{name}` has the name of a class; renamed to `{new}`"),
            );
            name = new;
        }
        let id = self.doc.new_package_id();
        let mut p = Package::new(name);
        p.comments = std::mem::take(&mut self.comments);
        self.doc.packages.insert(id, p);
        self.push_decl_in(container, Decl::Package(id));
        id
    }

    fn try_element(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let (kind, keyword_stereo) = if c.eat_word("abstract") {
            c.eat_word("class");
            (ElementKind::Abstract, None)
        } else if c.eat_word("class") {
            (ElementKind::Class, None)
        } else if c.eat_word("interface") {
            (ElementKind::Interface, None)
        } else if c.eat_word("enum") {
            (ElementKind::Enum, None)
        } else if let Some(w) = CLASS_LIKE.iter().find(|w| c.eat_word(w)) {
            (ElementKind::Class, Some(w.to_string()))
        } else {
            return false;
        };
        c.skip_ws();
        let name_quoted = c.rest().starts_with('"');
        let Some(name) = decl_name(&mut c) else {
            return false;
        };
        let mut dropped_generics = None;
        if c.rest().starts_with('<') && !c.rest().starts_with("<<") {
            dropped_generics = balanced_angle(&mut c);
        }
        let mut alias = None;
        let mut alias_quoted = false;
        if c.eat_word("as") {
            c.skip_ws();
            alias_quoted = c.rest().starts_with('"');
            alias = decl_name(&mut c);
            if alias.is_none() {
                return false;
            }
        }
        let mut stereotype = None;
        let mut parents: Vec<(RelationKind, String)> = Vec::new();
        loop {
            c.skip_ws();
            let r = c.rest();
            if r.starts_with("<<") {
                let Some(s) = stereo(&mut c) else {
                    return false;
                };
                if stereotype.is_none() && !s.is_empty() {
                    stereotype = Some(s);
                }
            } else if r.starts_with('#') {
                skip_token(&mut c);
            } else if c.eat_word("extends") {
                parents.extend(
                    name_list(&mut c)
                        .into_iter()
                        .map(|p| (RelationKind::Generalization, p)),
                );
            } else if c.eat_word("implements") {
                parents.extend(
                    name_list(&mut c)
                        .into_iter()
                        .map(|p| (RelationKind::Realization, p)),
                );
            } else {
                break;
            }
        }
        let rest = c.rest().trim();
        let (open, inline) = if rest.is_empty() {
            (false, None)
        } else if rest == "{" {
            (true, None)
        } else if rest.starts_with('{') && rest.ends_with('}') {
            (false, Some(rest[1..rest.len() - 1].trim()))
        } else {
            return false;
        };

        // The element's name is what PlantUML shows: the quoted one of
        // `class "Shown" as X` / `class X as "Shown"`, cleaned up into a
        // valid name if needed. The other one becomes an alias.
        let display = match &alias {
            Some(a) if alias_quoted && !name_quoted => a.clone(),
            _ => name.clone(),
        };
        let path = if is_name_path(&display) {
            display
        } else {
            clean(&display)
        };
        let id = self.declare(n, kind, &path, stereotype.or(keyword_stereo));
        for a in [Some(name.clone()), alias].into_iter().flatten() {
            if a != path {
                self.aliases.insert(a, id);
            }
        }
        if let Some(g) = dropped_generics {
            self.problem(n, format!("type parameters `{g}` of `{name}` dropped"));
        }
        let is_enum = kind == ElementKind::Enum;
        if let Some(inner) = inline {
            let el = self.doc.elements.get_mut(&id).unwrap();
            if is_enum {
                el.members.extend(enum_literals(inner));
            } else {
                el.members.extend(inner.split(';').filter_map(member));
            }
        }
        let container = self.container();
        for (k, parent) in parents {
            let rid = self.doc.new_relation_id();
            self.push_decl_in(container, Decl::Relation(rid));
            self.pending.push(Pending::Relation {
                id: rid,
                container,
                kind: k,
                from: (path.clone(), None),
                to: (parent, None),
                name: None,
                comments: Vec::new(),
            });
        }
        if open {
            self.mode = Some(Mode::Body { id, is_enum });
        } else if inline.is_none() {
            self.last_decl = Some((id, is_enum));
        }
        self.last_element = Some(id);
        true
    }

    /// Declares an element at `path` (dotted names are absolute, as in
    /// PlantUML). A second declaration of the same element updates it.
    fn declare(
        &mut self,
        n: usize,
        kind: ElementKind,
        path: &str,
        stereotype: Option<String>,
    ) -> ElementId {
        let parts: Vec<&str> = path.split('.').collect();
        let (name, pkgs) = parts.split_last().unwrap();
        let mut container = if pkgs.is_empty() {
            self.container()
        } else {
            None
        };
        for p in pkgs {
            container = Some(self.ensure_package(n, container, p));
        }
        let existing = self.doc.body(container).iter().find_map(|d| match d {
            Decl::Element(e) if self.doc.elements[e].name == *name => Some(*e),
            _ => None,
        });
        if let Some(id) = existing {
            let el = self.doc.elements.get_mut(&id).unwrap();
            el.kind = kind;
            if stereotype.is_some() {
                el.stereotype = stereotype;
            }
            el.comments.append(&mut self.comments);
            return id;
        }
        let mut name = name.to_string();
        if !self.doc.name_is_free(&name, container) {
            let new = self.doc.unique_name(&name, container);
            self.problem(
                n,
                format!("`{name}` is also a package name; renamed to `{new}`"),
            );
            name = new;
        }
        let id = self.doc.new_element_id();
        let mut el = Element::new(kind, name);
        el.stereotype = stereotype;
        el.comments = std::mem::take(&mut self.comments);
        self.doc.elements.insert(id, el);
        self.push_decl_in(container, Decl::Element(id));
        id
    }

    /// A new note element, named `alias` or `Note`, `Note2`, ...
    fn new_note(&mut self, n: usize, alias: Option<String>, text: &str) -> ElementId {
        let container = self.container();
        let id = match alias.as_deref() {
            Some(a) => self.declare(n, ElementKind::Note, &clean(a), None),
            None => {
                let name = self.doc.unique_name("Note", container);
                self.declare(n, ElementKind::Note, &name, None)
            }
        };
        if let Some(a) = alias.filter(|a| clean(a) != *a) {
            self.aliases.insert(a, id);
        }
        self.set_note_text(id, text);
        id
    }

    fn set_note_text(&mut self, id: ElementId, text: &str) {
        let el = self.doc.elements.get_mut(&id).unwrap();
        el.members = text.split('\n').map(|l| Member::plain(l.trim())).collect();
    }

    fn finish_note(&mut self, id: Option<ElementId>, lines: Vec<String>) {
        if let Some(id) = id {
            self.set_note_text(id, &lines.join("\n"));
        }
    }

    fn try_note(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("note") {
            return false;
        }
        // note on link
        if c.eat_word("on") {
            if !c.eat_word("link") {
                return false;
            }
            let block = !c.rest().contains(':');
            self.problem(
                n,
                format!(
                    "notes on links are not supported; {} skipped",
                    if block { "block" } else { "line" }
                ),
            );
            if block {
                self.mode = Some(Mode::Skip(SkipEnd::End("note")));
            }
            return true;
        }
        // note "text" as N
        if let Some(text) = c.string() {
            let alias = if c.eat_word("as") {
                decl_name(&mut c)
            } else {
                None
            };
            skip_decorations(&mut c);
            if !c.at_end() {
                return false;
            }
            self.new_note(n, alias, &text);
            return true;
        }
        // note as N ... end note
        if c.eat_word("as") {
            let Some(alias) = decl_name(&mut c) else {
                return false;
            };
            skip_decorations(&mut c);
            let id = self.new_note(n, Some(alias), "");
            self.mode = Some(Mode::Note {
                id: Some(id),
                lines: Vec::new(),
            });
            return true;
        }
        // note left|right|top|bottom [of X] [: text]
        let side = ["left", "right", "top", "bottom"]
            .into_iter()
            .find(|w| c.eat_word(w) || c.eat(w));
        if side.is_none() {
            return false;
        }
        let target = if c.eat_word("of") {
            match ref_name(&mut c) {
                Some(t) => Some(t),
                None => return false,
            }
        } else {
            None
        };
        skip_decorations(&mut c);
        let text = if c.eat(":") {
            Some(c.rest().trim().replace("\\n", "\n"))
        } else if c.at_end() {
            None
        } else {
            return false;
        };
        let id = self.new_note(n, None, text.as_deref().unwrap_or(""));
        let target = target.or_else(|| {
            self.last_element
                .filter(|e| *e != id)
                .map(|e| self.doc.full_name(e))
        });
        if let Some(target) = target {
            self.pending.push(Pending::NoteLink {
                container: self.container(),
                note: id,
                target,
            });
        }
        if text.is_none() {
            self.mode = Some(Mode::Note {
                id: Some(id),
                lines: Vec::new(),
            });
        }
        true
    }

    fn try_relation(&mut self, n: usize, t: &str) -> bool {
        if t.starts_with('(') {
            self.problem(n, "association classes are not supported; line skipped");
            return true;
        }
        let mut c = Cursor::new(t);
        let Some(a) = ref_name(&mut c) else {
            return false;
        };
        let la = c.string();
        let Some(arrow) = arrow_token(&mut c) else {
            return false;
        };
        let q = c.string();
        let (lb, b) = match ref_name(&mut c) {
            Some(b) => (q, b),
            None => match q {
                Some(b) => (None, b),
                None => return false,
            },
        };
        let name = if c.eat(":") {
            let label = c.rest().trim();
            let label = label
                .trim_start_matches(['<', '>'])
                .trim_end_matches(['<', '>']);
            Some(label.trim().replace("\\n", " ")).filter(|s| !s.is_empty())
        } else if c.at_end() {
            None
        } else {
            return false;
        };
        let (kind, reversed, unknown) = arrow_kind(&arrow);
        if let Some(head) = unknown {
            self.problem(
                n,
                format!("arrow end `{head}` in `{arrow}` is not supported; drawn as a plain line"),
            );
        }
        let (mut from, mut to) = ((a, la), (b, lb));
        if reversed {
            std::mem::swap(&mut from, &mut to);
        }
        let container = self.container();
        let id = self.doc.new_relation_id();
        self.push_decl_in(container, Decl::Relation(id));
        self.pending.push(Pending::Relation {
            id,
            container,
            kind,
            from,
            to,
            name,
            comments: std::mem::take(&mut self.comments),
        });
        true
    }

    /// `Name : +member` outside a class body.
    fn try_member(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let Some(target) = ref_name(&mut c) else {
            return false;
        };
        if !c.eat(":") {
            return false;
        }
        let Some(member) = member(c.rest()) else {
            return false;
        };
        self.pending.push(Pending::Member {
            container: self.container(),
            target,
            member,
        });
        true
    }

    fn finish(mut self) -> Parsed {
        let line = self.last_line.max(1);
        match self.mode.take() {
            Some(Mode::Body { .. }) => self.problem(line, "missing `}` at end of file"),
            Some(Mode::Note { id, lines }) => {
                self.finish_note(id, lines);
                self.problem(line, "missing `end note` at end of file");
            }
            Some(Mode::Title(lines)) => self.doc.title = Some(lines.join("\n")),
            _ => {}
        }
        if !self.stack.is_empty() {
            self.problem(line, "missing `}` at end of file");
        }

        for p in std::mem::take(&mut self.pending) {
            match p {
                Pending::Relation {
                    id,
                    container,
                    kind,
                    from,
                    to,
                    name,
                    comments,
                } => {
                    let f = self.resolve(container, &from.0);
                    let t = self.resolve(container, &to.0);
                    let relation = Relation {
                        kind,
                        from: End {
                            element: f,
                            label: from.1,
                        },
                        to: End {
                            element: t,
                            label: to.1,
                        },
                        name,
                        comments,
                    };
                    self.doc.relations.insert(id, relation);
                }
                Pending::Member {
                    container,
                    target,
                    member,
                } => {
                    let e = self.resolve(container, &target);
                    self.doc.elements.get_mut(&e).unwrap().members.push(member);
                }
                Pending::NoteLink {
                    container,
                    note,
                    target,
                } => {
                    let t = self.resolve(container, &target);
                    let id = self.doc.new_relation_id();
                    let pkg = self.doc.element_package(note);
                    self.push_decl_in(pkg, Decl::Relation(id));
                    self.doc
                        .relations
                        .insert(id, Relation::new(RelationKind::NoteLink, note, t));
                }
            }
        }

        self.doc.trailing_comments.append(&mut self.comments);
        self.problems.sort_by_key(|p| (p.line, p.col));
        Parsed {
            doc: self.doc,
            problems: self.problems,
            has_layout: false,
        }
    }

    /// Finds the element a PlantUML name refers to: an alias, a name in the
    /// enclosing packages, a full dotted name, or a plain name that exactly
    /// one element has. Unknown names create a class where they are used.
    fn resolve(&mut self, container: Option<PackageId>, r: &str) -> ElementId {
        if let Some(id) = self.aliases.get(r) {
            return *id;
        }
        let path = if is_name_path(r) {
            r.to_string()
        } else {
            clean(r)
        };
        let mut scope = container;
        loop {
            let full = match scope {
                Some(p) => format!("{}.{path}", self.doc.full_package_name(p)),
                None => path.clone(),
            };
            if let Some(id) = self.doc.find(&full) {
                return id;
            }
            match scope {
                Some(p) => scope = self.doc.container_of(&Decl::Package(p)),
                None => break,
            }
        }
        if !path.contains('.') {
            let matches: Vec<ElementId> = self
                .doc
                .elements
                .iter()
                .filter(|(_, e)| e.name == path)
                .map(|(id, _)| *id)
                .collect();
            if let [only] = matches[..] {
                return only;
            }
        }
        let saved = std::mem::replace(&mut self.stack, vec![container]);
        let id = self.declare(self.last_line, ElementKind::Class, &path, None);
        self.stack = saved;
        id
    }
}

/// `end note` or `endnote`.
fn is_end(t: &str, word: &str) -> bool {
    let t = t.to_ascii_lowercase();
    t == format!("end {word}") || t == format!("end{word}")
}

/// `--`, `..`, `==`, `__` separator lines inside a class body.
fn is_separator(t: &str) -> bool {
    ["--", "..", "==", "__"].iter().any(|s| t.starts_with(s))
}

fn enum_literals(t: &str) -> Vec<Member> {
    t.split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(Member::plain)
        .collect()
}

/// A member line, with PlantUML's `{field}`, `{method}` and `{classifier}`
/// markers handled and modifiers accepted before or after visibility.
fn member(t: &str) -> Option<Member> {
    let mut s = t.trim().trim_end_matches(';').trim().to_string();
    for (from, to) in [
        ("{field}", ""),
        ("{method}", ""),
        ("{classifier}", "{static}"),
    ] {
        s = s.replace(from, to);
    }
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut m = Member::parse_line(s);
    // `{static} +name`: visibility after a modifier.
    if m.vis.is_none() {
        if let Some(v) = m.text.chars().next().and_then(Visibility::from_char) {
            let again = Member::parse_line(&m.text);
            m = Member {
                vis: Some(v),
                is_static: m.is_static || again.is_static,
                is_abstract: m.is_abstract || again.is_abstract,
                text: again.text,
            };
        }
    }
    Some(m)
}

/// A declared name: quoted, or letters, digits, `_` and inner dots.
fn decl_name(c: &mut Cursor) -> Option<String> {
    c.skip_ws();
    if c.rest().starts_with('"') {
        return c.string().filter(|s| !s.trim().is_empty());
    }
    let r = c.rest();
    let chars: Vec<(usize, char)> = r.char_indices().collect();
    let mut end = 0;
    for (k, (i, ch)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|(_, c)| *c);
        let ok = ch.is_alphanumeric()
            || *ch == '_'
            || *ch == '$'
            || (*ch == '.' && k > 0 && next.is_some_and(|n| n.is_alphanumeric() || n == '_'));
        if !ok {
            break;
        }
        end = i + ch.len_utf8();
    }
    if end == 0 {
        return None;
    }
    c.pos += end;
    Some(r[..end].to_string())
}

/// A name used in a relation: like [`decl_name`], with a `::member`
/// suffix dropped.
fn ref_name(c: &mut Cursor) -> Option<String> {
    let name = decl_name(c)?;
    if c.rest().starts_with("::") {
        c.pos += 2;
        let _ = decl_name(c);
    }
    Some(name)
}

/// `A, B, C` after `extends` or `implements`.
fn name_list(c: &mut Cursor) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(n) = decl_name(c) {
        if c.rest().starts_with('<') && !c.rest().starts_with("<<") {
            balanced_angle(c);
        }
        out.push(n);
        if !c.eat(",") {
            break;
        }
    }
    out
}

/// Skips `<...>` with nesting; returns the skipped text.
fn balanced_angle(c: &mut Cursor) -> Option<String> {
    let r = c.rest();
    let mut depth = 0;
    for (i, ch) in r.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    c.pos += i + 1;
                    return Some(r[..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// `<<stereo>>`, with a spot such as `(S,#FF7700)` dropped.
fn stereo(c: &mut Cursor) -> Option<String> {
    let s = c.stereotype()?;
    let s = match (s.starts_with('('), s.find(')')) {
        (true, Some(i)) => s[i + 1..].trim().to_string(),
        _ => s,
    };
    Some(s)
}

/// Skips one whitespace-delimited token (a colour such as `#pink`).
fn skip_token(c: &mut Cursor) {
    c.skip_ws();
    let r = c.rest();
    let len = r
        .find(|ch: char| ch.is_whitespace() || ch == '{')
        .unwrap_or(r.len());
    c.pos += len;
}

/// Stereotypes and colours after a package or note name.
fn skip_decorations(c: &mut Cursor) {
    loop {
        c.skip_ws();
        if c.rest().starts_with("<<") {
            if stereo(c).is_none() {
                return;
            }
        } else if c.rest().starts_with('#') {
            skip_token(c);
        } else {
            return;
        }
    }
}

/// Reads a PlantUML arrow such as `-->`, `<|--`, `.left.>`,
/// `-[#red,dashed]->`, `*--`, `--o`.
fn arrow_token(c: &mut Cursor) -> Option<String> {
    c.skip_ws();
    let r = c.rest();
    let chars: Vec<(usize, char)> = r.char_indices().collect();
    let is_line = |ch: char| ch == '-' || ch == '.';
    let mut k = 0;
    let mut seen_line = false;
    while k < chars.len() {
        let ch = chars[k].1;
        if is_line(ch) || "<>|*#+^}{".contains(ch) {
            seen_line |= is_line(ch);
            k += 1;
        } else if ch == '[' {
            match chars[k..].iter().position(|(_, c)| *c == ']') {
                Some(j) => k += j + 1,
                None => return None,
            }
        } else if ch.is_alphabetic() {
            let start = k;
            while k < chars.len() && chars[k].1.is_alphabetic() {
                k += 1;
            }
            let word: String = chars[start..k].iter().map(|(_, c)| *c).collect();
            let before = start > 0 && is_line(chars[start - 1].1);
            let after = chars
                .get(k)
                .is_some_and(|(_, c)| is_line(*c) || "<>|*".contains(*c));
            let head = (word == "o" || word == "x") && (before || after);
            let direction = matches!(
                word.as_str(),
                "up" | "down" | "left" | "right" | "u" | "d" | "l" | "r" | "do" | "le" | "ri"
            ) && seen_line
                && after;
            if !(head || direction) {
                k = start;
                break;
            }
        } else {
            break;
        }
    }
    if !seen_line {
        return None;
    }
    let end = chars.get(k).map(|(i, _)| *i).unwrap_or(r.len());
    let token = &r[..end];
    c.pos += end;
    Some(token.to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum Head {
    None,
    Triangle,
    Arrow,
    Diamond,
    Hollow,
}

/// The relation kind of a PlantUML arrow, whether it was written reversed,
/// and an arrow end that has no meaning here.
fn arrow_kind(arrow: &str) -> (RelationKind, bool, Option<String>) {
    // Drop `[style]` but remember dashes asked for there.
    let mut plain = String::new();
    let mut dashed = false;
    let mut rest = arrow;
    while let Some(i) = rest.find('[') {
        plain.push_str(&rest[..i]);
        let j = rest[i..].find(']').map(|j| i + j).unwrap_or(rest.len() - 1);
        let style = &rest[i..=j];
        dashed |= style.contains("dashed") || style.contains("dotted");
        rest = &rest[j + 1..];
    }
    plain.push_str(rest);
    let is_line = |c: char| c == '-' || c == '.';
    let first = plain.find(is_line).unwrap_or(0);
    let last = plain.rfind(is_line).unwrap_or(0);
    let left = &plain[..first];
    let right = &plain[last + 1..];
    dashed |= plain[first..=last].contains('.');

    let mut unknown = None;
    let mut head = |s: &str| match s {
        "" => Head::None,
        "<|" | "|>" | "^" => Head::Triangle,
        "<" | ">" | "<<" | ">>" => Head::Arrow,
        "*" => Head::Diamond,
        "o" => Head::Hollow,
        other => {
            unknown = Some(other.to_string());
            Head::None
        }
    };
    let (l, r) = (head(left), head(right));
    use RelationKind::*;
    let hierarchy = if dashed { Realization } else { Generalization };
    let directed = if dashed { Dependency } else { Directed };
    let (kind, reversed) = match (l, r) {
        (Head::Triangle, _) => (hierarchy, true),
        (_, Head::Triangle) => (hierarchy, false),
        (Head::Diamond, _) => (Composition, false),
        (_, Head::Diamond) => (Composition, true),
        (Head::Hollow, _) => (Aggregation, false),
        (_, Head::Hollow) => (Aggregation, true),
        (Head::Arrow, Head::Arrow) => (Association, false),
        (Head::Arrow, _) => (directed, true),
        (_, Head::Arrow) => (directed, false),
        _ if dashed => (NoteLink, false),
        _ => (Association, false),
    };
    (kind, reversed, unknown)
}

/// Strips surrounding double quotes.
fn unquote(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

/// Is `s` a valid `.umpl` name, possibly dotted?
fn is_name_path(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_name)
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// A valid `.umpl` name made from any text: `Long name` → `Long_name`.
fn clean(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    match out.chars().next() {
        None => "Unnamed".to_string(),
        Some(c) if !c.is_alphabetic() => format!("_{out}"),
        _ => out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn names(doc: &Document) -> Vec<String> {
        doc.elements.keys().map(|id| doc.full_name(*id)).collect()
    }

    fn el<'a>(doc: &'a Document, name: &str) -> &'a Element {
        let id = doc
            .find(name)
            .unwrap_or_else(|| panic!("no {name} in {:?}", names(doc)));
        &doc.elements[&id]
    }

    fn rel(doc: &Document, from: &str, to: &str) -> Relation {
        let (f, t) = (doc.find(from).unwrap(), doc.find(to).unwrap());
        doc.relations
            .values()
            .find(|r| r.from.element == f && r.to.element == t)
            .unwrap_or_else(|| panic!("no relation {from} -> {to}"))
            .clone()
    }

    const SAMPLE: &str = r#"
some text before the diagram
@startuml
' A shop
title Shop model
skinparam classAttributeIconSize 0
skinparam class {
  BackgroundColor PaleGreen
  BorderColor DarkGreen
}
hide empty members
left to right direction
!theme plain

package com.shop <<Folder>> {
  abstract class Payment {
    {field} +amount : Money
    --
    {abstract} +pay() : Receipt
  }
  class CardPayment extends Payment
}

class "Purchase order" as PO #lightblue {
  +id : OrderId
  ..
  -lines : List<OrderLine>
  {static} +count() : int
}

interface Priced<T>
enum Status {
  NEW
  PAID, SHIPPED
}
entity Customer

PO : +total() : Money
PO "1" *-- "1..*" OrderLine : contains >
PO .up.|> Priced
PO -[#red,dashed]-> Payment : paid by
Customer "1" -- "*" PO
Customer --x Status
OrderLine::price -> Money

note "Created\nby import" as N1
N1 .. PO
note right of Customer : VIP\nonly
note top of Status
  Order states
  are final
end note
note on link : skipped
(PO, Customer) .. Status
@enduml
@startuml
class Second
@enduml
"#;

    #[test]
    fn imports_a_realistic_file() {
        let p = import(SAMPLE);
        let d = &p.doc;
        assert_eq!(d.title.as_deref(), Some("Shop model"));
        assert!(d.find("Second").is_none());

        let pay = el(d, "com.shop.Payment");
        assert_eq!(pay.kind, ElementKind::Abstract);
        assert_eq!(pay.members.len(), 2);
        assert_eq!(pay.members[0].to_line(), "+amount : Money");
        assert!(pay.members[1].is_abstract);
        assert_eq!(pay.members[1].vis, Some(Visibility::Public));
        assert_eq!(pay.comments, Vec::<String>::new());
        assert_eq!(d.packages.len(), 2);

        let po = el(d, "Purchase_order");
        assert_eq!(po.kind, ElementKind::Class);
        let lines: Vec<String> = po.members.iter().map(Member::to_line).collect();
        assert_eq!(
            lines,
            [
                "+id : OrderId",
                "-lines : List<OrderLine>",
                "+count() : int {static}",
                "+total() : Money"
            ]
        );
        assert_eq!(el(d, "Priced").kind, ElementKind::Interface);
        let status: Vec<&str> = el(d, "Status")
            .members
            .iter()
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(status, ["NEW", "PAID", "SHIPPED"]);
        assert_eq!(el(d, "Customer").stereotype.as_deref(), Some("entity"));

        assert_eq!(
            rel(d, "com.shop.CardPayment", "com.shop.Payment").kind,
            RelationKind::Generalization
        );
        let comp = rel(d, "Purchase_order", "OrderLine");
        assert_eq!(comp.kind, RelationKind::Composition);
        assert_eq!(comp.from.label.as_deref(), Some("1"));
        assert_eq!(comp.to.label.as_deref(), Some("1..*"));
        assert_eq!(comp.name.as_deref(), Some("contains"));
        assert_eq!(
            rel(d, "Purchase_order", "Priced").kind,
            RelationKind::Realization
        );
        let paid = rel(d, "Purchase_order", "com.shop.Payment");
        assert_eq!(paid.kind, RelationKind::Dependency);
        assert_eq!(paid.name.as_deref(), Some("paid by"));
        assert_eq!(
            rel(d, "Customer", "Purchase_order").kind,
            RelationKind::Association
        );
        assert_eq!(rel(d, "Customer", "Status").kind, RelationKind::Association);
        assert_eq!(rel(d, "OrderLine", "Money").kind, RelationKind::Directed);

        let n1 = el(d, "N1");
        assert_eq!(n1.kind, ElementKind::Note);
        assert_eq!(n1.members.len(), 2);
        assert_eq!(rel(d, "N1", "Purchase_order").kind, RelationKind::NoteLink);
        let vip = el(d, "Note");
        assert_eq!(vip.members[1].text, "only");
        rel(d, "Note", "Customer");
        let states = el(d, "Note2");
        assert_eq!(states.members[0].text, "Order states");
        rel(d, "Note2", "Status");

        let messages: Vec<(usize, &str)> = p
            .problems
            .iter()
            .map(|p| (p.line, p.message.as_str()))
            .collect();
        let at = |line: usize| {
            messages
                .iter()
                .filter(|(l, _)| *l == line)
                .map(|(_, m)| *m)
                .collect::<Vec<_>>()
                .join(" | ")
        };
        assert!(at(6).contains("skinparam"), "{messages:?}");
        assert!(at(7).contains("block skipped"), "{messages:?}");
        assert!(at(11).contains("hide"), "{messages:?}");
        assert!(at(12).contains("direction"), "{messages:?}");
        assert!(at(13).contains("!theme"), "{messages:?}");
        assert!(at(31).contains("<T>"), "{messages:?}");
        assert!(at(43).contains("`x`"), "{messages:?}");
        assert!(at(53).contains("links"), "{messages:?}");
        assert!(at(54).contains("association classes"), "{messages:?}");
        assert!(at(56).contains("first diagram"), "{messages:?}");
        // Skipped `skinparam` block lines are not reported one by one.
        assert!(at(8).is_empty() && at(9).is_empty());
        assert_eq!(messages.len(), 10, "{messages:?}");
    }

    #[test]
    fn arrows() {
        use RelationKind::*;
        let cases = [
            ("-->", Directed, false),
            ("->", Directed, false),
            ("<--", Directed, true),
            ("-up->", Directed, false),
            ("-l->", Directed, false),
            ("..>", Dependency, false),
            ("-[dashed]->", Dependency, false),
            ("<|--", Generalization, true),
            ("--|>", Generalization, false),
            ("<|..", Realization, true),
            (".left.|>", Realization, false),
            ("*--", Composition, false),
            ("--*", Composition, true),
            ("o--", Aggregation, false),
            ("--o", Aggregation, true),
            ("*-->", Composition, false),
            ("<-->", Association, false),
            ("--", Association, false),
            ("-", Association, false),
            ("..", NoteLink, false),
            (".", NoteLink, false),
        ];
        for (arrow, kind, reversed) in cases {
            let mut c = Cursor::new(arrow);
            let token = arrow_token(&mut c).unwrap_or_else(|| panic!("no arrow in {arrow}"));
            assert_eq!(token, arrow);
            let (k, r, unknown) = arrow_kind(&token);
            assert_eq!((k, r, unknown), (kind, reversed, None), "{arrow}");
        }
    }

    #[test]
    fn arrows_without_spaces_and_heads_next_to_names() {
        let d = import("A-->B\nC --o D\nE o-- F\nG<|-H\n").doc;
        assert_eq!(rel(&d, "A", "B").kind, RelationKind::Directed);
        assert_eq!(rel(&d, "D", "C").kind, RelationKind::Aggregation);
        assert_eq!(rel(&d, "E", "F").kind, RelationKind::Aggregation);
        assert_eq!(rel(&d, "H", "G").kind, RelationKind::Generalization);
        assert_eq!(d.elements.len(), 8);
    }

    #[test]
    fn names_resolve_into_packages_and_aliases() {
        let src = "package billing {\n  class Invoice\n  Invoice --> Line\n}\nOrder --> Invoice\nclass L as \"Long name\"\n\"Long name\" --> L\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![]);
        // Line is created where it is first used, inside billing.
        rel(d, "billing.Invoice", "billing.Line");
        rel(d, "Order", "billing.Invoice");
        let l = d.find("Long_name").unwrap();
        let self_rel = d.relations.values().find(|r| r.from.element == l).unwrap();
        assert_eq!(self_rel.to.element, l);
        assert_eq!(d.elements.len(), 4);
    }

    #[test]
    fn reopened_and_dotted_packages_merge() {
        let d = import("package a.b {\n  class X\n}\npackage a {\n  package b {\n    class Y\n  }\n}\nclass a.b.Z\n").doc;
        assert_eq!(d.packages.len(), 2);
        for n in ["a.b.X", "a.b.Y", "a.b.Z"] {
            el(&d, n);
        }
    }

    #[test]
    fn declaration_after_use_updates_the_element() {
        let d = import("A --> B\ninterface B <<port>> {\n  +run()\n}\n").doc;
        assert_eq!(d.elements.len(), 2);
        let b = el(&d, "B");
        assert_eq!(b.kind, ElementKind::Interface);
        assert_eq!(b.stereotype.as_deref(), Some("port"));
        assert_eq!(b.members.len(), 1);
    }

    #[test]
    fn brace_on_next_line_and_inline_bodies() {
        let d = import("class A\n{\n  +x: int\n}\nenum E { R, G }\nclass B {}\n").doc;
        assert_eq!(el(&d, "A").members.len(), 1);
        assert_eq!(el(&d, "E").members.len(), 2);
        assert!(el(&d, "B").members.is_empty());
    }

    #[test]
    fn comments_are_kept() {
        let d = import("' one\n/' two\nthree '/\nclass A\n/' four '/\n").doc;
        assert_eq!(el(&d, "A").comments, ["one", "two", "three"]);
        assert_eq!(d.trailing_comments, ["four"]);
    }

    #[test]
    fn unknown_lines_are_raw_and_unclosed_blocks_reported() {
        let p = import("class A {\n  +x: int\n}\n??? what\npackage p {\n");
        assert!(p.doc.root.contains(&Decl::Raw("??? what".into())));
        assert_eq!(p.problems.len(), 2);
        assert_eq!(p.problems[0].line, 4);
        assert!(p.problems[1].message.contains("missing"));
    }

    #[test]
    fn import_of_garbage_never_panics() {
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        let alphabet: Vec<char> = "ab{}<>|*o.-\"' :#()[]@\n,;!".chars().collect();
        for _ in 0..300 {
            let len = (rng % 80) as usize;
            let s: String = (0..len)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    alphabet[(rng % alphabet.len() as u64) as usize]
                })
                .collect();
            let p = import(&s);
            // The result is always a document the printer can write.
            let _ = crate::print(&p.doc);
        }
    }

    #[test]
    fn export_matches_plantuml_style() {
        let src = "diagram \"Shop\"\n\npackage billing {\n  abstract class Payment {\n    +pay(): Receipt {abstract}\n  }\n  class Item\n}\n\nclass Item\n\nnote N1 \"two\\nlines\"\n\n# owns\nOrder \"1\" *-- \"1..*\" Item : contains\nbilling.Item --|> billing.Payment\nN1 .. Order\nweird line\n";
        let out = export(&parse(src).doc);
        let expected = "@startuml\ntitle Shop\n\npackage billing {\n  abstract class Payment {\n    +pay(): Receipt {abstract}\n  }\n\n  class Item\n}\n\nclass Item\n\nnote \"two\\nlines\" as N1\n\n' owns\nOrder \"1\" *-- \"1..*\" Item : contains\nbilling.Item --|> Payment\nN1 .. Order\n\n' weird line\n\nclass Order\n@enduml\n";
        assert_eq!(out, expected);
    }
}
