//! Font and text-size settings, chosen in the Settings window and kept in
//! `settings.txt` with the others (layout, theme, grid, text panel).
//!
//! The diagram's own text is not among them: box sizes are measured from
//! it, so a saved layout would no longer fit its boxes at another size.

use egui::{Context, FontFamily, RichText, Slider, Ui};
use umpl_layout::Goal;

use crate::app::App;
use crate::commands::{Cmd, ThemePref};
use crate::storage::Storage;

/// The font of menus, panels and dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiFont {
    /// egui's proportional font.
    Sans,
    /// DejaVu Sans Mono, the font of the diagram.
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    /// Size of the interface text, as a factor of the usual.
    pub ui_scale: f32,
    pub ui_font: UiFont,
    /// Font size of the text panel, in points.
    pub text_size: f32,
}

pub const UI_SCALE: std::ops::RangeInclusive<f32> = 0.8..=1.6;
pub const TEXT_SIZE: std::ops::RangeInclusive<f32> = 9.0..=24.0;

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            ui_scale: 1.0,
            ui_font: UiFont::Sans,
            text_size: 13.0,
        }
    }
}

/// A number setting within `range`, else `default`.
fn number(setting: Option<String>, range: &std::ops::RangeInclusive<f32>, default: f32) -> f32 {
    setting
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| range.contains(v))
        .unwrap_or(default)
}

impl Prefs {
    pub fn load(storage: &Storage) -> Prefs {
        let d = Prefs::default();
        Prefs {
            ui_scale: number(storage.load_setting("ui_scale"), &UI_SCALE, d.ui_scale),
            ui_font: match storage.load_setting("ui_font").as_deref() {
                Some("mono") => UiFont::Mono,
                _ => UiFont::Sans,
            },
            text_size: number(storage.load_setting("text_size"), &TEXT_SIZE, d.text_size),
        }
    }

    pub fn save(&self, storage: &Storage) {
        storage.save_setting("ui_scale", &format!("{:.2}", self.ui_scale));
        let font = match self.ui_font {
            UiFont::Sans => "sans",
            UiFont::Mono => "mono",
        };
        storage.save_setting("ui_font", font);
        storage.save_setting("text_size", &format!("{:.0}", self.text_size));
    }

    /// Installs the fonts, sizes the interface text and sets the look.
    pub fn apply(&self, ctx: &Context) {
        crate::paint::install_fonts(ctx, self.ui_font == UiFont::Mono);
        look(ctx);
        let scale = self.ui_scale;
        ctx.all_styles_mut(|style| {
            style.text_styles = egui::style::default_text_styles();
            for font in style.text_styles.values_mut() {
                font.size = (font.size * scale).round();
                if self.ui_font == UiFont::Mono && font.family == FontFamily::Proportional {
                    // The mono face is wider: a little smaller reads alike.
                    font.size = (font.size * 0.92).round();
                }
            }
        });
    }

    /// `size` points of interface text at the chosen scale.
    pub fn ui(&self, size: f32) -> f32 {
        (size * self.ui_scale).round()
    }
}

/// The interface's look, in both themes: quiet panels with thin borders,
/// rounded buttons, menus and windows (as drawing tools such as Eraser
/// look), and a blue selection.
fn look(ctx: &Context) {
    use egui::{Color32, CornerRadius, Stroke};
    let rgb = Color32::from_rgb;
    for (theme, dark) in [(egui::Theme::Dark, true), (egui::Theme::Light, false)] {
        ctx.style_mut_of(theme, |style| {
            let v = &mut style.visuals;
            let (panel, window, extreme, faint, border, text, idle, hover, active, select) = if dark
            {
                (
                    rgb(0x18, 0x19, 0x1d),
                    rgb(0x1e, 0x1f, 0x24),
                    rgb(0x13, 0x14, 0x17),
                    rgb(0x1c, 0x1d, 0x21),
                    rgb(0x2c, 0x2e, 0x35),
                    rgb(0xe4, 0xe6, 0xeb),
                    rgb(0x24, 0x26, 0x2c),
                    rgb(0x2e, 0x30, 0x38),
                    rgb(0x36, 0x39, 0x42),
                    rgb(0x2b, 0x45, 0x75),
                )
            } else {
                (
                    rgb(0xf7, 0xf7, 0xf8),
                    rgb(0xff, 0xff, 0xff),
                    rgb(0xff, 0xff, 0xff),
                    rgb(0xf1, 0xf2, 0xf4),
                    rgb(0xe1, 0xe3, 0xe8),
                    rgb(0x1f, 0x23, 0x2b),
                    rgb(0xee, 0xef, 0xf2),
                    rgb(0xe4, 0xe6, 0xeb),
                    rgb(0xd8, 0xdb, 0xe2),
                    rgb(0xd6, 0xe4, 0xff),
                )
            };
            v.panel_fill = panel;
            v.window_fill = window;
            v.extreme_bg_color = extreme;
            v.faint_bg_color = faint;
            v.window_stroke = Stroke::new(1.0, border);
            v.window_corner_radius = CornerRadius::same(10);
            v.menu_corner_radius = CornerRadius::same(8);
            v.selection.bg_fill = select;
            v.widgets.noninteractive.bg_fill = panel;
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
            for (w, fill) in [
                (&mut v.widgets.inactive, idle),
                (&mut v.widgets.hovered, hover),
                (&mut v.widgets.active, active),
                (&mut v.widgets.open, hover),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.corner_radius = CornerRadius::same(6);
            }
            v.widgets.inactive.bg_stroke = Stroke::NONE;
            v.widgets.inactive.fg_stroke.color = text;
            v.widgets.noninteractive.corner_radius = CornerRadius::same(6);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        });
    }
}

