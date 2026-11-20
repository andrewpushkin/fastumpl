//! The text panel: the diagram's model as `.umpl` text, kept in step with
//! the diagram both ways.
//!
//! Typing changes the diagram once typing pauses: the text is parsed and the
//! document brought to it with [`reconcile`], which keeps ids so renamed
//! classes keep their places. Changes made anywhere else (canvas, inspector,
//! undo) reload the text. The `layout` block is not shown, so moving boxes
//! leaves the text alone; a `layout` block typed here is still applied.
//!
//! The panel also follows the selection: picking an item in the drawing
//! tints the lines that declare it, scrolls them into view and puts the text
//! cursor there; moving the text cursor into a declaration selects that item
//! in the drawing and brings it into view.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, Context, FontId, Id, RichText, TextBuffer, TextEdit, Ui};
use umpl_core::{reconcile, Removed};
use umpl_text::{parse, print_model, Item, Parsed, Problem, UNCLOSED};

use crate::app::App;
use crate::link::{line_of_char, line_start_char, Links};
use crate::view::Selection;

/// Seconds typing must pause before the text is applied to the diagram.
pub const SETTLE: f64 = 0.25;
/// Typing after a pause this long (in seconds) starts a new undo step.
const NEW_STEP_AFTER: f64 = 2.0;

pub const EDITOR_ID: &str = "source-text";

pub struct SourceState {
    pub shown: bool,
    pub text: String,
    /// The model text of the document when `text` was last loaded or
    /// applied; the text is reloaded only when the model moves away from it.
    model: String,
    /// Editor revision `text` was last checked against.
    revision: Option<u64>,
    /// Input time of the latest keystroke not yet applied.
    typed_at: Option<f64>,
    last_apply: f64,
    /// Elements that text edits removed, to restore if their names return.
    removed: Removed,
    /// Problems in `text`.
    pub problems: Vec<Problem>,
    /// Where `text` declares each item.
    links: Links,
    /// Counts parses of `text`, so lookups know when to run again.
    generation: u64,
    /// Lines tinted as the declaration of what is selected (1-based,
    /// inclusive).
    pub highlight: Vec<(usize, usize)>,
    /// The selection as last followed: the text cursor moves to an item
    /// only when the selection differs from this.
    seen_sel: Option<Selection>,
    /// The text generation `highlight` was worked out for.
    tinted: Option<u64>,
    /// Where the text cursor was last looked up: line, text generation and
    /// document revision.
    cursor_seen: Option<(usize, u64, u64)>,
    /// The item the text cursor is in, as last found.
    cursor_item: Option<Item>,
    /// A character to scroll into view once the text is laid out.
    scroll_to: Option<usize>,
    /// The text as last laid out, with what it was laid out for: a long
    /// text is coloured once, not every frame.
    galley: Option<(GalleyKey, Arc<egui::Galley>)>,
    /// Find's ranges in the text, for the query and text generation.
    found: Option<(String, u64, Vec<std::ops::Range<usize>>)>,
}

/// What a laid-out text depends on.
#[derive(PartialEq)]
struct GalleyKey {
    text: String,
    dark: bool,
    size: f32,
    problems: Vec<usize>,
    linked: Vec<(usize, usize)>,
    found: Vec<std::ops::Range<usize>>,
}

impl SourceState {
    pub fn new(shown: bool) -> SourceState {
        SourceState {
            shown,
            text: String::new(),
            model: String::new(),
            revision: None,
            typed_at: None,
            last_apply: f64::NEG_INFINITY,
            removed: Removed::new(),
            problems: Vec::new(),
            links: Links::default(),
            generation: 0,
            highlight: Vec::new(),
            seen_sel: None,
            tinted: None,
            cursor_seen: None,
            cursor_item: None,
            scroll_to: None,
            galley: None,
            found: None,
        }
    }

    /// Takes in a parse of `text`: its problems and declarations.
    fn take_parse(&mut self, parsed: &Parsed) {
        self.problems = parsed.problems.clone();
        self.links = Links::new(parsed);
        // Lines may have moved: the new generation tints them again.
        self.generation += 1;
    }

    /// Forgets the text (another diagram was opened); keeps visibility.
    pub fn reset(&mut self) {
        *self = SourceState::new(self.shown);
    }

    /// Whether typed text is waiting to be applied.
    pub fn pending(&self) -> bool {
        self.typed_at.is_some()
    }

    /// A `{` is still open: applying now would pull every following line
    /// into that body, so the text waits.
    pub fn waiting_for_brace(&self) -> bool {
        self.problems.iter().any(|p| p.message == UNCLOSED)
    }
}

