//! Mermaid class diagrams (`classDiagram`): export only.
//!
//! Mermaid lays diagrams out itself, so positions are not written. See
//! `docs/plans/2026-11-17-plantuml-mermaid.md` for the mapping.

use std::collections::{HashMap, HashSet};

use umpl_core::*;

/// Writes `doc` as a Mermaid class diagram.
pub fn export(doc: &Document) -> String {
    let mut w = Writer {
        doc,
        ids: class_ids(doc),
        out: String::new(),
    };
    if let Some(t) = &doc.title {
        w.out.push_str("---\ntitle: ");
        w.out.push_str(&yaml_string(t));
        w.out.push_str("\n---\n");
    }
    w.out.push_str("classDiagram\n");
    if doc.left_to_right {
        w.out.push_str("  direction LR\n");
    }

    w.decls(&doc.root, 1);
    for pkg in doc.packages_in_order() {
        let body = doc.body(Some(pkg));
        let has_classes = body.iter().any(|d| match d {
            Decl::Element(e) => doc
                .elements
                .get(e)
                .is_some_and(|e| e.kind != ElementKind::Note),
            Decl::Raw(_) => true,
            _ => false,
        });
        if !has_classes {
            continue;
        }
        w.out.push_str(&format!(
            "    namespace {} {{\n",
            doc.package_path(pkg).join("_")
        ));
        w.decls(body, 2);
        w.out.push_str("    }\n");
    }

    for id in doc.relations_in_order() {
        w.relation(id);
    }
    for id in doc.elements_in_order() {
        w.note(id);
    }
    // Colours the user gave classes.
    for id in doc.elements_in_order() {
        let el = &doc.elements[&id];
        if let (Some(c), Some((mid, _))) = (&el.color, w.ids.get(&id)) {
            let css = if matches!(c.len(), 3 | 6) && c.chars().all(|c| c.is_ascii_hexdigit()) {
                format!("#{c}")
            } else {
                c.clone()
            };
            w.out.push_str(&format!("    style {mid} fill:{css}\n"));
        }
    }
    for c in &doc.trailing_comments {
        w.out.push_str(&format!("    %% {c}\n"));
    }
    w.out
}

/// [`export`] wrapped in a Markdown code block, ready for a README.
pub fn export_markdown(doc: &Document) -> String {
    format!("```mermaid\n{}```\n", export(doc))
}

struct Writer<'a> {
    doc: &'a Document,
    /// Mermaid id and, when it differs from the element name, a label.
    ids: HashMap<ElementId, (String, Option<String>)>,
    out: String,
}

/// Plain names where they are unique among the classes, otherwise the full
/// name with `_` for `.` (and a label showing the plain name).
fn class_ids(doc: &Document) -> HashMap<ElementId, (String, Option<String>)> {
    let classes: Vec<ElementId> = doc
        .elements_in_order()
        .into_iter()
        .filter(|id| doc.elements[id].kind != ElementKind::Note)
        .collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for id in &classes {
        *count.entry(doc.elements[id].name.as_str()).or_default() += 1;
    }
    let mut used: HashSet<String> = classes
        .iter()
        .map(|id| &doc.elements[id].name)
        .filter(|n| count[n.as_str()] == 1)
        .cloned()
        .collect();
    let mut ids = HashMap::new();
    for id in classes {
        let name = &doc.elements[&id].name;
        if count[name.as_str()] == 1 {
            ids.insert(id, (name.clone(), None));
            continue;
        }
        let base = doc.full_name(id).replace('.', "_");
        let mut candidate = base.clone();
        let mut n = 2;
        while !used.insert(candidate.clone()) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        let label = (candidate != *name).then(|| name.clone());
        ids.insert(id, (candidate, label));
    }
    // What the box shows, if it differs from its name.
    for (id, (_, label)) in ids.iter_mut() {
        if let Some(l) = &doc.elements[id].label {
            *label = Some(l.clone());
        }
    }
    ids
}

