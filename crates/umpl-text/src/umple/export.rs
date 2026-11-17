//! Writes a [`Document`] as an Umple model.
//!
//! Umple has no packages, notes, aggregation or dependencies, and its
//! class names are global. Packages become `namespace` statements, notes
//! become comments above what they describe, and relations Umple cannot
//! express become comments, so nothing in the diagram is silently lost.

use std::collections::{HashMap, HashSet};

use umpl_core::*;

use crate::scan::{clean, is_name};

/// Writes `doc` as Umple. Positions are not written: Umple tools lay
/// diagrams out themselves.
pub fn export(doc: &Document) -> String {
    let mut w = Writer {
        doc,
        names: umple_names(doc),
        above: HashMap::new(),
        isa: HashMap::new(),
        associations: HashMap::new(),
        dependencies: HashMap::new(),
        linked_notes: HashSet::new(),
        out: String::new(),
    };
    w.sort_relations();
    if let Some(t) = &doc.title {
        w.block();
        for line in t.lines() {
            w.out.push_str(&format!("// {line}\n"));
        }
    }
    w.section(None);
    for pkg in doc.packages_in_order() {
        let has_content = doc.body(Some(pkg)).iter().any(|d| match d {
            Decl::Element(e) => !w.linked_notes.contains(e),
            Decl::Raw(_) => true,
            _ => false,
        });
        if has_content {
            w.block();
            w.comments(&doc.packages[&pkg].comments, "");
            let path: Vec<String> = doc.package_path(pkg).iter().map(|p| clean(p)).collect();
            w.out.push_str(&format!("namespace {};\n", path.join(".")));
            w.section(Some(pkg));
        }
    }
    if !doc.trailing_comments.is_empty() {
        w.block();
        w.comments(&doc.trailing_comments, "");
    }
    w.out
}

/// Umple class names are global: a name used in two packages gets its
/// package path, `billing_Item`.
fn umple_names(doc: &Document) -> HashMap<ElementId, String> {
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
                doc.full_name(*id).replace('.', "_")
            };
            (*id, if is_name(&name) { name } else { clean(&name) })
        })
        .collect()
}

/// Kinds that are Umple classes and can hold associations.
fn is_class(k: ElementKind) -> bool {
    matches!(k, ElementKind::Class | ElementKind::Abstract)
}

/// Kinds that can be the other end of an association, or take part in `isA`.
fn is_type(k: ElementKind) -> bool {
    is_class(k) || k == ElementKind::Interface
}

struct Writer<'a> {
    doc: &'a Document,
    names: HashMap<ElementId, String>,
    /// Comment lines above an element: notes attached to it, and relations
    /// from it that Umple cannot express.
    above: HashMap<ElementId, Vec<String>>,
    isa: HashMap<ElementId, Vec<ElementId>>,
    /// Association lines (with their comments) in the owning class body.
    associations: HashMap<ElementId, Vec<String>>,
    dependencies: HashMap<ElementId, Vec<String>>,
    linked_notes: HashSet<ElementId>,
    out: String,
}