impl App {
    /// The Settings window. Every change applies at once and is saved for
    /// the next run. True when the window is closed.
    pub fn settings_ui(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let mut run: Vec<Cmd> = Vec::new();
        let mut prefs = self.prefs;
        let mut closed = false;
        ui.heading("Settings");
        ui.label(RichText::new("Saved for the next time fastumpl starts.").weak());
        ui.add_space(8.0);
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([18.0, 12.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Layout").strong());
                ui.vertical(|ui| {
                    for (goal, label, hint) in [
                        (
                            Goal::FewestCrossings,
                            "Fewest line crossings",
                            "The drawing may come out wider or taller than the window",
                        ),
                        (
                            Goal::FillView,
                            "Fill the window",
                            "The drawing takes the window's shape, with a few more crossings \
                             where need be",
                        ),
                    ] {
                        let r = ui
                            .radio(self.layout_goal == goal, label)
                            .on_hover_text(hint);
                        if r.clicked() && self.layout_goal != goal {
                            run.push(Cmd::LayoutGoal(goal));
                        }
                    }
                    ui.label(
                        RichText::new("For diagrams opened from now on, and Tidy")
                            .small()
                            .weak(),
                    );
                });
                ui.end_row();

                ui.label(RichText::new("Theme").strong());
                ui.horizontal(|ui| {
                    for (theme, label) in [
                        (ThemePref::System, "Follow system"),
                        (ThemePref::Light, "Light"),
                        (ThemePref::Dark, "Dark"),
                    ] {
                        if ui.radio(self.theme_pref == theme, label).clicked() {
                            run.push(Cmd::Theme(theme));
                        }
                    }
                });
                ui.end_row();

                ui.label(RichText::new("Interface text").strong());
                ui.vertical(|ui| {
                    let mut percent = (prefs.ui_scale * 100.0).round();
                    let lo = UI_SCALE.start() * 100.0;
                    let hi = UI_SCALE.end() * 100.0;
                    let r = ui.add(
                        Slider::new(&mut percent, lo..=hi)
                            .step_by(10.0)
                            .fixed_decimals(0)
                            .suffix(" %")
                            .text("size"),
                    );
                    if r.changed() {
                        prefs.ui_scale = percent / 100.0;
                    }
                    ui.horizontal(|ui| {
                        for (font, label) in [(UiFont::Sans, "Sans"), (UiFont::Mono, "Monospace")] {
                            if ui.radio(prefs.ui_font == font, label).clicked() {
                                prefs.ui_font = font;
                            }
                        }
                    });
                });
                ui.end_row();

                ui.label(RichText::new("Text panel").strong());
                ui.vertical(|ui| {
                    let mut shown = self.source.shown;
                    if ui.checkbox(&mut shown, "Show the text panel").changed() {
                        run.push(Cmd::ToggleSource);
                    }
                    ui.add(
                        Slider::new(&mut prefs.text_size, TEXT_SIZE)
                            .step_by(1.0)
                            .fixed_decimals(0)
                            .suffix(" pt")
                            .text("font size"),
                    );
                });
                ui.end_row();

                ui.label(RichText::new("Canvas").strong());
                let mut grid = self.show_grid;
                if ui.checkbox(&mut grid, "Show the grid").changed() {
                    run.push(Cmd::ToggleGrid);
                }
                ui.end_row();
            });
        ui.label(
            RichText::new(
                "The diagram's own text keeps its size: boxes are measured from it, so \
                 zoom instead (Ctrl+scroll).",
            )
            .small()
            .weak(),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Reset to defaults").clicked() {
                prefs = Prefs::default();
                if self.layout_goal != Goal::FewestCrossings {
                    run.push(Cmd::LayoutGoal(Goal::FewestCrossings));
                }
                if self.theme_pref != ThemePref::System {
                    run.push(Cmd::Theme(ThemePref::System));
                }
                if self.source.shown {
                    run.push(Cmd::ToggleSource);
                }
                if !self.show_grid {
                    run.push(Cmd::ToggleGrid);
                }
            }
            if ui.button("Close").clicked() {
                closed = true;
            }
        });
        if prefs != self.prefs {
            self.prefs = prefs;
            prefs.save(&self.storage);
            prefs.apply(&ctx);
        }
        for cmd in run {
            self.run(&ctx, cmd);
        }
        closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_out_of_range_fall_back_to_the_default() {
        let d = Prefs::default();
        assert_eq!(number(Some("1.25".into()), &UI_SCALE, d.ui_scale), 1.25);
        assert_eq!(number(Some("9".into()), &UI_SCALE, d.ui_scale), 1.0);
        assert_eq!(number(Some("big".into()), &TEXT_SIZE, d.text_size), 13.0);
        assert_eq!(number(None, &TEXT_SIZE, d.text_size), 13.0);
    }
}