impl App {
    /// Applies typed text to the diagram once typing has paused, or at once
    /// with `now`.
    pub fn source_apply(&mut self, ctx: &Context, now: bool) {
        let Some(typed_at) = self.source.typed_at else {
            return;
        };
        let time = ctx.input(|i| i.time);
        if !now && time - typed_at < SETTLE {
            ctx.request_repaint_after(Duration::from_secs_f64(SETTLE - (time - typed_at)));
            return;
        }
        self.source.typed_at = None;
        let parsed = parse(&self.source.text);
        self.source.take_parse(&parsed);
        if self.source.waiting_for_brace() {
            return;
        }
        let Some(s) = &mut self.session else { return };
        if time - self.source.last_apply > NEW_STEP_AFTER {
            s.editor.seal();
        }
        self.source.last_apply = time;
        let ops = reconcile(&mut s.editor.doc, &parsed.doc, &mut self.source.removed);
        s.editor.transact_merge("text", "Edit text", ops);
        s.problems = parsed.problems;
        self.sel.retain_existing(&s.editor.doc);
        self.source.model = print_model(&s.editor.doc);
        self.source.revision = Some(s.editor.revision);
    }

    /// Reloads the text if the model was changed from outside the panel.
    pub fn source_refresh(&mut self) {
        let Some(s) = &self.session else { return };
        if !self.source.shown || self.source.revision == Some(s.editor.revision) {
            return;
        }
        self.source.revision = Some(s.editor.revision);
        let model = print_model(&s.editor.doc);
        if model != self.source.model {
            self.source.take_parse(&parse(&model));
            self.source.text = model.clone();
            self.source.model = model;
            self.source.typed_at = None;
        }
    }

    pub fn toggle_source(&mut self, ctx: &Context) {
        self.source_apply(ctx, true);
        self.source.shown = !self.source.shown;
        self.source.revision = None;
        // Hiding the text leaves the drawing, also from View: Text.
        if !self.source.shown {
            self.text_only = false;
        }
        let v = if self.source.shown { "shown" } else { "hidden" };
        self.storage.save_setting("text", v);
        let view = if self.text_only {
            "text"
        } else if self.source.shown {
            "both"
        } else {
            "canvas"
        };
        self.storage.save_setting("view", view);
    }