impl Writer<'_> {
    fn id(&self, e: ElementId) -> &str {
        self.ids.get(&e).map(|(id, _)| id.as_str()).unwrap_or("")
    }

    fn comments(&mut self, comments: &[String], indent: &str) {
        for c in comments {
            self.out.push_str(&format!("{indent}%% {c}\n"));
        }
    }

    fn decls(&mut self, decls: &[Decl], depth: usize) {
        let indent = "    ".repeat(depth);
        for d in decls {
            match d {
                Decl::Element(e) => self.element(*e, &indent),
                Decl::Raw(text) => self.out.push_str(&format!("{indent}%% {text}\n")),
                Decl::Relation(_) | Decl::Package(_) => {}
            }
        }
    }

    fn element(&mut self, id: ElementId, indent: &str) {
        let Some(el) = self.doc.elements.get(&id) else {
            return;
        };
        if el.kind == ElementKind::Note {
            return;
        }
        self.comments(&el.comments, indent);
        let (mid, label) = self.ids[&id].clone();
        let mut head = format!("{indent}class {mid}");
        if let Some(p) = &el.type_params {
            head.push_str(&format!("~{}~", generics(p)));
        }
        if let Some(l) = label {
            head.push_str(&format!("[\"{}\"]", text(&l)));
        }
        let annotation = match el.kind {
            ElementKind::Abstract => Some("abstract"),
            ElementKind::Interface => Some("interface"),
            ElementKind::Enum => Some("enumeration"),
            _ => el.stereotype.as_deref(),
        };
        if annotation.is_none() && el.members.is_empty() {
            self.out.push_str(&head);
            self.out.push('\n');
            return;
        }
        self.out.push_str(&head);
        self.out.push_str(" {\n");
        if let Some(a) = annotation {
            self.out.push_str(&format!("{indent}    <<{a}>>\n"));
        }
        for m in &el.members {
            let line = if el.kind == ElementKind::Enum {
                generics(&m.text)
            } else {
                member(m)
            };
            self.out.push_str(&format!("{indent}    {line}\n"));
        }
        self.out.push_str(&format!("{indent}}}\n"));
    }

    fn is_note(&self, e: ElementId) -> bool {
        self.doc
            .elements
            .get(&e)
            .is_some_and(|e| e.kind == ElementKind::Note)
    }

    fn relation(&mut self, id: RelationId) {
        let Some(r) = self.doc.relations.get(&id) else {
            return;
        };
        // Mermaid draws no lines to namespaces: those stay as a comment.
        let (Some(from), Some(to)) = (r.from.element(), r.to.element()) else {
            self.out.push_str(&format!(
                "    %% {} {} {} (between packages)\n",
                self.doc.endpoint_name(r.from.target),
                r.kind.arrow(),
                self.doc.endpoint_name(r.to.target)
            ));
            return;
        };
        // Notes are written as `note for`.
        if self.is_note(from) || self.is_note(to) {
            return;
        }
        self.comments(&r.comments, "    ");
        let mut line = format!("    {}", self.id(from));
        if let Some(l) = &r.from.label {
            line.push_str(&format!(" \"{}\"", text(l)));
        }
        line.push(' ');
        // Mermaid has no nesting line: a plain one, labelled below.
        line.push_str(match r.kind {
            RelationKind::Nesting => "--",
            k => k.arrow(),
        });
        if let Some(l) = &r.to.label {
            line.push_str(&format!(" \"{}\"", text(l)));
        }
        line.push(' ');
        line.push_str(self.id(to));
        let name = match (&r.name, r.kind) {
            (Some(n), _) => Some(n.replace('\n', " ")),
            (None, RelationKind::Nesting) => Some("nested".to_string()),
            _ => None,
        };
        if let Some(n) = name {
            line.push_str(" : ");
            line.push_str(&n);
        }
        line.push('\n');
        self.out.push_str(&line);
    }

    fn note(&mut self, id: ElementId) {
        let el = &self.doc.elements[&id];
        if el.kind != ElementKind::Note {
            return;
        }
        let body: Vec<&str> = el.members.iter().map(|m| m.text.as_str()).collect();
        let body = text(&body.join("\n"));
        let targets: Vec<ElementId> = self
            .doc
            .relations_in_order()
            .into_iter()
            .filter_map(|r| {
                let r = &self.doc.relations[&r];
                let other = if r.from.element() == Some(id) {
                    r.to.element()?
                } else if r.to.element() == Some(id) {
                    r.from.element()?
                } else {
                    return None;
                };
                (!self.is_note(other)).then_some(other)
            })
            .collect();
        self.comments(&el.comments, "    ");
        if targets.is_empty() {
            self.out.push_str(&format!("    note \"{body}\"\n"));
        }
        for t in targets {
            self.out
                .push_str(&format!("    note for {} \"{body}\"\n", self.id(t)));
        }
    }
}

/// A member line in Mermaid style: `+pay() Receipt*`, `-count: int$`.
fn member(m: &Member) -> String {
    let mut out = String::new();
    if let Some(v) = m.vis {
        out.push(v.symbol());
    }
    let body = if m.is_operation() {
        operation(&m.text)
    } else {
        m.text.clone()
    };
    out.push_str(&generics(&body));
    if m.is_abstract {
        out.push('*');
    }
    if m.is_static {
        out.push('$');
    }
    out
}

