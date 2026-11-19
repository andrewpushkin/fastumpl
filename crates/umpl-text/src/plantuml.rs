//! PlantUML class diagrams: import into a [`Document`] and export from one.
//!
//! Import never fails. Anything skipped or changed is reported as a
//! [`Problem`] at its line in the PlantUML file; lines that are not
//! understood are kept as raw lines, as the `.umpl` parser does. See
//! `docs/plans/2026-11-17-plantuml-mermaid.md` for the mapping.

use std::collections::HashMap;
use std::path::Path;

use umpl_core::*;

use crate::parser::{Parsed, Problem};
use crate::scan::{clean, is_name_path, Cursor};

mod kind;
mod preprocess;

pub use kind::{diagram_kind, DiagramKind};
pub use preprocess::Reader;

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
/// `!include`s are not followed; see [`import_path`].
pub fn import(src: &str) -> Parsed {
    import_with(src, None, &|_| None)
}

/// Reads a PlantUML file, following `!include`s of files next to it.
pub fn import_path(path: &Path) -> std::io::Result<Parsed> {
    let bytes = std::fs::read(path)?;
    let src = String::from_utf8_lossy(&bytes);
    let read = |p: &Path| {
        std::fs::read(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    Ok(import_with(&src, path.parent(), &read))
}

/// The message for a diagram fastumpl does not read.
pub fn unreadable_message(kind: DiagramKind) -> String {
    let name = kind.name();
    let article = if name.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!(
        "this is {article} {name}; fastumpl reads class diagrams, and component, deployment \
         and use case diagrams as boxes"
    )
}

/// Reads a PlantUML class diagram; `!include`s are looked up in `dir`
/// through `read`. Only the first diagram in the file is read.
pub fn import_with(src: &str, dir: Option<&Path>, read: Reader) -> Parsed {
    let lines: Vec<&str> = src.lines().collect();
    let is_start = |l: &str| l.trim_start().starts_with("@start");
    let is_end = |l: &str| l.trim_start().starts_with("@end");
    let start = lines.iter().position(|l| is_start(l));
    let begin = start.map_or(0, |i| i + 1);
    let end = lines[begin..]
        .iter()
        .position(|l| is_end(l))
        .map_or(lines.len(), |j| begin + j);
    let mut im = Importer::default();
    if let Some(j) = lines[end.min(lines.len())..]
        .iter()
        .position(|l| is_start(l))
    {
        im.problem(
            end + 1 + j,
            "only the first diagram in the file is imported",
        );
    }
    let unreadable = |kind: DiagramKind, line: usize, mut problems: Vec<Problem>| {
        problems.retain(|p| p.message.contains("!endif"));
        problems.insert(
            0,
            Problem {
                line,
                col: 1,
                message: unreadable_message(kind),
            },
        );
        Parsed {
            doc: Document::new(),
            problems,
            has_layout: false,
            spans: Vec::new(),
        }
    };
    let first_line = start.map_or(1, |i| i + 1);
    if let Some(i) = start {
        let kind = kind::other(kind::start_tag(lines[i].trim()));
        if !kind.readable() {
            return unreadable(kind, first_line, Vec::new());
        }
    }
    let body: Vec<(usize, &str)> = (begin..end).map(|i| (i + 1, lines[i])).collect();
    let (pre, problems) = preprocess::preprocess(&body, dir, read);
    let texts: Vec<&str> = pre.iter().map(|l| l.text.as_str()).collect();
    let kind = kind::kind_of_lines(&texts);
    if !kind.readable() {
        return unreadable(kind, first_line, problems);
    }
    im.problems.extend(problems);
    let mut k = 0;
    while k < pre.len() {
        let l = &pre[k];
        let mut text = l.text.trim().to_string();
        // A quoted name that goes on over the next lines.
        if opens_multiline_string(&text) {
            let mut j = k + 1;
            while j < pre.len() && j - k < 50 {
                text.push('\n');
                text.push_str(pre[j].text.trim());
                j += 1;
                if text.matches('"').count() % 2 == 0 {
                    break;
                }
            }
            if text.matches('"').count() % 2 == 0 {
                k = j - 1;
            } else {
                text = l.text.trim().to_string();
            }
        }
        im.origin = l.from.clone();
        im.last_line = l.line;
        im.line(l.line, &text);
        k += 1;
    }
    im.origin = None;
    im.finish()
}

/// PlantUML keywords are case-insensitive: `Title`, `Note right of`,
/// `Class`. The first word, if it is a keyword, is put in lower case.
fn lowercase_keyword(t: &str) -> std::borrow::Cow<'_, str> {
    let end = t
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(t.len());
    let word = &t[..end];
    // `Person(…)` is a call, not the `person` keyword.
    if word.is_empty() || word.chars().all(|c| c.is_ascii_lowercase()) || t[end..].starts_with('(')
    {
        return t.into();
    }
    let lower = word.to_ascii_lowercase();
    let keyword = [
        "title",
        "header",
        "footer",
        "legend",
        "note",
        "end",
        "class",
        "abstract",
        "interface",
        "enum",
        "package",
        "namespace",
        "skinparam",
        "hide",
        "show",
        "together",
        "caption",
        "scale",
    ]
    .contains(&lower.as_str())
        || CLASS_LIKE.contains(&lower.as_str())
        || CONTAINERS.contains(&lower.as_str())
        || BOXES.contains(&lower.as_str());
    if keyword {
        format!("{lower}{}", &t[end..]).into()
    } else {
        t.into()
    }
}

/// A declaration whose quoted name is not closed on its line:
/// `usecase U as "Line one`.
fn opens_multiline_string(t: &str) -> bool {
    let first = t
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let declares = CLASS_LIKE.contains(&first.as_str())
        || CONTAINERS.contains(&first.as_str())
        || BOXES.contains(&first.as_str())
        || ["class", "abstract", "interface", "enum"].contains(&first.as_str());
    declares && t.matches('"').count() % 2 == 1
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

/// Keywords of component, deployment and use case diagrams. Followed by
/// `{` they hold other elements and become packages; otherwise they are
/// boxes: a class with the keyword as stereotype.
const CONTAINERS: [&str; 15] = [
    "node",
    "rectangle",
    "folder",
    "frame",
    "cloud",
    "database",
    "component",
    "card",
    "artifact",
    "storage",
    "queue",
    "stack",
    "hexagon",
    "file",
    "agent",
];
const BOXES: [&str; 14] = [
    "actor",
    "usecase",
    "boundary",
    "control",
    "collections",
    "person",
    "label",
    "circle",
    "port",
    "portin",
    "portout",
    "action",
    "process",
    "participant",
];

/// Calls of C4 procedures that only arrange or style the drawing.
fn is_layout_call(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("LAY_")
        || upper.starts_with("LAYOUT_")
        || upper.starts_with("SHOW_")
        || upper.starts_with("HIDE_")
        || upper.starts_with("UPDATE")
        || upper.starts_with("ADD")
        || upper.starts_with("SET")
        || upper.starts_with("WITHOUT")
        || upper == "INCREMENT"
}

/// Commands that only change how PlantUML draws; skipped with a problem.
const STYLING: [&str; 14] = [
    "skinparam",
    "skin",
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
    /// For a line from an included file: that file and the line there.
    origin: Option<(String, usize)>,
    /// Stereotypes of elements first named in a short form (`[Comp]`,
    /// `(Use case)`, `:Actor:`) in a relation, for when they are created.
    implicit: HashMap<String, &'static str>,
    /// Library procedures read as boxes or lines, reported once each.
    calls_seen: std::collections::HashSet<String>,
    /// A package declared on the previous line without `{`, which a `{`
    /// on the next line opens.
    last_package: Option<PackageId>,
}

impl Importer {
    fn problem(&mut self, line: usize, message: impl Into<String>) {
        let message = match &self.origin {
            Some((file, l)) => format!("in `{file}` line {l}: {}", message.into()),
            None => message.into(),
        };
        self.problems.push(Problem {
            line,
            col: 1,
            message,
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
        // Links (`[[url]]`, `[[[url]]]` on members) only make the drawing
        // clickable; their text may hold braces that would unbalance bodies.
        let linkless = strip_links(t);
        let t = match self.mode {
            None | Some(Mode::Normal) | Some(Mode::Body { .. }) => linkless.as_ref(),
            _ => t,
        };
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
        let last_package = self.last_package.take();
        if t.is_empty() {
            self.last_decl = last_decl;
            self.last_package = last_package;
            return;
        }
        if t == "{" {
            if let Some((id, is_enum)) = last_decl {
                self.mode = Some(Mode::Body { id, is_enum });
                return;
            }
            if let Some(p) = last_package {
                self.stack.push(Some(p));
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
        if self.statement(n, t) {
            return;
        }
        // Keywords may be written in any case (`Title`, `Note right of`);
        // a capitalised word is first read as a name (`Card --> Account`).
        let lower = lowercase_keyword(t);
        if lower != t && self.statement(n, &lower) {
            return;
        }
        self.raw(n, t, "not understood; kept as written");
    }

    fn statement(&mut self, n: usize, t: &str) -> bool {
        self.try_title(t)
            || self.try_skipped(n, t)
            || self.try_package(n, t)
            || self.try_note(n, t)
            || self.try_element(n, t)
            || self.try_short_decl(n, t)
            || self.try_relation(n, t)
            || self.try_call(n, t)
            || self.try_member(t)
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
        // `right footer`, `center header` and the like.
        let aligned = ["left", "right", "center"].contains(&first)
            && matches!(
                after.split_whitespace().next(),
                Some("header" | "footer" | "legend")
            );
        if aligned {
            let what = after.split_whitespace().next().unwrap_or("");
            let w: &'static str = match what {
                "header" => "header",
                "footer" => "footer",
                _ => "legend",
            };
            let block = after.trim() == w;
            return skip(self, w, block.then_some(SkipEnd::End(w)));
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
        let container = CONTAINERS.iter().any(|w| c.eat_word(w));
        if !(container || c.eat_word("package") || c.eat_word("namespace")) {
            return false;
        }
        // `package [src] as C_0001 {` (clang-uml).
        let Some(name) = short_form(&mut c)
            .filter(|(_, s)| *s == "component")
            .map(|(n, _)| n)
            .or_else(|| decl_name(&mut c))
        else {
            return false;
        };
        if c.eat_word("as") && decl_name(&mut c).is_none() {
            return false;
        }
        skip_decorations(&mut c);
        if container && !c.rest().starts_with('{') {
            // `node Web` without a body is a box.
            return false;
        }
        let open = c.eat("{") && !c.eat("}");
        if !c.at_end() {
            return false;
        }
        let mut pkg = self.container();
        let parts: Vec<String> = name.replace("::", ".").split('.').map(clean).collect();
        for part in &parts {
            pkg = Some(self.ensure_package(n, pkg, part));
        }
        if open {
            self.stack.push(pkg);
        } else {
            // `{` may come on the next line.
            self.last_package = pkg;
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
        // `+class Inner {`: a visibility before the keyword.
        let t = match t.strip_prefix(['+', '-', '#', '~']) {
            Some(rest) if rest.starts_with(|ch: char| ch.is_ascii_lowercase()) => rest,
            _ => t,
        };
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
        } else if let Some(w) = CLASS_LIKE
            .iter()
            .chain(&CONTAINERS)
            .chain(&BOXES)
            .find(|w| c.eat_word(w))
        {
            (ElementKind::Class, Some(w.to_string()))
        } else {
            return false;
        };
        c.skip_ws();
        let name_quoted = c.rest().starts_with('"') || short_form(&mut c.clone()).is_some();
        let Some(name) = short_form(&mut c)
            .map(|(n, _)| n)
            .or_else(|| decl_name(&mut c))
        else {
            return false;
        };
        let mut dropped_generics = None;
        // `class A<T>` or `class A <T>`.
        let mut probe = c.clone();
        probe.skip_ws();
        if probe.rest().starts_with('<') && !probe.rest().starts_with("<<") {
            c = probe;
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
            } else if alias.is_none() && c.eat_word("as") {
                // `component M <<FXML>> as View`: the alias after the
                // stereotype.
                c.skip_ws();
                alias_quoted = c.rest().starts_with('"');
                alias = decl_name(&mut c);
                if alias.is_none() {
                    return false;
                }
            } else if r.starts_with('<') {
                // Type parameters after the alias: `as a.B<T> {`.
                if balanced_angle(&mut c).is_none() {
                    return false;
                }
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
            // `ns::Class` is `Class` in package `ns`.
            _ if !name_quoted => name.replace("::", "."),
            _ => name.clone(),
        };
        let path = if is_name_path(&display) {
            display
        } else {
            clean(&display)
        };
        // A name over several lines: the first is the name, the others
        // (separators left out) are shown as members.
        let (path, more) = match display_lines(&name, alias.as_deref(), alias_quoted, name_quoted) {
            Some((first, rest)) => (first, rest),
            None => (path, Vec::new()),
        };
        // `class C_01 { … }` after `class "Long name" as C_01`: the same
        // element, opened again by its alias.
        let known = (!name_quoted && alias.is_none())
            .then(|| self.aliases.get(&name).copied())
            .flatten();
        let id = match known {
            Some(id) => {
                let el = self.doc.elements.get_mut(&id).unwrap();
                el.kind = kind;
                if let Some(s) = stereotype.or(keyword_stereo) {
                    el.stereotype = Some(s);
                }
                el.comments.append(&mut self.comments);
                id
            }
            None => self.declare(n, kind, &path, stereotype.or(keyword_stereo)),
        };
        for a in [Some(name.clone()), alias].into_iter().flatten() {
            if a != path {
                self.aliases.insert(a, id);
            }
        }
        if !more.is_empty() {
            let el = self.doc.elements.get_mut(&id).unwrap();
            el.members.extend(more.into_iter().map(Member::plain));
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
                Some(t) => {
                    // A note on a method: `of A::run()`.
                    if c.rest().starts_with('(') {
                        match matching_paren(c.rest()) {
                            Some(i) => c.pos += i + 1,
                            None => return false,
                        }
                    }
                    Some(t)
                }
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
        if t.starts_with('(') && t[..t.find(')').unwrap_or(t.len())].contains(',') {
            self.problem(n, "association classes are not supported; line skipped");
            return true;
        }
        let mut c = Cursor::new(t);
        let Some(a) = self.endpoint(&mut c) else {
            return false;
        };
        let la = c.string();
        let Some(arrow) = arrow_token(&mut c) else {
            return false;
        };
        let q = c.string();
        let (lb, b) = match self.endpoint(&mut c) {
            Some(b) => (q, b),
            None => match q {
                Some(b) => (None, b),
                None => return false,
            },
        };
        // A line style after the target: `#green;text:green`, `#line.dashed`.
        c.skip_ws();
        if c.rest().starts_with('#') {
            skip_token(&mut c);
        }
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
            // Once per kind of end: a diagram often uses one throughout.
            if self.calls_seen.insert(format!("head {head}")) {
                self.problem(
                    n,
                    format!(
                        "arrow end `{head}` in `{arrow}` is not supported; drawn as a plain line \
                         (here and wherever else it is used)"
                    ),
                );
            }
        }
        // Crow's feet (`||--o{`) give multiplicities, unless written ones do.
        let (fa, fb) = crows_feet(&arrow);
        let la = la.or(fa.map(str::to_string));
        let lb = lb.or(fb.map(str::to_string));
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

    /// One end of a relation: a name, or a short form such as `[Comp]`,
    /// whose stereotype is kept for when the element is created.
    fn endpoint(&mut self, c: &mut Cursor) -> Option<String> {
        if let Some((name, stereo)) = short_form(c) {
            self.implicit.entry(clean(&name)).or_insert(stereo);
            return Some(name);
        }
        ref_name(c)
    }

    /// A short form on its own: `[Comp] as C`, `(Login) as UC`, `:User:`.
    fn try_short_decl(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let Some((name, stereo)) = short_form(&mut c) else {
            return false;
        };
        let alias = if c.eat_word("as") {
            match short_form(&mut c)
                .map(|s| s.0)
                .or_else(|| decl_name(&mut c))
            {
                Some(a) => Some(a),
                None => return false,
            }
        } else {
            None
        };
        let mut stereotype = Some(stereo.to_string());
        loop {
            c.skip_ws();
            if c.rest().starts_with("<<") {
                match stereo_of(&mut c) {
                    Some(s) if !s.is_empty() => stereotype = Some(s),
                    Some(_) => {}
                    None => return false,
                }
            } else if c.rest().starts_with('#') {
                skip_token(&mut c);
            } else {
                break;
            }
        }
        if !c.at_end() {
            return false;
        }
        let path = clean(&name);
        let id = self.declare(n, ElementKind::Class, &path, stereotype);
        for a in [Some(name), alias].into_iter().flatten() {
            if a != path {
                self.aliases.insert(a, id);
            }
        }
        self.last_element = Some(id);
        true
    }

    /// A call of a procedure from a library that was not read, such as C4's
    /// `Person(user, "User")`, `Container(api, "API", "Rust")` and
    /// `Rel(user, api, "Uses")`: elements become boxes with the procedure as
    /// stereotype, `Rel…` calls lines, and calls ending in `{` packages.
    fn try_call(&mut self, n: usize, t: &str) -> bool {
        let t = t.strip_prefix('$').unwrap_or(t);
        // `LAYOUT_LEFT_RIGHT` and the like, without parentheses.
        if t.chars().all(|c| c.is_ascii_uppercase() || c == '_') && is_layout_call(t) {
            if self.calls_seen.insert(t.to_string()) {
                self.problem(n, format!("`{t}` arranges or styles the drawing; skipped"));
            }
            return true;
        }
        let name_end = t
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(t.len());
        let (name, rest) = t.split_at(name_end);
        if name.is_empty() || !name.starts_with(char::is_alphabetic) || !rest.starts_with('(') {
            return false;
        }
        let Some(close) = matching_paren(rest) else {
            return false;
        };
        let tail = rest[close + 1..].trim();
        let open = tail == "{";
        if !(tail.is_empty() || open) {
            return false;
        }
        let args: Vec<String> = split_call_args(&rest[1..close]);
        let positional: Vec<&str> = args
            .iter()
            .map(String::as_str)
            .filter(|a| !(a.starts_with('$') && a.contains('=')))
            .collect();
        if is_layout_call(name) {
            if self.calls_seen.insert(name.to_string()) {
                self.problem(
                    n,
                    format!("`{name}(…)` arranges or styles the drawing; skipped"),
                );
            }
            return true;
        }
        if self.calls_seen.insert(name.to_string()) {
            let what = if name.contains("Rel") {
                "a line"
            } else if open {
                "a package"
            } else {
                "a box"
            };
            self.problem(
                n,
                format!("`{name}(…)` comes from a library that was not read; drawn as {what}"),
            );
        }
        let unq = |s: &str| unquote(s.trim()).replace("\\n", " ");
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("rel") || lower.starts_with("birel") {
            let (Some(a), Some(b)) = (positional.first(), positional.get(1)) else {
                return true;
            };
            let (mut from, mut to) = (unq(a), unq(b));
            if lower.contains("back") {
                std::mem::swap(&mut from, &mut to);
            }
            let kind = if lower.starts_with("birel") {
                RelationKind::Association
            } else {
                RelationKind::Directed
            };
            let container = self.container();
            let id = self.doc.new_relation_id();
            self.push_decl_in(container, Decl::Relation(id));
            self.pending.push(Pending::Relation {
                id,
                container,
                kind,
                from: (from, None),
                to: (to, None),
                name: positional.get(2).map(|l| unq(l)).filter(|l| !l.is_empty()),
                comments: std::mem::take(&mut self.comments),
            });
            return true;
        }
        let Some(alias) = positional.first().map(|a| unq(a)) else {
            return false;
        };
        let label = positional
            .get(1)
            .map(|l| unq(l))
            .filter(|l| !l.trim().is_empty());
        let display = label.clone().unwrap_or_else(|| alias.clone());
        let path = clean(&display);
        if open {
            let container = self.container();
            let pkg = self.ensure_package(n, container, &path);
            self.stack.push(Some(pkg));
            return true;
        }
        let id = self.declare(n, ElementKind::Class, &path, Some(name.to_string()));
        for a in [alias, display] {
            if a != path {
                self.aliases.insert(a, id);
            }
        }
        let el = self.doc.elements.get_mut(&id).unwrap();
        // Technology and description: prose, wrapped as C4 wraps it.
        for extra in positional.iter().skip(2) {
            let text = strip_format(&unquote(extra.trim())).into_owned();
            for line in text.split("\\n").flat_map(|l| wrap(l.trim(), WRAP)) {
                el.members.push(Member::plain(line));
            }
        }
        self.last_element = Some(id);
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
            spans: Vec::new(),
        }
    }

    /// Finds the element a PlantUML name refers to: an alias, a name in the
    /// enclosing packages, a full dotted name, or a plain name that exactly
    /// one element has. Unknown names create a class where they are used.
    fn resolve(&mut self, container: Option<PackageId>, r: &str) -> ElementId {
        if let Some(id) = self.aliases.get(r) {
            return *id;
        }
        if r.contains("::") {
            // `ns::Class` in a namespace, else `Class::member`.
            let dotted = r.replace("::", ".");
            if let Some(id) = self.lookup(container, &dotted) {
                return id;
            }
            if let Some((owner, _member)) = r.rsplit_once("::") {
                if let Some(id) = self.aliases.get(owner).copied() {
                    return id;
                }
                if let Some(id) = self.lookup(container, &owner.replace("::", ".")) {
                    return id;
                }
            }
            return self.resolve(container, &dotted);
        }
        let path = if is_name_path(r) {
            r.to_string()
        } else {
            clean(r)
        };
        if let Some(id) = self.lookup(container, &path) {
            return id;
        }
        let saved = std::mem::replace(&mut self.stack, vec![container]);
        let stereo = self.implicit.get(&path).map(|s| s.to_string());
        let id = self.declare(self.last_line, ElementKind::Class, &path, stereo);
        self.stack = saved;
        id
    }

    /// The element `path` names, seen from `container`: in it or an
    /// enclosing package, or anywhere if only one element has that name.
    fn lookup(&self, container: Option<PackageId>, path: &str) -> Option<ElementId> {
        let path = if is_name_path(path) {
            path.to_string()
        } else {
            clean(path)
        };
        let mut scope = container;
        loop {
            let full = match scope {
                Some(p) => format!("{}.{path}", self.doc.full_package_name(p)),
                None => path.clone(),
            };
            if let Some(id) = self.doc.find(&full) {
                return Some(id);
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
                return Some(only);
            }
        }
        None
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

/// Tags of PlantUML's formatting (creole and HTML-like): `<b>`, `</b>`,
/// `<color:red>`, `<size:12>`, `<font color=x>`. Only these names, so
/// `List<String>` keeps its type parameter.
const FORMAT_TAGS: [&str; 16] = [
    "b", "i", "u", "s", "w", "strike", "color", "back", "size", "font", "sub", "sup", "plain",
    "del", "img", "code",
];

/// `t` without formatting tags.
fn strip_format(t: &str) -> std::borrow::Cow<'_, str> {
    if !t.contains('<') {
        return t.into();
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let tag = after.strip_prefix('/').unwrap_or(after);
        let name_end = tag
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(tag.len());
        let name = tag[..name_end].to_ascii_lowercase();
        let known = FORMAT_TAGS.contains(&name.as_str())
            && tag[name_end..].starts_with(['>', ':', ' ', '=']);
        match (known, after.find('>')) {
            (true, Some(close)) => rest = &after[close + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.into()
}

/// A member line, with PlantUML's `{field}`, `{method}` and `{classifier}`
/// markers handled and modifiers accepted before or after visibility.
fn member(t: &str) -> Option<Member> {
    let t = strip_format(t);
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
        // `a::b` (C++ and other namespaces); the second `:` is taken with
        // the first.
        if *ch == ':' && k > 0 && next == Some(':') {
            let after = chars.get(k + 2).map(|(_, c)| *c);
            if after.is_some_and(|n| n.is_alphanumeric() || n == '_') {
                continue;
            }
            break;
        }
        if *ch == ':' && k > 0 && chars[k - 1].1 == ':' {
            end = i + 1;
            continue;
        }
        // `.a.b.C` (absolute) and `a.b.C`; `libs/x` as some aliases are.
        let joins = |n: Option<char>| n.is_some_and(|n| n.is_alphanumeric() || n == '_');
        let ok = ch.is_alphanumeric()
            || *ch == '_'
            || *ch == '$'
            || (*ch == '.' && joins(next))
            || (*ch == '/' && k > 0 && joins(next));
        if !ok {
            break;
        }
        end = i + ch.len_utf8();
    }
    if end == 0 {
        return None;
    }
    c.pos += end;
    // `.a.b.C` names `a.b.C` from the top.
    Some(r[..end].trim_start_matches('.').to_string()).filter(|s| !s.is_empty())
}

/// A name used in a relation: like [`decl_name`]. `A::b` is kept whole:
/// it names `b` in namespace `A`, or member `b` of class `A`, which only
/// resolving can tell.
fn ref_name(c: &mut Cursor) -> Option<String> {
    decl_name(c)
}

/// Removes `[[url]]` and `[[[url]]]` links outside quotes.
fn strip_links(t: &str) -> std::borrow::Cow<'_, str> {
    if !t.contains("[[") {
        return t.into();
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    let mut quote = false;
    while !rest.is_empty() {
        if !quote && rest.starts_with("[[") {
            let triple = rest.starts_with("[[[");
            let close = if triple { "]]]" } else { "]]" };
            if let Some(end) = rest.find(close) {
                rest = &rest[end + close.len()..];
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        if ch == '"' {
            quote = !quote;
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.into()
}

/// For a quoted display name over several lines (`usecase U as "Take
/// role\n--\nExtension points"`): the element name from its first line and
/// the other lines, without separators.
fn display_lines(
    name: &str,
    alias: Option<&str>,
    alias_quoted: bool,
    name_quoted: bool,
) -> Option<(String, Vec<String>)> {
    let shown = match alias {
        Some(a) if alias_quoted && !name_quoted => a,
        _ => name,
    };
    if !shown.contains('\n') {
        return None;
    }
    let mut lines = shown.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next()?;
    let rest = lines
        .filter(|l| !is_separator(l))
        .map(str::to_string)
        .collect();
    Some((clean(first), rest))
}

/// A short form: `[Component]`, `(Use case)` or `:Actor:`, with the text
/// inside and the stereotype it stands for.
fn short_form(c: &mut Cursor) -> Option<(String, &'static str)> {
    c.skip_ws();
    let r = c.rest();
    let (open, close, stereo) = match r.chars().next()? {
        '[' => ('[', ']', "component"),
        '(' => ('(', ')', "usecase"),
        ':' => (':', ':', "actor"),
        _ => return None,
    };
    let inner_end = r[1..].find(close)? + 1;
    let inner = r[1..inner_end].trim();
    // `(*)` is an activity start; `(A, B)` an association class.
    if inner.is_empty() || inner == "*" || (open == '(' && inner.contains(',')) {
        return None;
    }
    if open == ':' && inner.contains(['"', '<']) {
        return None;
    }
    c.pos += inner_end + 1;
    Some((inner.replace("\\n", " "), stereo))
}

fn stereo_of(c: &mut Cursor) -> Option<String> {
    stereo(c)
}

/// The index of the `)` closing the `(` at the start of `s`.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut quote = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Width, in characters, that descriptions from library calls wrap at.
const WRAP: usize = 60;

/// `text` broken into lines of at most `width` characters at spaces (a
/// longer word stays whole).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Arguments of a call, split at top-level commas.
fn split_call_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut cur) = (0, false, String::new());
    for ch in s.chars() {
        match ch {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                out.push(std::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
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
            // `-[#red,dashed]->`: a style inside the arrow, not the
            // `[Component]` after it.
            let inside = k > 0 && is_line(chars[k - 1].1);
            match chars[k..].iter().position(|(_, c)| *c == ']') {
                Some(j) if inside => k += j + 1,
                _ => break,
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

/// The multiplicity a crow's-foot end of an entity-relationship line
/// stands for: `||` one, `|o` zero or one, `}o` zero or more, `}|` one or
/// more (written mirrored on the right end).
fn crow_foot(end: &str, left: bool) -> Option<&'static str> {
    let end: String = if left {
        end.to_string()
    } else {
        end.chars()
            .rev()
            .map(|c| match c {
                '{' => '}',
                c => c,
            })
            .collect()
    };
    match end.as_str() {
        "||" => Some("1"),
        "|o" => Some("0..1"),
        "}o" => Some("0..*"),
        "}|" => Some("1..*"),
        _ => None,
    }
}

/// The crow's-foot multiplicities at the left and right end of `arrow`.
fn crows_feet(arrow: &str) -> (Option<&'static str>, Option<&'static str>) {
    let is_line = |c: char| c == '-' || c == '.';
    let plain: String = {
        let mut out = String::new();
        let mut depth = 0;
        for c in arrow.chars() {
            match c {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        out
    };
    let (Some(first), Some(last)) = (plain.find(is_line), plain.rfind(is_line)) else {
        return (None, None);
    };
    (
        crow_foot(&plain[..first], true),
        crow_foot(&plain[last + 1..], false),
    )
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
    let mut feet = false;
    let mut head = |s: &str, is_left: bool| match s {
        "" => Head::None,
        "<|" | "|>" | "^" => Head::Triangle,
        "<" | ">" | "<<" | ">>" => Head::Arrow,
        "*" => Head::Diamond,
        "o" => Head::Hollow,
        other if crow_foot(other, is_left).is_some() => {
            feet = true;
            Head::None
        }
        other => {
            unknown = Some(other.to_string());
            Head::None
        }
    };
    let (l, r) = (head(left, true), head(right, false));
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
        // A dashed entity-relationship line is a relationship too.
        _ if feet => (Association, false),
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
    fn formatting_tags_are_dropped_and_type_parameters_kept() {
        assert_eq!(strip_format("<b><color:Gold>PK</color></b> id"), "PK id");
        assert_eq!(
            strip_format("<size:12><u>name</u></size> : List<String>"),
            "name : List<String>"
        );
        assert_eq!(strip_format("<font color=red>x</font> a < b"), "x a < b");
        assert_eq!(strip_format("Map<K, V>"), "Map<K, V>");
        let d = import("class A {\n  <b>+id</b> : int\n  +tags : List<Tag>\n}\n").doc;
        let a = el(&d, "A");
        assert_eq!(a.members[0].to_line(), "+id : int");
        assert_eq!(a.members[1].to_line(), "+tags : List<Tag>");
    }

    #[test]
    fn crows_feet_become_multiplicities() {
        let p = import("entity User\nentity Order\nentity Tag\nUser ||--o{ Order : places\nOrder }|..|{ Tag\nUser |o--|| Tag\nA +-- B\nC +-- D\n");
        let d = &p.doc;
        let r = rel(d, "User", "Order");
        assert_eq!(r.kind, RelationKind::Association);
        assert_eq!(
            (r.from.label.as_deref(), r.to.label.as_deref()),
            (Some("1"), Some("0..*"))
        );
        assert_eq!(r.name.as_deref(), Some("places"));
        let r = rel(d, "Order", "Tag");
        assert_eq!(r.kind, RelationKind::Association);
        assert_eq!(
            (r.from.label.as_deref(), r.to.label.as_deref()),
            (Some("1..*"), Some("1..*"))
        );
        let r = rel(d, "User", "Tag");
        assert_eq!(
            (r.from.label.as_deref(), r.to.label.as_deref()),
            (Some("0..1"), Some("1"))
        );
        // `+--` is reported once, not on every line.
        assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
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
    fn component_deployment_and_use_case_diagrams_read_as_boxes() {
        let src = r#"
actor User
:Admin: as A
node "Web Server" as WS {
  [Frontend] as FE
  component Api
}
database Db
rectangle Shop {
  (Buy) as UC1
  usecase (Pay)
}
User --> (Buy)
A --> UC1
[Frontend]-->[Api]
FE ..> Db : reads
User -[#red]-> [Mail]
"#;
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![], "{:?}", p.problems);
        let stereo = |n: &str| el(d, n).stereotype.clone().unwrap_or_default();
        assert_eq!(stereo("User"), "actor");
        assert_eq!(stereo("Admin"), "actor");
        assert_eq!(stereo("Web_Server.Frontend"), "component");
        assert_eq!(stereo("Web_Server.Api"), "component");
        assert_eq!(stereo("Db"), "database");
        assert_eq!(stereo("Shop.Buy"), "usecase");
        assert_eq!(stereo("Shop.Pay"), "usecase");
        assert_eq!(stereo("Mail"), "component");
        assert_eq!(d.packages.len(), 2);
        rel(d, "User", "Shop.Buy");
        rel(d, "Admin", "Shop.Buy");
        rel(d, "Web_Server.Frontend", "Web_Server.Api");
        assert_eq!(
            rel(d, "Web_Server.Frontend", "Db").name.as_deref(),
            Some("reads")
        );
        assert_eq!(d.elements.len(), 8);
    }

    #[test]
    fn keywords_in_any_case_and_names_over_several_lines() {
        let src = "Title Shop\nClass Order\nNote right of Order\n  paid\nend Note\nusecase Take as \"Take role\n--\nExtension points\n\"\nOrder --> Take\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![], "{:?}", p.problems);
        assert_eq!(d.title.as_deref(), Some("Shop"));
        let take = el(d, "Take_role");
        assert_eq!(take.members.len(), 1);
        assert_eq!(take.members[0].text, "Extension points");
        rel(d, "Order", "Take_role");
        rel(d, "Note", "Order");
    }

    #[test]
    fn namespaces_links_and_line_styles() {
        let src = r#"
set namespaceSeparator ::
class util::render::Camera [[https://x.org/a{b}]] {
  +zoom : float [[[https://x.org/c{d}]]]
}
abstract class util::Shape
enum util::geometry::Kind {
  A
}
util::render::Camera --> util::Shape #green;text:green : draws
util::Shape::kind --> util::geometry::Kind
component Main <<FXML>> as View
View ..> util::Shape
"#;
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems.len(), 1, "{:?}", p.problems); // `set`
        let cam = el(d, "util.render.Camera");
        assert_eq!(cam.members.len(), 1);
        assert_eq!(cam.members[0].to_line(), "+zoom : float");
        assert_eq!(el(d, "util.Shape").kind, ElementKind::Abstract);
        assert_eq!(
            rel(d, "util.render.Camera", "util.Shape").name.as_deref(),
            Some("draws")
        );
        // `Shape::kind` is a member of Shape.
        rel(d, "util.Shape", "util.geometry.Kind");
        assert_eq!(el(d, "Main").stereotype.as_deref(), Some("FXML"));
        rel(d, "Main", "util.Shape");
        assert_eq!(d.elements.len(), 4);
    }

    #[test]
    fn generator_output_forms() {
        let src = "package [src] as C_01 {\n  +class Inner {\n    -x : int\n  }\n  class A <E> << (T, #aa9999) >>\n}\npackage com.app\n{\n  class Main implements .com.api.Runner\n  class \"Ev\" as com.app.Ev<S extends T> {\n  }\n}\nnote left of Main::run()\n  starts\nend note\nright footer\n  made by a tool\nendfooter\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems.len(), 2, "{:?}", p.problems); // two type parameter lists dropped
        assert_eq!(el(d, "src.Inner").members.len(), 1);
        el(d, "src.A");
        rel(d, "com.app.Main", "com.api.Runner");
        el(d, "com.app.Ev");
        rel(d, "Note", "com.app.Main");
    }

    #[test]
    fn a_declaration_by_alias_reopens_the_element() {
        let p = import("class \"::std::pair<T1,T2>\" as C_01\nclass C_01 {\n  +first : T1\n}\nabstract C_01\nC_01 --> C_02\n");
        let d = &p.doc;
        assert_eq!(
            d.elements.len(),
            2,
            "{:?}",
            d.elements.values().map(|e| &e.name).collect::<Vec<_>>()
        );
        let pair = el(d, "std_pair_T1_T2");
        assert_eq!(pair.members.len(), 1);
        assert_eq!(pair.kind, ElementKind::Abstract);
        rel(d, "std_pair_T1_T2", "C_02");
    }

    #[test]
    fn library_calls_become_boxes_and_lines() {
        let src = r#"
!include <C4/C4_Container>
LAYOUT_WITH_LEGEND()
Person(user, "Customer", "A user")
System_Boundary(b, "Shop") {
  Container(api, "API", "Rust", "Serves orders")
}
Rel(user, api, "Uses", "HTTPS")
Rel_Back(api, user, "Replies")
"#;
        let p = import(src);
        let d = &p.doc;
        let customer = el(d, "Customer");
        assert_eq!(customer.stereotype.as_deref(), Some("Person"));
        assert_eq!(wrap("aa bb cc dd", 5), ["aa bb", "cc dd"]);
        assert_eq!(
            wrap("averyveryverylongword x", 5),
            ["averyveryverylongword", "x"]
        );
        assert_eq!(customer.members[0].text, "A user");
        let api = el(d, "Shop.API");
        assert_eq!(api.members.len(), 2);
        assert_eq!(rel(d, "Customer", "Shop.API").name.as_deref(), Some("Uses"));
        rel(d, "Customer", "Shop.API");
        assert_eq!(d.relations.len(), 2);
        let messages: Vec<&str> = p.problems.iter().map(|p| p.message.as_str()).collect();
        assert!(messages[0].contains("standard library"), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains("LAYOUT_WITH_LEGEND")));
        // One problem per procedure, not per call.
        assert_eq!(messages.iter().filter(|m| m.contains("`Rel(")).count(), 1);
    }

    #[test]
    fn other_kinds_of_diagram_are_not_read() {
        let p = import("@startuml\nAlice -> Bob : hello\nactivate Bob\n@enduml\n");
        assert!(p.doc.elements.is_empty());
        assert_eq!(p.problems.len(), 1);
        assert!(p.problems[0].message.contains("sequence diagram"));
        let p = import("@startmindmap\n* root\n@endmindmap\n");
        assert!(p.problems[0].message.contains("mindmap"));
    }

    #[test]
    fn includes_and_macros_are_followed() {
        let dir = std::env::temp_dir().join(format!("umpl-include-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(
            dir.join("lib/tables.iuml"),
            "!define Table(name,desc) class name as \"desc\" << (T,#FFAAAA) >>\n!define pk(x) <b>x</b>\nskinparam x y\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.puml"),
            "@startuml\n!include lib/tables.iuml\nTable(user, \"User\") {\n  pk(id) INTEGER\n}\nTable(order, \"Order\")\nuser --> order\n@enduml\n",
        )
        .unwrap();
        let p = import_path(&dir.join("main.puml")).unwrap();
        let d = &p.doc;
        // `pk(id)` gave `<b>id</b>`; the formatting is dropped.
        assert_eq!(el(d, "User").members[0].text, "id INTEGER");
        // The `(T,#FFAAAA)` spot is a letter in a circle, not a stereotype.
        assert_eq!(el(d, "User").stereotype, None);
        rel(d, "User", "Order");
        // The skipped line in the included file names that file.
        assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
        assert_eq!(p.problems[0].line, 2);
        assert!(p.problems[0]
            .message
            .starts_with("in `tables.iuml` line 3:"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn export_matches_plantuml_style() {
        let src = "diagram \"Shop\"\n\npackage billing {\n  abstract class Payment {\n    +pay(): Receipt {abstract}\n  }\n  class Item\n}\n\nclass Item\n\nnote N1 \"two\\nlines\"\n\n# owns\nOrder \"1\" *-- \"1..*\" Item : contains\nbilling.Item --|> billing.Payment\nN1 .. Order\nweird line\n";
        let out = export(&parse(src).doc);
        let expected = "@startuml\ntitle Shop\n\npackage billing {\n  abstract class Payment {\n    +pay(): Receipt {abstract}\n  }\n\n  class Item\n}\n\nclass Item\n\nnote \"two\\nlines\" as N1\n\n' owns\nOrder \"1\" *-- \"1..*\" Item : contains\nbilling.Item --|> Payment\nN1 .. Order\n\n' weird line\n\nclass Order\n@enduml\n";
        assert_eq!(out, expected);
    }
}
