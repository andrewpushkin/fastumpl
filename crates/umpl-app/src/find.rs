//! Find (Ctrl+F): a bar under the menu that searches the diagram.
//!
//! Matches are items of the document whose text holds the query, ignoring
//! case: a class by its name, stereotype or any member; a relation by its
//! name or end labels; a package by its name. Every match is outlined on
//! the canvas and every occurrence in the text panel is tinted. Enter and
//! Shift+Enter (or F3 and Shift+F3) step through the matches: each in turn
//! is selected and brought into view, which also scrolls the text panel to
//! its declaration.

use std::ops::Range;

use egui::{Color32, Context, Id, Key, RichText, TextEdit};
use umpl_core::Document;
use umpl_text::Item;

use crate::app::App;
use crate::view::Selection;

pub const FIELD_ID: &str = "find-field";

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    /// Matching items, in declaration order.
    pub matches: Vec<Item>,
    /// The match last stepped to.
    pub current: Option<usize>,
    /// Query and document revision the matches belong to.
    key: Option<(String, u64)>,
    /// Put the keyboard in the field next frame.
    focus: bool,
}

impl FindState {
    pub fn active(&self) -> bool {
        self.open && !self.query.trim().is_empty()
    }

    /// Forgets the matches (another diagram was opened).
    pub fn forget(&mut self) {
        self.key = None;
        self.matches.clear();
        self.current = None;
    }
}

/// Does `hay` contain `needle`, ignoring case?
fn contains(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(needle)
}

/// The items of `doc` whose text holds `query` (ignoring case): elements in
/// declaration order, then packages, then relations.
pub fn find_matches(doc: &Document, query: &str) -> Vec<Item> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for id in doc.elements_in_order() {
        let el = &doc.elements[&id];
        let hit = contains(&el.name, &q)
            || el.stereotype.as_deref().is_some_and(|s| contains(s, &q))
            || el.members.iter().any(|m| contains(&m.to_line(), &q));
        if hit {
            out.push(Item::Element(id));
        }
    }
    for id in doc.packages_in_order() {
        if contains(&doc.packages[&id].name, &q) {
            out.push(Item::Package(id));
        }
    }
    for id in doc.relations_in_order() {
        let r = &doc.relations[&id];
        let hit = [&r.name, &r.from.label, &r.to.label]
            .iter()
            .any(|t| t.as_deref().is_some_and(|t| contains(t, &q)));
        if hit {
            out.push(Item::Relation(id));
        }
    }
    out
}

/// Byte ranges of every occurrence of `query` in `text`, ignoring case.
pub fn text_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let q: Vec<char> = query.trim().chars().flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut starts = text.char_indices().map(|(i, _)| i).peekable();
    while let Some(start) = starts.next() {
        let mut k = 0;
        let mut end = start;
        for (i, ch) in text[start..].char_indices() {
            let mut lower = ch.to_lowercase();
            let all = lower.all(|l| {
                let ok = q.get(k) == Some(&l);
                k += 1;
                ok
            });
            if !all {
                k = usize::MAX;
                break;
            }
            if k >= q.len() {
                end = start + i + ch.len_utf8();
                break;
            }
        }
        if k == q.len() {
            out.push(start..end);
            // Occurrences do not overlap.
            while starts.peek().is_some_and(|&i| i < end) {
                starts.next();
            }
        }
    }
    out
}

