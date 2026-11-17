//! The application: window layout, commands, files and background work.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use egui::{
    Button, Color32, Context, Frame, Key, Margin, RichText, ThemePreference, ViewportCommand,
};
use umpl_core::{ElementId, Pos};
use umpl_draw::svg::to_svg;
use umpl_draw::{build_scene, Diagram, SceneOptions, Theme};
use umpl_layout::Options;
use umpl_text::{Format, PLANTUML_EXTENSIONS};

use crate::camera::Camera;
use crate::canvas::CanvasState;
use crate::commands::{Cmd, ThemePref, Tool};
use crate::inspector::InspectorState;
use crate::session::Session;
use crate::storage::{push_recent, Storage};
use crate::view::{Selection, View};

/// A Tidy running on a background thread.
pub struct TidyJob {
    revision: u64,
    rx: mpsc::Receiver<Diagram>,
    cancel: Arc<AtomicBool>,
}

/// What to do once unsaved changes are dealt with.
#[derive(Clone, Debug)]
pub enum Pending {
    New,
    Open(Option<PathBuf>),
    OpenText(String, Option<PathBuf>),
    Quit,
}

pub enum Dialog {
    Unsaved(Pending),
    DiskChanged,
    Recovery(Vec<PathBuf>),
    Message(String, String),
    Problems,
    Shortcuts,
    About,
}

pub struct App {
    pub storage: Storage,
    pub session: Option<Session>,
    pub view: View,
    pub camera: Camera,
    pub fit_pending: bool,
    pub sel: Selection,
    pub tool: Tool,
    pub theme_pref: ThemePref,
    pub show_grid: bool,
    pub recent: Vec<PathBuf>,
    pub tidy: Option<TidyJob>,
    pub dialog: Option<Dialog>,
    pub palette: Option<(String, usize)>,
    pub status: Option<(String, Instant)>,
    pub canvas: CanvasState,
    pub inspector: InspectorState,
    allow_close: bool,
    last_disk_check: Instant,
    last_recovery: Instant,
    title: String,
    screenshot: Option<(PathBuf, u32)>,
}

