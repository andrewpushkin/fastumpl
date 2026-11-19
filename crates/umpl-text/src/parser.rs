use std::collections::HashMap;

use umpl_core::*;

use crate::scan::Cursor;

/// Something in a file the parser could not use, with its location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column.
    pub col: usize,
    pub message: String,
}

/// The problem reported when a `{` is still open at the end of the text.
pub const UNCLOSED: &str = "missing `}` at end of file";

#[derive(Debug)]
pub struct Parsed {
    pub doc: Document,
    pub problems: Vec<Problem>,
    /// Whether the file had a `layout` block. Without one the editor lays
    /// the whole diagram out on open.
    pub has_layout: bool,
    /// Where each element, relation and package is declared in the text.
    /// Only [`parse`] fills this in; imports leave it empty.
    pub spans: Vec<Span>,
}

/// Something declared in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Element(ElementId),
    Relation(RelationId),
    Package(PackageId),
}

/// The lines that declare an item: from its first line to the `}` that
/// closes its body (1-based, inclusive). A package's span holds its
/// contents, so the innermost span containing a line names what is there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub item: Item,
    pub first: usize,
    pub last: usize,
}

impl Parsed {
    /// The item declared at `line` (1-based): the innermost span holding it.
    pub fn item_at(&self, line: usize) -> Option<Item> {
        self.spans
            .iter()
            .filter(|s| s.first <= line && line <= s.last)
            .min_by_key(|s| s.last - s.first)
            .map(|s| s.item)
    }

    /// The span of `item`, if it was declared in the text.
    pub fn span_of(&self, item: Item) -> Option<Span> {
        self.spans.iter().find(|s| s.item == item).copied()
    }
}

enum Ctx {
    Package(PackageId),
    Body { id: ElementId, is_enum: bool },
    Layout,
}

struct PendingRelation {
    id: RelationId,
    container: Option<PackageId>,
    kind: RelationKind,
    from: (String, Option<String>),
    to: (String, Option<String>),
    name: Option<String>,
    comments: Vec<String>,
    text: String,
    line: usize,
}

struct PendingPos {
    name: String,
    pos: Pos,
    line: usize,
}

#[derive(Default)]
struct Parser {
    doc: Document,
    problems: Vec<Problem>,
    stack: Vec<Ctx>,
    comments: Vec<String>,
    relations: Vec<PendingRelation>,
    positions: Vec<PendingPos>,
    has_layout: bool,
    last_line: usize,
    spans: Vec<Span>,
}

/// Parses a `.umpl` file. Never fails; see [`Parsed::problems`].
pub fn parse(src: &str) -> Parsed {
    let mut p = Parser::default();
    for (i, line) in src.lines().enumerate() {
        p.last_line = i + 1;
        p.line(i + 1, line);
    }
    p.finish()
}

impl Parser {
    fn problem(&mut self, line: usize, col: usize, message: impl Into<String>) {
        self.problems.push(Problem {
            line,
            col,
            message: message.into(),
        });
    }

    fn container(&self) -> Option<PackageId> {
        self.stack.iter().rev().find_map(|c| match c {
            Ctx::Package(p) => Some(*p),
            _ => None,
        })
    }

    fn push_decl(&mut self, d: Decl) {
        match self.container() {
            Some(p) => self.doc.packages.get_mut(&p).unwrap().body.push(d),
            None => self.doc.root.push(d),
        }
    }

    fn open_span(&mut self, item: Item, n: usize) {
        self.spans.push(Span {
            item,
            first: n,
            last: n,
        });
    }

    /// Ends the span of `item` at line `n`, where its `}` is.
    fn close_span(&mut self, item: Item, n: usize) {
        if let Some(s) = self.spans.iter_mut().rev().find(|s| s.item == item) {
            s.last = n;
        }
    }

    /// Keeps a line we could not understand, with any comments before it.
    fn raw(&mut self, n: usize, text: &str, message: impl Into<String>) {
        for c in std::mem::take(&mut self.comments) {
            self.push_decl(Decl::Raw(comment_line(&c)));
        }
        self.push_decl(Decl::Raw(text.to_string()));
        self.problem(n, 1, message);
    }

