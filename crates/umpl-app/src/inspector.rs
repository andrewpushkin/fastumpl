//! The panel on the right: edits whatever is selected as form fields.

use egui::{Color32, ComboBox, RichText, TextEdit, Ui};
use umpl_core::{ElementId, ElementKind, PackageId, RelationId, RelationKind};

use crate::app::App;
use crate::canvas::{members_text, parse_members};
use crate::commands::Cmd;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Nothing,
    Element(ElementId),
    Relation(RelationId),
    Package(PackageId),
    Many,
}

/// Text being edited in the inspector, reloaded when the selection or the
/// document changes while nothing is being typed.
#[derive(Default)]
pub struct InspectorState {
    target: Option<Target>,
    revision: u64,
    name: String,
    type_params: String,
    stereotype: String,
    members: String,
    from_label: String,
    to_label: String,
    rel_name: String,
    title: String,
    error: Option<String>,
}

fn error_ui(ui: &mut Ui, error: &Option<String>) {
    if let Some(e) = error {
        ui.colored_label(Color32::from_rgb(0xd9, 0x48, 0x3b), e);
    }
}

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

impl App {
    fn inspector_target(&self) -> Target {
        let s = &self.sel;
        match (s.elements.len(), s.relations.len(), s.packages.len()) {
            (0, 0, 0) => Target::Nothing,
            (1, 0, 0) => Target::Element(s.elements[0]),
            (0, 1, 0) => Target::Relation(s.relations[0]),
            (0, 0, 1) => Target::Package(s.packages[0]),
            _ => Target::Many,
        }
    }

    fn load_inspector(&mut self, target: Target) {
        let Some(s) = &self.session else { return };
        let doc = &s.editor.doc;
        let st = &mut self.inspector;
        st.target = Some(target);
        st.revision = s.editor.revision;
        st.error = None;
        match target {
            Target::Element(id) => {
                if let Some(el) = doc.elements.get(&id) {
                    st.name = el.name.clone();
                    st.type_params = el.type_params.clone().unwrap_or_default();
                    st.stereotype = el.stereotype.clone().unwrap_or_default();
                    st.members = members_text(el);
                }
            }
            Target::Relation(id) => {
                if let Some(r) = doc.relations.get(&id) {
                    st.from_label = r.from.label.clone().unwrap_or_default();
                    st.to_label = r.to.label.clone().unwrap_or_default();
                    st.rel_name = r.name.clone().unwrap_or_default();
                }
            }
            Target::Package(id) => {
                if let Some(p) = doc.packages.get(&id) {
                    st.name = p.name.clone();
                }
            }
            Target::Nothing => st.title = doc.title.clone().unwrap_or_default(),
            Target::Many => {}
        }
    }

    pub fn inspector_ui(&mut self, ui: &mut Ui) {
        let target = self.inspector_target();
        let Some(s) = &self.session else { return };
        let typing = ui.ctx().wants_keyboard_input();
        if self.inspector.target != Some(target)
            || (self.inspector.revision != s.editor.revision && !typing)
        {
            self.load_inspector(target);
        }
        egui::ScrollArea::vertical().show(ui, |ui| match target {
            Target::Element(id) => self.element_ui(ui, id),
            Target::Relation(id) => self.relation_ui(ui, id),
            Target::Package(id) => self.package_ui(ui, id),
            Target::Many => self.many_ui(ui),
            Target::Nothing => self.diagram_ui(ui),
        });
    }