impl Writer<'_> {
    fn kind(&self, id: ElementId) -> ElementKind {
        self.doc.elements[&id].kind
    }

    fn name(&self, id: ElementId) -> &str {
        &self.names[&id]
    }

    /// A blank line before the next block, except at the start.
    fn block(&mut self) {
        if !self.out.is_empty() {
            self.out.push('\n');
        }
    }

    fn comments(&mut self, lines: &[String], indent: &str) {
        for c in lines {
            for line in c.lines() {
                self.out.push_str(&format!("{indent}// {line}\n"));
            }
            if c.is_empty() {
                self.out.push_str(&format!("{indent}//\n"));
            }
        }
    }

    /// Decides where each relation is written.
    fn sort_relations(&mut self) {
        for rid in self.doc.relations_in_order() {
            let r = &self.doc.relations[&rid];
            let (f, t) = (r.from.element, r.to.element);
            let (fk, tk) = (self.kind(f), self.kind(t));
            match r.kind {
                RelationKind::NoteLink => {
                    let (note, target) = match (fk, tk) {
                        (ElementKind::Note, k) if k != ElementKind::Note => (f, t),
                        (k, ElementKind::Note) if k != ElementKind::Note => (t, f),
                        _ => continue,
                    };
                    let text = note_lines(&self.doc.elements[&note]);
                    self.above.entry(target).or_default().extend(text);
                    self.linked_notes.insert(note);
                }
                RelationKind::Generalization | RelationKind::Realization
                    if is_type(fk) && is_type(tk) =>
                {
                    self.isa.entry(f).or_default().push(t);
                }
                RelationKind::Association
                | RelationKind::Directed
                | RelationKind::Aggregation
                | RelationKind::Composition
                    if (is_class(fk) && is_type(tk)) || (is_type(fk) && is_class(tk)) =>
                {
                    let swapped = !is_class(fk);
                    let owner = if swapped { t } else { f };
                    let lines = self.association_lines(r, swapped);
                    self.associations.entry(owner).or_default().extend(lines);
                }
                RelationKind::Dependency if is_type(fk) => {
                    let line = format!("// depends on {}", self.name(t));
                    self.dependencies.entry(f).or_default().push(line);
                }
                _ => {
                    let line = describe(self.doc, r);
                    self.above.entry(f).or_default().push(line);
                }
            }
        }
    }

    /// `1 role -- * Other role;` written in the body of the owning end,
    /// which is `to` when `swapped`.
    fn association_lines(&self, r: &Relation, swapped: bool) -> Vec<String> {
        let mut lines: Vec<String> = r.comments.iter().map(|c| format!("// {c}")).collect();
        if let Some(n) = &r.name {
            lines.push(format!("// {n}"));
        }
        if r.kind == RelationKind::Aggregation {
            lines.push("// aggregation".into());
        }
        // A composite has one whole; any other end defaults to many.
        let whole = if r.kind == RelationKind::Composition {
            "1"
        } else {
            "*"
        };
        let (from_mult, from_role) = split_label(r.from.label.as_deref(), whole);
        let (to_mult, to_role) = split_label(r.to.label.as_deref(), "*");
        let (own, other, other_id) = if swapped {
            ((to_mult, to_role), (from_mult, from_role), r.from.element)
        } else {
            ((from_mult, from_role), (to_mult, to_role), r.to.element)
        };
        let arrow = match (r.kind, swapped) {
            (RelationKind::Directed, false) => "->",
            (RelationKind::Directed, true) => "<-",
            (RelationKind::Composition, false) => "<@>-",
            (RelationKind::Composition, true) => "-<@>",
            _ => "--",
        };
        let role = |r: Option<String>| r.map(|r| format!(" {r}")).unwrap_or_default();
        lines.push(format!(
            "{}{} {arrow} {} {}{};",
            own.0,
            role(own.1),
            other.0,
            self.name(other_id),
            role(other.1)
        ));
        lines
    }

    /// The elements and raw lines directly in `pkg`.
    fn section(&mut self, pkg: Option<PackageId>) {
        for d in self.doc.body(pkg).to_vec() {
            match d {
                Decl::Element(id) => self.element(id),
                Decl::Raw(text) => {
                    self.block();
                    self.out.push_str(&format!("// {text}\n"));
                }
                Decl::Relation(_) | Decl::Package(_) => {}
            }
        }
    }

    fn element(&mut self, id: ElementId) {
        let doc = self.doc;
        let el = &doc.elements[&id];
        if el.kind == ElementKind::Note {
            if !self.linked_notes.contains(&id) {
                self.block();
                self.comments(&note_lines(el), "");
            }
            return;
        }
        self.block();
        self.comments(&el.comments, "");
        let above = self.above.remove(&id).unwrap_or_default();
        self.comments(&above, "");
        let stereotypes: Vec<&str> = el
            .stereotype
            .as_deref()
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let has = |s: &str| stereotypes.contains(&s);
        for s in &stereotypes {
            if !matches!(*s, "singleton" | "immutable" | "trait" | "external") {
                self.out.push_str(&format!("// «{s}»\n"));
            }
        }
        let name = self.name(id).to_string();
        if el.kind == ElementKind::Enum {
            let literals: Vec<String> = el.members.iter().map(|m| clean(&m.text)).collect();
            self.out
                .push_str(&format!("enum {name} {{ {} }}\n", literals.join(", ")));
            return;
        }
        let interface = el.kind == ElementKind::Interface;
        let keyword = if interface {
            "interface"
        } else if has("trait") {
            "trait"
        } else if has("external") {
            "external"
        } else {
            "class"
        };

        let mut body = Vec::new();
        if el.kind == ElementKind::Abstract {
            body.push("abstract;".to_string());
        }
        for s in ["singleton", "immutable"] {
            if has(s) && !interface {
                body.push(format!("{s};"));
            }
        }
        if let Some(parents) = self.isa.get(&id) {
            let names: Vec<&str> = parents.iter().map(|p| self.name(*p)).collect();
            body.push(format!("isA {};", names.join(", ")));
        }
        body.extend(el.attributes().map(|m| attribute(m, interface)));
        body.extend(self.associations.remove(&id).unwrap_or_default());
        body.extend(self.dependencies.remove(&id).unwrap_or_default());
        body.extend(el.operations().map(|m| operation(m, interface)));
        if body.is_empty() {
            self.out.push_str(&format!("{keyword} {name} {{}}\n"));
        } else {
            self.out.push_str(&format!("{keyword} {name} {{\n"));
            for line in body {
                self.out.push_str(&format!("  {line}\n"));
            }
            self.out.push_str("}\n");
        }
    }
}