    fn line(&mut self, n: usize, raw: &str) {
        let t = raw.trim();
        match self.stack.last() {
            Some(Ctx::Body { id, is_enum }) => {
                let (id, is_enum) = (*id, *is_enum);
                if t == "}" {
                    self.stack.pop();
                    self.close_span(Item::Element(id), n);
                } else if !t.is_empty() {
                    let el = self.doc.elements.get_mut(&id).unwrap();
                    if is_enum {
                        el.members.extend(
                            t.split(',')
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(Member::plain),
                        );
                    } else {
                        el.members.push(Member::parse_line(t));
                    }
                }
                return;
            }
            Some(Ctx::Layout) => {
                if t == "}" {
                    self.stack.pop();
                } else if let Some(c) = t.strip_prefix('#') {
                    self.comments.push(strip_comment(c));
                } else if !t.is_empty() {
                    self.layout_line(n, t);
                }
                return;
            }
            _ => {}
        }

        if t.is_empty() {
            return;
        }
        if let Some(c) = t.strip_prefix('#') {
            self.comments.push(strip_comment(c));
            return;
        }
        if t == "}" {
            if let Some(Ctx::Package(p)) = self.stack.last() {
                let p = *p;
                self.stack.pop();
                self.close_span(Item::Package(p), n);
            } else {
                self.raw(n, t, "`}` without a matching `{`");
            }
            return;
        }
        if self.try_title(t)
            || self.try_package(n, t)
            || self.try_layout(n, t)
            || self.try_element(n, t)
            || self.try_note(n, t)
            || self.try_relation(n, t)
        {
            return;
        }
        self.raw(n, t, "not understood; kept as written");
    }

    fn try_title(&mut self, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("diagram") {
            return false;
        }
        match c.string() {
            Some(s) if c.at_end() => {
                self.doc.title = Some(s);
                true
            }
            _ => false,
        }
    }

    fn try_package(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("package") {
            return false;
        }
        let Some(name) = c.segment() else {
            return false;
        };
        if !c.eat("{") {
            return false;
        }
        let open = !c.eat("}");
        if !c.at_end() {
            return false;
        }
        let container = self.container();
        let name = self.free_name(n, &name, container);
        let id = self.doc.new_package_id();
        let mut pkg = Package::new(name);
        pkg.comments = std::mem::take(&mut self.comments);
        self.doc.packages.insert(id, pkg);
        self.push_decl(Decl::Package(id));
        self.open_span(Item::Package(id), n);
        if open {
            self.stack.push(Ctx::Package(id));
        }
        true
    }

    fn try_layout(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("layout") || !c.eat("{") {
            return false;
        }
        let closed = c.eat("}");
        if !c.at_end() {
            return false;
        }
        if !self.stack.is_empty() {
            self.raw(n, t, "`layout` is only allowed at the top level");
            return true;
        }
        self.has_layout = true;
        // Comments before the layout block stay at the end of the file.
        let comments = std::mem::take(&mut self.comments);
        self.doc.trailing_comments.extend(comments);
        if !closed {
            self.stack.push(Ctx::Layout);
        }
        true
    }

    fn layout_line(&mut self, n: usize, t: &str) {
        let mut c = Cursor::new(t);
        let parsed = (|| {
            let name = c.path()?;
            if !c.eat_word("at") {
                return None;
            }
            let x = c.int()?;
            if !c.eat(",") {
                return None;
            }
            let y = c.int()?;
            c.at_end().then_some((name, Pos::new(x, y)))
        })();
        match parsed {
            Some((name, pos)) => self.positions.push(PendingPos { name, pos, line: n }),
            None => self.problem(n, 1, "expected `Name at x, y`; line ignored"),
        }
    }

    /// `name` if it is free in `container`, otherwise a numbered variant
    /// (with a problem reported).
    fn free_name(&mut self, n: usize, name: &str, container: Option<PackageId>) -> String {
        if self.doc.name_is_free(name, container) {
            return name.to_string();
        }
        let new = self.doc.unique_name(name, container);
        self.problem(
            n,
            1,
            format!("`{name}` is declared twice; the second one was renamed to `{new}`"),
        );
        new
    }

