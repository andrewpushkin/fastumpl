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
use crate::scan::Cursor;

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
                umpl_core::name_in_path(&el.name).into_owned()
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
        let name = if umpl_core::is_plain_name(&el.name) {
            el.name.clone()
        } else {
            pquote(&el.name)
        };
        match &el.label {
            Some(l) => out.push_str(&format!(
                "{indent}{} {} as {name}",
                el.kind.keyword(),
                pquote(l)
            )),
            None => out.push_str(&format!("{indent}{} {name}", el.kind.keyword())),
        }
        if let Some(p) = &el.type_params {
            out.push_str(&format!("<{p}>"));
        }
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
        let name = |t: Endpoint| match t {
            Endpoint::Element(e) => self.refs.get(&e).cloned().unwrap_or_default(),
            Endpoint::Package(p) => self.doc.full_package_name(p),
        };
        let mut line = format!("{indent}{}", name(r.from.target));
        if let Some(l) = &r.from.label {
            line.push_str(&format!(" {}", pquote(l)));
        }
        line.push_str(&format!(" {}", r.kind.arrow()));
        if let Some(l) = &r.to.label {
            line.push_str(&format!(" {}", pquote(l)));
        }
        line.push_str(&format!(" {}", name(r.to.target)));
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
    import_path_nth(path, 0)
}

/// Reads diagram `n` (from 0, see [`diagrams`]) of a PlantUML file.
pub fn import_path_nth(path: &Path, n: usize) -> std::io::Result<Parsed> {
    let bytes = std::fs::read(path)?;
    let src = String::from_utf8_lossy(&bytes);
    let read = |p: &Path| {
        std::fs::read(p)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    Ok(import_nth(&src, path.parent(), &read, n))
}

/// The text with one kind of line end, `\n`, and without a byte order
/// mark: PlantUML also reads files whose lines end in a lone `\r` (old Mac
/// files) and files that start with U+FEFF.
pub(crate) fn one_line_end(src: &str) -> std::borrow::Cow<'_, str> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    if src.contains('\r') && !src.contains('\n') {
        src.replace('\r', "\n").into()
    } else {
        src.into()
    }
}

/// One diagram of a PlantUML file, as PlantUML draws it: a block from
/// `@start…` to `@end…`, or one page of a block split by `newpage`.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagramInfo {
    /// The name after `@startuml`, the page's title, or none.
    pub name: Option<String>,
    pub kind: DiagramKind,
    /// The line (from 1) where the block or page starts.
    pub line: usize,
}

/// A block of a PlantUML file: the `@start…` line (if any) and the lines
/// of its body (indices), split into pages at `newpage`.
struct Block {
    start: Option<usize>,
    pages: Vec<std::ops::Range<usize>>,
}

fn is_newpage(l: &str) -> bool {
    let l = l.trim().to_ascii_lowercase();
    l == "newpage" || l.starts_with("newpage ")
}

fn blocks(lines: &[&str]) -> Vec<Block> {
    let is_start = |l: &str| l.trim_start().starts_with("@start");
    let is_end = |l: &str| l.trim_start().starts_with("@end");
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_start(lines[i]) {
            i += 1;
            continue;
        }
        let begin = i + 1;
        let end = lines[begin..]
            .iter()
            .position(|l| is_end(l))
            .map_or(lines.len(), |j| begin + j);
        // Another `@start` before the `@end` starts the diagram again, as in
        // PlantUML: what came before it is not part of any diagram.
        if let Some(j) = lines[begin..end].iter().rposition(|l| is_start(l)) {
            i = begin + j;
            continue;
        }
        let mut pages = Vec::new();
        let mut from = begin;
        for (k, line) in lines.iter().enumerate().take(end).skip(begin) {
            if is_newpage(line) {
                pages.push(from..k);
                from = k + 1;
            }
        }
        pages.push(from..end);
        out.push(Block {
            start: Some(i),
            pages,
        });
        i = end + 1;
    }
    if out.is_empty() {
        // Text without `@start`: one diagram, all of it.
        out.push(Block {
            start: None,
            pages: std::iter::once(0..lines.len()).collect(),
        });
    }
    out
}