fn note_lines(note: &Element) -> Vec<String> {
    note.members.iter().map(|m| m.text.clone()).collect()
}

/// A relation Umple cannot express, as `.umpl` text for a comment.
fn describe(doc: &Document, r: &Relation) -> String {
    let end = |l: &Option<String>| l.as_ref().map(|l| format!(" \"{l}\"")).unwrap_or_default();
    let mut s = format!(
        "{}{} {}{} {}",
        doc.elements[&r.from.element].name,
        end(&r.from.label),
        r.kind.arrow(),
        end(&r.to.label),
        doc.elements[&r.to.element].name
    );
    if let Some(n) = &r.name {
        s.push_str(&format!(" : {n}"));
    }
    s
}

/// `"0..* items"` as multiplicity and role; a missing multiplicity is
/// `default`. UML's `n` is Umple's `*`.
fn split_label(label: Option<&str>, default: &str) -> (String, Option<String>) {
    let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) else {
        return (default.to_string(), None);
    };
    let (first, rest) = label.split_once(char::is_whitespace).unwrap_or((label, ""));
    let (mult, role) = match multiplicity(first) {
        Some(m) => (m, rest.trim()),
        None => (default.to_string(), label),
    };
    let role = (!role.is_empty()).then(|| clean(role));
    (mult, role)
}

fn multiplicity(s: &str) -> Option<String> {
    let bound = |b: &str| match b {
        "*" | "n" | "N" | "m" | "M" => Some("*".to_string()),
        b if !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()) => Some(b.to_string()),
        _ => None,
    };
    match s.split_once("..") {
        Some((lo, hi)) => Some(format!("{}..{}", bound(lo)?, bound(hi)?)),
        None => bound(s),
    }
}

