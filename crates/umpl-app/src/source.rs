//! The text panel: the diagram's model as `.umpl` text, kept in step with
//! the diagram both ways.
//!
//! Typing changes the diagram once typing pauses: the text is parsed and the
//! document brought to it with [`reconcile`], which keeps ids so renamed
//! classes keep their places. Changes made anywhere else (canvas, inspector,
//! undo) reload the text. The `layout` block is not shown, so moving boxes
//! leaves the text alone; a `layout` block typed here is still applied.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, Context, FontId, Id, RichText, TextBuffer, TextEdit, Ui};
use umpl_core::{reconcile, Removed};
use umpl_text::{parse, print_model, Problem, UNCLOSED};

use crate::app::App;

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
        }
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
        self.source.problems = parsed.problems.clone();
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
            self.source.problems = parse(&model).problems;
            self.source.text = model.clone();
            self.source.model = model;
            self.source.typed_at = None;
        }
    }

    pub fn toggle_source(&mut self, ctx: &Context) {
        self.source_apply(ctx, true);
        self.source.shown = !self.source.shown;
        self.source.revision = None;
        let v = if self.source.shown { "shown" } else { "hidden" };
        self.storage.save_setting("text", v);
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

        let problem_lines: HashSet<usize> = self
            .source
            .problems
            .iter()
            .filter(|p| p.message != UNCLOSED)
            .map(|p| p.line)
            .collect();
        let dark = ui.visuals().dark_mode;
        let mut layouter = |ui: &Ui, text: &dyn TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
            let job = highlight(text.as_str(), dark, &problem_lines);
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let bottom = if problem_lines.is_empty() { 0.0 } else { 70.0 };
        let r = egui::ScrollArea::both()
            .id_salt("source-scroll")
            .auto_shrink([false, false])
            .max_height(ui.available_height() - bottom)
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut self.source.text)
                        .id(Id::new(EDITOR_ID))
                        .code_editor()
                        .frame(false)
                        .desired_width(f32::INFINITY)
                        .min_size(ui.available_size())
                        .hint_text("class Order {\n  +total(): Money\n}\nOrder --> Customer")
                        .layouter(&mut layouter),
                )
            })
            .inner;
        if r.changed() {
            let time = ui.input(|i| i.time);
            // Applied SETTLE after the latest keystroke.
            self.source.typed_at = Some(time);
            self.source.problems = parse(&self.source.text).problems;
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(SETTLE));
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
            }
        }
    }
}

/// Colours `.umpl` text: keywords at the start of a line, strings,
/// `<<stereotypes>>`, arrows and comments; lines with problems get a red
/// background. Inside a class body every line is a member, so `#x: int`
/// there is a protected attribute, not a comment.
pub fn highlight(text: &str, dark: bool, problem_lines: &HashSet<usize>) -> LayoutJob {
    let pal = Palette::new(dark);
    let font = FontId::new(13.0, crate::paint::family(false, false));
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut in_body = false;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let background = if problem_lines.contains(&(n + 1)) {
            pal.problem_bg
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
        let job = highlight(text, false, &HashSet::from([7]));
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
    }
}
