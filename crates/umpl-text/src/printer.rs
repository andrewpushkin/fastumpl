use umpl_core::*;

use crate::scan::quote;

/// Writes a document in the canonical `.umpl` style.
pub fn print(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(t) = &doc.title {
        out.push_str("diagram ");
        out.push_str(&quote(t));
        out.push('\n');
    }
    let mut w = Writer { doc, out };
    w.decls(&doc.root, 0, doc.title.is_some());

    let mut out = w.out;
    if !doc.trailing_comments.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        for c in &doc.trailing_comments {
            push_comment(&mut out, "", c);
        }
    }

    let mut positions: Vec<(String, Pos)> = doc
        .elements
        .iter()
        .filter_map(|(id, el)| el.pos.map(|p| (doc.full_name(*id), p)))
        .collect();
    positions.sort_by(|a, b| a.0.cmp(&b.0));
    if !positions.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let width = positions.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
        out.push_str("layout {\n");
        for (name, p) in positions {
            let pad = width - name.chars().count();
            out.push_str(&format!("  {name}{} at {}, {}\n", " ".repeat(pad), p.x, p.y));
        }
        out.push_str("}\n");
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
        let Some(p) = self.doc.packages.get(&id) else { return };
        let indent = "  ".repeat(depth);
        self.comments(&p.comments, &indent);
        if p.body.is_empty() {
            self.out.push_str(&format!("{indent}package {} {{}}\n", p.name));
        } else {
            self.out.push_str(&format!("{indent}package {} {{\n", p.name));
            self.decls(&p.body, depth + 1, false);
            self.out.push_str(&format!("{indent}}}\n"));
        }
    }

    fn element(&mut self, id: ElementId, indent: &str) {
        let Some(el) = self.doc.elements.get(&id) else { return };
        self.comments(&el.comments, indent);
        let out = &mut self.out;
        if el.kind == ElementKind::Note {
            let text: Vec<&str> = el.members.iter().map(|m| m.text.as_str()).collect();
            out.push_str(&format!("{indent}note {} {}\n", el.name, quote(&text.join("\n"))));
            return;
        }
        out.push_str(indent);
        out.push_str(el.kind.keyword());
        out.push(' ');
        out.push_str(&el.name);
        if let Some(s) = &el.stereotype {
            out.push_str(&format!(" <<{s}>>"));
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
        let Some(r) = self.doc.relations.get(&id) else { return };
        self.comments(&r.comments, indent);
        let mut line = String::from(indent);
        line.push_str(&self.doc.full_name(r.from.element));
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
        line.push_str(&self.doc.full_name(r.to.element));
        if let Some(n) = &r.name {
            line.push_str(" : ");
            line.push_str(n);
        }
        line.push('\n');
        self.out.push_str(&line);
    }
}