    fn try_element(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let kind = if c.eat_word("abstract") {
            c.eat_word("class");
            ElementKind::Abstract
        } else if c.eat_word("class") {
            ElementKind::Class
        } else if c.eat_word("interface") {
            ElementKind::Interface
        } else if c.eat_word("enum") {
            ElementKind::Enum
        } else if c.eat_word("diamond") {
            ElementKind::Diamond
        } else {
            return false;
        };
        let Some(name) = c.segment() else {
            return false;
        };
        let type_params = c.type_params();
        // `class PO as "Purchase order"`: what the box shows.
        let label = if c.eat_word("as") {
            match c.string() {
                Some(l) => Some(l),
                None => return false,
            }
        } else {
            None
        };
        let stereotype = c.stereotype();
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

        let container = self.container();
        let name = self.free_name(n, &name, container);
        let id = self.doc.new_element_id();
        let mut el = Element::new(kind, name);
        el.type_params = type_params;
        el.label = label;
        el.stereotype = stereotype;
        el.comments = std::mem::take(&mut self.comments);
        let is_enum = kind == ElementKind::Enum;
        if let Some(inner) = inline {
            let sep = if is_enum { ',' } else { ';' };
            for part in inner.split(sep).map(str::trim).filter(|s| !s.is_empty()) {
                el.members.push(if is_enum {
                    Member::plain(part)
                } else {
                    Member::parse_line(part)
                });
            }
        }
        self.doc.elements.insert(id, el);
        self.push_decl(Decl::Element(id));
        self.open_span(Item::Element(id), n);
        if open {
            self.stack.push(Ctx::Body { id, is_enum });
        }
        true
    }

    fn try_note(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        if !c.eat_word("note") {
            return false;
        }
        let Some(name) = c.segment() else {
            return false;
        };
        let Some(text) = c.string() else { return false };
        if !c.at_end() {
            return false;
        }
        let container = self.container();
        let name = self.free_name(n, &name, container);
        let id = self.doc.new_element_id();
        let mut el = Element::new(ElementKind::Note, name);
        el.members = text.split('\n').map(Member::plain).collect();
        el.comments = std::mem::take(&mut self.comments);
        self.doc.elements.insert(id, el);
        self.push_decl(Decl::Element(id));
        self.open_span(Item::Element(id), n);
        true
    }

    fn try_relation(&mut self, n: usize, t: &str) -> bool {
        let mut c = Cursor::new(t);
        let Some(a) = c.path() else { return false };
        let la = c.string();
        let Some((kind, reversed)) = c.arrow() else {
            return false;
        };
        // `--> "*" B` or `--> "Quoted name"`: a label only if a name follows.
        let at = c.pos;
        let (lb, b) = match c.path() {
            Some(b) if c.at_end() || c.rest().trim_start().starts_with(':') => (None, b),
            _ => {
                c.pos = at;
                let lb = c.string();
                let Some(b) = c.path() else { return false };
                (lb, b)
            }
        };
        let name = if c.eat(":") {
            Some(c.rest().trim().to_string()).filter(|s| !s.is_empty())
        } else if c.at_end() {
            None
        } else {
            return false;
        };
        let (mut from, mut to) = ((a, la), (b, lb));
        if reversed {
            std::mem::swap(&mut from, &mut to);
        }
        let id = self.doc.new_relation_id();
        self.push_decl(Decl::Relation(id));
        self.open_span(Item::Relation(id), n);
        self.relations.push(PendingRelation {
            id,
            container: self.container(),
            kind,
            from,
            to,
            name,
            comments: std::mem::take(&mut self.comments),
            text: t.to_string(),
            line: n,
        });
        true
    }