/// The diagrams of a PlantUML file, in order.
pub fn diagrams(src: &str) -> Vec<DiagramInfo> {
    let src = one_line_end(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for b in blocks(&lines) {
        let start_line = b.start.map(|i| lines[i].trim());
        let block_name = start_line
            .and_then(|l| l.split_once(char::is_whitespace))
            .map(|(_, n)| unquote(n.trim()))
            .filter(|n| !n.is_empty() && !n.starts_with('('));
        let all: Vec<&str> = b
            .pages
            .iter()
            .flat_map(|r| lines[r.clone()].iter().copied())
            .collect();
        let tag_kind = start_line.map(|l| kind::other(kind::start_tag(l)));
        let kind = match tag_kind {
            Some(k) if !k.readable() => k,
            _ => kind::kind_of_lines(&all),
        };
        let several = b.pages.len() > 1;
        for (k, r) in b.pages.iter().enumerate() {
            let title = lines[r.clone()].iter().find_map(|l| {
                let t = l.trim();
                let rest = t
                    .strip_prefix("title ")
                    .or_else(|| t.strip_prefix("Title "))?;
                Some(unquote(rest.trim()))
            });
            let name = match (&block_name, several) {
                (Some(n), true) => Some(format!("{n}, page {}", k + 1)),
                (Some(n), false) => Some(n.clone()),
                (None, _) => title,
            };
            let line = if k == 0 {
                b.start.map_or(1, |i| i + 1)
            } else {
                r.start
            };
            out.push(DiagramInfo { name, kind, line });
        }
    }
    out
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
    import_nth(src, dir, read, 0)
}

/// Reads diagram `n` (from 0) of a PlantUML file: see [`diagrams`]. Past
/// the last diagram, the last one.
pub fn import_nth(src: &str, dir: Option<&Path>, read: Reader, n: usize) -> Parsed {
    let src = one_line_end(src);
    let lines: Vec<&str> = src.lines().collect();
    let all = blocks(&lines);
    let mut which = (all.len() - 1, all[all.len() - 1].pages.len() - 1);
    let mut k = 0;
    'find: for (b, block) in all.iter().enumerate() {
        for p in 0..block.pages.len() {
            if k == n {
                which = (b, p);
                break 'find;
            }
            k += 1;
        }
    }
    let block = &all[which.0];
    let page = which.1;
    let start = block.start;
    let begin = block.pages[0].start;
    let end = block.pages[block.pages.len() - 1].end;
    let mut im = Importer::default();
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
    let (pre, problems, libraries) = preprocess::preprocess(&body, dir, read);
    // PlantUML splits the preprocessed text into pages; keep this one.
    let pre: Vec<preprocess::Line> = {
        let mut pages: Vec<Vec<preprocess::Line>> = vec![Vec::new()];
        for l in pre {
            if is_newpage(&l.text) {
                pages.push(Vec::new());
            } else {
                pages.last_mut().unwrap().push(l);
            }
        }
        let n = pages.len();
        pages.swap_remove(page.min(n - 1))
    };
    let texts: Vec<&str> = pre.iter().map(|l| l.text.as_str()).collect();
    let kind = kind::kind_of_lines(&texts);
    if !kind.readable() {
        return unreadable(kind, first_line, problems);
    }
    im.problems.extend(problems);
    im.libraries = libraries;
    im.description = kind == DiagramKind::Description;
    let mut k = 0;
    while k < pre.len() {
        let l = &pre[k];
        let mut text = l.text.trim().to_string();
        // A line ending in `\` goes on on the next one.
        while text.ends_with('\\') && !text.ends_with("\\\\") && k + 1 < pre.len() {
            text.pop();
            k += 1;
            text.push_str(pre[k].text.trim());
        }
        // A quoted name that goes on over the next lines.
        if opens_multiline_string(&text) {
            let mut j = k + 1;
            while j < pre.len() && j - k < 50 {
                text.push('\n');
                text.push_str(pre[j].text.trim());
                j += 1;
                if text.matches('"').count().is_multiple_of(2) {
                    break;
                }
            }
            if text.matches('"').count().is_multiple_of(2) {
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
    if !keyword {
        return t.into();
    }
    // `Abstract Class X`.
    let rest = &t[end..];
    let after = rest.trim_start();
    if lower == "abstract" && starts_with_word_ci(after, "class") {
        let gap = &rest[..rest.len() - after.len()];
        return format!("abstract{gap}class{}", &after[5..]).into();
    }
    format!("{lower}{rest}").into()
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
const CLASS_LIKE: [&str; 10] = [
    "annotation",
    "stereotype",
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

/// Commands that only change how PlantUML draws, or steer its layout.
/// fastumpl draws in its own style and lays out itself, so they are
/// skipped without a problem (with their `{ … }` block, if any).
const STYLING: [&str; 7] = [
    "skinparam",
    "skin",
    "scale",
    "allowmixing",
    "allow_mixing",
    "sprite",
    "set",
];

/// Preprocessor directives left after preprocessing that only style the
/// drawing: `!theme`, `!pragma layout smetana`, …
const STYLING_DIRECTIVES: [&str; 4] = ["theme", "pragma", "option", "assume"];

/// What `hide`, `show`, `remove` and `restore` apply to when it is not
/// whole elements: parts of boxes, which fastumpl always shows.
const BOX_PARTS: [&str; 14] = [
    "members",
    "member",
    "methods",
    "method",
    "fields",
    "field",
    "attributes",
    "attribute",
    "circle",
    "circles",
    "stereotype",
    "stereotypes",
    "footbox",
    "description",
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
    /// A `legend`, `header`, `footer` or `caption` block, up to its `end…`.
    Text(&'static str, Vec<String>),
    /// A `note on link` block: its text goes on the last line drawn.
    LinkNote(Vec<String>),
    /// The text of a note written in braces, up to `}`.
    NoteBraces(ElementId, Vec<String>),
    /// The `key => value` rows of a `map`.
    Map(ElementId),
    /// The text of a box written in brackets: `node N [` … `]`.
    Description(ElementId, Vec<String>),
    /// The lines of a `json` object, with the depth of `{` and `[`.
    Json(ElementId, usize),
}

enum SkipEnd {
    /// `}` that brings the brace depth to zero.
    Brace(usize),
    /// An exact closing line such as `</style>`.
    Line(&'static str),
}

#[derive(Default)]
struct Importer {
    doc: Document,
    problems: Vec<Problem>,
    mode: Option<Mode>,
    /// The container of each open `{`; the current container is the last.
    stack: Vec<Option<PackageId>>,
    comments: Vec<String>,
    /// PlantUML's code names (its quarks): the path of code names of each
    /// element and package, from the top, as PlantUML looks them up. The
    /// document's names are what the boxes show.
    quarks: HashMap<Vec<String>, Endpoint>,
    quark_of: HashMap<Endpoint, Vec<String>>,
    /// Everything with a given last code name, for a plain name PlantUML
    /// finds anywhere when only one thing has it.
    by_last: HashMap<String, Vec<Endpoint>>,
    /// Elements named by the text they show (`class "Order" as O` is named
    /// `Order`): another element known by that name takes the name from
    /// them, and they keep the text as their label.
    shown_named: std::collections::HashSet<ElementId>,
    /// The line made last, for `note on link`.
    last_relation: Option<RelationId>,
    /// Elements first named in a line and not declared yet: a declaration
    /// moves them to where it is.
    undeclared: std::collections::HashSet<ElementId>,
    /// A component, deployment or use case diagram: PlantUML does not split
    /// its names into packages at dots.
    description: bool,
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
    /// `package P` on the previous line, without `{`: a box drawn as a
    /// package, which a `{` on the next line makes a package, or the
    /// package P already is, which it opens.
    last_package: Option<Endpoint>,
    /// `set namespaceSeparator`: what splits a name into packages. `None`
    /// until set (`.`, and `::`); `Some(None)` for `none`.
    separator: Option<Option<String>>,
    /// `hide`/`remove` (false) and `show`/`restore` (true) of elements, in
    /// file order: line, show, target.
    shown: Vec<(usize, bool, String)>,
    /// `$tags` of elements, for `hide $tag`.
    tags: HashMap<ElementId, Vec<String>>,
    /// Text of each `legend`, with its line.
    legends: Vec<(usize, String)>,
    /// Known libraries the diagram includes (see
    /// [`preprocess::known_library`]): their procedures are read as boxes,
    /// lines and frames without a problem.
    libraries: Vec<String>,
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

    /// The separator `set namespaceSeparator` chose: until then `.` (none
    /// in a component diagram), `None` for `none`.
    fn separator(&self) -> Option<&str> {
        match &self.separator {
            None if self.description => None,
            None => Some("."),
            Some(s) => s.as_deref(),
        }
    }

    /// The parts of a PlantUML code name, split at the namespace separator
    /// (`com.shop.Order` is `Order` in package `shop` in `com`), and whether
    /// it names from the top: a leading separator, `.com.shop.Order`.
    fn code_parts(&self, code: &str) -> (bool, Vec<String>) {
        // A blank quoted name, `class " "`, is a name.
        if code.trim().is_empty() {
            return (false, vec![code.to_string()]);
        }
        let code = code.trim();
        let Some(sep) = self.separator() else {
            return (false, vec![code.to_string()]);
        };
        let parts: Vec<String> = code
            .split(sep)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return (false, vec![code.to_string()]);
        }
        (code.starts_with(sep), parts)
    }

    /// The code names of a package from the top (none for the top).
    fn path_of(&self, pkg: Option<PackageId>) -> Vec<String> {
        pkg.and_then(|p| self.quark_of.get(&Endpoint::Package(p)).cloned())
            .unwrap_or_default()
    }

    /// Where PlantUML places the code name `code` written in package
    /// `from`: from the top if it starts with the separator, or if it has
    /// several parts and the first names something at the top; else in
    /// `from`.
    fn quark_path(&self, from: Option<PackageId>, code: &str) -> Vec<String> {
        let (absolute, parts) = self.code_parts(code);
        if absolute || (parts.len() > 1 && self.quarks.contains_key(&parts[..1])) {
            return parts;
        }
        let mut path = self.path_of(from);
        path.extend(parts);
        path
    }

    fn register(&mut self, path: Vec<String>, thing: Endpoint) {
        if let Some(last) = path.last() {
            self.by_last.entry(last.clone()).or_default().push(thing);
        }
        self.quark_of.insert(thing, path.clone());
        self.quarks.insert(path, thing);
    }

    fn unregister(&mut self, thing: Endpoint) {
        if let Some(path) = self.quark_of.remove(&thing) {
            self.quarks.remove(&path);
            if let Some(all) = path.last().and_then(|l| self.by_last.get_mut(l)) {
                all.retain(|t| *t != thing);
            }
        }
    }

    /// The package that holds an element or package.
    fn parent_of(&self, thing: Endpoint) -> Option<PackageId> {
        match self.quark_of.get(&thing) {
            Some(path) => match self.quarks.get(&path[..path.len().saturating_sub(1)]) {
                Some(Endpoint::Package(p)) => Some(*p),
                _ => None,
            },
            None => match thing {
                Endpoint::Element(e) => self.doc.element_package(e),
                Endpoint::Package(p) => self.doc.container_of(&Decl::Package(p)),
            },
        }
    }

    /// Whether `name` is free in `container`, after taking it from an
    /// element that only shows it: that one is renamed to its code name and
    /// keeps the text as its label.
    fn claim_name(&mut self, container: Option<PackageId>, name: &str) -> bool {
        if self.doc.name_is_free(name, container) {
            return true;
        }
        let holder = self.doc.body(container).iter().find_map(|d| match d {
            Decl::Element(e)
                if self.shown_named.contains(e) && self.doc.elements[e].name == name =>
            {
                Some(*e)
            }
            _ => None,
        });
        let Some(id) = holder else {
            return false;
        };
        self.shown_named.remove(&id);
        let code = self
            .quark_of
            .get(&Endpoint::Element(id))
            .and_then(|p| p.last().cloned())
            .unwrap_or_else(|| name.to_string());
        let new = self.doc.unique_name(&code, container);
        let el = self.doc.elements.get_mut(&id).unwrap();
        el.label = Some(std::mem::replace(&mut el.name, new));
        true
    }

    /// The name and label of a new element in `container` known by `code`
    /// and showing `shown`: the text it shows where that is free, else its
    /// code with the text as label. And whether it is named by the text.
    fn element_names(
        &mut self,
        container: Option<PackageId>,
        code: &str,
        shown: Option<&str>,
    ) -> (String, Option<String>, bool) {
        let text = shown.unwrap_or(code);
        if text != code && self.doc.name_is_free(text, container) {
            return (text.to_string(), None, true);
        }
        let label = (text != code).then(|| text.to_string());
        if self.claim_name(container, code) {
            return (code.to_string(), label, false);
        }
        // Taken by a package, or by an element named by its own code.
        let name = self.doc.unique_name(code, container);
        (name, Some(text.to_string()), false)
    }

    /// The element at code path `path`, made (with the packages on the way)
    /// if there is none. A declaration (`explicit`) sets the kind and
    /// stereotype and takes the comments before it; an element first named
    /// in a line is a class.
    fn element_at(
        &mut self,
        path: Vec<String>,
        kind: ElementKind,
        shown: Option<&str>,
        stereotype: Option<String>,
        explicit: bool,
    ) -> ElementId {
        if let Some(Endpoint::Element(id)) = self.quarks.get(&path).copied() {
            if explicit && self.undeclared.remove(&id) {
                let container = self.parent_of(Endpoint::Element(id));
                let body = match container {
                    Some(p) => &mut self.doc.packages.get_mut(&p).unwrap().body,
                    None => &mut self.doc.root,
                };
                body.retain(|d| *d != Decl::Element(id));
                body.push(Decl::Element(id));
            }
            if explicit {
                let el = self.doc.elements.get_mut(&id).unwrap();
                el.kind = kind;
                if stereotype.is_some() {
                    el.stereotype = stereotype;
                }
                el.comments.append(&mut self.comments);
                if let Some(text) = shown {
                    self.show(id, text);
                }
            }
            return id;
        }
        let (code, pkgs) = path.split_last().unwrap();
        // The comments are about the element, not the packages made for it.
        let comments = std::mem::take(&mut self.comments);
        let container = self.package_at(pkgs, None, false);
        self.comments = comments;
        let (name, label, by_text) = self.element_names(container, code, shown);
        let id = self.doc.new_element_id();
        let mut el = Element::new(kind, name);
        el.label = label;
        el.stereotype = stereotype;
        if explicit {
            el.comments = std::mem::take(&mut self.comments);
        }
        self.doc.elements.insert(id, el);
        self.push_decl_in(container, Decl::Element(id));
        if by_text {
            self.shown_named.insert(id);
        }
        if !explicit {
            self.undeclared.insert(id);
        }
        // A package may hold the path: the class is then not found by it.
        if !self.quarks.contains_key(&path) {
            self.register(path, Endpoint::Element(id));
        }
        id
    }

    /// Makes element `id` show `text`: as its name where that is free, else
    /// as its label.
    fn show(&mut self, id: ElementId, text: &str) {
        if self.doc.elements[&id].shown() == text {
            return;
        }
        let container = self.parent_of(Endpoint::Element(id));
        let code = self
            .quark_of
            .get(&Endpoint::Element(id))
            .and_then(|p| p.last().cloned());
        if self.doc.name_is_free(text, container) {
            let el = self.doc.elements.get_mut(&id).unwrap();
            el.name = text.to_string();
            el.label = None;
            if code.as_deref() == Some(text) {
                self.shown_named.remove(&id);
            } else {
                self.shown_named.insert(id);
            }
        } else {
            self.doc.elements.get_mut(&id).unwrap().label = Some(text.to_string());
        }
    }

    /// The package at code path `path`, made with the packages on the way
    /// if there is none; a new last one shows `shown` if given. Declaring a
    /// package (`declared`) where a class is makes the class the package,
    /// as in PlantUML; a class on the way stays a class, and what is under
    /// its name goes beside it.
    fn package_at(
        &mut self,
        path: &[String],
        shown: Option<&str>,
        declared: bool,
    ) -> Option<PackageId> {
        let mut container = None;
        for i in 0..path.len() {
            let prefix = &path[..=i];
            let last = i + 1 == path.len();
            container = Some(match self.quarks.get(prefix).copied() {
                Some(Endpoint::Package(p)) => p,
                Some(Endpoint::Element(e)) if declared && last => {
                    self.element_to_package(e, container)
                }
                Some(Endpoint::Element(e)) => {
                    let parent = self.parent_of(Endpoint::Element(e));
                    match parent {
                        Some(p) => {
                            container = Some(p);
                            continue;
                        }
                        None => {
                            container = None;
                            continue;
                        }
                    }
                }
                None => {
                    let text = match shown {
                        Some(s) if i + 1 == path.len() => s.to_string(),
                        _ => path[i].clone(),
                    };
                    let name = if self.claim_name(container, &text) {
                        text
                    } else {
                        self.doc.unique_name(&text, container)
                    };
                    let id = self.doc.new_package_id();
                    let mut p = Package::new(name);
                    p.comments = std::mem::take(&mut self.comments);
                    self.doc.packages.insert(id, p);
                    self.push_decl_in(container, Decl::Package(id));
                    self.register(prefix.to_vec(), Endpoint::Package(id));
                    id
                }
            });
        }
        container
    }

    /// `class A` and then `package A { … }`: PlantUML makes the class the
    /// package (its members go), and lines to it lines to the package.
    fn element_to_package(&mut self, e: ElementId, container: Option<PackageId>) -> PackageId {
        let path = self.quark_of.get(&Endpoint::Element(e)).cloned();
        self.unregister(Endpoint::Element(e));
        self.shown_named.remove(&e);
        self.tags.remove(&e);
        if self.last_element == Some(e) {
            self.last_element = None;
        }
        if self.last_decl.is_some_and(|(d, _)| d == e) {
            self.last_decl = None;
        }
        let el = self.doc.elements.remove(&e).unwrap();
        self.doc.root.retain(|d| *d != Decl::Element(e));
        for p in self.doc.packages.values_mut() {
            p.body.retain(|d| *d != Decl::Element(e));
        }
        let id = self.doc.new_package_id();
        let mut p = Package::new(el.name);
        p.comments = el.comments;
        p.comments.append(&mut self.comments);
        self.doc.packages.insert(id, p);
        self.push_decl_in(container, Decl::Package(id));
        for r in self.doc.relations.values_mut() {
            for end in [&mut r.from, &mut r.to] {
                if end.target == Endpoint::Element(e) {
                    end.target = Endpoint::Package(id);
                }
            }
        }
        // An element no code name found (one beside a package of its
        // name) gets a path of its own name.
        let path = path.unwrap_or_else(|| {
            let mut p = self.path_of(container);
            p.push(self.doc.packages[&id].name.clone());
            p
        });
        if !self.quarks.contains_key(&path) {
            self.register(path, Endpoint::Package(id));
        }
        id
    }

    /// Declares the element PlantUML knows by `code` (seen from the current
    /// package), showing `shown` (else the last part of the code).
    fn declare_named(
        &mut self,
        kind: ElementKind,
        code: &str,
        shown: Option<&str>,
        stereotype: Option<String>,
    ) -> ElementId {
        let anywhere = self.description;
        self.declare_where(kind, code, shown, stereotype, anywhere)
    }

    /// [`Importer::declare_named`], finding the element anywhere first if
    /// `anywhere`: a component diagram declares an element again wherever
    /// it is first (`file "index.ts"` in a second package is the first
    /// one), and so does `object X`.
    fn declare_where(
        &mut self,
        kind: ElementKind,
        code: &str,
        shown: Option<&str>,
        stereotype: Option<String>,
        anywhere: bool,
    ) -> ElementId {
        let mut path = self.quark_path(self.container(), code);
        if anywhere && !self.quarks.contains_key(&path) {
            let first = self.by_last.get(code.trim()).and_then(|all| {
                all.iter()
                    .find(|t| matches!(t, Endpoint::Element(_)))
                    .copied()
            });
            if let Some(found) = first.and_then(|t| self.quark_of.get(&t)) {
                path = found.clone();
            }
        }
        self.element_at(path, kind, shown, stereotype, true)
    }

    /// A new element with a name made up from `base` (`Note`, `Note2`, …)
    /// in `container`.
    fn new_local(
        &mut self,
        container: Option<PackageId>,
        kind: ElementKind,
        base: &str,
    ) -> ElementId {
        let mut path = self.path_of(container);
        let name = (1..)
            .map(|k| {
                if k == 1 {
                    base.to_string()
                } else {
                    format!("{base}{k}")
                }
            })
            .find(|n| {
                path.push(n.clone());
                let free = self.doc.name_is_free(n, container) && !self.quarks.contains_key(&path);
                path.pop();
                free
            })
            .unwrap();
        path.push(name);
        self.element_at(path, kind, None, None, true)
    }

    /// Adds a line, in the current package.
    fn add_relation(
        &mut self,
        kind: RelationKind,
        from: (Endpoint, Option<String>),
        to: (Endpoint, Option<String>),
        name: Option<String>,
        comments: Vec<String>,
    ) -> RelationId {
        let id = self.doc.new_relation_id();
        let relation = Relation {
            kind,
            from: End {
                target: from.0,
                label: from.1,
            },
            to: End {
                target: to.0,
                label: to.1,
            },
            name,
            comments,
        };
        self.doc.relations.insert(id, relation);
        self.push_decl_in(self.container(), Decl::Relation(id));
        self.last_relation = Some(id);
        id
    }

    /// What a name in a line refers to, as PlantUML finds it: the element
    /// or package at the place the name gives (see
    /// [`Importer::quark_path`]), or, for a plain name, the one thing so
    /// named anywhere if only one is.
    fn lookup(&self, from: Option<PackageId>, code: &str) -> Option<Endpoint> {
        let path = self.quark_path(from, code);
        if let Some(t) = self.quarks.get(&path) {
            return Some(*t);
        }
        let (absolute, parts) = self.code_parts(code);
        if let (false, [only]) = (absolute, parts.as_slice()) {
            if let Some([t]) = self.by_last.get(only).map(Vec::as_slice) {
                return Some(*t);
            }
        }
        None
    }

    /// The element or package an end of a line names (lines may join
    /// packages); an unknown name makes a class where the name places it.
    fn resolve_end(&mut self, from: Option<PackageId>, r: &str) -> Endpoint {
        let r = r.trim();
        if let Some(t) = self.lookup(from, r) {
            return t;
        }
        // `Class::member` names a member of the class (made if needed),
        // where `::` does not split names: `dart::io::File` is `File` in
        // `dart::io`.
        if self.separator() != Some("::") {
            if let Some((owner, _member)) = r.rsplit_once("::") {
                let owner = owner.trim();
                if !owner.is_empty() {
                    if let Some(t) = self.lookup(from, owner) {
                        return t;
                    }
                    let path = self.quark_path(from, owner);
                    let stereo = self.implicit.get(owner).map(|s| s.to_string());
                    return Endpoint::Element(self.element_at(
                        path,
                        ElementKind::Class,
                        None,
                        stereo,
                        false,
                    ));
                }
            }
        }
        let path = self.quark_path(from, r);
        let stereo = self.implicit.get(r).map(|s| s.to_string());
        Endpoint::Element(self.element_at(path, ElementKind::Class, None, stereo, false))
    }

    /// The element a name refers to, as [`Importer::resolve_end`] finds it.
    fn resolve(&mut self, from: Option<PackageId>, r: &str) -> ElementId {
        match self.resolve_end(from, r) {
            Endpoint::Element(e) => e,
            // A package of that name: a class of its own beside it.
            Endpoint::Package(_) => {
                let path = self.quark_path(from, r);
                self.element_at(path, ElementKind::Class, None, None, false)
            }
        }
    }

    /// The parent `extends` or `implements` names: PlantUML looks only at
    /// the place the name gives, and makes it there if there is none.
    fn resolve_parent(&mut self, from: Option<PackageId>, r: &str) -> Endpoint {
        let path = self.quark_path(from, r.trim());
        if let Some(t) = self.quarks.get(&path) {
            return *t;
        }
        Endpoint::Element(self.element_at(path, ElementKind::Class, None, None, false))
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
        // `class A /' why '/` and `} /' end of package '/`.
        let uncommented = strip_inline_comments(t);
        let t = match self.mode {
            None | Some(Mode::Normal) | Some(Mode::Body { .. }) => uncommented.as_ref(),
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
            Mode::LinkNote(mut lines) => {
                if is_end(t, "note") {
                    self.note_on_link(&lines.join(" "));
                } else {
                    lines.push(t.trim().to_string());
                    self.mode = Some(Mode::LinkNote(lines));
                }
            }
            Mode::Map(id) => self.map_line(t, id),
            Mode::NoteBraces(id, mut lines) => {
                if t.trim() == "}" {
                    self.finish_note(Some(id), lines);
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::NoteBraces(id, lines));
                }
            }
            Mode::Json(id, depth) => {
                // The JSON text, line by line, up to the `}` that closes it.
                let opens = t.matches(['{', '[']).count();
                let depth = (depth + opens).saturating_sub(t.matches(['}', ']']).count());
                if depth > 0 {
                    let line = t.trim().trim_end_matches(',');
                    if !line.is_empty() {
                        let el = self.doc.elements.get_mut(&id).unwrap();
                        el.members.push(Member::plain(line.to_string()));
                    }
                    self.mode = Some(Mode::Json(id, depth));
                }
            }
            Mode::Description(id, mut lines) => {
                if t.trim() == "]" {
                    self.describe(id, lines);
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Description(id, lines));
                }
            }
            Mode::Text(what, mut lines) => {
                if is_end(t, what) {
                    self.finish_text(n, what, lines);
                } else {
                    lines.push(t.to_string());
                    self.mode = Some(Mode::Text(what, lines));
                }
            }
        }
    }

    /// The text of a `legend` becomes a note; the text of a `header`,
    /// `footer` or `caption` (page furniture, not part of the model) is
    /// kept as comments.
    fn finish_text(&mut self, n: usize, what: &'static str, lines: Vec<String>) {
        let lines: Vec<String> = lines
            .iter()
            .map(|l| strip_format(&l.replace("\\n", "\n")).trim().to_string())
            .flat_map(|l| l.split('\n').map(str::to_string).collect::<Vec<_>>())
            .collect();
        let first = lines
            .iter()
            .position(|l| !l.is_empty())
            .unwrap_or(lines.len());
        let last = lines
            .iter()
            .rposition(|l| !l.is_empty())
            .map_or(first, |i| i + 1);
        let lines = &lines[first..last.max(first)];
        if lines.is_empty() {
            return;
        }
        if what == "legend" {
            // Made at the end: a legend is not an element `hide` can name.
            self.legends.push((n, lines.join("\n")));
        } else {
            self.comments.push(format!("{what}: {}", lines[0]));
            self.comments.extend(lines[1..].iter().cloned());
        }
    }

    /// Puts the text of a `note on link` on the last line drawn, as its
    /// label (PlantUML draws the note next to the line).
    fn note_on_link(&mut self, text: &str) {
        let text = strip_format(text.trim()).replace("\\n", " ");
        if text.is_empty() {
            return;
        }
        if let Some(r) = self
            .last_relation
            .and_then(|r| self.doc.relations.get_mut(&r))
        {
            r.name = Some(match r.name.take() {
                Some(n) => format!("{n} – {text}"),
                None => text,
            });
        }
    }

    /// A row of a `map`: `key => value`, or `key *-> Target`, a line from
    /// the map to `Target`.
    fn map_line(&mut self, t: &str, id: ElementId) {
        let t = t.trim();
        if t == "}" {
            return;
        }
        self.mode = Some(Mode::Map(id));
        if t.is_empty() || t.starts_with('\'') {
            return;
        }
        if let Some((key, target)) = t.split_once("*->") {
            self.doc
                .elements
                .get_mut(&id)
                .unwrap()
                .members
                .push(Member::plain(strip_format(key.trim()).into_owned()));
            let to = self.resolve_end(self.container(), target.trim());
            let from = (Endpoint::Element(id), None);
            self.add_relation(RelationKind::Directed, from, (to, None), None, Vec::new());
            return;
        }
        let row = match t.split_once("=>") {
            Some((k, v)) => format!("{} => {}", strip_format(k.trim()), strip_format(v.trim())),
            None => strip_format(t).into_owned(),
        };
        self.doc
            .elements
            .get_mut(&id)
            .unwrap()
            .members
            .push(Member::plain(row));
    }

    /// The text in brackets is what the box shows (`node N [` … `]`): it
    /// becomes the name, and the code name `N` still refers to the box.
    fn describe(&mut self, id: ElementId, lines: Vec<String>) {
        let lines: Vec<String> = lines
            .iter()
            .map(|l| strip_format(l.trim()).into_owned())
            .filter(|l| !is_separator(l))
            .collect();
        let first = lines
            .iter()
            .position(|l| !l.is_empty())
            .unwrap_or(lines.len());
        let last = lines
            .iter()
            .rposition(|l| !l.is_empty())
            .map_or(first, |i| i + 1);
        let text = lines[first..last.max(first)].join("\n");
        if text.is_empty() {
            return;
        }
        self.show(id, &text);
    }

    fn skip_line(&mut self, t: &str, end: SkipEnd) {
        let next = match end {
            SkipEnd::Brace(depth) => {
                let opened = t.matches('{').count();
                let closed = t.matches('}').count();
                let depth = (depth + opened).saturating_sub(closed);
                (depth > 0).then_some(SkipEnd::Brace(depth))
            }
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
            match last_package {
                Some(Endpoint::Element(e)) if self.doc.elements.contains_key(&e) => {
                    let parent = self.parent_of(Endpoint::Element(e));
                    let p = self.element_to_package(e, parent);
                    self.stack.push(Some(p));
                    return;
                }
                Some(Endpoint::Package(p)) => {
                    self.stack.push(Some(p));
                    return;
                }
                _ => {}
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
            // A `}` closing nothing ends no group: PlantUML ignores it (and
            // fails on any declaration after it).
            self.stack.pop();
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
        if self.try_dotted_link(t) {
            return;
        }
        self.raw(n, t, "not understood; kept as written");
    }

    /// `Shared.Region` alone on a line: PlantUML reads a dotted line from
    /// `Shared`, pointing right (`.R`), to `egion`; `a.b` is a dotted line
    /// from `a` to `b`.
    fn try_dotted_link(&mut self, t: &str) -> bool {
        let Some((a, b)) = t.split_once('.') else {
            return false;
        };
        let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !plain(a) || !plain(b) {
            return false;
        }
        let b = match b.strip_prefix(['R', 'L', 'U', 'D', 'r', 'l', 'u', 'd']) {
            Some(rest) if !rest.is_empty() => rest,
            _ => b,
        };
        let comments = std::mem::take(&mut self.comments);
        let container = self.container();
        let from = self.resolve_end(container, a);
        let to = self.resolve_end(container, b);
        self.add_relation(
            RelationKind::NoteLink,
            (from, None),
            (to, None),
            None,
            comments,
        );
        true
    }

    fn statement(&mut self, n: usize, t: &str) -> bool {
        self.try_title(t)
            || self.try_skipped(n, t)
            || self.try_package(t)
            || self.try_note(t)
            || self.try_element(t)
            || self.try_short_decl(t)
            || self.try_relation(n, t)
            || self.try_call(n, t)
            || self.try_member(t)
            || self.try_stereotype(t)
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
        // Skips a line, and the `{ … }` block it opens if any.
        let silent = |me: &mut Self| {
            let depth = t
                .matches('{')
                .count()
                .saturating_sub(t.matches('}').count());
            me.mode = (depth > 0).then_some(Mode::Skip(SkipEnd::Brace(depth)));
            true
        };
        if lower.starts_with('!') {
            let word = word.trim_start_matches('!');
            if STYLING_DIRECTIVES.contains(&word) {
                return silent(self);
            }
            self.problem(
                n,
                format!("PlantUML `!{word}` is not supported; line skipped"),
            );
            return true;
        }
        if lower.starts_with("<style>") {
            self.mode =
                (!lower.contains("</style>")).then_some(Mode::Skip(SkipEnd::Line("</style>")));
            return true;
        }
        // `left to right direction`, `top to bottom direction`.
        if lower.ends_with(" direction") && lower.split_whitespace().count() == 4 {
            return true;
        }
        let first = t
            .split(|c: char| c.is_whitespace() || c == '{')
            .next()
            .unwrap_or("");
        let after = t[first.len()..].trim_start();
        // `footer "Page 1"`, `caption "…"`: text, not a relation.
        // `footer "Page 1"`, `caption <b>Zones</b>`, `hide *`: not relations.
        let text_line = [
            "footer", "header", "caption", "legend", "hide", "show", "remove", "restore",
        ]
        .iter()
        .any(|w| first.eq_ignore_ascii_case(w))
            && !(after.starts_with("--") || after.starts_with("..") || after.starts_with("->"));
        let looks_like_relation = !text_line
            && (after.starts_with(['-', '.', '*', ':', '"'])
                || (after.starts_with('<') && !after.starts_with("<<")));
        if looks_like_relation {
            return false;
        }
        if first == "set" {
            self.set(after);
            return silent(self);
        }
        // `page 2x1`: how PlantUML splits the image into pages.
        if first == "page"
            && after.contains('x')
            && after.chars().all(|c| c.is_ascii_digit() || c == 'x')
        {
            return true;
        }
        // A stray `@startuml` or `@enduml` inside a diagram; a separator
        // line `=======` outside a class body.
        let rule = ["--", "..", "==", "__"].iter().any(|r| {
            t.starts_with(r) && (t.ends_with(r) || t.chars().all(|c| c == r.as_bytes()[0] as char))
        });
        if lower.starts_with("@start") || lower.starts_with("@end") || rule {
            return true;
        }
        if STYLING.contains(&first) {
            return silent(self);
        }
        if matches!(first, "hide" | "show" | "remove" | "restore") {
            self.visibility(n, first, after);
            return true;
        }
        if first == "mainframe" || first == "caption" {
            let text = unquote(after.trim()).replace("\\n", "\n");
            self.finish_text(
                n,
                if first == "caption" {
                    "caption"
                } else {
                    "frame"
                },
                vec![text],
            );
            return true;
        }
        // `legend`, `header`, `footer`, optionally aligned: `right footer`,
        // `rightfooter`, `legend top left`.
        let mut words = lower.split_whitespace();
        let mut w = words.next().unwrap_or("");
        let mut rest_at = first.len();
        for align in ["left", "right", "center"] {
            if let Some(x @ ("header" | "footer")) = w.strip_prefix(align) {
                w = x;
                break;
            }
        }
        if ["left", "right", "center"].contains(&w) {
            match words.next() {
                Some(x @ ("header" | "footer" | "legend")) => {
                    w = x;
                    rest_at = t.find(x).map_or(t.len(), |i| i + x.len());
                }
                _ => return false,
            }
        }
        let what: &'static str = match w {
            "legend" => "legend",
            "header" => "header",
            "footer" => "footer",
            _ => return false,
        };
        let rest = t[rest_at..].trim();
        let placement = rest.split_whitespace().all(|p| {
            ["left", "right", "center", "top", "bottom"].contains(&p.to_ascii_lowercase().as_str())
        });
        if placement {
            self.mode = Some(Mode::Text(what, Vec::new()));
        } else {
            self.finish_text(n, what, vec![unquote(rest)]);
        }
        true
    }

    /// `set namespaceSeparator ::` (or `none`): how dotted names split into
    /// packages. Other `set` commands only style the drawing.
    fn set(&mut self, after: &str) {
        let mut words = after.split_whitespace();
        let what = words.next().unwrap_or("").to_ascii_lowercase();
        if what == "namespaceseparator" || what == "separator" {
            let sep = words.next().unwrap_or("");
            self.separator = Some(
                (!sep.eq_ignore_ascii_case("none") && !sep.is_empty()).then(|| sep.to_string()),
            );
        }
    }

    /// `hide X`, `remove X`, `show X`, `restore X`: X is an element (name,
    /// pattern with `*`, `<<stereotype>>`, `$tag`, `@unlinked`, `*`), to
    /// leave out of the drawing or bring back. Hiding a part of the boxes
    /// (`hide empty members`, `hide circle`, `hide <<S>> stereotype`) only
    /// styles the drawing: fastumpl always shows every member.
    fn visibility(&mut self, n: usize, command: &str, after: &str) {
        let words: Vec<&str> = after.split_whitespace().collect();
        let part = words
            .last()
            .is_some_and(|w| BOX_PARTS.contains(&w.to_ascii_lowercase().as_str()))
            || words.first().is_some_and(|w| {
                ["empty", "private", "protected", "public", "package"]
                    .contains(&w.to_ascii_lowercase().as_str())
            });
        if part || words.is_empty() {
            return;
        }
        // `hide class` / `hide enum`: every element of a kind (rare).
        let target = words.join(" ");
        self.shown
            .push((n, command == "show" || command == "restore", target));
    }

    fn try_package(&mut self, t: &str) -> bool {
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
        c.skip_ws();
        let name_quoted = c.rest().starts_with(['"', '[']);
        // `rectangle {`, `package {`: a frame without a name.
        if c.rest().trim() == "{" {
            let container = self.container();
            let mut path = self.path_of(container);
            path.push(" ".to_string());
            while !self.doc.name_is_free(path.last().unwrap(), container)
                || self.quarks.contains_key(&path)
            {
                path.last_mut().unwrap().push(' ');
            }
            let pkg = self.package_at(&path, None, true);
            self.stack.push(pkg);
            return true;
        }
        // `package [src]`, `package (javautil)`, `package "Shop"`, or a word
        // PlantUML takes up to a space: `textil-backend`, `org.apache.*`.
        let Some(name) = short_form(&mut c)
            .filter(|(_, s)| *s != "actor")
            .map(|(n, _)| n)
            .or_else(|| {
                c.skip_ws();
                if c.rest().starts_with('"') {
                    c.string()
                } else {
                    package_word(&mut c)
                }
            })
        else {
            return false;
        };
        skip_decorations(&mut c);
        let mut alias = None;
        if c.eat_word("as") {
            c.skip_ws();
            let alias_quoted = c.rest().starts_with('"');
            let Some(a) = decl_name(&mut c) else {
                return false;
            };
            alias = Some((a, alias_quoted));
        }
        skip_decorations(&mut c);
        if container && !c.rest().starts_with('{') {
            // `node Web` without a body is a box.
            return false;
        }
        if c.rest().trim() == "[" {
            // `package Signup [` … `]`: a box showing the text.
            let code = alias.map_or(name, |(a, _)| a);
            let id = self.declare_named(ElementKind::Class, &code, None, Some("package".into()));
            self.mode = Some(Mode::Description(id, Vec::new()));
            return true;
        }
        let braces = c.eat("{");
        let open = braces && !c.eat("}");
        if !c.at_end() {
            return false;
        }
        // `package "Shown" as code`: one package, shown as written, known
        // by its code name; without `as`, the name's parts nest.
        let (code, shown) = match alias {
            Some((a, alias_quoted)) => {
                let (code, shown) = if alias_quoted && !name_quoted {
                    (name, a)
                } else {
                    (a, name)
                };
                let shown = if shown.trim().is_empty() {
                    shown
                } else {
                    shown.trim().to_string()
                };
                (code, Some(shown))
            }
            None => (name, None),
        };
        let path = self.quark_path(self.container(), &code);
        if !braces {
            // `package P` without a body is a box drawn as a package (as a
            // component diagram has it); a `{` on the next line makes it a
            // package. If P is a package already, `{` opens it.
            if let Some(t @ Endpoint::Package(_)) = self.quarks.get(&path).copied() {
                self.last_package = Some(t);
                return true;
            }
            let stereotype = Some("package".to_string());
            let id = self.declare_named(ElementKind::Class, &code, shown.as_deref(), stereotype);
            self.last_package = Some(Endpoint::Element(id));
            self.last_element = Some(id);
            return true;
        }
        let pkg = self.package_at(&path, shown.as_deref(), true);
        if open {
            self.stack.push(pkg);
        }
        true
    }

    fn try_element(&mut self, t: &str) -> bool {
        // `+class Inner {`: a visibility before the keyword.
        let t = match t.strip_prefix(['+', '-', '#', '~']) {
            Some(rest) if rest.starts_with(|ch: char| ch.is_ascii_lowercase()) => rest,
            _ => t,
        };
        // `static class Log`; `mix_actor Bob` (`allowmixing`) is `actor Bob`.
        let t = t.strip_prefix("static ").map_or(t, str::trim_start);
        let t = t.strip_prefix("mix_").unwrap_or(t);
        let mut c = Cursor::new(t);
        let (kind, keyword_stereo) = if c.eat_word("abstract") {
            // `abstract Class View`.
            c.skip_ws();
            let r = c.rest();
            if starts_with_word_ci(r, "class")
                && r[5..]
                    .trim_start()
                    .starts_with(|ch: char| ch.is_alphanumeric() || ch == '"' || ch == '_')
            {
                c.pos += 5;
            }
            (ElementKind::Abstract, None)
        } else if c.eat_word("class") {
            (ElementKind::Class, None)
        } else if c.eat_word("interface") {
            (ElementKind::Interface, None)
        } else if c.eat_word("enum") {
            (ElementKind::Enum, None)
        } else if c.eat_word("diamond") {
            (ElementKind::Diamond, None)
        } else if c.eat_word("map") {
            (ElementKind::Class, Some("map".to_string()))
        } else if c.eat_word("json") {
            (ElementKind::Class, Some("json".to_string()))
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
        let mut generics = None;
        // `class A<T>` or `class A <T>`.
        let mut probe = c.clone();
        probe.skip_ws();
        if probe.rest().starts_with('<') && !probe.rest().starts_with("<<") {
            c = probe;
            generics = balanced_angle(&mut c);
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
        let mut tags: Vec<String> = Vec::new();
        let mut parents: Vec<(RelationKind, String)> = Vec::new();
        loop {
            c.skip_ws();
            let r = c.rest();
            if r.starts_with("<<") {
                let Some(s) = stereo(&mut c) else {
                    return false;
                };
                // `<<Model>>, <<Record>>`, and a stray `>`: `<<Entity>>>`.
                while c.rest().starts_with('>') {
                    c.pos += 1;
                }
                let more = c.rest().trim_start().strip_prefix(',').map(str::trim_start);
                if more.is_some_and(|m| m.starts_with("<<")) {
                    c.skip_ws();
                    c.pos += 1;
                }
                if !s.is_empty() {
                    stereotype = Some(match stereotype {
                        Some(prev) => format!("{prev}, {s}"),
                        None => s,
                    });
                }
            } else if r.starts_with('#') {
                skip_token(&mut c);
            } else if r.starts_with('$') {
                // `class A $tag`: tags, for `hide $tag`.
                let start = c.pos;
                skip_token(&mut c);
                tags.push(t[start..c.pos].to_string());
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
                let Some(g) = balanced_angle(&mut c) else {
                    return false;
                };
                generics = generics.or(Some(g));
            } else if c.eat_word("extends") || c.eat_word("Extends") {
                parents.extend(
                    name_list(&mut c)
                        .into_iter()
                        .map(|p| (RelationKind::Generalization, p)),
                );
            } else if c.eat_word("implements") || c.eat_word("Implements") {
                parents.extend(
                    name_list(&mut c)
                        .into_iter()
                        .map(|p| (RelationKind::Realization, p)),
                );
            } else {
                break;
            }
        }
        // `class Pipeline;`
        let rest = c.rest().trim().trim_end_matches(';').trim_end();
        let described = rest == "[";
        let (open, inline) = if rest.is_empty() || described {
            (false, None)
        } else if rest == "{" {
            (true, None)
        } else if rest.starts_with('{') && rest.ends_with('}') {
            (false, Some(rest[1..rest.len() - 1].trim()))
        } else {
            return false;
        };

        // PlantUML knows an element by its code name, which also gives its
        // packages: the alias of `class "Shown" as code`, the name of `class
        // code as "Shown"`, or the one name. The element's name is what
        // PlantUML shows: the quoted one, or the last part of the code name.
        let (code, shown) = match &alias {
            Some(a) if alias_quoted && !name_quoted => (name.clone(), Some(a.clone())),
            Some(a) => (a.clone(), Some(name.clone())),
            None => (name.clone(), None),
        };
        // A shown name with separator lines (`Take role\n--\nExtension
        // points`): the part above the first is the name, the lines below
        // are shown as members.
        let (shown, more) = match shown.as_deref().map(split_display) {
            Some((first, rest)) => (Some(first), rest),
            None => (None, Vec::new()),
        };
        // `class C_01 { … }` after `class "Long name" as C_01` opens the same
        // element again: it is known by its code name only.
        let is_map = keyword_stereo.as_deref() == Some("map") && kind == ElementKind::Class;
        let is_json = keyword_stereo.as_deref() == Some("json") && kind == ElementKind::Class;
        // Component-style boxes (`component C`, `portin p`) and objects are
        // found anywhere first, as PlantUML declares them; classes not.
        let anywhere = self.description
            || keyword_stereo
                .as_deref()
                .is_some_and(|k| k == "object" || CONTAINERS.contains(&k) || BOXES.contains(&k));
        let id = self.declare_where(
            kind,
            &code,
            shown.as_deref(),
            stereotype.or(keyword_stereo),
            anywhere,
        );
        if !more.is_empty() {
            let el = self.doc.elements.get_mut(&id).unwrap();
            el.members.extend(more.into_iter().map(Member::plain));
        }
        if !tags.is_empty() {
            self.tags.entry(id).or_default().extend(tags);
        }
        if let Some(g) = generics {
            let inner = g.trim().strip_prefix('<').and_then(|g| g.strip_suffix('>'));
            let inner = inner.unwrap_or(&g).trim();
            if !inner.is_empty() {
                let el = self.doc.elements.get_mut(&id).unwrap();
                el.type_params = Some(inner.to_string());
            }
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
            let to = self.resolve_parent(container, &parent);
            self.add_relation(
                k,
                (Endpoint::Element(id), None),
                (to, None),
                None,
                Vec::new(),
            );
        }
        if described {
            self.mode = Some(Mode::Description(id, Vec::new()));
        } else if open && is_map {
            self.mode = Some(Mode::Map(id));
        } else if open && is_json {
            self.mode = Some(Mode::Json(id, 1));
        } else if open {
            self.mode = Some(Mode::Body { id, is_enum });
        } else if inline.is_none() {
            self.last_decl = Some((id, is_enum));
        }
        self.last_element = Some(id);
        true
    }

    /// A new note element, known by `alias` or named `Note`, `Note2`, ...
    fn new_note(&mut self, alias: Option<String>, text: &str) -> ElementId {
        let id = match alias.as_deref() {
            Some(a) => self.declare_named(ElementKind::Note, a.trim(), None, None),
            None => self.new_local(self.container(), ElementKind::Note, "Note"),
        };
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

    fn try_note(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("note") {
            return false;
        }
        // note [left|right|top|bottom] on link [: text]
        let lowered = t.to_ascii_lowercase();
        let mut probe = Cursor::new(&lowered);
        probe.eat_word("note");
        for side in ["left", "right", "top", "bottom"] {
            if probe.eat_word(side) {
                break;
            }
        }
        if probe.eat_word("on") {
            if !(probe.eat_word("link") || probe.eat("link")) {
                return false;
            }
            let mut probe = Cursor::new(&t[probe.pos..]);
            skip_decorations(&mut probe);
            if probe.eat(":") {
                let text = probe.rest().to_string();
                self.note_on_link(&text);
            } else {
                self.mode = Some(Mode::LinkNote(Vec::new()));
            }
            return true;
        }
        // note "text" as N
        if let Some(text) = c.string() {
            let alias = if c.eat_word("as") || c.eat_word("AS") || c.eat_word("As") {
                decl_name(&mut c)
            } else {
                None
            };
            skip_decorations(&mut c);
            if !c.at_end() {
                return false;
            }
            self.new_note(alias, &text);
            return true;
        }
        // note as N ... end note
        if c.eat_word("as") {
            let Some(alias) = decl_name(&mut c) else {
                return false;
            };
            skip_decorations(&mut c);
            let id = self.new_note(Some(alias), "");
            self.mode = Some(Mode::Note {
                id: Some(id),
                lines: Vec::new(),
            });
            return true;
        }
        // note left|right|top|bottom [of X] [: text], in any case.
        let side = ["left", "right", "top", "bottom"].into_iter().find(|w| {
            c.skip_ws();
            let r = c.rest();
            let hit = starts_with_word_ci(r, w);
            if hit {
                c.pos += w.len();
            }
            hit
        });
        if side.is_none() {
            return false;
        }
        let target = if c.eat_word("of") || c.eat_word("Of") || c.eat_word("OF") {
            // `of [Component]`, `of (Use case)`.
            let short = short_form(&mut c).map(|(n, _)| n);
            match short.or_else(|| ref_name(&mut c)) {
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
        // `note right of X {` … `}`: the text in braces.
        let braces = c.rest().trim() == "{";
        let text = if c.eat(":") {
            Some(c.rest().trim().replace("\\n", "\n"))
        } else if c.at_end() || braces {
            None
        } else {
            return false;
        };
        let id = self.new_note(None, text.as_deref().unwrap_or(""));
        match target {
            Some(target) => {
                let t = self.resolve_end(self.container(), &target);
                let rid = self.doc.new_relation_id();
                let pkg = self.parent_of(Endpoint::Element(id));
                self.push_decl_in(pkg, Decl::Relation(rid));
                let mut r = Relation::new(RelationKind::NoteLink, id, id);
                r.to = End::at(t);
                self.doc.relations.insert(rid, r);
            }
            // A note without `of` is about the element declared last.
            None => {
                if let Some(e) = self.last_element.filter(|e| *e != id) {
                    let rid = self.doc.new_relation_id();
                    let pkg = self.parent_of(Endpoint::Element(id));
                    self.push_decl_in(pkg, Decl::Relation(rid));
                    self.doc
                        .relations
                        .insert(rid, Relation::new(RelationKind::NoteLink, id, e));
                }
            }
        }
        if braces {
            self.mode = Some(Mode::NoteBraces(id, Vec::new()));
        } else if text.is_none() {
            self.mode = Some(Mode::Note {
                id: Some(id),
                lines: Vec::new(),
            });
        }
        true
    }

    fn try_relation(&mut self, n: usize, t: &str) -> bool {
        // `(A, B) .. C`: C is the association class of the association
        // between A and B.
        if t.starts_with('(') && t[..t.find(')').unwrap_or(t.len())].contains(',') {
            return self.try_association_class(t);
        }
        let mut c = Cursor::new(t);
        let Some(a) = self.endpoint(&mut c, true) else {
            return false;
        };
        let la = c.string();
        let Some(arrow) = arrow_token(&mut c) else {
            return false;
        };
        let q = c.string();
        let (lb, b) = match self.endpoint(&mut c, false) {
            Some(b) => (q, b),
            None => match q {
                Some(b) => (None, b),
                None => return false,
            },
        };
        // A line style after the target: `#green;text:green`, `#line.dashed`;
        // a stereotype: `A *-- B <<comp>>`, shown on the line.
        c.skip_ws();
        if c.rest().starts_with('#') {
            skip_token(&mut c);
        }
        let link_stereo = if c.rest().trim_start().starts_with("<<") {
            stereo(&mut c)
        } else {
            None
        };
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
        let name = match (name, link_stereo) {
            (None, Some(st)) => Some(format!("«{st}»")),
            (name, _) => name,
        };
        // `-[hidden]-` only steers PlantUML's layout: nothing is drawn, but
        // its ends exist.
        let hidden = arrow.to_ascii_lowercase().contains("hidden");
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
        // The ends exist (in the order written) even for a hidden line.
        let comments = if hidden {
            Vec::new()
        } else {
            std::mem::take(&mut self.comments)
        };
        let container = self.container();
        let ea = self.resolve_end(container, &a);
        let eb = self.resolve_end(container, &b);
        if hidden {
            return true;
        }
        let (mut from, mut to) = ((ea, la), (eb, lb));
        if reversed {
            std::mem::swap(&mut from, &mut to);
        }
        self.add_relation(kind, from, to, name, comments);
        true
    }

    fn try_association_class(&mut self, t: &str) -> bool {
        let Some(close) = t.find(')') else {
            return false;
        };
        let pair: Vec<String> = split_call_args(&t[1..close])
            .iter()
            .map(|a| unquote(a.trim()))
            .collect();
        let [a, b] = &pair[..] else {
            return false;
        };
        let mut c = Cursor::new(&t[close + 1..]);
        if arrow_token(&mut c).is_none() {
            return false;
        }
        let Some(class) = self.endpoint(&mut c, false) else {
            return false;
        };
        c.skip_ws();
        if !(c.at_end() || c.rest().starts_with(':')) {
            return false;
        }
        let container = self.container();
        self.association_class(container, [a.clone(), b.clone()], class);
        true
    }

    /// One end of a relation: a name, or a short form such as `[Comp]`,
    /// whose stereotype is kept for when the element is created.
    fn endpoint(&mut self, c: &mut Cursor, first: bool) -> Option<String> {
        if let Some((name, stereo)) = short_form(c) {
            self.implicit.entry(name.clone()).or_insert(stereo);
            return Some(name);
        }
        if first {
            line_end_name(c)
        } else {
            decl_name(c)
        }
    }

    /// A short form on its own: `[Comp] as C`, `(Login) as UC`, `:User:`.
    fn try_short_decl(&mut self, t: &str) -> bool {
        // `"navegador web" as nav`: a box in a description diagram.
        if t.starts_with('"') {
            let mut c = Cursor::new(t);
            let Some(shown) = c.string() else {
                return false;
            };
            if !(c.eat_word("as") || c.eat_word("AS")) {
                return false;
            }
            let Some(code) = decl_name(&mut c) else {
                return false;
            };
            skip_decorations(&mut c);
            if !c.at_end() {
                return false;
            }
            let shown = strip_format(&shown).into_owned();
            let id = self.declare_where(ElementKind::Class, &code, Some(&shown), None, true);
            self.last_element = Some(id);
            return true;
        }
        // `<> name`: an association diamond; `() name`: an interface.
        for (mark, kind) in [("<>", ElementKind::Diamond), ("()", ElementKind::Interface)] {
            if let Some(rest) = t.strip_prefix(mark) {
                let mut c = Cursor::new(rest);
                let Some(name) = decl_name(&mut c) else {
                    return false;
                };
                let alias = if c.eat_word("as") {
                    decl_name(&mut c)
                } else {
                    None
                };
                skip_decorations(&mut c);
                if !c.at_end() {
                    return false;
                }
                // `() I` is a component diagram's interface.
                let anywhere = self.description || mark == "()";
                let id = match &alias {
                    Some(a) => self.declare_where(kind, a, Some(&name), None, anywhere),
                    None => self.declare_where(kind, &name, None, None, anywhere),
                };
                self.last_element = Some(id);
                return true;
            }
        }
        let mut c = Cursor::new(t);
        let Some((name, stereo)) = short_form(&mut c) else {
            return false;
        };
        let alias = if c.eat_word("as") || c.eat_word("AS") {
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
        // `[Shown] as code`.
        let id = match &alias {
            Some(a) => self.declare_where(ElementKind::Class, a, Some(&name), stereotype, true),
            None => self.declare_where(ElementKind::Class, &name, None, stereotype, true),
        };
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
        // They only steer the layout, which fastumpl does itself.
        if t.chars().all(|c| c.is_ascii_uppercase() || c == '_') && is_layout_call(t) {
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
        // `Person(u, "User") #Grey`, `Component(c, "C") <<folder>> {`,
        // `Rel(a, b, "Calls") ' why`.
        let tail_text = rest[close + 1..].trim();
        let tail_text = match tail_text.find(" '") {
            Some(i) => &tail_text[..i],
            None if tail_text.starts_with('\'') => "",
            None => tail_text,
        };
        let mut tc = Cursor::new(tail_text.trim());
        skip_decorations(&mut tc);
        let tail = tc.rest().trim();
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
            return true;
        }
        // A procedure of a library the diagram includes and fastumpl knows,
        // or of one it does not (an include that was not read).
        let known = !self.libraries.is_empty();
        if !known && self.calls_seen.insert(name.to_string()) {
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
        // `$descr="…"`, `$techn="…"`: arguments by name.
        let named = |key: &str| {
            args.iter().find_map(|a| {
                let (k, v) = a.split_once('=')?;
                (k.trim().eq_ignore_ascii_case(key)).then(|| unquote(v.trim()))
            })
        };
        let lower = name.to_ascii_lowercase();
        // C4 dynamic diagrams: `RelIndex(1, from, to, "label")`.
        let (index, positional) = match lower.starts_with("relindex") {
            true => (
                positional.first().map(|i| unq(i)),
                positional.get(1..).unwrap_or(&[]).to_vec(),
            ),
            false => (None, positional),
        };
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
            // The label, its technology in brackets as C4 shows it, and the
            // step number of a dynamic diagram.
            let label = positional
                .get(2)
                .map(|l| unq(l))
                .or_else(|| named("$label"));
            let techn = positional
                .get(3)
                .map(|t| unq(t))
                .or_else(|| named("$techn"));
            let mut text = label.unwrap_or_default();
            if let Some(t) = techn.filter(|t| !t.trim().is_empty()) {
                text = format!("{text} [{t}]").trim().to_string();
            }
            if let Some(i) = index {
                text = format!("{i}: {text}")
                    .trim_end_matches([' ', ':'])
                    .to_string();
            }
            let comments = std::mem::take(&mut self.comments);
            let container = self.container();
            let f = self.resolve_end(container, &from);
            let t = self.resolve_end(container, &to);
            let name = Some(text).filter(|l| !l.trim().is_empty());
            self.add_relation(kind, (f, None), (t, None), name, comments);
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
        if open {
            let path = self.quark_path(self.container(), &alias);
            let pkg = self.package_at(&path, Some(display.trim()), true);
            self.stack.push(pkg);
            return true;
        }
        let id = self.declare_named(
            ElementKind::Class,
            &alias,
            Some(display.trim()),
            Some(name.to_string()),
        );
        // Technology (in brackets, as C4 shows it) and description; persons
        // and systems have a description only. Prose is wrapped as C4
        // wraps it.
        let has_techn = !(lower.starts_with("person") || lower.starts_with("system"));
        let mut extras: Vec<String> = positional
            .iter()
            .skip(2)
            .take(if has_techn { 2 } else { 1 })
            .map(|e| unquote(e.trim()))
            .collect();
        if has_techn && extras.is_empty() {
            extras.extend(named("$techn"));
        }
        if extras.len() < if has_techn { 2 } else { 1 } {
            extras.extend(named("$descr"));
        }
        let el = self.doc.elements.get_mut(&id).unwrap();
        for (k, extra) in extras.iter().enumerate() {
            let text = strip_format(extra).into_owned();
            if text.trim().is_empty() {
                continue;
            }
            if has_techn && k == 0 && positional.len() > 2 {
                el.members.push(Member::plain(format!("[{}]", text.trim())));
                continue;
            }
            for line in text.split("\\n").flat_map(|l| wrap(l.trim(), WRAP)) {
                el.members.push(Member::plain(line));
            }
        }
        self.last_element = Some(id);
        true
    }

    /// `Name <<Interface>>`: a stereotype for an element declared elsewhere.
    fn try_stereotype(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let Some(target) = ref_name(&mut c) else {
            return false;
        };
        c.skip_ws();
        if !c.rest().starts_with("<<") {
            return false;
        }
        let Some(st) = stereo(&mut c) else {
            return false;
        };
        skip_decorations(&mut c);
        if !c.at_end() {
            return false;
        }
        let e = self.resolve(self.container(), &target);
        if !st.is_empty() {
            self.doc.elements.get_mut(&e).unwrap().stereotype = Some(st);
        }
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
        let e = self.resolve(self.container(), &target);
        self.doc.elements.get_mut(&e).unwrap().members.push(member);
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
            Some(Mode::Text(what, lines)) => self.finish_text(line, what, lines),
            _ => {}
        }
        // Packages still open end with the diagram, as in PlantUML.
        // Comments after the last declaration stay at the end, not with a
        // class first named in a relation.
        self.doc.trailing_comments.append(&mut self.comments);
        self.stack.clear();

        self.apply_visibility();
        for (_, text) in std::mem::take(&mut self.legends) {
            let id = self.new_local(None, ElementKind::Note, "Legend");
            self.set_note_text(id, &text);
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

    fn association_class(
        &mut self,
        container: Option<PackageId>,
        ends: [String; 2],
        class: String,
    ) {
        let a = self.resolve_end(container, &ends[0]);
        let b = self.resolve_end(container, &ends[1]);
        let c = self.resolve_end(container, &class);
        let point = self.new_local(container, ElementKind::Diamond, "Association");
        // The last line between A and B before it, if any, goes through the
        // diamond. (Lines further down are not made yet.)
        let existing = self.doc.relations_in_order().into_iter().rev().find(|r| {
            self.doc.relations.get(r).is_some_and(|r| {
                (r.from.target, r.to.target) == (a, b) || (r.from.target, r.to.target) == (b, a)
            })
        });
        let p = Endpoint::Element(point);
        let mut new = Vec::new();
        match existing {
            Some(id) => {
                let old = self.doc.relations.remove(&id).unwrap();
                // Decorations stay at their ends: a whole-part diamond or
                // a navigable end.
                let head_at_to = matches!(
                    old.kind,
                    RelationKind::Directed
                        | RelationKind::Dependency
                        | RelationKind::Generalization
                        | RelationKind::Realization
                );
                let (k1, k2) = if head_at_to {
                    (RelationKind::Association, old.kind)
                } else if old.kind == RelationKind::Association {
                    (old.kind, old.kind)
                } else {
                    (old.kind, RelationKind::Association)
                };
                let mut first = Relation::new(k1, point, point);
                first.from = old.from.clone();
                first.to = End::at(p);
                first.name = old.name.clone();
                first.comments = old.comments.clone();
                let mut second = Relation::new(k2, point, point);
                second.from = End::at(p);
                second.to = old.to.clone();
                let ids = [self.doc.new_relation_id(), self.doc.new_relation_id()];
                self.doc.relations.insert(ids[0], first);
                self.doc.relations.insert(ids[1], second);
                let replace = |body: &mut Vec<Decl>| {
                    if let Some(i) = body.iter().position(|d| *d == Decl::Relation(id)) {
                        body.splice(i..=i, ids.iter().map(|r| Decl::Relation(*r)));
                    }
                };
                replace(&mut self.doc.root);
                for pkg in self.doc.packages.values_mut() {
                    replace(&mut pkg.body);
                }
            }
            None => {
                for (from, to) in [(a, p), (p, b)] {
                    let mut r = Relation::new(RelationKind::Association, point, point);
                    r.from = End::at(from);
                    r.to = End::at(to);
                    new.push(r);
                }
            }
        }
        let mut dashed = Relation::new(RelationKind::NoteLink, point, point);
        dashed.from = End::at(c);
        new.push(dashed);
        for r in new {
            let id = self.doc.new_relation_id();
            self.doc.relations.insert(id, r);
            self.push_decl_in(container, Decl::Relation(id));
        }
    }

    /// Leaves out the elements `hide` and `remove` name (unless `show` or
    /// `restore` brings them back later), with their relations.
    fn apply_visibility(&mut self) {
        if self.shown.is_empty() {
            return;
        }
        let linked: std::collections::HashSet<ElementId> = self
            .doc
            .relations
            .values()
            .flat_map(|r| [r.from.element(), r.to.element()])
            .flatten()
            .collect();
        let ids: Vec<ElementId> = self.doc.elements_in_order();
        let mut hidden: std::collections::HashSet<ElementId> = Default::default();
        for (_, show, target) in std::mem::take(&mut self.shown) {
            let target = target.trim();
            let matches = |id: &ElementId| -> bool {
                let el = &self.doc.elements[id];
                if target == "*" {
                    return true;
                }
                if target.eq_ignore_ascii_case("@unlinked") {
                    return !linked.contains(id);
                }
                if let Some(st) = target.strip_prefix("<<").and_then(|t| t.strip_suffix(">>")) {
                    return el.stereotype.as_deref().is_some_and(|s| {
                        s.split(',')
                            .any(|s| s.trim().eq_ignore_ascii_case(st.trim()))
                    });
                }
                if target.starts_with('$') {
                    return self
                        .tags
                        .get(id)
                        .is_some_and(|t| t.iter().any(|t| t == target));
                }
                let full = self.doc.full_name(*id);
                let code = self.quark_of.get(&Endpoint::Element(*id));
                let code_path = code.map(|p| p.join(".")).unwrap_or_default();
                let code_last = code.and_then(|p| p.last()).map_or("", String::as_str);
                let names = [
                    full.as_str(),
                    el.name.as_str(),
                    el.shown(),
                    &code_path,
                    code_last,
                ];
                names
                    .iter()
                    .any(|n| wildcard(target, n) || wildcard(target, &n.replace('.', "::")))
            };
            let picked: Vec<ElementId> = ids.iter().copied().filter(|id| matches(id)).collect();
            for id in picked {
                if show {
                    hidden.remove(&id);
                } else {
                    hidden.insert(id);
                }
            }
        }
        if hidden.is_empty() {
            return;
        }
        let rels: Vec<RelationId> = self
            .doc
            .relations
            .iter()
            .filter(|(_, r)| {
                [r.from.element(), r.to.element()]
                    .iter()
                    .flatten()
                    .any(|e| hidden.contains(e))
            })
            .map(|(id, _)| *id)
            .collect();
        for r in &rels {
            self.doc.relations.remove(r);
        }
        for e in &hidden {
            self.doc.elements.remove(e);
        }
        let gone = |d: &Decl| match d {
            Decl::Element(e) => hidden.contains(e),
            Decl::Relation(r) => rels.contains(r),
            _ => false,
        };
        self.doc.root.retain(|d| !gone(d));
        for p in self.doc.packages.values_mut() {
            p.body.retain(|d| !gone(d));
        }
    }
}

/// `end note` or `endnote`.
fn is_end(t: &str, word: &str) -> bool {
    let t = t.to_ascii_lowercase();
    t == format!("end {word}") || t == format!("end{word}")
}

/// `--`, `..`, `==`, `__` separator lines inside a class body.
/// A separator line in a box: `--`, `==`, `..` or `__`, alone or around a
/// title (`-- Getters --`), as PlantUML has them. `==Heading` is text.
fn is_separator(t: &str) -> bool {
    let t = t.trim();
    t != "..."
        && ["--", "..", "==", "__"]
            .iter()
            .any(|s| t.starts_with(s) && t.ends_with(s))
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
    // Creole bold, `**text**`: only when the marks pair up.
    let bold = t.matches("**").count();
    if bold >= 2 && bold.is_multiple_of(2) {
        return strip_format(&t.replace("**", "")).into_owned().into();
    }
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
    scan_name(c, false)
}

/// A name at the start of a line between two elements. As PlantUML reads
/// it, the name takes `-word` while an arrow still follows:
/// `C-left->D` is a line from `C-left`, `Q-down-R` one from `Q-down`.
fn line_end_name(c: &mut Cursor) -> Option<String> {
    scan_name(c, true)
}

fn scan_name(c: &mut Cursor, before_arrow: bool) -> Option<String> {
    c.skip_ws();
    if c.rest().starts_with('"') {
        // `class " " as Spacer`: a blank name is a name.
        return c.string().filter(|s| !s.is_empty());
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
            // `arraysx.*` (Go packages).
            || (*ch == '*' && k > 0 && chars[k - 1].1 == '.')
            || (*ch == '.' && next == Some('*'))
            // `::BitReader` from the top.
            || (*ch == ':' && k == 0 && next == Some(':'))
            || (*ch == ':' && k == 1 && chars[0].1 == ':')
            // `UserManager~TUser~`, `@login_required`: PlantUML keeps them.
            || *ch == '~'
            || (*ch == '@' && k == 0)
            || (*ch == '.' && joins(next))
            || (*ch == '/' && k > 0 && joins(next))
            // `CompositeType,EnumType` is one name.
            || (*ch == ',' && k > 0 && joins(next))
            // `service-subscriptions`, not the arrow of `A-up->B`.
            || (*ch == '-' && k > 0 && dash_joins(&chars[k + 1..], before_arrow))
            // `Vendor\\Pkg\\Class` with `set namespaceSeparator \\`.
            || (*ch == '\\' && (joins(next) || next == Some('\\')));
        if !ok {
            break;
        }
        end = i + ch.len_utf8();
    }
    if end == 0 {
        return None;
    }
    // `collection[T]` (Go): the brackets are part of the name.
    if r[end..].starts_with('[') && !r[end..].starts_with("[[") {
        if let Some(close) = r[end..].find(']') {
            if !r[end + 1..end + close].contains([' ', '"']) {
                end += close + 1;
            }
        }
    }
    // `class Shape()`, `class ParseError(ValueError)`: the parentheses are
    // part of the name.
    if r[end..].starts_with('(') {
        if let Some(close) = r[end..].find(')') {
            let inner = &r[end + 1..end + close];
            let after = &r[end + close + 1..];
            if !inner.contains(['"', '(', ',']) && !after.starts_with(['-', '.']) {
                end += close + 1;
            }
        }
    }
    c.pos += end;
    // `.a.b.C` names `a.b.C` from the top (see `Importer::quark_path`);
    // `::a::C` names `a::C`.
    let name = &r[..end];
    let name = name.strip_prefix("::").unwrap_or(name);
    Some(name.to_string()).filter(|s| !s.is_empty() && s != ".")
}

/// Does a `-` inside a name continue it: `service-subscriptions`,
/// `im-a-teapot`? Not when it starts an arrow (`A-up->B`, `A--B`).
fn dash_joins(after: &[(usize, char)], before_arrow: bool) -> bool {
    let word: String = after
        .iter()
        .map(|(_, c)| *c)
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if word.is_empty() {
        return false;
    }
    let n = word.chars().count();
    if before_arrow {
        // Only while what follows can still start the line: `-word->`,
        // `-word "1" -->`, `-word <|--`, not `-word` at the end or before
        // the label.
        let tail: String = after[n..].iter().map(|(_, c)| *c).collect();
        return tail.trim_start().starts_with([
            '-', '.', '=', '<', '>', 'o', '*', '+', '#', 'x', '}', '|', '"', '[',
        ]);
    }
    match after.get(n).map(|(_, c)| *c) {
        // `a-b-c`: on if a name follows the next dash too.
        Some('-') => dash_joins(&after[n + 1..], false),
        Some(c) => !"<>[|*".contains(c),
        None => true,
    }
}

/// A name used in a relation: like [`decl_name`]. `A::b` is kept whole:
/// it names `b` in namespace `A`, or member `b` of class `A`, which only
/// resolving can tell.
fn ref_name(c: &mut Cursor) -> Option<String> {
    decl_name(c)
}

/// Removes `/' comments '/` that end on the line they start, after some
/// text (a line that starts with one is a comment block).
fn strip_inline_comments(t: &str) -> std::borrow::Cow<'_, str> {
    if t.trim_start().starts_with("/'") || !t.contains("/'") {
        return t.into();
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    let mut quote = false;
    while !rest.is_empty() {
        if !quote && rest.starts_with("/'") {
            match rest[2..].find("'/") {
                Some(end) => {
                    rest = &rest[2 + end + 2..];
                    continue;
                }
                None => break,
            }
        }
        let ch = rest.chars().next().unwrap();
        if ch == '"' {
            quote = !quote;
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out.trim_end().to_string().into()
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

/// A shown name with separator lines (`Take role\n--\nExtension points`):
/// the lines above the first separator (the name, which may span lines)
/// and the other lines below, without separators. A name without
/// separators is kept whole.
fn split_display(shown: &str) -> (String, Vec<String>) {
    if shown.trim().is_empty() {
        return (shown.to_string(), Vec::new());
    }
    let lines: Vec<&str> = shown.lines().map(str::trim).collect();
    match lines.iter().position(|l| is_separator(l)) {
        None => (shown.trim().to_string(), Vec::new()),
        Some(k) => {
            let name: Vec<&str> = lines[..k]
                .iter()
                .copied()
                .filter(|l| !l.is_empty())
                .collect();
            let rest = lines[k..]
                .iter()
                .filter(|l| !l.is_empty() && !is_separator(l))
                .map(|l| l.to_string())
                .collect();
            (name.join("\n"), rest)
        }
    }
}

/// A package name PlantUML takes without quotes: anything up to a space,
/// `{`, `<<` or `#`: `go\\core\\vm`, `org.apache.*`.
fn package_word(c: &mut Cursor) -> Option<String> {
    c.skip_ws();
    let r = c.rest();
    let end = r
        .find(|ch: char| ch.is_whitespace() || ch == '{' || ch == '#')
        .unwrap_or(r.len());
    let end = r[..end].find("<<").unwrap_or(end);
    if end == 0 {
        return None;
    }
    c.pos += end;
    Some(r[..end].to_string())
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
    // `(*)` is an activity start. (`(A, B) .. C` at the start of a line is
    // an association class, read before this.)
    if inner.is_empty() || inner == "*" {
        return None;
    }
    // `:Actor:`, not a label `: rationalCoeffs: one…`.
    if open == ':' && (inner.contains(['"', '<']) || r[1..].starts_with(char::is_whitespace)) {
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
    // `<<BaseDTO<UUID>>>`: brackets inside the stereotype.
    c.skip_ws();
    let r = c.rest();
    if let Some(inner) = r.strip_prefix("<<") {
        let mut depth = 0i32;
        for (i, ch) in inner.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' if depth > 0 => depth -= 1,
                '>' if inner[i..].starts_with(">>") => {
                    let s = inner[..i].trim().to_string();
                    let has_inner = s.contains('<');
                    if has_inner {
                        c.pos += 2 + i + 2;
                        return Some(s);
                    }
                    break;
                }
                _ => {}
            }
        }
    }
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
    let mut len = r
        .find(|ch: char| ch.is_whitespace() || ch == '{')
        .unwrap_or(r.len());
    // `#000000: label`, but `#green;text:green` is one token.
    if let Some(i) = r[..len].find(':').filter(|i| *i + 1 == len) {
        len = i;
    }
    // `#red;line.dotted[`: the description starts (but `##[bold]red` is
    // a border style).
    if r[..len].ends_with('[') && !r[..len].ends_with("##[") {
        len -= 1;
    }
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
        } else if c.rest().starts_with('#') || c.rest().starts_with('$') {
            // A colour, or a `$tag`.
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
    // `=` draws a thick line, `~` a dotted one: `<===>`, `~~>`.
    let is_line = |ch: char| ch == '-' || ch == '.' || ch == '=' || ch == '~';
    let mut k = 0;
    let mut seen_line = false;
    while k < chars.len() {
        let ch = chars[k].1;
        if is_line(ch) || "<>|*#+^}{".contains(ch) {
            seen_line |= is_line(ch);
            k += 1;
        } else if (ch == '(' || ch == ')')
            && (k > 0 && (is_line(chars[k - 1].1) || chars[k - 1].1 == '(')
                || chars.get(k + 1).is_some_and(|(_, c)| is_line(*c))
                || (ch == '(' && chars.get(k + 1).is_some_and(|(_, c)| *c == ')')))
        {
            // `-()`, `-(`, `)-`: interface marks of component diagrams.
            k += 1;
        } else if ch == '[' {
            // `-[#red,dashed]->`, `-down[hidden]-`: a style inside the arrow,
            // not the `[Component]` after it.
            let inside = k > 0 && (is_line(chars[k - 1].1) || chars[k - 1].1.is_alphabetic());
            match chars[k..].iter().position(|(_, c)| *c == ']') {
                Some(j) if inside => k += j + 1,
                _ => break,
            }
        } else if ch.is_alphabetic() {
            let start = k;
            while k < chars.len() && chars[k].1.is_alphabetic() {
                k += 1;
            }
            let word: String = chars[start..k]
                .iter()
                .map(|(_, c)| c.to_ascii_lowercase())
                .collect();
            // After a line or a `[style]`: `-[#red]o{`, `.U.|>`.
            let before = start > 0 && (is_line(chars[start - 1].1) || chars[start - 1].1 == ']');
            let after = chars
                .get(k)
                .is_some_and(|(_, c)| is_line(*c) || "<>|*[(".contains(*c));
            // `-right "*" B`, `-left B`: a direction ends the arrow (with
            // a target after it: `Q-down-R` is a line to `R`).
            let ends = chars
                .get(k)
                .is_some_and(|(_, c)| c.is_whitespace() || *c == '"');
            let head = (word == "o" || word == "x") && (before || after);
            let direction = matches!(
                word.as_str(),
                "up" | "down" | "left" | "right" | "u" | "d" | "l" | "r" | "do" | "le" | "ri"
            ) && seen_line
                && (after || (before && ends));
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
    /// `+`: a circled plus, the outer end of a nesting line.
    Plus,
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
        "}" => Some("*"),
        _ => None,
    }
}

/// The crow's-foot multiplicities at the left and right end of `arrow`.
fn crows_feet(arrow: &str) -> (Option<&'static str>, Option<&'static str>) {
    let is_line = |c: char| c == '-' || c == '.' || c == '=';
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
    // Direction words (`-up->`, `-r|>`, `.left.`) only steer PlantUML's
    // layout; `o` and `x` are ends.
    let plain = {
        let mut out = String::new();
        let mut word = String::new();
        for ch in plain.chars().chain(std::iter::once(' ')) {
            if ch.is_alphabetic() {
                word.push(ch);
                continue;
            }
            if word == "o" || word == "x" {
                out.push_str(&word);
            }
            // (Any other word is a direction: `up`, `U`, `left`.)
            word.clear();
            if ch != ' ' {
                out.push(ch);
            }
        }
        out
    };
    let is_line = |c: char| c == '-' || c == '.' || c == '=' || c == '~';
    let first = plain.find(is_line).unwrap_or(0);
    let last = plain.rfind(is_line).unwrap_or(0);
    let left = &plain[..first];
    let right = &plain[last + 1..];
    dashed |= plain[first..=last].contains(['.', '~']);

    let mut unknown = None;
    let mut feet = false;
    let mut head = |s: &str, is_left: bool| match s {
        "" => Head::None,
        "<|" | "|>" | "^" | "<||" | "||>" => Head::Triangle,
        // `\\` and `//` are half arrow heads.
        "<" | ">" | "<<" | ">>" | "\\" | "//" | "\\\\" | "////" => Head::Arrow,
        "*" => Head::Diamond,
        "o" => Head::Hollow,
        "+" => Head::Plus,
        // `x` marks an end that is not navigable, which an association
        // without an arrow head there already says; a square end has no
        // meaning in UML; `()` and `(` are the provided and required
        // interface marks of component diagrams, on a plain line.
        "x" | "#" | "()" | "(" | ")" | "0" | "0)" | "(0" => Head::None,
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
        (Head::Plus, _) => (Nesting, false),
        (_, Head::Plus) => (Nesting, true),
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

/// Does `t` start with the word `w` (ASCII, any case), not followed by a
/// letter or digit?
fn starts_with_word_ci(t: &str, w: &str) -> bool {
    t.get(..w.len()).is_some_and(|p| p.eq_ignore_ascii_case(w))
        && !t[w.len()..].starts_with(|ch: char| ch.is_alphanumeric())
}

/// Does `name` match `pattern`, where `*` stands for any text?
fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim_matches('"');
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
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
            .find(|r| r.from.element() == Some(f) && r.to.element() == Some(t))
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

        let po = el(d, "\"Purchase order\"");
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
        let comp = rel(d, "\"Purchase order\"", "OrderLine");
        assert_eq!(comp.kind, RelationKind::Composition);
        assert_eq!(comp.from.label.as_deref(), Some("1"));
        assert_eq!(comp.to.label.as_deref(), Some("1..*"));
        assert_eq!(comp.name.as_deref(), Some("contains"));
        assert_eq!(
            rel(d, "\"Purchase order\"", "Priced").kind,
            RelationKind::Realization
        );
        let paid = rel(d, "\"Purchase order\"", "com.shop.Payment");
        assert_eq!(paid.kind, RelationKind::Dependency);
        assert_eq!(paid.name.as_deref(), Some("paid by"));
        // `(PO, Customer) .. Status`: the line between them goes through a
        // diamond that Status hangs from, as PlantUML draws it.
        let split = rel(d, "Customer", "Association");
        assert_eq!(split.from.label.as_deref(), Some("1"));
        assert_eq!(
            rel(d, "Association", "\"Purchase order\"")
                .to
                .label
                .as_deref(),
            Some("*")
        );
        assert_eq!(rel(d, "Status", "Association").kind, RelationKind::NoteLink);
        assert_eq!(el(d, "Association").kind, ElementKind::Diamond);
        assert_eq!(rel(d, "Customer", "Status").kind, RelationKind::Association);
        assert_eq!(rel(d, "OrderLine", "Money").kind, RelationKind::Directed);

        let n1 = el(d, "N1");
        assert_eq!(n1.kind, ElementKind::Note);
        assert_eq!(n1.members.len(), 2);
        assert_eq!(
            rel(d, "N1", "\"Purchase order\"").kind,
            RelationKind::NoteLink
        );
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
        // Styling and layout hints are skipped without a problem.
        assert!((6..=13).all(|l| at(l).is_empty()), "{messages:?}");
        assert_eq!(el(d, "Priced").type_params.as_deref(), Some("T"));
        assert!(messages.is_empty(), "{messages:?}");
        // `note on link` labels the line before it.
        assert_eq!(
            rel(d, "N1", "\"Purchase order\"").name.as_deref(),
            Some("skipped")
        );
    }

    #[test]
    fn a_second_start_begins_the_diagram_again() {
        let src = "@startuml\nclass Lost\n@startuml\nclass Kept\n@enduml\nclass After\n";
        assert_eq!(diagrams(src).len(), 1);
        let p = import(src);
        assert_eq!(names(&p.doc), vec!["Kept".to_string()]);
    }

    #[test]
    fn each_diagram_and_page_is_listed_and_read_on_its_own() {
        let src = "@startuml first\n!define K class\nK A\nnewpage\ntitle Second page\nK B\n@enduml\n\n@startuml\ntitle Talk\nAlice -> Bob : hi\n@enduml\n@startuml\nclass C\n@enduml\n";
        let list = diagrams(src);
        let shown: Vec<(Option<&str>, DiagramKind, usize)> = list
            .iter()
            .map(|d| (d.name.as_deref(), d.kind, d.line))
            .collect();
        assert_eq!(
            shown,
            [
                (Some("first, page 1"), DiagramKind::Class, 1),
                (Some("first, page 2"), DiagramKind::Class, 4),
                (Some("Talk"), DiagramKind::Sequence, 9),
                (None, DiagramKind::Class, 13),
            ]
        );
        let names = |n: usize| -> Vec<String> {
            let p = import_nth(src, None, &|_| None, n);
            assert!(
                p.problems.iter().all(|p| p.message.starts_with("this is")),
                "{:?}",
                p.problems
            );
            names(&p.doc)
        };
        // Macros defined on page 1 work on page 2, as in PlantUML.
        assert_eq!(names(0), ["A"]);
        assert_eq!(names(1), ["B"]);
        assert!(names(2).is_empty());
        assert_eq!(names(3), ["C"]);
        assert_eq!(diagrams("class X\n").len(), 1);
    }

    #[test]
    fn maps_descriptions_diamonds_and_interfaces() {
        let src = "map \"Config\" as cfg {\n  port => 8080\n  db *-> Store\n}\nnode N [\n  Line one\n  ---\n  Line two\n]\nN --> cfg\n<> D\nA -- D\n() \"Api\" as api\nA -() api\nclass X\nclass Y\nX -down[hidden]- Hid\nX -right \"*\" Y\n";
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        let cfg = el(d, "Config");
        let rows: Vec<&str> = cfg.members.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(rows, ["port => 8080", "db"]);
        rel(d, "Config", "Store");
        rel(d, "\"Line one\\nLine two\"", "Config");
        assert_eq!(el(d, "D").kind, ElementKind::Diamond);
        assert_eq!(el(d, "Api").kind, ElementKind::Interface);
        rel(d, "A", "Api");
        // A hidden line is not drawn, but its ends exist.
        el(d, "Hid");
        assert_eq!(rel(d, "X", "Y").to.label.as_deref(), Some("*"));
        assert_eq!(d.relations.len(), 5);
    }

    #[test]
    fn separators_described_packages_and_old_line_ends() {
        let p = import("set namespaceSeparator \\\\\nclass Vendor\\\\Pkg\\\\Item\nVendor\\\\Pkg\\\\Item --> Other\n");
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        el(&p.doc, "Vendor.Pkg.Item");
        let p = import("rectangle R {\n  package Signup [\n    **ID:** UC1\n  ]\n}\n");
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let signup = el(&p.doc, "R.\"ID: UC1\"");
        assert_eq!(signup.stereotype.as_deref(), Some("package"));
        let p = import("@startuml\rclass A\rA --> B\r@enduml\r");
        assert_eq!(names(&p.doc).len(), 2);
    }

    #[test]
    fn plantuml_name_and_line_forms() {
        let src = r#"!include <C4/C4_Container>
class Film <<Model>>, <<Record>> {
}
class service-subscriptions
class UserManager~TUser~
class ParseError(ValueError) { }
class Pipeline;
abstract RawNode  /' extends Node2D '/
CompositeType,EnumType <|-- TypeComment
entity Header <<entity>>>
EUCProtocol <<Interface>>
PSS <---- EEI_PS #000000: Plan
A .U.|> B
C <===> D : thick
E ||--[#1A7FBF]o{ F
G -[#6B21A8,dashed]->   H
json "Opposes" as c18 {
  "a": [1, 2],
  "b": { "c": 3 }
}
rectangle {
  class InFrame
}
Person(u, "User") #Grey
note Left of Film : n
A --> B
note on link: why
"#;
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        assert_eq!(el(d, "Film").stereotype.as_deref(), Some("Model, Record"));
        for n in [
            "\"service-subscriptions\"",
            "\"UserManager~TUser~\"",
            "\"ParseError(ValueError)\"",
            "Pipeline",
            "RawNode",
            "Header",
        ] {
            el(d, n);
        }
        assert_eq!(el(d, "RawNode").kind, ElementKind::Abstract);
        rel(d, "TypeComment", "\"CompositeType,EnumType\"");
        assert_eq!(
            el(d, "EUCProtocol").stereotype.as_deref(),
            Some("Interface")
        );
        assert_eq!(rel(d, "EEI_PS", "PSS").name.as_deref(), Some("Plan"));
        assert_eq!(rel(d, "A", "B").kind, RelationKind::Realization);
        rel(d, "C", "D");
        assert_eq!(rel(d, "E", "F").to.label.as_deref(), Some("0..*"));
        rel(d, "G", "H");
        assert_eq!(el(d, "Opposes").members.len(), 2);
        el(d, "\" \".InFrame");
        el(d, "User");
        let why = d
            .relations
            .values()
            .filter(|r| r.name.as_deref() == Some("why"))
            .count();
        assert_eq!(why, 1);
    }

    #[test]
    fn more_forms_plantuml_accepts() {
        let src = "abstract Class View{\n}\nstatic class Log {\n}\nclass C Implements I{\n}\nstereotype Cfg {\n}\nmix_actor Bob\n\"**web browser**\" as web\nnote right of Log{\n  logs\n}\nnote \"n\" AS N\nShared.Region\nfile x #red;line.dotted[\n  shown\n]\npackage {\n  class InIt\n}\nfooter \"Page 1\"\ncaption \"Fig\\n1\"\n";
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        assert_eq!(el(d, "View").kind, ElementKind::Abstract);
        el(d, "Log");
        rel(d, "C", "I");
        assert_eq!(el(d, "Cfg").stereotype.as_deref(), Some("stereotype"));
        assert_eq!(el(d, "Bob").stereotype.as_deref(), Some("actor"));
        el(d, "\"web browser\"");
        assert_eq!(rel(d, "Note", "Log").kind, RelationKind::NoteLink);
        rel(d, "Shared", "egion");
        el(d, "shown");
        el(d, "\" \".InIt");
        assert_eq!(d.trailing_comments, ["footer: Page 1", "caption: Fig", "1"]);
    }

    #[test]
    fn last_corpus_forms() {
        // An association class before more lines.
        let p = import("A -- B\n(A, B) .. C\nA --> D\n");
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        assert_eq!(p.doc.relations.len(), 4);
        let src = "class SwapChain #pink ##[bold]red\nenum E #ece4ff ##[bold] {\n  A\n}\nConsumable <|.. (Pot, Scroll)\nclass arraysx.* << (G,DarkSeaGreen) >> {\n}\nBlock o--- ::BitReader\nclass collection[T] {\n}\nCLI ()-- i\nPayment ()- Escrow\nM -||> N\ntraefik ~~~> web : \"port 80\"\n=======\n\"Setting\" ..> \"User\" : rationalCoeffs: one\n:Any\\nUser: AS U\n";
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        el(d, "SwapChain");
        assert_eq!(el(d, "E").members.len(), 1);
        rel(d, "\"Pot, Scroll\"", "Consumable");
        el(d, "arraysx.\"*\"");
        rel(d, "Block", "BitReader");
        el(d, "\"collection[T]\"");
        rel(d, "CLI", "i");
        rel(d, "Payment", "Escrow");
        assert_eq!(rel(d, "M", "N").kind, RelationKind::Generalization);
        assert_eq!(rel(d, "traefik", "web").kind, RelationKind::Dependency);
        assert_eq!(
            rel(d, "Setting", "User").name.as_deref(),
            Some("rationalCoeffs: one")
        );
    }

    #[test]
    fn lines_may_join_packages() {
        let src = "package p1 {\n  class X\n}\npackage p2 {\n}\np1 --> p2\nX --> p2\nclass A\nA --> B\npackage A {\n  class Q\n}\n";
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        let pkg = |n: &str| Endpoint::Package(d.find_package(n).unwrap());
        let ends: Vec<(Endpoint, Endpoint)> = d
            .relations_in_order()
            .iter()
            .map(|r| (d.relations[r].from.target, d.relations[r].to.target))
            .collect();
        let el = |n: &str| Endpoint::Element(d.find(n).unwrap());
        // `class A` became package `A`, as in PlantUML.
        assert_eq!(
            ends,
            [
                (pkg("p1"), pkg("p2")),
                (el("p1.X"), pkg("p2")),
                (pkg("A"), el("B"))
            ]
        );
        assert!(d.find("A").is_none());
        el("A.Q");
    }

    #[test]
    fn hidden_elements_are_left_out_and_legends_become_notes() {
        let src = "class A\nclass B <<internal>>\nclass C $draft\nclass Lone\nclass util.Helper\nclass util.Log\nA --> B\nA --> C\nA --> util.Helper\nA --> util.Log\nhide empty members\nhide <<internal>> stereotype\nhide <<internal>>\nremove $draft\nhide @unlinked\nremove util.*\nrestore util.Log\nlegend right\n  <b>A</b> is the root\n  B is internal\nendlegend\ncaption Figure 1\n";
        let p = import(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        let mut names = names(d);
        names.sort();
        assert_eq!(names, ["A", "Legend", "util.Log"]);
        assert_eq!(d.relations.len(), 1);
        let legend = el(d, "Legend");
        assert_eq!(legend.kind, ElementKind::Note);
        let text: Vec<&str> = legend.members.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(text, ["A is the root", "B is internal"]);
        assert_eq!(d.trailing_comments, ["caption: Figure 1"]);
        assert!(wildcard("cds.*impl*", "cds.x.impl.y") && !wildcard("a*b", "ab_c"));
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
            ("+--", Nesting, false),
            ("--+", Nesting, true),
            ("-up|>", Generalization, false),
            ("-r>", Directed, false),
            ("<-right-", Directed, true),
            ("x-->", Directed, false),
            ("<--x", Directed, true),
            ("x--", Association, false),
            ("x--x", Association, false),
            ("#--", Association, false),
            ("#..", NoteLink, false),
            ("^--", Generalization, true),
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
        assert_eq!(strip_format("**ID:** UC1 **x**"), "ID: UC1 x");
        assert_eq!(strip_format("a ** b"), "a ** b");
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
        assert_eq!(rel(d, "A", "B").kind, RelationKind::Nesting);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
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
        let src = "package billing {\n  class Invoice\n  Invoice --> Line\n}\nOrder --> Invoice\nclass L as \"Long name\"\nL --> L2\nclass \"Shown\" as S\nS --> L\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![]);
        // Line is created where it is first used, inside billing.
        rel(d, "billing.Invoice", "billing.Line");
        rel(d, "Order", "billing.Invoice");
        // Names are what PlantUML shows; code names refer to them.
        rel(d, "\"Long name\"", "L2");
        rel(d, "Shown", "\"Long name\"");
        assert_eq!(d.elements.len(), 6);
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
        let p = import("class A {\n  +x: int\n}\n??? what\npackage p {\nclass B {\n");
        assert!(p.doc.root.contains(&Decl::Raw("??? what".into())));
        // An open package ends with the diagram, as in PlantUML; an open
        // class body is a problem.
        assert_eq!(p.problems.len(), 2, "{:?}", p.problems);
        assert_eq!(p.problems[0].line, 4);
        assert!(p.problems[1].message.contains("missing"));
    }

    #[test]
    fn import_of_garbage_never_panics() {
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        // Characters, multibyte text, and words that steer the reader.
        let tokens: Vec<&str> = "a b { } < > | * o . - \" ' : # ( ) [ ] @ , ; ! \n \n \n é 日本 ß \u{feff} \r :: .a -left-> \
             class package object node file as <<s>> \\ = ~ hide remove set\u{20}namespaceSeparator\u{20}::"
            .split(' ')
            .collect();
        for _ in 0..2000 {
            let len = (rng % 60) as usize;
            let s: String = (0..len)
                .map(|_| {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let t = tokens[(rng % tokens.len() as u64) as usize];
                    if rng.is_multiple_of(3) {
                        format!("{t} ")
                    } else {
                        t.to_string()
                    }
                })
                .collect();
            let p = import(&s);
            // The result is always a document the printer can write and
            // read back.
            let text = crate::print(&p.doc);
            let _ = parse(&text);
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
        assert_eq!(stereo("\"Web Server\".Frontend"), "component");
        assert_eq!(stereo("\"Web Server\".Api"), "component");
        assert_eq!(stereo("Db"), "database");
        assert_eq!(stereo("Shop.Buy"), "usecase");
        assert_eq!(stereo("Shop.Pay"), "usecase");
        assert_eq!(stereo("Mail"), "component");
        assert_eq!(d.packages.len(), 2);
        // As in PlantUML: `(Buy)` and `[Frontend]` name by the shown text,
        // which the boxes declared with `as` are not known by, so they are
        // new boxes; `[Api]` outside any package is the first `Api`.
        rel(d, "User", "Buy");
        rel(d, "Admin", "Shop.Buy");
        rel(d, "Frontend", "\"Web Server\".Api");
        assert_eq!(
            rel(d, "\"Web Server\".Frontend", "Db").name.as_deref(),
            Some("reads")
        );
        assert_eq!(d.elements.len(), 10);
    }

    /// Each case as PlantUML 1.2026 reads it (checked with its own parser):
    /// the full names of the elements made and the lines between them.
    #[test]
    fn names_are_found_as_plantuml_finds_them() {
        // Source, the elements made, and the lines between them.
        type Case = (
            &'static str,
            &'static [&'static str],
            &'static [(&'static str, &'static str)],
        );
        let cases: [Case; 14] = [
            // A dotted name inside a package is inside it, unless its first
            // part is something at the top.
            (
                "package a {\n  class b.C\n  class \"d.E\"\n  b.C --> x.Y\n}\nb.C --> F\na.b.C --> G\n",
                &["a.b.C", "a.d.E", "a.x.Y", "b.C", "F", "G"],
                &[("a.b.C", "a.x.Y"), ("b.C", "F"), ("a.b.C", "G")],
            ),
            (
                "class p.Q\npackage a {\n  class p.Q\n  class .p.R\n  p.Q --> Z\n}\n",
                &["p.Q", "p.R", "a.Z"],
                &[("p.Q", "a.Z")],
            ),
            (
                "package a {\n  class X\n}\npackage b {\n  class a.X\n  class a.Y\n}\n",
                &["a.X", "a.Y"],
                &[],
            ),
            (
                "package a {\n  package b {\n    class C\n  }\n  package c {\n    b.C --> Q\n  }\n}\n",
                &["a.b.C", "a.c.b.C", "a.c.Q"],
                &[("a.c.b.C", "a.c.Q")],
            ),
            // A plain name: in the package, else the one so named anywhere,
            // else a new one in the package (enclosing packages are not
            // looked in).
            (
                "class X\npackage a {\n  class X\n  package c {\n    X --> Q\n  }\n}\n",
                &["X", "a.X", "a.c.X", "a.c.Q"],
                &[("a.c.X", "a.c.Q")],
            ),
            (
                "package a {\n  class Obs\n}\npackage b {\n  X --> Obs\n}\n",
                &["a.Obs", "b.X"],
                &[("b.X", "a.Obs")],
            ),
            // A parent after `implements` is only looked for in the package.
            (
                "package a {\n  class Obs\n}\npackage b {\n  class X implements Obs\n}\n",
                &["a.Obs", "b.X", "b.Obs"],
                &[("b.X", "b.Obs")],
            ),
            // Names are found when the line is read: B is made at the top
            // before the package declares its own.
            (
                "A --> B\npackage p {\n  class B\n}\n",
                &["A", "B", "p.B"],
                &[("A", "B")],
            ),
            // A leading `.` names from the top.
            (
                "package M {\n  class S\n  class I\n  .S <|-- .I\n}\n",
                &["M.S", "M.I", "S", "I"],
                &[("I", "S")],
            ),
            // A class on the way of a dotted name stays a class; declaring
            // a package of its name makes it the package.
            (
                "package p {\n  class A\n}\np.A.B --> C\nclass D\npackage D {\n  class X\n}\n",
                &["p.A", "p.B", "C", "D.X"],
                &[("p.B", "C")],
            ),
            // `-word` goes with the name before a line while an arrow
            // still follows.
            (
                "A-up->B\nC-left->D\nQ-down-R\nO -left-> P\nX --> service-x\n",
                &["\"A-up\"", "B", "\"C-left\"", "D", "\"Q-down\"", "R", "O", "P", "X", "\"service-x\""],
                &[
                    ("\"A-up\"", "B"),
                    ("\"C-left\"", "D"),
                    ("\"Q-down\"", "R"),
                    ("O", "P"),
                    ("X", "\"service-x\""),
                ],
            ),
            // So does `object X` (a class a line made stays a class).
            (
                "package p {\n  A --> B\n}\npackage q {\n  object B\n  class C\n}\n",
                &["p.A", "p.B", "q.C"],
                &[("p.A", "p.B")],
            ),
            // A component diagram declares an element again where it is.
            (
                "package a {\n  file x\n}\npackage b {\n  file x\n  [y]\n}\n",
                &["a.x", "b.y"],
                &[],
            ),
            // A component diagram does not split names at dots.
            (
                "package core {\n  file \"Engine.ts\"\n  [Api.ts] --> \"Engine.ts\"\n}\n",
                &["core.\"Engine.ts\"", "core.\"Api.ts\""],
                &[("core.\"Api.ts\"", "core.\"Engine.ts\"")],
            ),
        ];
        for (src, elements, lines) in cases {
            let p = import(src);
            let d = &p.doc;
            assert_eq!(p.problems, vec![], "{src}");
            let mut got = names(d);
            got.sort();
            let mut want: Vec<String> = elements.iter().map(|e| e.to_string()).collect();
            want.sort();
            assert_eq!(got, want, "{src}");
            assert_eq!(d.relations.len(), lines.len(), "{src}");
            for (from, to) in lines {
                rel(d, from, to);
            }
        }
    }

    #[test]
    fn boxes_showing_the_same_text_stay_apart() {
        // What PlantUML makes of it: F, G and Foo are three classes all
        // shown as "Foo"; p.B and p.Bar two shown as "Bar".
        let src = "class \"Foo\" as F\nclass \"Foo\" as G\nFoo --> F\npackage p {\n  class \"Bar\" as B\n  Bar --> B\n}\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![], "{:?}", p.problems);
        assert_eq!(d.elements.len(), 5, "{:?}", names(d));
        for (name, label) in [("F", Some("Foo")), ("G", Some("Foo")), ("Foo", None)] {
            assert_eq!(el(d, name).label.as_deref(), label, "{name}");
            assert_eq!(el(d, name).title(), "Foo");
        }
        rel(d, "Foo", "F");
        assert_eq!(el(d, "p.B").label.as_deref(), Some("Bar"));
        rel(d, "p.Bar", "p.B");
        // Written as `.umpl` and read back, the labels are kept.
        let text = crate::print(d);
        let back = parse(&text);
        assert_eq!(back.problems, vec![], "{text}");
        let back = back.doc;
        assert_eq!(el(&back, "G").label.as_deref(), Some("Foo"), "{text}");
    }

    #[test]
    fn keywords_in_any_case_and_names_over_several_lines() {
        let src = "Title Shop\nClass Order\nNote right of Order\n  paid\nend Note\nusecase Take as \"Take role\n--\nExtension points\n\"\nOrder --> Take\n";
        let p = import(src);
        let d = &p.doc;
        assert_eq!(p.problems, vec![], "{:?}", p.problems);
        assert_eq!(d.title.as_deref(), Some("Shop"));
        let take = el(d, "\"Take role\"");
        assert_eq!(take.members.len(), 1);
        assert_eq!(take.members[0].text, "Extension points");
        rel(d, "Order", "\"Take role\"");
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
util::Shape --> util::geometry::Kind
component Main <<FXML>> as View
View ..> util::Shape
"#;
        let p = import(src);
        let d = &p.doc;
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let cam = el(d, "util.render.Camera");
        assert_eq!(cam.members.len(), 1);
        assert_eq!(cam.members[0].to_line(), "+zoom : float");
        assert_eq!(el(d, "util.Shape").kind, ElementKind::Abstract);
        assert_eq!(
            rel(d, "util.render.Camera", "util.Shape").name.as_deref(),
            Some("draws")
        );
        // With `::` splitting names, a full name says where an element is.
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
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        assert_eq!(el(d, "src.Inner").members.len(), 1);
        assert_eq!(el(d, "src.A").type_params.as_deref(), Some("E"));
        rel(d, "com.app.Main", "com.api.Runner");
        assert_eq!(
            el(d, "com.app.Ev").type_params.as_deref(),
            Some("S extends T")
        );
        rel(d, "Note", "com.app.Main");
        // The footer is kept as a comment.
        assert_eq!(d.trailing_comments, ["footer: made by a tool"]);
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
        let pair = el(d, "\"::std::pair<T1,T2>\"");
        assert_eq!(pair.members.len(), 1);
        assert_eq!(pair.kind, ElementKind::Abstract);
        rel(d, "\"::std::pair<T1,T2>\"", "C_02");
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
        let lines: Vec<&str> = api.members.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(lines, ["[Rust]", "Serves orders"]);
        assert_eq!(
            rel(d, "Customer", "Shop.API").name.as_deref(),
            Some("Uses [HTTPS]")
        );
        assert_eq!(d.relations.len(), 2);
        // C4 is part of PlantUML's standard library: nothing is missing.
        assert!(p.problems.is_empty(), "{:?}", p.problems);

        // Without the include, the calls are guesses.
        let p = import("Person(user, \"Customer\")\nRel(user, api, \"Uses\")\nRel(api, user, \"Replies\")\nRelIndex(1, user, api, \"Asks\")\n");
        let messages: Vec<&str> = p.problems.iter().map(|p| p.message.as_str()).collect();
        // One problem per procedure, not per call.
        assert_eq!(messages.iter().filter(|m| m.contains("`Rel(")).count(), 1);
        assert_eq!(rel(&p.doc, "Customer", "api").name.as_deref(), Some("Uses"));
        assert!(p
            .doc
            .relations
            .values()
            .any(|r| r.name.as_deref() == Some("1: Asks")));
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
            "!define Table(name,desc) class name as \"desc\" << (T,#FFAAAA) >>\n!define pk(x) <b>x</b>\n!foo\n",
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