/// `total(a: int): Money` → `total(a: int) Money`.
fn operation(t: &str) -> String {
    let Some(close) = t.rfind(')') else {
        return t.to_string();
    };
    let (head, tail) = t.split_at(close + 1);
    let tail = tail.trim();
    let ret = tail.strip_prefix(':').map(str::trim).unwrap_or(tail);
    if ret.is_empty() {
        head.to_string()
    } else {
        format!("{head} {ret}")
    }
}

/// Mermaid writes generics with tildes: `List<T>` → `List~T~`.
fn generics(t: &str) -> String {
    t.replace(['<', '>'], "~")
}

/// Text inside a Mermaid double-quoted string.
fn text(s: &str) -> String {
    s.replace('"', "#quot;").replace('\n', "\\n")
}

fn yaml_string(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || " -_.,()!?".contains(c))
        && !s.starts_with([' ', '-', '?'])
        && !s.ends_with(' ');
    if plain {
        s.to_string()
    } else {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!("\"{escaped}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const SHOP: &str = r#"diagram "Shop"

package billing {
  abstract class Payment {
    +amount: Money
    +pay(): Receipt {abstract}
  }
  class CardPayment
}

# the order
class Order {
  +id: OrderId
  -lines: List<OrderLine>
  +total(): Money
  +{static} count: int
}

interface Priced {
  +price(): Money
}

enum Status { New, Paid, Shipped }

class Customer <<entity>>

note N1 "an order owns\nits \"lines\""

Order "1" *-- "1..* items" OrderLine : contains
billing.CardPayment --|> billing.Payment
Order ..|> Priced
Order --> billing.Payment : paid by
N1 .. Order
"#;

    #[test]
    fn exports_design_example() {
        let out = export(&parse(SHOP).doc);
        let expected = r#"---
title: Shop
---
classDiagram
    %% the order
    class Order {
        +id: OrderId
        -lines: List~OrderLine~
        +total() Money
        +count: int$
    }
    class Priced {
        <<interface>>
        +price() Money
    }
    class Status {
        <<enumeration>>
        New
        Paid
        Shipped
    }
    class Customer {
        <<entity>>
    }
    class OrderLine
    namespace billing {
        class Payment {
            <<abstract>>
            +amount: Money
            +pay() Receipt*
        }
        class CardPayment
    }
    Order "1" *-- "1..* items" OrderLine : contains
    CardPayment --|> Payment
    Order ..|> Priced
    Order --> Payment : paid by
    note for Order "an order owns\nits #quot;lines#quot;"
"#;
        assert_eq!(out, expected);
    }

    #[test]
    fn duplicate_names_get_ids_and_labels() {
        let doc = parse(
            "package a {\n  class Item\n}\npackage b {\n  package c {\n    class Item\n  }\n}\na.Item --> b.c.Item\n",
        )
        .doc;
        let out = export(&doc);
        assert!(
            out.contains("namespace a {\n        class a_Item[\"Item\"]\n"),
            "{out}"
        );
        assert!(
            out.contains("namespace b_c {\n        class b_c_Item[\"Item\"]\n"),
            "{out}"
        );
        // `b` holds only a package, so it gets no namespace of its own.
        assert!(!out.contains("namespace b {"), "{out}");
        assert!(out.contains("    a_Item --> b_c_Item\n"), "{out}");
    }

    #[test]
    fn unlinked_note_and_markdown() {
        let doc = parse("note Hint \"remember\"\nclass A\n").doc;
        let md = export_markdown(&doc);
        assert!(md.starts_with("```mermaid\nclassDiagram\n"));
        assert!(md.contains("    note \"remember\"\n"));
        assert!(md.ends_with("```\n"));
    }

    #[test]
    fn raw_lines_become_comments() {
        let doc = parse("class A\nthis is not umpl\n").doc;
        assert!(export(&doc).contains("    %% this is not umpl\n"));
    }

    #[test]
    fn titles_are_quoted_when_needed() {
        assert_eq!(yaml_string("Shop"), "Shop");
        assert_eq!(yaml_string("a: b"), "\"a: b\"");
        assert_eq!(yaml_string("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn operations_move_return_type() {
        assert_eq!(operation("f(): int"), "f() int");
        assert_eq!(operation("f(a: int)"), "f(a: int)");
        assert_eq!(operation("f(a: int) : List<T>"), "f(a: int) List<T>");
    }
}