    fn finish(mut self) -> Parsed {
        if !self.stack.is_empty() {
            let line = self.last_line.max(1);
            self.problem(line, 1, UNCLOSED);
            // Bodies still open run to the end of the text.
            for c in std::mem::take(&mut self.stack) {
                match c {
                    Ctx::Package(p) => self.close_span(Item::Package(p), line),
                    Ctx::Body { id, .. } => self.close_span(Item::Element(id), line),
                    Ctx::Layout => {}
                }
            }
        }

        // Full names of packages, then of elements (which win).
        let mut names: HashMap<String, Endpoint> = self
            .doc
            .packages
            .keys()
            .map(|id| (self.doc.full_package_name(*id), Endpoint::Package(*id)))
            .collect();
        names.extend(
            self.doc
                .elements
                .keys()
                .map(|id| (self.doc.full_name(*id), Endpoint::Element(*id))),
        );

        for rel in std::mem::take(&mut self.relations) {
            let prefix = rel.container.map(|p| self.doc.full_package_name(p));
            let mut ends = Vec::new();
            for (name, _) in [&rel.from, &rel.to] {
                ends.push(self.resolve(&mut names, prefix.as_deref(), name));
            }
            match (ends[0], ends[1]) {
                (Some(f), Some(t)) => {
                    let relation = Relation {
                        kind: rel.kind,
                        from: End {
                            target: f,
                            label: rel.from.1,
                        },
                        to: End {
                            target: t,
                            label: rel.to.1,
                        },
                        name: rel.name,
                        comments: rel.comments,
                    };
                    self.doc.relations.insert(rel.id, relation);
                }
                _ => {
                    let bad = if ends[0].is_none() {
                        &rel.from.0
                    } else {
                        &rel.to.0
                    };
                    let col = rel.text.find(bad.as_str()).map(|i| i + 1).unwrap_or(1);
                    let message = format!("unknown element `{bad}`; line kept as written");
                    self.problem(rel.line, col, message);
                    self.spans.retain(|s| s.item != Item::Relation(rel.id));
                    let mut replacement: Vec<Decl> = rel
                        .comments
                        .iter()
                        .map(|c| Decl::Raw(comment_line(c)))
                        .collect();
                    replacement.push(Decl::Raw(rel.text.clone()));
                    let body = match rel.container {
                        Some(p) => &mut self.doc.packages.get_mut(&p).unwrap().body,
                        None => &mut self.doc.root,
                    };
                    if let Some(i) = body.iter().position(|d| *d == Decl::Relation(rel.id)) {
                        body.splice(i..=i, replacement);
                    }
                }
            }
        }

        for p in std::mem::take(&mut self.positions) {
            match names.get(&p.name) {
                Some(Endpoint::Element(id)) => {
                    self.doc.elements.get_mut(id).unwrap().pos = Some(p.pos)
                }
                _ => self.problem(
                    p.line,
                    1,
                    format!("position for unknown element `{}` ignored", p.name),
                ),
            }
        }

        self.doc.trailing_comments.append(&mut self.comments);
        self.problems.sort_by_key(|p| (p.line, p.col));
        Parsed {
            doc: self.doc,
            problems: self.problems,
            has_layout: self.has_layout,
            spans: self.spans,
        }
    }

    /// Finds an element (or else a package) by name, relative to the
    /// relation's package first. An unknown single name creates a class at
    /// the top level, as PlantUML does; an unknown dotted name is an error.
    fn resolve(
        &mut self,
        names: &mut HashMap<String, Endpoint>,
        prefix: Option<&str>,
        name: &str,
    ) -> Option<Endpoint> {
        if let Some(prefix) = prefix {
            if let Some(t) = names.get(&format!("{prefix}.{name}")) {
                return Some(*t);
            }
        }
        if let Some(t) = names.get(name) {
            return Some(*t);
        }
        // One name, as written (unquoted).
        let mut c = Cursor::new(name);
        let single = c.segment().filter(|_| c.at_end())?;
        let id = self.doc.new_element_id();
        self.doc
            .elements
            .insert(id, Element::new(ElementKind::Class, single));
        self.doc.root.push(Decl::Element(id));
        names.insert(name.to_string(), Endpoint::Element(id));
        Some(Endpoint::Element(id))
    }
}

fn strip_comment(c: &str) -> String {
    c.strip_prefix(' ').unwrap_or(c).trim_end().to_string()
}