    fn element_ui(&mut self, ui: &mut Ui, id: ElementId) {
        let Some(s) = &mut self.session else { return };
        let Some(el) = s.editor.doc.elements.get(&id).cloned() else {
            return;
        };
        ui.heading(el.kind.label());
        ui.add_space(6.0);

        ui.label("Name");
        let r = ui.add(TextEdit::singleline(&mut self.inspector.name).desired_width(f32::INFINITY));
        if r.lost_focus() && self.inspector.name.trim() != el.name {
            match s.editor.rename(id, &self.inspector.name) {
                Ok(()) => self.inspector.error = None,
                Err(e) => self.inspector.error = Some(e),
            }
        }
        error_ui(ui, &self.inspector.error);

        ui.add_space(4.0);
        ui.label("Kind");
        let mut kind = el.kind;
        ComboBox::from_id_salt("kind")
            .selected_text(kind.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for k in ElementKind::ALL {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
        if kind != el.kind {
            let mut new = el.clone();
            new.kind = kind;
            s.editor.set_element(id, new);
        }

        if el.kind != ElementKind::Note {
            ui.add_space(4.0);
            ui.label("Type parameters");
            let r = ui.add(
                TextEdit::singleline(&mut self.inspector.type_params)
                    .hint_text("e.g. K, V")
                    .desired_width(f32::INFINITY),
            );
            let params = opt(self.inspector.type_params.trim_matches(['<', '>', ' ']));
            if r.lost_focus() && params != el.type_params {
                let mut new = el.clone();
                new.type_params = params;
                s.editor.set_element(id, new);
            }

            ui.add_space(4.0);
            ui.label("Stereotype");
            let r = ui.add(
                TextEdit::singleline(&mut self.inspector.stereotype)
                    .hint_text("e.g. entity")
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() && opt(&self.inspector.stereotype) != el.stereotype {
                let mut new = el.clone();
                new.stereotype = opt(&self.inspector.stereotype);
                s.editor.set_element(id, new);
            }
        }

        ui.add_space(4.0);
        ui.label(match el.kind {
            ElementKind::Note => "Text",
            ElementKind::Enum => "Literals",
            _ => "Members (one per line)",
        });
        let r = ui.add(
            TextEdit::multiline(&mut self.inspector.members)
                .font(egui::FontId::new(
                    self.prefs.ui(12.5),
                    crate::paint::family(false, false),
                ))
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
        if r.lost_focus() {
            let members = parse_members(el.kind, &self.inspector.members);
            if members != el.members {
                let mut new = el.clone();
                new.members = members;
                s.editor.set_element(id, new);
            }
        }
        if el.kind != ElementKind::Note && el.kind != ElementKind::Enum {
            ui.label(
                RichText::new("+ public  - private  # protected  ~ package\n{static}  {abstract}")
                    .small()
                    .weak(),
            );
        }

        ui.add_space(8.0);
        let doc = &s.editor.doc;
        if let Some(p) = doc.element_package(id) {
            ui.label(RichText::new(format!("In package {}", doc.full_package_name(p))).weak());
        }
        if let Some(p) = el.pos {
            ui.label(RichText::new(format!("Position {}, {}", p.x, p.y)).weak());
        }
        let rels = doc.relations.values().filter(|r| r.touches(id)).count();
        ui.label(RichText::new(format!("{rels} relation(s)")).weak());

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .button("Edit in place")
                .on_hover_text("Double-click the box")
                .clicked()
            {
                self.start_edit(id);
            }
            if ui.button("Delete").clicked() {
                self.run(ui.ctx(), Cmd::Delete);
            }
        });
    }

    fn relation_ui(&mut self, ui: &mut Ui, id: RelationId) {
        let Some(s) = &mut self.session else { return };
        let Some(rel) = s.editor.doc.relations.get(&id).cloned() else {
            return;
        };
        ui.heading("Relation");
        ui.add_space(4.0);
        let from = s.editor.doc.endpoint_name(rel.from.target);
        let to = s.editor.doc.endpoint_name(rel.to.target);
        ui.label(RichText::new(format!("{from}  {}  {to}", rel.kind.arrow())).monospace());
        ui.add_space(6.0);

        ui.label("Kind");
        let mut kind = rel.kind;
        ComboBox::from_id_salt("rel-kind")
            .selected_text(kind.label())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for k in RelationKind::ALL {
                    ui.selectable_value(&mut kind, k, format!("{}  {}", k.arrow(), k.label()));
                }
            });
        if kind != rel.kind {
            let mut new = rel.clone();
            new.kind = kind;
            s.editor.set_relation(id, new);
        }

        let mut changed = None;
        ui.add_space(4.0);
        ui.label("Name");
        if ui
            .add(TextEdit::singleline(&mut self.inspector.rel_name).desired_width(f32::INFINITY))
            .lost_focus()
        {
            changed = Some(());
        }
        ui.add_space(4.0);
        ui.label(format!("At {from}"));
        if ui
            .add(
                TextEdit::singleline(&mut self.inspector.from_label)
                    .hint_text("multiplicity role, e.g. 1 owner")
                    .desired_width(f32::INFINITY),
            )
            .lost_focus()
        {
            changed = Some(());
        }
        ui.label(format!("At {to}"));
        if ui
            .add(
                TextEdit::singleline(&mut self.inspector.to_label)
                    .hint_text("e.g. 0..* items")
                    .desired_width(f32::INFINITY),
            )
            .lost_focus()
        {
            changed = Some(());
        }
        if changed.is_some() {
            let mut new = rel.clone();
            new.name = opt(&self.inspector.rel_name);
            new.from.label = opt(&self.inspector.from_label);
            new.to.label = opt(&self.inspector.to_label);
            s.editor.set_relation(id, new);
        }

        ui.add_space(8.0);
        let (mut swap, mut delete) = (false, false);
        ui.horizontal(|ui| {
            swap = ui.button("Swap ends").clicked();
            delete = ui.button("Delete").clicked();
        });
        if swap {
            let mut new = rel.clone();
            std::mem::swap(&mut new.from, &mut new.to);
            s.editor.set_relation(id, new);
        }
        if delete {
            self.run(ui.ctx(), Cmd::Delete);
        }
    }