const RECOVERY_EVERY: Duration = Duration::from_secs(30);

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> App {
        crate::paint::install_fonts(&cc.egui_ctx);
        let storage = Storage::new();
        let theme_pref = match storage.load_setting("theme").as_deref() {
            Some("light") => ThemePref::Light,
            Some("dark") => ThemePref::Dark,
            _ => ThemePref::System,
        };
        let mut app = App {
            recent: storage.load_recent(),
            storage,
            session: None,
            view: View::new(),
            camera: Camera::default(),
            fit_pending: true,
            sel: Selection::default(),
            tool: Tool::Select,
            theme_pref,
            show_grid: true,
            tidy: None,
            dialog: None,
            palette: None,
            status: None,
            canvas: CanvasState::default(),
            inspector: InspectorState::default(),
            allow_close: false,
            last_disk_check: Instant::now(),
            last_recovery: Instant::now(),
            title: String::new(),
            screenshot: std::env::var_os("UMPL_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.apply_theme(&cc.egui_ctx);
        let recovered = app.find_recovery();
        if let Some(f) = file {
            app.open_path(&f);
        }
        if !recovered.is_empty() {
            app.dialog = Some(Dialog::Recovery(recovered));
        }
        if let (Some(s), Ok(name)) = (&app.session, std::env::var("UMPL_SELECT")) {
            if let Some(id) = s.editor.doc.find(&name) {
                app.sel = Selection::only_element(id);
            }
        }
        app
    }

    // ----- helpers ---------------------------------------------------------

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn error(&mut self, title: &str, msg: impl Into<String>) {
        self.dialog = Some(Dialog::Message(title.into(), msg.into()));
    }

    pub fn diagram_theme(&self, ctx: &Context) -> Theme {
        if ctx.theme() == egui::Theme::Dark {
            Theme::dark()
        } else {
            Theme::light()
        }
    }

    fn apply_theme(&self, ctx: &Context) {
        ctx.set_theme(match self.theme_pref {
            ThemePref::System => ThemePreference::System,
            ThemePref::Light => ThemePreference::Light,
            ThemePref::Dark => ThemePreference::Dark,
        });
    }

    fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(Session::is_dirty)
    }

    // ----- files -----------------------------------------------------------

    fn install_session(&mut self, s: Session) {
        if let Some(p) = &s.path {
            push_recent(&mut self.recent, p);
            self.storage.save_recent(&self.recent);
        }
        if !s.problems.is_empty() {
            self.notify(format!(
                "{} problem(s) in file — see Problems",
                s.problems.len()
            ));
        }
        self.cancel_tidy();
        self.session = Some(s);
        self.view.reset();
        self.sel.clear();
        self.canvas = CanvasState::default();
        self.inspector = InspectorState::default();
        self.fit_pending = true;
    }

    pub fn open_path(&mut self, path: &Path) {
        if let Some(f @ (Format::PlantUml | Format::Umple)) = Format::from_path(path) {
            return self.import_path(path, f);
        }
        match Session::open(path) {
            Ok(s) => self.install_session(s),
            Err(e) => {
                self.recent.retain(|p| p != path);
                self.storage.save_recent(&self.recent);
                self.error("Cannot open file", e);
            }
        }
    }

    /// Opens a PlantUML or Umple file as a new, unsaved diagram.
    fn import_path(&mut self, path: &Path, format: Format) {
        match Session::import(path, format) {
            Ok(s) => {
                let problems = s.problems.len();
                self.install_session(s);
                push_recent(&mut self.recent, path);
                self.storage.save_recent(&self.recent);
                let name = path.display();
                self.notify(if problems == 0 {
                    format!("Imported {name}; Save writes a .umpl file")
                } else {
                    format!("Imported {name} with {problems} problem(s) — see Problems")
                });
            }
            Err(e) => {
                self.recent.retain(|p| p != path);
                self.storage.save_recent(&self.recent);
                self.error("Cannot import file", e);
            }
        }
    }

    /// Runs `p` now, or first asks what to do with unsaved changes.
    pub fn request(&mut self, ctx: &Context, p: Pending) {
        if self.dirty() {
            self.dialog = Some(Dialog::Unsaved(p));
        } else {
            self.perform(ctx, p);
        }
    }

    pub fn perform(&mut self, ctx: &Context, p: Pending) {
        match p {
            Pending::New => {
                self.clear_recovery();
                self.install_session(Session::new_empty());
            }
            Pending::Open(Some(path)) => {
                self.clear_recovery();
                self.open_path(&path);
            }
            Pending::Open(None) => {
                let mut diagrams = vec!["umpl"];
                diagrams.extend(PLANTUML_EXTENSIONS);
                diagrams.push("ump");
                let mut dlg = rfd::FileDialog::new()
                    .add_filter("Diagrams", &diagrams)
                    .add_filter("fastumpl diagram", &["umpl"])
                    .add_filter("PlantUML (imported)", &PLANTUML_EXTENSIONS)
                    .add_filter("Umple (imported)", &["ump"])
                    .add_filter("All files", &["*"]);
                if let Some(dir) = self
                    .session
                    .as_ref()
                    .and_then(|s| s.path.as_ref())
                    .and_then(|p| p.parent())
                {
                    dlg = dlg.set_directory(dir);
                }
                if let Some(path) = dlg.pick_file() {
                    self.clear_recovery();
                    self.open_path(&path);
                }
            }
            Pending::OpenText(text, path) => {
                let mut s = Session::from_text(&text);
                s.path = path;
                s.mark_dirty();
                self.install_session(s);
            }
            Pending::Quit => {
                self.clear_recovery();
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    /// Saves; asks for a file name if there is none. Returns success.
    pub fn save(&mut self) -> bool {
        let Some(s) = &mut self.session else {
            return false;
        };
        match s.path.clone() {
            Some(p) => match s.save_to(&p) {
                Ok(()) => {
                    self.clear_recovery();
                    self.notify(format!("Saved {}", p.display()));
                    true
                }
                Err(e) => {
                    self.error("Cannot save", e);
                    false
                }
            },
            None => self.save_as(),
        }
    }

    pub fn save_as(&mut self) -> bool {
        let Some(s) = &self.session else { return false };
        let mut dlg = rfd::FileDialog::new()
            .add_filter("fastumpl diagram", &["umpl"])
            .set_file_name(s.display_name());
        if let Some(dir) = s.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(mut path) = dlg.save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("umpl");
        }
        let s = self.session.as_mut().unwrap();
        match s.save_to(&path) {
            Ok(()) => {
                self.clear_recovery();
                push_recent(&mut self.recent, &path);
                self.storage.save_recent(&self.recent);
                self.notify(format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.error("Cannot save", e);
                false
            }
        }
    }

    fn export_svg_text(&self, ctx: &Context) -> Option<String> {
        let s = self.session.as_ref()?;
        let theme = self.diagram_theme(ctx);
        let scene = build_scene(
            &s.editor.doc,
            &self.view.diagram,
            &theme,
            &SceneOptions::default(),
        );
        Some(to_svg(&scene, &theme))
    }

    fn export(&mut self, ctx: &Context, png: bool) {
        let Some(svg) = self.export_svg_text(ctx) else {
            return;
        };
        let s = self.session.as_ref().unwrap();
        let stem = s.stem();
        let ext = if png { "png" } else { "svg" };
        let mut dlg = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{stem}.{ext}"));
        if let Some(dir) = s.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else { return };
        let bytes = if png {
            match umpl_draw::png::svg_to_png(&svg, 2.0) {
                Ok(b) => b,
                Err(e) => return self.error("Cannot export", e),
            }
        } else {
            svg.into_bytes()
        };
        match umpl_text::write_atomic(&path, &bytes) {
            Ok(()) => self.notify(format!("Exported {}", path.display())),
            Err(e) => self.error("Cannot export", e.to_string()),
        }
    }

    /// Exports the diagram as PlantUML, Umple or Mermaid text (Mermaid as `.mmd`
    /// or, when the chosen name ends in `.md`, as a Markdown code block).
    fn export_text(&mut self, format: Format) {
        let Some(s) = &self.session else { return };
        let mut dlg = rfd::FileDialog::new();
        let ext = match format {
            Format::PlantUml => {
                dlg = dlg.add_filter("PlantUML", &["puml"]);
                "puml"
            }
            Format::Umple => {
                dlg = dlg.add_filter("Umple", &["ump"]);
                "ump"
            }
            _ => {
                dlg = dlg
                    .add_filter("Mermaid", &["mmd"])
                    .add_filter("Markdown", &["md"]);
                "mmd"
            }
        };
        dlg = dlg.set_file_name(format!("{}.{ext}", s.stem()));
        if let Some(dir) = s.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(mut path) = dlg.save_file() else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(ext);
        }
        let format = match (format, Format::from_path(&path)) {
            (Format::Mermaid, Some(Format::MermaidMarkdown)) => Format::MermaidMarkdown,
            _ => format,
        };
        let text = self.session.as_ref().unwrap().export_text(format);
        match umpl_text::write_atomic(&path, text.as_bytes()) {
            Ok(()) => self.notify(format!("Exported {}", path.display())),
            Err(e) => self.error("Cannot export", e.to_string()),
        }
    }

    /// Puts the diagram on the clipboard as Mermaid text, ready to paste
    /// into a Markdown `mermaid` block.
    fn copy_mermaid(&mut self, ctx: &Context) {
        let Some(s) = &self.session else { return };
        ctx.copy_text(s.export_text(Format::Mermaid));
        self.notify("Copied Mermaid to clipboard");
    }

    /// Copies the selection (or the whole diagram) to the clipboard as an
    /// image.
    fn copy_image(&mut self, ctx: &Context) {
        let Some(s) = &self.session else { return };
        let theme = self.diagram_theme(ctx);
        let mut scene = build_scene(
            &s.editor.doc,
            &self.view.diagram,
            &theme,
            &SceneOptions::default(),
        );
        let picked: Vec<_> = scene
            .elements
            .iter()
            .filter(|(id, _)| self.sel.elements.contains(id))
            .map(|(_, r)| *r)
            .collect();
        if let Some(first) = picked.first() {
            scene.bounds = picked.iter().fold(*first, |a, b| a.union(b)).expand(16.0);
        }
        let svg = to_svg(&scene, &theme);
        match umpl_draw::png::svg_to_rgba(&svg, 2.0) {
            Ok(img) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                ctx.copy_image(image);
                self.notify("Copied image to clipboard");
            }
            Err(e) => self.error("Cannot copy", e),
        }
    }

    // ----- recovery --------------------------------------------------------

    fn recovery_file(&self) -> Option<PathBuf> {
        use std::hash::{Hash, Hasher};
        let s = self.session.as_ref()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.path.hash(&mut h);
        let stem = s
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        Some(
            self.storage
                .recovery_dir()
                .join(format!("{stem}-{:016x}.umpl", h.finish())),
        )
    }

    fn write_recovery(&mut self) {
        let Some(file) = self.recovery_file() else {
            return;
        };
        let s = self.session.as_ref().unwrap();
        let _ = std::fs::create_dir_all(self.storage.recovery_dir());
        let _ = umpl_text::write_atomic(&file, s.text().as_bytes());
        let origin = s
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let _ = std::fs::write(file.with_extension("path"), origin);
    }

    pub fn clear_recovery(&mut self) {
        if let Some(file) = self.recovery_file() {
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(file.with_extension("path"));
        }
    }

    fn find_recovery(&self) -> Vec<PathBuf> {
        let Ok(dir) = std::fs::read_dir(self.storage.recovery_dir()) else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "umpl"))
            .collect();
        v.sort();
        v
    }

    pub fn recover(&mut self, ctx: &Context, file: &Path) {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let origin = std::fs::read_to_string(file.with_extension("path"))
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(file.with_extension("path"));
        self.request(ctx, Pending::OpenText(text, origin));
    }

    pub fn discard_recovery(file: &Path) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(file.with_extension("path"));
    }

    // ----- layout ----------------------------------------------------------

    pub fn start_tidy(&mut self) {
        self.cancel_tidy();
        let Some(s) = &self.session else { return };
        let doc = s.editor.doc.clone();
        let revision = s.editor.revision;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let c = cancel.clone();
        std::thread::spawn(move || {
            let opts = Options {
                cancel: Some(c),
                ..Options::default()
            };
            let _ = tx.send(Diagram::tidy(&doc, &opts));
        });
        self.tidy = Some(TidyJob {
            revision,
            rx,
            cancel,
        });
    }

    pub fn cancel_tidy(&mut self) {
        if let Some(job) = self.tidy.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_tidy(&mut self, ctx: &Context) {
        let Some(job) = &self.tidy else { return };
        match job.rx.try_recv() {
            Ok(diagram) => {
                let job = self.tidy.take().unwrap();
                let Some(s) = &mut self.session else { return };
                if s.editor.revision != job.revision {
                    self.notify("Diagram changed while tidying; run Tidy again");
                    return;
                }
                s.editor.set_positions("Tidy", &diagram.positions());
                let crossings = diagram.layout.stats.crossings;
                self.view.install(diagram, s.editor.revision);
                self.notify(format!("Tidy done: {crossings} crossing(s)"));
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
            Err(mpsc::TryRecvError::Disconnected) => self.tidy = None,
        }
    }

    /// Places the selected elements again, keeping everything else.
    fn tidy_selection(&mut self) {
        let Some(s) = &mut self.session else { return };
        if self.sel.elements.is_empty() {
            return self.start_tidy();
        }
        let mut graph = self.view.diagram.graph.clone();
        for (i, p) in self.view.diagram.layout.nodes.iter().enumerate() {
            graph.nodes[i].pos = Some(*p);
        }
        for id in &self.sel.elements {
            if let Some(i) = self.view.diagram.mapping.node_of(*id) {
                graph.nodes[i].pos = None;
            }
        }
        let layout = umpl_layout::place_new(&graph);
        let positions: Vec<(ElementId, Option<Pos>)> = self
            .sel
            .elements
            .iter()
            .filter_map(|id| {
                let i = self.view.diagram.mapping.node_of(*id)?;
                let p = layout.nodes[i];
                Some((*id, Some(Pos::new(p.0, p.1))))
            })
            .collect();
        s.editor.set_positions("Tidy selection", &positions);
    }

    // ----- commands --------------------------------------------------------

    pub fn delete_selection(&mut self) {
        let Some(s) = &mut self.session else { return };
        if self.sel.is_empty() {
            return;
        }
        let rels: Vec<_> = self
            .sel
            .relations
            .iter()
            .copied()
            .filter(|r| s.editor.doc.relations.contains_key(r))
            .collect();
        // One undo step: relations of deleted elements go with them.
        s.editor
            .delete_items(&self.sel.elements, &self.sel.packages, &rels);
        self.sel.clear();
    }

    pub fn run(&mut self, ctx: &Context, cmd: Cmd) {
        match cmd {
            Cmd::New => self.request(ctx, Pending::New),
            Cmd::Open => self.request(ctx, Pending::Open(None)),
            Cmd::Save => {
                self.save();
            }
            Cmd::SaveAs => {
                self.save_as();
            }
            Cmd::ExportSvg => self.export(ctx, false),
            Cmd::ExportPng => self.export(ctx, true),
            Cmd::ExportPlantUml => self.export_text(Format::PlantUml),
            Cmd::ExportMermaid => self.export_text(Format::Mermaid),
            Cmd::ExportUmple => self.export_text(Format::Umple),
            Cmd::CopyImage => self.copy_image(ctx),
            Cmd::CopyMermaid => self.copy_mermaid(ctx),
            Cmd::Quit => self.request(ctx, Pending::Quit),
            Cmd::Undo => {
                if let Some(s) = &mut self.session {
                    let label = s.editor.undo_label().map(str::to_string);
                    if s.editor.undo() {
                        self.sel.retain_existing(&s.editor.doc);
                        self.notify(format!("Undone: {}", label.unwrap_or_default()));
                    }
                }
            }
            Cmd::Redo => {
                if let Some(s) = &mut self.session {
                    let label = s.editor.redo_label().map(str::to_string);
                    if s.editor.redo() {
                        self.sel.retain_existing(&s.editor.doc);
                        self.notify(format!("Redone: {}", label.unwrap_or_default()));
                    }
                }
            }
            Cmd::Delete => self.delete_selection(),
            Cmd::SelectAll => {
                if let Some(s) = &self.session {
                    self.sel = Selection {
                        elements: s.editor.doc.elements.keys().copied().collect(),
                        ..Selection::default()
                    };
                }
            }
            Cmd::Tidy => self.start_tidy(),
            Cmd::TidySelection => self.tidy_selection(),
            Cmd::Fit => self.fit_pending = true,
            Cmd::ZoomIn => self.camera.zoom_at(self.canvas.rect.center(), 1.25),
            Cmd::ZoomOut => self.camera.zoom_at(self.canvas.rect.center(), 0.8),
            Cmd::ZoomReset => {
                let f = 1.0 / self.camera.zoom;
                self.camera.zoom_at(self.canvas.rect.center(), f);
            }
            Cmd::Theme(t) => {
                self.theme_pref = t;
                self.apply_theme(ctx);
                let v = match t {
                    ThemePref::System => "system",
                    ThemePref::Light => "light",
                    ThemePref::Dark => "dark",
                };
                self.storage.save_setting("theme", v);
            }
            Cmd::ToggleGrid => self.show_grid = !self.show_grid,
            Cmd::Problems => self.dialog = Some(Dialog::Problems),
            Cmd::Palette => self.palette = Some((String::new(), 0)),
            Cmd::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Cmd::About => self.dialog = Some(Dialog::About),
            Cmd::Tool(t) => {
                self.tool = t;
                self.canvas.cancel();
            }
        }
    }

    /// The label of `cmd` in menus and the palette; Undo and Redo name
    /// the step they would undo or redo: "Undo Move Order".
    pub fn command_label(&self, cmd: Cmd) -> String {
        let step = self.session.as_ref().and_then(|s| match cmd {
            Cmd::Undo => s.editor.undo_label(),
            Cmd::Redo => s.editor.redo_label(),
            _ => None,
        });
        match step {
            Some(step) => format!("{} {step}", cmd.label()),
            None => cmd.label(),
        }
    }

    fn enabled(&self, cmd: Cmd) -> bool {
        let has = self.session.is_some();
        match cmd {
            Cmd::New
            | Cmd::Open
            | Cmd::Quit
            | Cmd::Theme(_)
            | Cmd::Shortcuts
            | Cmd::About
            | Cmd::Palette => true,
            Cmd::Undo => self.session.as_ref().is_some_and(|s| s.editor.can_undo()),
            Cmd::Redo => self.session.as_ref().is_some_and(|s| s.editor.can_redo()),
            Cmd::Delete => has && !self.sel.is_empty(),
            Cmd::TidySelection => has && !self.sel.elements.is_empty(),
            _ => has,
        }
    }

    fn keyboard(&mut self, ctx: &Context) {
        if self.dialog.is_some() || self.palette.is_some() {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        for cmd in Cmd::keyboard_order() {
            let sc = cmd.shortcut().unwrap();
            // Single-key shortcuts must not fire while typing.
            if typing
                && (sc.modifiers.is_none() || matches!(cmd, Cmd::Undo | Cmd::Redo | Cmd::SelectAll))
            {
                continue;
            }
            if self.enabled(cmd) && ctx.input_mut(|i| i.consume_shortcut(&sc)) {
                self.run(ctx, cmd);
            }
        }
        if !typing {
            let redo_y = ctx.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    Key::Y,
                ))
            });
            if redo_y && self.enabled(Cmd::Redo) {
                self.run(ctx, Cmd::Redo);
            }
            if ctx.input(|i| i.key_pressed(Key::Backspace)) && self.enabled(Cmd::Delete) {
                self.run(ctx, Cmd::Delete);
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                if self.tidy.is_some() {
                    self.cancel_tidy();
                    self.notify("Tidy cancelled");
                } else if self.canvas.busy() {
                    self.canvas.cancel();
                } else if self.tool != Tool::Select {
                    self.tool = Tool::Select;
                } else {
                    self.sel.clear();
                }
            }
            self.nudge(ctx);
        }
    }

    /// Arrow keys move the selection by one grid unit (ten with Shift).
    fn nudge(&mut self, ctx: &Context) {
        let Some(s) = &mut self.session else { return };
        if self.sel.elements.is_empty() {
            return;
        }
        let (dx, dy, shift) = ctx.input(|i| {
            let mut d = (0, 0);
            if i.key_pressed(Key::ArrowLeft) {
                d.0 -= 1;
            }
            if i.key_pressed(Key::ArrowRight) {
                d.0 += 1;
            }
            if i.key_pressed(Key::ArrowUp) {
                d.1 -= 1;
            }
            if i.key_pressed(Key::ArrowDown) {
                d.1 += 1;
            }
            (d.0, d.1, i.modifiers.shift)
        });
        if dx == 0 && dy == 0 {
            return;
        }
        let step = if shift { 10 } else { 1 };
        let moves: Vec<(ElementId, Pos)> = self
            .sel
            .elements
            .iter()
            .filter_map(|id| {
                let p = s.editor.doc.elements.get(id)?.pos?;
                Some((*id, Pos::new(p.x + dx * step, p.y + dy * step)))
            })
            .collect();
        // Nudges merge into one step while the selection stays the same.
        let mut ids: Vec<ElementId> = moves.iter().map(|(id, _)| *id).collect();
        ids.sort();
        s.editor
            .move_elements(&moves, Some(&format!("nudge {ids:?}")));
    }

    // ----- frame -----------------------------------------------------------

    fn handle_events(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            self.request(ctx, Pending::Open(Some(p)));
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved(Pending::Quit));
        }
        // Screenshot mode, used to check the interface automatically.
        if let Some((path, frames)) = &mut self.screenshot {
            *frames += 1;
            if *frames == 20 {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
            }
            let path = path.clone();
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Ok(png) =
                    umpl_draw::png::rgba_to_png(image.size[0] as u32, image.size[1] as u32, &rgba)
                {
                    let _ = std::fs::write(&path, png);
                }
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }

    fn periodic(&mut self, ctx: &Context) {
        if self.last_disk_check.elapsed() > Duration::from_secs(1) {
            self.last_disk_check = Instant::now();
            if self.dialog.is_none() && self.session.as_ref().is_some_and(Session::changed_on_disk)
            {
                self.dialog = Some(Dialog::DiskChanged);
            }
        }
        if self.last_recovery.elapsed() > RECOVERY_EVERY {
            self.last_recovery = Instant::now();
            if self.dirty() {
                self.write_recovery();
            }
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn update_title(&mut self, ctx: &Context) {
        let title = match &self.session {
            Some(s) => format!(
                "{}{} — fastumpl",
                s.display_name(),
                if s.is_dirty() { " •" } else { "" }
            ),
            None => "fastumpl".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn menu_item(&mut self, ui: &mut egui::Ui, cmd: Cmd) {
        let mut b = Button::new(self.command_label(cmd));
        if let Some(sc) = cmd.shortcut() {
            b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
        }
        if ui.add_enabled(self.enabled(cmd), b).clicked() {
            ui.close();
            self.run(ui.ctx(), cmd);
        }
    }

    fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    self.menu_item(ui, Cmd::New);
                    self.menu_item(ui, Cmd::Open);
                    ui.menu_button("Open recent", |ui| {
                        if self.recent.is_empty() {
                            ui.label("No recent files");
                        }
                        for p in self.recent.clone() {
                            if ui.button(p.display().to_string()).clicked() {
                                ui.close();
                                self.request(ui.ctx(), Pending::Open(Some(p)));
                            }
                        }
                    });
                    ui.separator();
                    self.menu_item(ui, Cmd::Save);
                    self.menu_item(ui, Cmd::SaveAs);
                    ui.separator();
                    self.menu_item(ui, Cmd::ExportSvg);
                    self.menu_item(ui, Cmd::ExportPng);
                    self.menu_item(ui, Cmd::ExportPlantUml);
                    self.menu_item(ui, Cmd::ExportMermaid);
                    self.menu_item(ui, Cmd::ExportUmple);
                    self.menu_item(ui, Cmd::CopyImage);
                    self.menu_item(ui, Cmd::CopyMermaid);
                    ui.separator();
                    self.menu_item(ui, Cmd::Quit);
                });
                ui.menu_button("Edit", |ui| {
                    self.menu_item(ui, Cmd::Undo);
                    self.menu_item(ui, Cmd::Redo);
                    ui.separator();
                    self.menu_item(ui, Cmd::Delete);
                    self.menu_item(ui, Cmd::SelectAll);
                    ui.separator();
                    self.menu_item(ui, Cmd::Palette);
                });
                ui.menu_button("View", |ui| {
                    self.menu_item(ui, Cmd::Fit);
                    self.menu_item(ui, Cmd::ZoomIn);
                    self.menu_item(ui, Cmd::ZoomOut);
                    self.menu_item(ui, Cmd::ZoomReset);
                    ui.separator();
                    self.menu_item(ui, Cmd::ToggleGrid);
                    self.menu_item(ui, Cmd::Theme(ThemePref::System));
                    self.menu_item(ui, Cmd::Theme(ThemePref::Light));
                    self.menu_item(ui, Cmd::Theme(ThemePref::Dark));
                    ui.separator();
                    self.menu_item(ui, Cmd::Problems);
                });
                ui.menu_button("Arrange", |ui| {
                    self.menu_item(ui, Cmd::Tidy);
                    self.menu_item(ui, Cmd::TidySelection);
                });
                ui.menu_button("Help", |ui| {
                    self.menu_item(ui, Cmd::Shortcuts);
                    self.menu_item(ui, Cmd::About);
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.session.is_none() {
                        return;
                    }
                    let busy = self.tidy.is_some();
                    let tidy =
                        ui.add_enabled(!busy, Button::new(RichText::new("✨ Tidy").strong()));
                    if tidy
                        .on_hover_text("Lay out the whole diagram again (Ctrl+T)")
                        .clicked()
                    {
                        self.run(ui.ctx(), Cmd::Tidy);
                    }
                    if busy {
                        ui.spinner();
                    }
                    if ui
                        .button("Fit")
                        .on_hover_text("Show the whole diagram (F)")
                        .clicked()
                    {
                        self.run(ui.ctx(), Cmd::Fit);
                    }
                    ui.label(format!("{:.0}%", self.camera.zoom * 100.0));
                    ui.separator();
                    // Right to left: Redo first, so Undo shows on its left.
                    for (cmd, icon) in [(Cmd::Redo, "⟳"), (Cmd::Undo, "⟲")] {
                        let sc = cmd.shortcut().map(|sc| ui.ctx().format_shortcut(&sc));
                        let tip = match sc {
                            Some(sc) => format!("{} ({sc})", self.command_label(cmd)),
                            None => self.command_label(cmd),
                        };
                        let b = ui
                            .add_enabled(
                                self.enabled(cmd),
                                Button::new(RichText::new(icon).size(15.0)),
                            )
                            .on_hover_text(&tip)
                            .on_disabled_hover_text(format!(
                                "Nothing to {}",
                                cmd.label().to_lowercase()
                            ));
                        if b.clicked() {
                            self.run(ui.ctx(), cmd);
                        }
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let Some(s) = &self.session else {
                    ui.label("fastumpl");
                    return;
                };
                let name = s.display_name();
                if s.is_dirty() {
                    ui.label(RichText::new(format!("{name} •")).strong());
                } else {
                    ui.label(name);
                }
                ui.separator();
                let doc = &s.editor.doc;
                let classes = doc
                    .elements
                    .values()
                    .filter(|e| e.kind != umpl_core::ElementKind::Note)
                    .count();
                ui.label(format!("{classes} classes"));
                ui.label(format!("{} relations", doc.relations.len()));
                ui.separator();
                let crossings = self.view.diagram.layout.stats.crossings;
                let text = format!(
                    "{crossings} crossing{}",
                    if crossings == 1 { "" } else { "s" }
                );
                if crossings > 0 {
                    ui.label(RichText::new(text).color(Color32::from_rgb(0xd0, 0x83, 0x2a)))
                        .on_hover_text("Tidy (Ctrl+T) may remove crossings");
                } else {
                    ui.label(text);
                }
                let problems = s.problems.len();
                if problems > 0 {
                    ui.separator();
                    if ui.link(format!("Problems: {problems}")).clicked() {
                        self.dialog = Some(Dialog::Problems);
                    }
                }
                if self.tidy.is_some() {
                    ui.separator();
                    ui.label("Tidying… (Esc to cancel)");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((msg, at)) = &self.status {
                        if at.elapsed() < Duration::from_secs(6) {
                            ui.label(RichText::new(msg).weak());
                        }
                    }
                });
            });
        });
    }

    fn tools_panel(&mut self, ctx: &Context) {
        egui::SidePanel::left("tools")
            .resizable(false)
            .exact_width(46.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    for t in Tool::ALL {
                        let sel = self.tool == t;
                        let r = ui.add_sized(
                            [34.0, 30.0],
                            Button::selectable(sel, RichText::new(t.icon()).size(16.0)),
                        );
                        if r.on_hover_text(format!("{} ({:?})", t.label(), t.key()))
                            .clicked()
                        {
                            self.run(ctx, Cmd::Tool(t));
                        }
                        if t == Tool::Select || t == Tool::Element(umpl_core::ElementKind::Note) {
                            ui.separator();
                        }
                    }
                });
            });
    }

    fn start_screen(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space((ui.available_height() * 0.18).max(20.0));
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("fastumpl").size(34.0).strong());
                ui.label(
                    RichText::new("UML class diagrams that lay themselves out.")
                        .size(16.0)
                        .weak(),
                );
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    let w = 300.0;
                    ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                    if ui
                        .add_sized([140.0, 34.0], Button::new("New diagram"))
                        .clicked()
                    {
                        self.run(ctx, Cmd::New);
                    }
                    ui.add_space(12.0);
                    if ui.add_sized([140.0, 34.0], Button::new("Open…")).clicked() {
                        self.run(ctx, Cmd::Open);
                    }
                });
                ui.add_space(28.0);
                if !self.recent.is_empty() {
                    ui.label(RichText::new("Recent").strong());
                    ui.add_space(6.0);
                    for p in self.recent.clone() {
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let r = ui
                            .link(RichText::new(name).size(15.0))
                            .on_hover_text(p.display().to_string());
                        if r.clicked() {
                            self.request(ctx, Pending::Open(Some(p)));
                        }
                    }
                } else {
                    ui.label(RichText::new("Drop a .umpl file here to open it.").weak());
                }
            });
        });
    }
}