    pub fn source_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Text").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("«")
                    .on_hover_text(format!(
                        "Hide text ({})",
                        shortcut_text(ui.ctx(), crate::commands::Cmd::ToggleSource)
                    ))
                    .clicked()
                {
                    self.toggle_source(ui.ctx());
                }
                let warn = Color32::from_rgb(0xd0, 0x83, 0x2a);
                let problems = self
                    .source
                    .problems
                    .iter()
                    .filter(|p| p.message != UNCLOSED)
                    .count();
                if self.source.waiting_for_brace() {
                    ui.label(RichText::new("waiting for `}`").color(warn))
                        .on_hover_text("A `{` is not closed yet; the diagram updates once it is");
                } else if self.source.pending() {
                    ui.label(RichText::new("typing…").weak());
                } else if problems > 0 {
                    ui.label(RichText::new(format!("{problems} problem(s)")).color(warn))
                        .on_hover_text("Lines not understood are kept as written");
                } else {
                    ui.label(RichText::new("in sync").weak());
                }
            });
        });
        ui.separator();

        self.follow_selection(ui.ctx());
        let problem_lines: HashSet<usize> = self
            .source
            .problems
            .iter()
            .filter(|p| p.message != UNCLOSED)
            .map(|p| p.line)
            .collect();
        let dark = ui.visuals().dark_mode;
        let size = self.prefs.text_size;
        let linked = self.source.highlight.clone();
        let found = self.found_ranges();
        let found_bg = if dark {
            Color32::from_rgba_unmultiplied(0xe8, 0xb0, 0x30, 90)
        } else {
            Color32::from_rgba_unmultiplied(0xff, 0xc8, 0x3d, 140)
        };
        let mut problems: Vec<usize> = problem_lines.iter().copied().collect();
        problems.sort_unstable();
        let cache = &mut self.source.galley;
        let mut layouter = |ui: &Ui, text: &dyn TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            let text = text.as_str();
            if let Some((key, galley)) = cache.as_ref() {
                let same = key.text == text
                    && key.dark == dark
                    && key.size == size
                    && key.problems == problems
                    && key.linked == linked
                    && key.found == found;
                if same {
                    return galley.clone();
                }
            }
            let mut job = highlight(text, dark, size, &problem_lines, &linked);
            // Ranges found in an older text are dropped, not misplaced.
            if found.iter().all(|r| r.end <= job.text.len()) {
                crate::find::mark(&mut job, &found, found_bg);
            }
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            *cache = Some((
                GalleyKey {
                    text: text.to_string(),
                    dark,
                    size,
                    problems: problems.clone(),
                    linked: linked.clone(),
                    found: found.clone(),
                },
                galley.clone(),
            ));
            galley
        };
        let bottom = if problem_lines.is_empty() { 0.0 } else { 70.0 };
        let scroll_to = self.source.scroll_to.take();
        let out = egui::ScrollArea::both()
            .id_salt("source-scroll")
            .auto_shrink([false, false])
            .max_height(ui.available_height() - bottom)
            .show(ui, |ui| {
                let out = TextEdit::multiline(&mut self.source.text)
                    .id(Id::new(EDITOR_ID))
                    .code_editor()
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .min_size(ui.available_size())
                    .hint_text("class Order {\n  +total(): Money\n}\nOrder --> Customer")
                    .layouter(&mut layouter)
                    .show(ui);
                if let Some(c) = scroll_to {
                    let at = out
                        .galley
                        .pos_from_cursor(CCursor::new(c))
                        .translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(at.expand2(egui::vec2(40.0, 0.0)), Some(egui::Align::Center));
                }
                out
            })
            .inner;
        let r = out.response;
        if r.changed() {
            let time = ui.input(|i| i.time);
            // Applied SETTLE after the latest keystroke.
            self.source.typed_at = Some(time);
            let parsed = parse(&self.source.text);
            self.source.take_parse(&parsed);
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(SETTLE));
        }
        if r.has_focus() {
            if let Some(range) = out.cursor_range {
                let line = line_of_char(&self.source.text, range.primary.index);
                self.text_cursor_at(line);
            }
        }
        if r.lost_focus() {
            self.source_apply(ui.ctx(), true);
            if let Some(s) = &mut self.session {
                s.editor.seal();
            }
        }

        if !problem_lines.is_empty() {
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("source-problems")
                .show(ui, |ui| {
                    for p in self
                        .source
                        .problems
                        .iter()
                        .filter(|p| p.message != UNCLOSED)
                    {
                        ui.label(
                            RichText::new(format!("line {}: {}", p.line, p.message))
                                .small()
                                .color(Color32::from_rgb(0xd9, 0x48, 0x3b)),
                        );
                    }
                });
        }
    }
}

impl App {
    /// Find's ranges in the text panel, found again only when the query or
    /// the text changed.
    fn found_ranges(&mut self) -> Vec<std::ops::Range<usize>> {
        if !self.find.active() {
            return Vec::new();
        }
        let generation = self.source.generation;
        let fresh = self
            .source
            .found
            .as_ref()
            .is_some_and(|(q, g, _)| *q == self.find.query && *g == generation);
        if !fresh {
            let ranges = self.find_text_ranges();
            self.source.found = Some((self.find.query.clone(), generation, ranges));
        }
        self.source
            .found
            .as_ref()
            .map(|f| f.2.clone())
            .unwrap_or_default()
    }

    /// The item a selection is about: its first element, else its first
    /// relation, else its first package.
    fn selection_items(sel: &Selection) -> Vec<Item> {
        let mut items: Vec<Item> = sel.elements.iter().map(|e| Item::Element(*e)).collect();
        items.extend(sel.relations.iter().map(|r| Item::Relation(*r)));
        items.extend(sel.packages.iter().map(|p| Item::Package(*p)));
        items
    }

    /// Drawing to text: when the selection changed, tints the lines that
    /// declare the selected items, scrolls to the first and puts the text
    /// cursor there. The keyboard stays with the canvas.
    fn follow_selection(&mut self, ctx: &Context) {
        // Typing re-parses the text; that tints the lines again but must
        // not move the cursor, which is where the typing is.
        let changed = self.source.seen_sel.as_ref() != Some(&self.sel);
        if !changed && self.source.tinted == Some(self.source.generation) {
            return;
        }
        self.source.seen_sel = Some(self.sel.clone());
        self.source.tinted = Some(self.source.generation);
        let Some(s) = &self.session else { return };
        let doc = &s.editor.doc;
        let items = App::selection_items(&self.sel);
        self.source.highlight = items
            .iter()
            .filter_map(|i| self.source.links.lines_of(doc, *i))
            .collect();
        // The cursor already in the first item stays where it is.
        let first = items.first().copied();
        if !changed || first.is_none() || first == self.source.cursor_item {
            return;
        }
        self.source.cursor_item = first;
        if let Some(&(line, _)) = self.source.highlight.first() {
            let c = line_start_char(&self.source.text, line);
            self.source.scroll_to = Some(c);
            let id = Id::new(EDITOR_ID);
            let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            TextEdit::store_state(ctx, id, state);
            // Already looked up: the cursor is in that item.
            self.source.cursor_seen = Some((line, self.source.generation, s.editor.revision));
        }
    }

