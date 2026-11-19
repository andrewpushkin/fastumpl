use umpl_core::*;

use crate::scan::quote;

/// Writes a document in the canonical `.umpl` style.
pub fn print(doc: &Document) -> String {
    // A sequence diagram is kept as PlantUML text: it has no positions.
    if doc.sequence.is_some() {
        return crate::plantuml::export(doc);
    }
    let mut out = print_model(doc);
    // Full name, position, and the size a box was resized to.
    type Placed = (String, Pos, Option<(i32, i32)>);
    let mut positions: Vec<Placed> = doc
        .elements
        .iter()
        .filter_map(|(id, el)| el.pos.map(|p| (doc.full_name(*id), p, el.size)))
        .collect();
    positions.sort_by(|a, b| a.0.cmp(&b.0));
    if !positions.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let width = positions
            .iter()
            .map(|(n, _, _)| n.chars().count())
            .max()
            .unwrap_or(0);
        out.push_str("layout {\n");
        for (name, p, size) in positions {
            let pad = width - name.chars().count();
            out.push_str(&format!("  {name}{} at {}, {}", " ".repeat(pad), p.x, p.y));
            if let Some((w, h)) = size {
                out.push_str(&format!(" size {w}, {h}"));
            }
            out.push('\n');
        }
        out.push_str("}\n");
    }
    out
}

/// The model part of [`print`]: everything except the `layout` block.
pub fn print_model(doc: &Document) -> String {
    if doc.sequence.is_some() {
        return crate::plantuml::export(doc);
    }
    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str("diagram ");
        out.push_str(&quote(t));
        out.push('\n');
    }
    if doc.left_to_right {
        out.push_str("direction left to right\n");
    }
    let mut w = Writer { doc, out };
    w.decls(&doc.root, 0, doc.title.is_some() || doc.left_to_right);

    let mut out = w.out;
    if !doc.trailing_comments.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        for c in &doc.trailing_comments {
            push_comment(&mut out, "", c);
        }
    }
    out
}

struct Writer<'a> {
    doc: &'a Document,
    out: String,
}

fn push_comment(out: &mut String, indent: &str, c: &str) {
    out.push_str(indent);
    if c.is_empty() {
        out.push_str("#\n");
    } else {
        out.push_str("# ");
        out.push_str(c);
        out.push('\n');
    }
}

impl Writer<'_> {
    fn decls(&mut self, decls: &[Decl], depth: usize, mut need_gap: bool) {
        let indent = "  ".repeat(depth);
        let mut prev: Option<&Decl> = None;
        for d in decls {
            // Consecutive relations and raw lines stay together.
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
                    self.out.push_str(text);
                    self.out.push('\n');
                }
            }
        }
    }

    fn comments(&mut self, comments: &[String], indent: &str) {
        for c in comments {
            push_comment(&mut self.out, indent, c);
        }
    }

    fn package(&mut self, id: PackageId, depth: usize) {
        let Some(p) = self.doc.packages.get(&id) else {
            return;
        };
        let indent = "  ".repeat(depth);
        self.comments(&p.comments, &indent);
        let color = p
            .color
            .as_ref()
            .map(|c| format!(" #{c}"))
            .unwrap_or_default();
        if p.body.is_empty() {
            self.out.push_str(&format!(
                "{indent}package {}{color} {{}}\n",
                name_in_path(&p.name)
            ));
        } else {
            self.out.push_str(&format!(
                "{indent}package {}{color} {{\n",
                name_in_path(&p.name)
            ));
            self.decls(&p.body, depth + 1, false);
            self.out.push_str(&format!("{indent}}}\n"));
        }
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
                "{indent}note {} {}",
                name_in_path(&el.name),
                quote(&text.join("\n"))
            ));
            if let Some(c) = &el.color {
                out.push_str(&format!(" #{c}"));
            }
            out.push('\n');
            return;
        }
        out.push_str(indent);
        out.push_str(el.kind.keyword());
        out.push(' ');
        out.push_str(&name_in_path(&el.name));
        if let Some(p) = &el.type_params {
            out.push_str(&format!("<{p}>"));
        }
        if let Some(l) = &el.label {
            out.push_str(&format!(" as {}", quote(l)));
        }
        if let Some(s) = &el.stereotype {
            out.push_str(&format!(" <<{s}>>"));
        }
        if let Some(c) = &el.color {
            out.push_str(&format!(" #{c}"));
        }
        if el.members.is_empty() {
            out.push('\n');
            return;
        }
        if el.kind == ElementKind::Enum {
            let literals: Vec<String> = el.members.iter().map(|m| m.to_line()).collect();
            let inline = literals.join(", ");
            if inline.len() <= 60 && !inline.contains('\n') {
                out.push_str(&format!(" {{ {inline} }}\n"));
                return;
            }
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
        let mut line = String::from(indent);
        line.push_str(&self.doc.endpoint_name(r.from.target));
        if let Some(l) = &r.from.label {
            line.push(' ');
            line.push_str(&quote(l));
        }
        line.push(' ');
        line.push_str(r.kind.arrow());
        if let Some(l) = &r.to.label {
            line.push(' ');
            line.push_str(&quote(l));
        }
        line.push(' ');
        line.push_str(&self.doc.endpoint_name(r.to.target));
        if let Some(n) = &r.name {
            // A label over several lines: `a\nb`.
            line.push_str(" : ");
            line.push_str(&n.replace('\n', "\\n"));
        }
        line.push('\n');
        self.out.push_str(&line);
    }
}