/// Gives the parts of `job` inside `ranges` the background `color`.
pub fn mark(job: &mut egui::text::LayoutJob, ranges: &[Range<usize>], color: Color32) {
    if ranges.is_empty() {
        return;
    }
    let mut out = Vec::with_capacity(job.sections.len() + 2 * ranges.len());
    for sec in job.sections.drain(..) {
        let mut cuts = vec![sec.byte_range.start, sec.byte_range.end];
        for r in ranges {
            for p in [r.start, r.end] {
                if p > sec.byte_range.start
                    && p < sec.byte_range.end
                    && job.text.is_char_boundary(p)
                {
                    cuts.push(p);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for (k, w) in cuts.windows(2).enumerate() {
            let mut part = sec.clone();
            part.byte_range = w[0]..w[1];
            if k > 0 {
                part.leading_space = 0.0;
            }
            if ranges.iter().any(|r| r.start <= w[0] && w[1] <= r.end) {
                part.format.background = color;
            }
            out.push(part);
        }
    }
    job.sections = out;
}

/// The selection that picks `item` alone.
pub fn select(item: Item) -> Selection {
    match item {
        Item::Element(e) => Selection::only_element(e),
        Item::Relation(r) => Selection {
            relations: vec![r],
            ..Selection::default()
        },
        Item::Package(p) => Selection {
            packages: vec![p],
            ..Selection::default()
        },
    }
}

impl App {
    pub fn open_find(&mut self) {
        self.find.open = true;
        self.find.focus = true;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
        self.find.key = None;
        self.find.matches.clear();
        self.find.current = None;
    }

    /// Brings the matches up to date with the query and the document.
    fn refresh_matches(&mut self) {
        let Some(s) = &self.session else { return };
        let key = (self.find.query.clone(), s.editor.revision);
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        let query_changed = self.find.key.as_ref().is_none_or(|k| k.0 != key.0);
        self.find.matches = find_matches(&s.editor.doc, &self.find.query);
        self.find.key = Some(key);
        if query_changed {
            self.find.current = None;
            if !self.find.matches.is_empty() {
                self.step(1);
            }
        } else if let Some(c) = self.find.current {
            if c >= self.find.matches.len() {
                self.find.current = None;
            }
        }
    }

    /// Steps to the next (`1`) or previous (`-1`) match: selects it and
    /// brings it into view.
    pub fn step(&mut self, by: i32) {
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        let next = match self.find.current {
            None if by > 0 => 0,
            None => n - 1,
            Some(c) => (c as i64 + by as i64).rem_euclid(n as i64) as usize,
        };
        self.find.current = Some(next);
        let item = self.find.matches[next];
        self.sel = select(item);
        self.reveal(item);
    }

    /// Text ranges to tint in the text panel.
    pub fn find_text_ranges(&self) -> Vec<Range<usize>> {
        if !self.find.active() {
            return Vec::new();
        }
        text_ranges(&self.source.text, &self.find.query)
    }

    /// The find bar under the menu.
    pub fn find_bar(&mut self, ctx: &Context) {
        if !self.find.open || self.session.is_none() {
            return;
        }
        egui::TopBottomPanel::top("find").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Find");
                let id = Id::new(FIELD_ID);
                let r = ui.add(
                    TextEdit::singleline(&mut self.find.query)
                        .id(id)
                        .desired_width(260.0)
                        .hint_text("class, member, relation or package"),
                );
                if self.find.focus {
                    r.request_focus();
                    self.find.focus = false;
                }
                let (enter, shift, escape) = ui.input(|i| {
                    (
                        i.key_pressed(Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(Key::Escape),
                    )
                });
                if r.lost_focus() && escape {
                    self.close_find();
                    return;
                }
                self.refresh_matches();
                if r.lost_focus() && enter {
                    self.step(if shift { -1 } else { 1 });
                    r.request_focus();
                }
                let n = self.find.matches.len();
                let status = match (n, self.find.current) {
                    _ if self.find.query.trim().is_empty() => String::new(),
                    (0, _) => "No matches".into(),
                    (n, Some(c)) => format!("{} of {n}", c + 1),
                    (n, None) => format!("{n} matches"),
                };
                let color = if n == 0 && !self.find.query.trim().is_empty() {
                    Color32::from_rgb(0xd9, 0x48, 0x3b)
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(RichText::new(status).color(color));
                if ui
                    .add_enabled(n > 0, egui::Button::new("▲"))
                    .on_hover_text("Previous match (Shift+Enter, Shift+F3)")
                    .clicked()
                {
                    self.step(-1);
                }
                if ui
                    .add_enabled(n > 0, egui::Button::new("▼"))
                    .on_hover_text("Next match (Enter, F3)")
                    .clicked()
                {
                    self.step(1);
                }
                if ui.button("✕").on_hover_text("Close (Esc)").clicked() {
                    self.close_find();
                }
            });
        });
        // F3 and Shift+F3 step from anywhere while the bar is open.
        let f3 = ctx.input(|i| i.key_pressed(Key::F3).then_some(i.modifiers.shift));
        if let Some(shift) = f3 {
            self.step(if shift { -1 } else { 1 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_names_members_labels_and_packages() {
        let doc = umpl_text::parse(
            "package billing {\n  class Invoice {\n    +total(): Money\n  }\n}\nclass Order <<entity>>\nclass Cart\nOrder --> billing.Invoice : bills\nCart \"1\" -- \"*\" Order\n",
        )
        .doc;
        let names = |q: &str| -> Vec<String> {
            find_matches(&doc, q)
                .into_iter()
                .map(|i| match i {
                    Item::Element(e) => doc.full_name(e),
                    Item::Package(p) => format!("pkg {}", doc.full_package_name(p)),
                    Item::Relation(r) => format!("rel {:?}", doc.relations[&r].name),
                })
                .collect()
        };
        assert_eq!(names("ORDER"), ["Order"]);
        assert_eq!(names("money"), ["billing.Invoice"]);
        assert_eq!(names("ENTITY"), ["Order"]);
        assert_eq!(names("bill"), ["pkg billing", "rel Some(\"bills\")"]);
        assert_eq!(names("*"), ["rel None"]);
        assert!(names("  ").is_empty());
        assert!(names("nothing").is_empty());
    }

    #[test]
    fn text_ranges_ignore_case_and_do_not_overlap() {
        assert_eq!(
            text_ranges("Order order ORDER", "order"),
            [0..5, 6..11, 12..17]
        );
        assert_eq!(text_ranges("aaaa", "aa"), [0..2, 2..4]);
        assert_eq!(text_ranges("Ünï ünï", "ÜNÏ"), [0..5, 6..11]);
        assert!(text_ranges("abc", "").is_empty());
    }

    #[test]
    fn marking_splits_sections() {
        let mut job = egui::text::LayoutJob::default();
        job.append("class Order", 0.0, Default::default());
        job.append("\nOrder --> X", 0.0, Default::default());
        let ranges = text_ranges(&job.text, "order");
        mark(&mut job, &ranges, Color32::YELLOW);
        let marked: Vec<&str> = job
            .sections
            .iter()
            .filter(|s| s.format.background == Color32::YELLOW)
            .map(|s| &job.text[s.byte_range.clone()])
            .collect();
        assert_eq!(marked, ["Order", "Order"]);
        let all: String = job
            .sections
            .iter()
            .map(|s| &job.text[s.byte_range.clone()])
            .collect();
        assert_eq!(all, job.text);
    }
}