    /// Text to drawing: the text cursor is on `line`; selects the item
    /// declared there and brings it into view. A line that declares
    /// nothing (blank, a comment, half-typed text) leaves the selection.
    fn text_cursor_at(&mut self, line: usize) {
        let Some(s) = &self.session else { return };
        let key = (line, self.source.generation, s.editor.revision);
        if self.source.cursor_seen == Some(key) {
            return;
        }
        self.source.cursor_seen = Some(key);
        let Some((item, lines)) = self.source.links.item_at(&s.editor.doc, line) else {
            self.source.cursor_item = None;
            return;
        };
        self.source.highlight = vec![lines];
        if Some(item) == self.source.cursor_item {
            return;
        }
        self.source.cursor_item = Some(item);
        self.sel = match item {
            Item::Element(e) => Selection::only_element(e),
            Item::Relation(r) => Selection {
                relations: vec![r],
                ..Selection::default()
            },
            Item::Package(p) => Selection {
                packages: vec![p],
                ..Selection::default()
            },
        };
        self.source.seen_sel = Some(self.sel.clone());
        self.reveal(item);
    }
}

fn shortcut_text(ctx: &Context, cmd: crate::commands::Cmd) -> String {
    cmd.shortcut()
        .map(|sc| ctx.format_shortcut(&sc))
        .unwrap_or_default()
}

const KEYWORDS: [&str; 8] = [
    "diagram",
    "package",
    "class",
    "abstract",
    "interface",
    "enum",
    "note",
    "layout",
];

struct Palette {
    plain: Color32,
    keyword: Color32,
    string: Color32,
    comment: Color32,
    arrow: Color32,
    stereotype: Color32,
    problem_bg: Color32,
    linked_bg: Color32,
}

impl Palette {
    fn new(dark: bool) -> Palette {
        if dark {
            Palette {
                plain: Color32::from_rgb(0xd4, 0xd4, 0xd4),
                keyword: Color32::from_rgb(0x6c, 0xb6, 0xff),
                string: Color32::from_rgb(0x9c, 0xd6, 0x8a),
                comment: Color32::from_rgb(0x80, 0x86, 0x8f),
                arrow: Color32::from_rgb(0xe0, 0x9a, 0xf0),
                stereotype: Color32::from_rgb(0x4e, 0xc9, 0xb0),
                problem_bg: Color32::from_rgba_unmultiplied(0xd9, 0x48, 0x3b, 60),
                linked_bg: Color32::from_rgba_unmultiplied(0x3d, 0x8b, 0xfd, 50),
            }
        } else {
            Palette {
                plain: Color32::from_rgb(0x24, 0x29, 0x2e),
                keyword: Color32::from_rgb(0x00, 0x5c, 0xc5),
                string: Color32::from_rgb(0x22, 0x86, 0x3a),
                comment: Color32::from_rgb(0x6a, 0x73, 0x7d),
                arrow: Color32::from_rgb(0x8a, 0x3f, 0xb0),
                stereotype: Color32::from_rgb(0x0b, 0x7a, 0x6e),
                problem_bg: Color32::from_rgba_unmultiplied(0xd9, 0x48, 0x3b, 40),
                linked_bg: Color32::from_rgba_unmultiplied(0x0b, 0x69, 0xd8, 34),
            }
        }
    }
}