impl Selection {
    /// Drops ids that no longer exist (after undo or redo).
    pub fn retain_existing(&mut self, doc: &umpl_core::Document) {
        self.elements.retain(|e| doc.elements.contains_key(e));
        self.relations.retain(|r| doc.relations.contains_key(r));
        self.packages.retain(|p| doc.packages.contains_key(p));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.poll_tidy(ctx);
        self.periodic(ctx);
        if let Some(s) = &mut self.session {
            self.view.sync(&mut s.editor);
        }
        self.keyboard(ctx);
        self.menu_bar(ctx);
        self.status_bar(ctx);
        if self.session.is_some() {
            self.tools_panel(ctx);
            egui::SidePanel::right("inspector")
                .resizable(true)
                .default_width(270.0)
                .frame(Frame::side_top_panel(&ctx.style()).inner_margin(Margin::same(10)))
                .show(ctx, |ui| self.inspector_ui(ui));
            let bg = crate::paint::c32(self.diagram_theme(ctx).background);
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(bg))
                .show(ctx, |ui| self.canvas_ui(ui));
        } else {
            self.start_screen(ctx);
        }
        self.dialogs(ctx);
        self.palette_ui(ctx);
        self.update_title(ctx);
        if self.view.animating() {
            ctx.request_repaint();
        }
    }
}