fn comment_line(c: &str) -> String {
    if c.is_empty() {
        "#".to_string()
    } else {
        format!("# {c}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const DESIGN_EXAMPLE: &str = r#"diagram "Shop"

package billing {
  abstract class Payment {
    +amount: Money
    +pay(): Receipt {abstract}
  }
  class CardPayment
}

class Order {
  +id: OrderId
  -lines: List<OrderLine>
  +total(): Money
}

interface Priced {
  +price(): Money
}

enum Status { New, Paid, Shipped }

# an order owns its lines
Order "1" *-- "1..*" OrderLine : contains
billing.CardPayment --|> billing.Payment
Order ..|> Priced
Order --> billing.Payment : paid by

layout {
  Order               at 24, 8
  OrderLine           at 24, 22
  billing.Payment     at 48, 8
  billing.CardPayment at 48, 22
}
"#;

    fn el<'a>(doc: &'a Document, name: &str) -> &'a Element {
        &doc.elements[&doc.find(name).unwrap_or_else(|| panic!("no {name}"))]
    }

    #[test]
    fn parses_design_example() {
        let p = parse(DESIGN_EXAMPLE);
        assert_eq!(p.problems, vec![]);
        assert!(p.has_layout);
        let d = &p.doc;
        assert_eq!(d.title.as_deref(), Some("Shop"));
        assert_eq!(d.elements.len(), 6);
        assert_eq!(d.relations.len(), 4);
        assert_eq!(el(d, "billing.Payment").kind, ElementKind::Abstract);
        assert_eq!(el(d, "billing.Payment").members.len(), 2);
        assert!(el(d, "billing.Payment").members[1].is_abstract);
        assert_eq!(el(d, "Order").members.len(), 3);
        assert_eq!(el(d, "Priced").kind, ElementKind::Interface);
        let status = el(d, "Status");
        assert_eq!(status.kind, ElementKind::Enum);
        assert_eq!(
            status
                .members
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>(),
            ["New", "Paid", "Shipped"]
        );
        assert_eq!(el(d, "Order").pos, Some(Pos::new(24, 8)));
        assert_eq!(el(d, "billing.CardPayment").pos, Some(Pos::new(48, 22)));
        // OrderLine is only mentioned in a relation, so it was created.
        assert_eq!(el(d, "OrderLine").pos, Some(Pos::new(24, 22)));

        let comp = d
            .relations
            .values()
            .find(|r| r.kind == RelationKind::Composition)
            .unwrap();
        assert_eq!(comp.from.element().unwrap(), d.find("Order").unwrap());
        assert_eq!(comp.from.label.as_deref(), Some("1"));
        assert_eq!(comp.to.label.as_deref(), Some("1..*"));
        assert_eq!(comp.name.as_deref(), Some("contains"));
        assert_eq!(comp.comments, vec!["an order owns its lines"]);
    }

    #[test]
    fn reversed_arrows_normalise() {
        let p = parse("class A\nclass B\nA <|-- B\nA --* B\n");
        let d = &p.doc;
        let (a, b) = (d.find("A").unwrap(), d.find("B").unwrap());
        let rels: Vec<_> = d.relations.values().collect();
        assert_eq!(rels[0].kind, RelationKind::Generalization);
        assert_eq!(
            (
                rels[0].from.element().unwrap(),
                rels[0].to.element().unwrap()
            ),
            (b, a)
        );
        assert_eq!(rels[1].kind, RelationKind::Composition);
        assert_eq!(
            (
                rels[1].from.element().unwrap(),
                rels[1].to.element().unwrap()
            ),
            (b, a)
        );
    }

    #[test]
    fn unknown_line_kept_as_raw_with_problem() {
        let p = parse("class A\nthis is nonsense\n");
        assert_eq!(p.doc.root[1], Decl::Raw("this is nonsense".into()));
        assert_eq!(p.problems.len(), 1);
        assert_eq!(p.problems[0].line, 2);
    }

    #[test]
    fn relation_to_unknown_element_is_raw() {
        let p = parse("class A\nA --> nowhere.B\n");
        assert!(p.doc.relations.is_empty());
        assert_eq!(p.doc.root[1], Decl::Raw("A --> nowhere.B".into()));
        assert_eq!(p.problems[0].line, 2);
        assert!(p.problems[0].message.contains("nowhere.B"));
    }

    #[test]
    fn empty_and_layout_only_files() {
        let p = parse("");
        assert!(p.doc.elements.is_empty() && p.problems.is_empty() && !p.has_layout);
        let p = parse("layout {\n}\n");
        assert!(p.doc.elements.is_empty() && p.problems.is_empty() && p.has_layout);
        let p = parse("layout {\n  Ghost at 1, 2\n}\n");
        assert_eq!(p.problems.len(), 1);
    }

    #[test]
    fn comments_attach_to_next_decl() {
        let p = parse("# first\n# second\nclass A\n# tail\n");
        let a = &p.doc.elements[&p.doc.find("A").unwrap()];
        assert_eq!(a.comments, vec!["first", "second"]);
        assert_eq!(p.doc.trailing_comments, vec!["tail"]);
    }

    #[test]
    fn relations_inside_packages_resolve_relative_names() {
        let p = parse("package p {\n  class A\n  class B\n  A --> B\n}\n");
        assert_eq!(p.problems, vec![]);
        assert_eq!(p.doc.relations.len(), 1);
        assert_eq!(p.doc.elements.len(), 2);
    }

    #[test]
    fn duplicate_names_are_renamed() {
        let p = parse("class A\nclass A\n");
        assert_eq!(p.doc.elements.len(), 2);
        assert!(p.doc.find("A2").is_some());
        assert_eq!(p.problems.len(), 1);
    }

    #[test]
    fn missing_brace_reported() {
        let p = parse("class A {\n  +x: int\n");
        assert_eq!(p.doc.elements.len(), 1);
        assert_eq!(p.problems.len(), 1);
    }

    #[test]
    fn notes_and_stereotypes() {
        let p = parse("note N1 \"line one\\nline two\"\nclass A <<entity>>\nN1 .. A\n");
        assert_eq!(p.problems, vec![]);
        let n = &p.doc.elements[&p.doc.find("N1").unwrap()];
        assert_eq!(n.kind, ElementKind::Note);
        assert_eq!(n.members.len(), 2);
        let a = &p.doc.elements[&p.doc.find("A").unwrap()];
        assert_eq!(a.stereotype.as_deref(), Some("entity"));
        assert_eq!(
            p.doc.relations.values().next().unwrap().kind,
            RelationKind::NoteLink
        );
    }

    #[test]
    fn lines_join_packages_and_names_may_be_quoted() {
        let src = "package shop {\n  class \"Order line\"<T>\n}\npackage \"web app\" {}\n\"web app\" ..> shop\nshop.\"Order line\" --> \"web app\" : shown in\n\"New one\" -- shop\n";
        let p = parse(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let d = &p.doc;
        let line = d.find("shop.\"Order line\"").unwrap();
        assert_eq!(d.elements[&line].name, "Order line");
        assert_eq!(d.elements[&line].type_params.as_deref(), Some("T"));
        let shop = d.find_package("shop").unwrap();
        let web = d.find_package("\"web app\"").unwrap();
        let rels = d.relations_in_order();
        let r = |i: usize| &d.relations[&rels[i]];
        assert_eq!(
            (r(0).from.target, r(0).to.target),
            (Endpoint::Package(web), Endpoint::Package(shop))
        );
        assert_eq!(r(1).to.target, Endpoint::Package(web));
        // An unknown quoted name is a new class, named as written.
        assert_eq!(d.elements[&r(2).from.element().unwrap()].name, "New one");
        // Printing gives the same text back.
        assert_eq!(
            crate::print(d),
            src.replace("package \"web app\" {}\n", "\npackage \"web app\" {}\n")
                .replace("\"web app\" ..>", "\n\"web app\" ..>")
                .replace("\n\"New one\" -- shop\n", "\n\nclass \"New one\"\n")
                .replace(
                    "\"web app\" : shown in\n\n",
                    "\"web app\" : shown in\n\"New one\" -- shop\n\n"
                )
        );
    }

    #[test]
    fn spans_name_the_lines_of_each_declaration() {
        let src = "diagram \"S\"\n\npackage p {\n  class A {\n    +x: int\n  }\n  note N \"n\"\n}\nclass B\nA --> B\nweird line\nC --> D.E\nenum E {\n  R\n";
        let p = parse(src);
        let d = &p.doc;
        let el = |n: &str| Item::Element(d.find(n).unwrap());
        let span = |item| p.span_of(item).map(|s| (s.first, s.last));
        let pkg = Item::Package(*d.packages.keys().next().unwrap());
        assert_eq!(span(pkg), Some((3, 8)));
        assert_eq!(span(el("p.A")), Some((4, 6)));
        assert_eq!(span(el("p.N")), Some((7, 7)));
        assert_eq!(span(el("B")), Some((9, 9)));
        let rel = Item::Relation(*d.relations.keys().next().unwrap());
        assert_eq!(span(rel), Some((10, 10)));
        // An unclosed body runs to the end.
        assert_eq!(span(el("E")), Some((13, 14)));
        // Innermost span wins; lines outside any span name nothing.
        assert_eq!(p.item_at(5), Some(el("p.A")));
        assert_eq!(p.item_at(4), Some(el("p.A")));
        assert_eq!(p.item_at(3), Some(pkg));
        assert_eq!(p.item_at(8), Some(pkg));
        assert_eq!(p.item_at(10), Some(rel));
        assert_eq!(p.item_at(2), None);
        assert_eq!(p.item_at(11), None);
        // A relation kept as raw text has no span.
        assert_eq!(p.item_at(12), None);
        assert_eq!(p.spans.len(), 6);
    }

    #[test]
    fn self_relation_parses() {
        let p = parse("class Node\nNode --> Node : next\n");
        let r = p.doc.relations.values().next().unwrap();
        assert_eq!(r.from.element().unwrap(), r.to.element().unwrap());
    }
}
