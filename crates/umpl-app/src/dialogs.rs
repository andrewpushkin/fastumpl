//! Modal dialogs and the command palette.

use egui::{Context, Id, Key, Modal, RichText, TextEdit};

use crate::app::{App, Dialog};
use crate::commands::Cmd;
use crate::session::Session;

impl App {
    pub fn dialogs(&mut self, ctx: &Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let modal = Modal::new(Id::new("dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            match &dialog {
                Dialog::Unsaved(pending) => {
                    let name = self.session.as_ref().map(Session::display_name).unwrap_or_default();
                    ui.heading("Unsaved changes");
                    ui.label(format!("Save changes to {name} first?"));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            keep = false;
                            if self.save() {
                                self.perform(ctx, pending.clone());
                            }
                        }
                        if ui.button("Don't save").clicked() {
                            keep = false;
                            self.perform(ctx, pending.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                }
                Dialog::DiskChanged => {
                    let (name, dirty) = self.session.as_ref().map_or((String::new(), false), |s| (s.display_name(), s.is_dirty()));
                    ui.heading("File changed on disk");
                    ui.label(format!("{name} was changed by another program."));
                    if dirty {
                        ui.label("You also have unsaved changes here. Reloading loses them.");
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Reload from disk").clicked() {
                            keep = false;
                            if let Some(p) = self.session.as_ref().and_then(|s| s.path.clone()) {
                                self.clear_recovery();
                                self.open_path(ui.ctx(), &p);
                            }
                        }
                        if ui.button("Keep my version").clicked() {
                            keep = false;
                            if let Some(s) = &mut self.session {
                                s.acknowledge_disk();
                                s.mark_dirty();
                            }
                        }
                    });
                }
                Dialog::Recovery(files) => {
                    ui.heading("Recover unsaved work?");
                    ui.label("fastumpl closed with unsaved changes last time.");
                    ui.add_space(8.0);
                    let mut remaining = files.clone();
                    for f in files {
                        let origin = std::fs::read_to_string(f.with_extension("path")).unwrap_or_default();
                        let label = if origin.trim().is_empty() { "Untitled diagram".to_string() } else { origin };
                        ui.horizontal(|ui| {
                            ui.label(label);
                            if ui.button("Recover").clicked() {
                                remaining.retain(|x| x != f);
                                self.recover(ctx, f);
                            }
                            if ui.button("Discard").clicked() {
                                remaining.retain(|x| x != f);
                                App::discard_recovery(f);
                            }
                        });
                    }
                    ui.add_space(8.0);
                    if ui.button("Later").clicked() || remaining.is_empty() {
                        keep = false;
                    } else if remaining.len() != files.len() {
                        keep = false;
                        if self.dialog.is_none() {
                            self.dialog = Some(Dialog::Recovery(remaining));
                        }
                    }
                }
                Dialog::PickDiagram(path, list) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.heading("Open which diagram?");
                    ui.label(format!("{name} holds {} diagrams; PlantUML draws each on its own.", list.len()));
                    ui.add_space(8.0);
                    let mut chosen = None;
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (k, d) in list.iter().enumerate() {
                            let title = d.name.clone().unwrap_or_else(|| format!("Diagram {}", k + 1));
                            let readable = d.kind.readable();
                            let text = if readable {
                                format!("{title}  ·  line {}", d.line)
                            } else {
                                format!("{title}  ·  {} (not read)", d.kind.name())
                            };
                            if ui.add_enabled(readable, egui::Button::new(text)).clicked() {
                                chosen = Some(k);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    if let Some(k) = chosen {
                        keep = false;
                        let aspect = self.view_aspect(ctx);
                        self.import_nth(path, umpl_text::Format::PlantUml, k, aspect);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        keep = false;
                    }
                }
                Dialog::Message(title, text) => {
                    ui.heading(title);
                    ui.label(text);
                    ui.add_space(10.0);
                    if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        keep = false;
                    }
                }
                Dialog::Problems => {
                    ui.heading("Problems in file");
                    let problems = self.session.as_ref().map(|s| s.problems.clone()).unwrap_or_default();
                    if problems.is_empty() {
                        ui.label("None.");
                    } else {
                        ui.label("These lines are kept as written and saved back unchanged.");
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for p in problems {
                                ui.label(RichText::new(format!("line {}:{}  {}", p.line, p.col, p.message)).monospace());
                            }
                        });
                    }
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
                Dialog::Shortcuts => {
                    ui.heading("Keyboard and mouse");
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        egui::Grid::new("keys").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                            for cmd in Cmd::all().into_iter().chain([Cmd::Palette]) {
                                if let Some(sc) = cmd.shortcut() {
                                    ui.label(cmd.label());
                                    ui.label(RichText::new(ctx.format_shortcut(&sc)).monospace());
                                    ui.end_row();
                                }
                            }
                            for (what, how) in [
                                ("Pan", "Right- or middle-drag, scroll"),
                                ("Zoom", "Ctrl+scroll, pinch"),
                                ("Add a class", "Double-click empty space"),
                                ("Edit a box", "Double-click it"),
                                ("Connect", "Drag from a dot on a box edge"),
                                ("Select several", "Shift+click, or drag a rectangle"),
                                ("Nudge", "Arrow keys (Shift: 10 units)"),
                            ] {
                                ui.label(what);
                                ui.label(RichText::new(how).weak());
                                ui.end_row();
                            }
                        });
                    });
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
                Dialog::About => {
                    ui.heading("fastumpl");
                    ui.label(format!("Version {}", crate::VERSION));
                    ui.label("A UML class diagram editor that lays diagrams out with as few line crossings as possible.");
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                }
            }
        });
        let dismissible = !matches!(dialog, Dialog::Unsaved(_) | Dialog::DiskChanged);
        if modal.should_close() && dismissible {
            keep = false;
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    pub fn palette_ui(&mut self, ctx: &Context) {
        let Some((mut query, mut index)) = self.palette.take() else {
            return;
        };
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let matches: Vec<Cmd> = Cmd::all()
            .into_iter()
            .filter(|c| {
                let l = self.command_label(*c).to_lowercase();
                words.iter().all(|w| l.contains(w.as_str()))
            })
            .collect();
        index = index.min(matches.len().saturating_sub(1));
        let mut run = None;
        let mut close = false;
        let modal = Modal::new(Id::new("palette")).show(ctx, |ui| {
            ui.set_width(420.0);
            let r = ui.add(
                TextEdit::singleline(&mut query)
                    .hint_text("Type a command…")
                    .desired_width(f32::INFINITY),
            );
            r.request_focus();
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, cmd) in matches.iter().enumerate() {
                        let sc = cmd
                            .shortcut()
                            .map(|s| ctx.format_shortcut(&s))
                            .unwrap_or_default();
                        let text = format!("{:<34}{}", self.command_label(*cmd), sc);
                        let resp = ui.selectable_label(i == index, RichText::new(text).monospace());
                        if resp.clicked() {
                            run = Some(*cmd);
                        }
                        if i == index {
                            resp.scroll_to_me(None);
                        }
                    }
                });
        });
        ctx.input(|i| {
            if i.key_pressed(Key::ArrowDown) {
                index = (index + 1).min(matches.len().saturating_sub(1));
            }
            if i.key_pressed(Key::ArrowUp) {
                index = index.saturating_sub(1);
            }
            if i.key_pressed(Key::Enter) {
                run = matches.get(index).copied();
            }
            if i.key_pressed(Key::Escape) {
                close = true;
            }
        });
        if modal.should_close() {
            close = true;
        }
        if let Some(cmd) = run {
            self.run(ctx, cmd);
        } else if !close {
            self.palette = Some((query, index));
        }
    }
}