/// Splits at the first `sep` outside `<>`, `()`, `[]` and quotes.
fn split_top(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            _ if quoted => {}
            c if c == sep && depth == 0 => return Some((&s[..i], &s[i + c.len_utf8()..])),
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// All parts of `s` split at top-level `sep`.
fn split_all(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((a, b)) = split_top(rest, sep) {
        out.push(a);
        rest = b;
    }
    out.push(rest);
    out
}

/// `T[*]` and `T[0..*]` are Umple's `T[]`.
fn umple_type(ty: &str) -> String {
    let ty = ty.trim();
    match ty.find('[') {
        Some(i) if ty.ends_with(']') => format!("{}[]", ty[..i].trim_end()),
        _ => ty.to_string(),
    }
}

/// `name: Type = value` as `Type name = value;`. Umple attributes have no
/// visibility; static ones are constants.
fn attribute(m: &Member, interface: bool) -> String {
    let text = m.text.trim();
    let text = text.strip_prefix('/').unwrap_or(text);
    let (decl, value) = match split_top(text, '=') {
        Some((d, v)) => (d, Some(v.trim())),
        None => (text, None),
    };
    let (name, ty) = match split_top(decl, ':') {
        Some((n, t)) => (n.trim(), Some(umple_type(t))),
        None => (decl.trim(), None),
    };
    if !is_name(name) || ty.as_deref() == Some("") {
        return format!("// {}", m.to_line());
    }
    let mut out = String::new();
    if m.is_static || interface {
        out.push_str("const ");
    }
    if let Some(t) = ty {
        out.push_str(&t);
        out.push(' ');
    }
    out.push_str(name);
    if let Some(v) = value.filter(|v| !v.is_empty()) {
        out.push_str(" = ");
        out.push_str(v);
    }
    out.push(';');
    out
}

/// `+total(a: int): Money` as `public Money total(int a) {}`; in an
/// interface, `Money total(int a);`.
fn operation(m: &Member, interface: bool) -> String {
    let text = m.text.trim();
    let parsed = (|| {
        let open = text.find('(')?;
        let name = text[..open].trim();
        let (params, rest) = split_top(&text[open + 1..], ')')?;
        let ret = match rest.trim() {
            "" => "void".to_string(),
            r => umple_type(r.strip_prefix(':')?),
        };
        is_name(name).then_some((name, params, ret))
    })();
    let Some((name, params, ret)) = parsed.filter(|(_, _, r)| !r.is_empty()) else {
        return format!("// {}", m.to_line());
    };
    let params: Vec<String> = split_all(params, ',')
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match split_top(p, ':') {
            Some((n, t)) => format!("{} {}", umple_type(t), n.trim()),
            None => p.to_string(),
        })
        .collect();
    let signature = format!("{ret} {name}({})", params.join(", "));
    if interface {
        return format!("{signature};");
    }
    let vis = match m.vis {
        Some(Visibility::Public) => "public ",
        Some(Visibility::Protected) => "protected ",
        // Umple has no private abstract methods, and no package visibility.
        Some(Visibility::Private) if !m.is_abstract => "private ",
        _ => "",
    };
    let stat = if m.is_static { "static " } else { "" };
    if m.is_abstract {
        format!("{vis}{stat}abstract {signature};")
    } else {
        format!("{vis}{stat}{signature} {{}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, print};

    fn umple(umpl: &str) -> String {
        let p = parse(umpl);
        assert_eq!(p.problems, vec![], "{umpl}");
        export(&p.doc)
    }

    #[test]
    fn classes_members_and_kinds() {
        let src = "\
class Order <<entity>> {
  +id: OrderId
  -lines: List<OrderLine>
  #count: int {static}
  +total(): Money
  ~helper(a: int, b: String): Map<K, V>
  log()
  /size: int
  tags: String[*]
  status: Status = Status.New
  weird member text
}

abstract class Payment {
  +amount: Money
  -pay(): Receipt {abstract}
  +refund() {abstract}
}

interface Shape {
  +area(): Double
  MAX: int = 10
}

enum Status { New, Paid, Shipped }

class Empty <<singleton>>
";
        assert_eq!(
            umple(src),
            "\
// «entity»
class Order {
  OrderId id;
  List<OrderLine> lines;
  const int count;
  int size;
  String[] tags;
  Status status = Status.New;
  // weird member text
  public Money total() {}
  Map<K, V> helper(int a, String b) {}
  void log() {}
}

class Payment {
  abstract;
  Money amount;
  abstract Receipt pay();
  public abstract void refund();
}

interface Shape {
  const int MAX = 10;
  Double area();
}

enum Status { New, Paid, Shipped }

class Empty {
  singleton;
}
"
        );
    }

    #[test]
    fn relations_become_isa_inline_associations_or_comments() {
        let src = "\
class A
class B
interface I
enum E { X }
note N \"first line\\nsecond\"
interface J

A \"1 owner\" -- \"* items\" B : holds
A --> B
B *-- \"1..*\" A
A o-- B
A ..> B
A --|> B
A ..|> I
I --|> J
I -- A
E -- A
I -- J
N .. A
";
        assert_eq!(
            umple(src),
            "\
// first line
// second
class A {
  isA B, I;
  // holds
  1 owner -- * B items;
  * -> * B;
  // aggregation
  * -- * B;
  * -- * I;
  // depends on B
}

class B {
  1 <@>- 1..* A;
}

// I -- J
interface I {
  isA J;
}

// E -- A
enum E { X }

interface J {}
"
        );
    }

    #[test]
    fn packages_become_namespaces() {
        let src = "\
diagram \"Shop\"

# top comment
class Root

note Loose \"a loose note\"

package billing {
  # the payment
  class Payment
  package cards {
    class Card
  }
}

package shipping {
  class Item
}

package returns {
  class Item
}

Root --> billing.cards.Card
shipping.Item -- returns.Item
";
        assert_eq!(
            umple(src),
            "\
// Shop

// top comment
class Root {
  * -> * Card;
}

// a loose note

namespace billing;

// the payment
class Payment {}

namespace billing.cards;

class Card {}

namespace shipping;

class shipping_Item {
  * -- * returns_Item;
}

namespace returns;

class returns_Item {}
"
        );
    }

    /// Elements and relations as sorted lines, ignoring declaration order,
    /// which export changes (associations move into class bodies).
    fn summary(doc: &Document) -> Vec<String> {
        let mut out: Vec<String> = doc
            .elements
            .iter()
            .map(|(id, e)| {
                let members: Vec<String> = e.members.iter().map(Member::to_line).collect();
                format!(
                    "{} {:?} {:?} {:?} [{}]",
                    doc.full_name(*id),
                    e.kind,
                    e.stereotype,
                    e.comments,
                    members.join("; ")
                )
            })
            .collect();
        out.extend(doc.relations.values().map(|r| {
            format!(
                "{} {:?} {:?} {:?} {} {:?}",
                doc.full_name(r.from.element),
                r.from.label,
                r.kind,
                r.to.label,
                doc.full_name(r.to.element),
                r.comments
            )
        }));
        out.sort();
        out
    }

    #[test]
    fn umple_models_survive_export_and_import() {
        let src = "
namespace shop;
// An order
class Order {
  abstract;
  Integer id;
  String[] tags;
  const Integer MAX = 5;
  isA Entity;
  // its lines
  1 -- * OrderLine lines;
  * -> 0..1 Customer buyer;
  1 <@>- * Note;
  0..1 <- * Clerk;
  * -<@> 1 Store;
  public Money total() {}
  void log(String msg, List<Map<String, Integer>> extra) {}
  public abstract Integer weight();
}
class OrderLine { Integer qty; }
class Customer { singleton; }
class Note {}
class Clerk {}
class Store {}
interface Entity { const String PREFIX = \"e\"; Integer key(); }
enum Status { Open, Closed }
namespace shop.billing;
class Invoice { isA Order; 0..1 -- 1 Customer; }
";
        let first = super::super::import(src);
        assert_eq!(first.problems, vec![]);
        let text = export(&first.doc);
        let second = super::super::import(&text);
        assert_eq!(second.problems, vec![], "{text}");
        assert_eq!(summary(&second.doc), summary(&first.doc), "{text}");
        // And the `.umpl` text of both reads back the same.
        assert_eq!(
            summary(&parse(&print(&second.doc)).doc),
            summary(&first.doc)
        );
    }
}