    fn package_ui(&mut self, ui: &mut Ui, id: PackageId) {
        let Some(s) = &mut self.session else { return };
        let Some(p) = s.editor.doc.packages.get(&id).cloned() else {
            return;
        };
        ui.heading("Package");
        ui.add_space(6.0);
        ui.label("Name");
        let r = ui.add(TextEdit::singleline(&mut self.inspector.name).desired_width(f32::INFINITY));
        if r.lost_focus() && self.inspector.name.trim() != p.name {
            match s.editor.rename_package(id, &self.inspector.name) {
                Ok(()) => self.inspector.error = None,
                Err(e) => self.inspector.error = Some(e),
            }
        }
        error_ui(ui, &self.inspector.error);
        let n = s.editor.doc.elements_in(id).len();
        ui.label(RichText::new(format!("{n} element(s) inside")).weak());
        ui.add_space(8.0);
        if ui.button("Delete package and contents").clicked() {
            self.run(ui.ctx(), Cmd::Delete);
        }
    }

    fn many_ui(&mut self, ui: &mut Ui) {
        let sel = &self.sel;
        ui.heading("Selection");
        ui.label(format!(
            "{} element(s), {} relation(s), {} package(s)",
            sel.elements.len(),
            sel.relations.len(),
            sel.packages.len()
        ));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.sel.elements.is_empty(),
                    egui::Button::new("Tidy selection"),
                )
                .clicked()
            {
                self.run(ui.ctx(), Cmd::TidySelection);
            }
            if ui.button("Delete").clicked() {
                self.run(ui.ctx(), Cmd::Delete);
            }
        });
    }

    fn diagram_ui(&mut self, ui: &mut Ui) {
        let Some(s) = &mut self.session else { return };
        ui.heading("Diagram");
        ui.add_space(6.0);
        ui.label("Title");
        let r =
            ui.add(TextEdit::singleline(&mut self.inspector.title).desired_width(f32::INFINITY));
        if r.lost_focus() && opt(&self.inspector.title) != s.editor.doc.title {
            s.editor.set_title(opt(&self.inspector.title));
        }
        ui.add_space(10.0);
        let st = self.view.diagram.layout.stats;
        egui::Grid::new("stats")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                ui.label("Crossings");
                ui.label(st.crossings.to_string());
                ui.end_row();
                ui.label("Bends");
                ui.label(st.bends.to_string());
                ui.end_row();
                ui.label("Line length");
                ui.label(format!("{}", st.length));
                ui.end_row();
                ui.label("Score");
                ui.label(format!("{:.1}", st.score()));
                ui.end_row();
            });
        ui.add_space(14.0);
        ui.label(RichText::new("Tips").strong());
        for tip in [
            "Double-click empty space to add a class.",
            "Double-click a box to edit it in place.",
            "Drag from a dot on a box edge to connect.",
            "Ctrl+T lays everything out again.",
            "Ctrl+K finds any command.",
            "Right- or middle-drag pans; Ctrl+scroll zooms.",
        ] {
            ui.label(RichText::new(format!("• {tip}")).weak());
        }
    }
}