/// Colours `.umpl` text: keywords at the start of a line, strings,
/// `<<stereotypes>>`, arrows and comments; lines with problems get a red
/// background, lines in `linked` (the declaration of what is selected) a
/// blue one. Inside a class body every line is a member, so `#x: int`
/// there is a protected attribute, not a comment.
pub fn highlight(
    text: &str,
    dark: bool,
    size: f32,
    problem_lines: &HashSet<usize>,
    linked: &[(usize, usize)],
) -> LayoutJob {
    let pal = Palette::new(dark);
    let font = FontId::new(size, crate::paint::family(false, false));
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut in_body = false;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let background = if problem_lines.contains(&(n + 1)) {
            pal.problem_bg
        } else if linked.iter().any(|(a, b)| *a <= n + 1 && n < *b) {
            pal.linked_bg
        } else {
            Color32::TRANSPARENT
        };
        let mut push = |s: &str, color: Color32| {
            job.append(
                s,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color,
                    background,
                    ..Default::default()
                },
            );
        };
        let trimmed = line.trim();
        if in_body {
            in_body = trimmed != "}";
            push(line, pal.plain);
            continue;
        }
        if trimmed.starts_with('#') {
            push(line, pal.comment);
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        in_body =
            ["class", "abstract", "interface", "enum"].contains(&first) && trimmed.ends_with('{');
        let bytes = line.as_bytes();
        let mut i = 0;
        // Still in the leading keywords (`abstract class`)?
        let mut lead = true;
        while i < bytes.len() {
            let c = line[i..].chars().next().unwrap();
            let start = i;
            if c == '"' {
                let mut end = bytes.len();
                let mut escaped = false;
                for (k, ch) in line[i + 1..].char_indices() {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        end = i + 1 + k + 1;
                        break;
                    }
                }
                i = end;
                push(&line[start..i], pal.string);
                lead = false;
            } else if line[i..].starts_with("<<") {
                i = line[i..].find(">>").map_or(bytes.len(), |e| i + e + 2);
                push(&line[start..i], pal.stereotype);
                lead = false;
            } else if c.is_alphanumeric() || c == '_' {
                while let Some(c) = line[i..].chars().next() {
                    if !(c.is_alphanumeric() || c == '_' || c == '.') {
                        break;
                    }
                    i += c.len_utf8();
                }
                let word = &line[start..i];
                let is_kw = lead && KEYWORDS.contains(&word);
                lead &= is_kw;
                push(word, if is_kw { pal.keyword } else { pal.plain });
            } else if "-.|<>*".contains(c) {
                while i < bytes.len() && b"-.|<>*".contains(&bytes[i]) {
                    i += 1;
                }
                let run = &line[start..i];
                let color = if run.len() >= 2 { pal.arrow } else { pal.plain };
                push(run, color);
                lead = false;
            } else {
                i += c.len_utf8();
                push(&line[start..i], pal.plain);
                if !c.is_whitespace() {
                    lead = false;
                }
            }
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_keeps_every_character() {
        let text = "diagram \"Shop\"\n# c\nabstract class A <<x>> {\n  #f(): \"q\\é\"\n}\nA \"1\" *-- \"*\" B : has\nnöt ünderstood\n";
        let job = highlight(text, false, 13.0, &HashSet::from([7]), &[(3, 5)]);
        assert_eq!(job.text, text);
        let kw = Palette::new(false).keyword;
        let colored = |s: &str| {
            job.sections
                .iter()
                .find(|sec| &job.text[sec.byte_range.clone()] == s)
                .map(|sec| sec.format.color)
        };
        assert_eq!(colored("abstract"), Some(kw));
        assert_eq!(colored("class"), Some(kw));
        assert_eq!(colored("*--"), Some(Palette::new(false).arrow));
        assert_ne!(colored("A"), Some(kw));
        let pal = Palette::new(false);
        assert_eq!(colored("# c\n"), Some(pal.comment));
        // In a body `#` marks a protected member.
        assert_eq!(colored("  #f(): \"q\\é\"\n"), Some(pal.plain));
        let background = |s: &str| {
            job.sections
                .iter()
                .find(|sec| &job.text[sec.byte_range.clone()] == s)
                .map(|sec| sec.format.background)
        };
        assert_eq!(background("abstract"), Some(pal.linked_bg));
        assert_eq!(background("}\n"), Some(pal.linked_bg));
        assert_eq!(background("# c\n"), Some(Color32::TRANSPARENT));
        assert_eq!(background("nöt"), Some(pal.problem_bg));
    }
}

#[cfg(test)]
mod timing {
    /// Manual: `UMPL_TEXT=file cargo test --release -p umpl-app timing -- --ignored --nocapture`
    #[test]
    #[ignore = "manual timing"]
    fn highlight_time() {
        let Ok(path) = std::env::var("UMPL_TEXT") else {
            return;
        };
        let text =
            umpl_text::print_model(&umpl_text::parse(&std::fs::read_to_string(path).unwrap()).doc);
        let start = std::time::Instant::now();
        let job = super::highlight(&text, false, 13.0, &Default::default(), &[(10, 20)]);
        let t1 = start.elapsed();
        let ranges = crate::find::text_ranges(&text, "account");
        let t2 = start.elapsed() - t1;
        println!(
            "{} bytes, {} sections: highlight {t1:?}, find {t2:?} ({} hits)",
            text.len(),
            job.sections.len(),
            ranges.len()
        );
    }
}
