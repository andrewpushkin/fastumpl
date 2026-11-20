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
use umpl_layout::{Goal, Options};
use umpl_text::{Format, PLANTUML_EXTENSIONS};

use crate::camera::Camera;
use crate::canvas::CanvasState;
use crate::commands::{Cmd, ThemePref, Tool, ViewMode};
use crate::inspector::InspectorState;
use crate::prefs::Prefs;
use crate::session::Session;
use crate::source::SourceState;
use crate::storage::{push_recent, Storage};
use crate::view::{Selection, View};

/// A Tidy running on a background thread.
pub struct TidyJob {
    revision: u64,
    rx: mpsc::Receiver<Diagram>,
    cancel: Arc<AtomicBool>,
    /// The first layout of a diagram opened without one.
    pub initial: bool,
}

/// What to do once unsaved changes are dealt with.
#[derive(Clone, Debug)]
pub enum Pending {
    New,
    Open(Option<PathBuf>),
    /// Opens a recovery copy (and then deletes it).
    Recover(PathBuf),
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
    Settings,
    /// A PlantUML file with several diagrams: which one to open.
    PickDiagram(PathBuf, Vec<umpl_text::plantuml::DiagramInfo>),
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
    /// Fewest line crossings (the default) or fill the window.
    pub layout_goal: Goal,
    /// Fonts and text sizes.
    pub prefs: Prefs,
    /// The text alone fills the window (View: Text).
    pub text_only: bool,
    /// The file named on the command line, opened on the first frame.
    open_at_start: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    pub tidy: Option<TidyJob>,
    pub dialog: Option<Dialog>,
    pub palette: Option<(String, usize)>,
    pub status: Option<(String, Instant)>,
    pub canvas: CanvasState,
    pub inspector: InspectorState,
    pub source: SourceState,
    pub find: crate::find::FindState,
    allow_close: bool,
    last_disk_check: Instant,
    last_recovery: Instant,
    title: String,
    screenshot: Option<(PathBuf, u32)>,
}

const RECOVERY_EVERY: Duration = Duration::from_secs(30);

/// Whether the text panel is shown, from the saved setting: hidden unless
/// the user turned it on.
pub fn text_shown(setting: Option<&str>) -> bool {
    setting == Some("shown")
}

/// What the window shows, from the saved `view` setting, or else from the
/// older `text` one (the text panel shown or not).
pub fn view_mode(view: Option<&str>, text: Option<&str>) -> ViewMode {
    match view {
        Some("text") => ViewMode::Text,
        Some("both") => ViewMode::Both,
        Some("canvas") => ViewMode::Canvas,
        _ if text_shown(text) => ViewMode::Both,
        _ => ViewMode::Canvas,
    }
}

/// Whether the grid is shown, from the saved setting: shown unless the
/// user turned it off.
pub fn grid_shown(setting: Option<&str>) -> bool {
    setting != Some("hidden")
}

/// What layouts favour, from the saved setting: the fewest crossings
/// unless the user chose to fill the window.
pub fn layout_goal(setting: Option<&str>) -> Goal {
    match setting {
        Some("fill") => Goal::FillView,
        _ => Goal::FewestCrossings,
    }
}

/// The saved form of a layout goal.
pub fn layout_goal_setting(goal: Goal) -> &'static str {
    match goal {
        Goal::FillView => "fill",
        Goal::FewestCrossings => "crossings",
    }
}

/// File → Export, in order; `None` is a separator.
pub const EXPORT_MENU: [(Option<Cmd>, &str); 8] = [
    (Some(Cmd::ExportSvg), "SVG…"),
    (Some(Cmd::ExportPng), "PNG…"),
    (Some(Cmd::ExportPlantUml), "PlantUML…"),
    (Some(Cmd::ExportMermaid), "Mermaid…"),
    (Some(Cmd::ExportUmple), "Umple…"),
    (None, ""),
    (Some(Cmd::CopyImage), "Copy as image"),
    (Some(Cmd::CopyMermaid), "Copy as Mermaid"),
];

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> App {
        let storage = Storage::new();
        let prefs = Prefs::load(&storage);
        prefs.apply(&cc.egui_ctx);
        let theme_pref = match storage.load_setting("theme").as_deref() {
            Some("light") => ThemePref::Light,
            Some("dark") => ThemePref::Dark,
            _ => ThemePref::System,
        };
        let mode = view_mode(
            storage.load_setting("view").as_deref(),
            storage.load_setting("text").as_deref(),
        );
        let source_shown = mode != ViewMode::Canvas;
        let show_grid = grid_shown(storage.load_setting("grid").as_deref());
        let layout_goal = layout_goal(storage.load_setting("layout").as_deref());
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
            show_grid,
            layout_goal,
            prefs,
            text_only: mode == ViewMode::Text,
            open_at_start: None,
            tidy: None,
            dialog: None,
            palette: None,
            status: None,
            canvas: CanvasState::default(),
            inspector: InspectorState::default(),
            source: SourceState::new(source_shown),
            find: crate::find::FindState::default(),
            allow_close: false,
            last_disk_check: Instant::now(),
            last_recovery: Instant::now(),
            title: String::new(),
            screenshot: std::env::var_os("UMPL_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.apply_theme(&cc.egui_ctx);
        let recovered = app.find_recovery();
        // Opened on the first frame, when the window's size is known, so
        // the layout fills it.
        app.open_at_start = file;
        if !recovered.is_empty() {
            app.dialog = Some(Dialog::Recovery(recovered));
        }
        // For screenshots of a dialog: `UMPL_SHOW=settings`.
        if std::env::var("UMPL_SHOW").as_deref() == Ok("settings") {
            app.dialog = Some(Dialog::Settings);
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

    /// Width : height of the canvas, which layouts should fill. Before the
    /// first frame, the window less the panels around the canvas.
    pub fn view_aspect(&self, ctx: &Context) -> f64 {
        let r = self.canvas.rect;
        let (w, h) = if r.is_positive() && r.width() > 50.0 && r.height() > 50.0 {
            (r.width(), r.height())
        } else {
            let screen = ctx.content_rect();
            let text = if self.source.shown { 340.0 } else { 0.0 };
            (screen.width() - 46.0 - 270.0 - text, screen.height() - 60.0)
        };
        if w > 50.0 && h > 50.0 {
            (w / h) as f64
        } else {
            umpl_layout::DEFAULT_ASPECT
        }
        .clamp(0.4, 4.0)
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
        self.source.reset();
        self.find.forget();
        self.fit_pending = true;
        if self.session.as_ref().is_some_and(Session::needs_layout) {
            self.start_tidy_with(true);
        }
    }

    /// How a diagram opened now is laid out: for the canvas's shape, with
    /// the goal chosen in Arrange.
    pub fn layout_options(&self, ctx: &Context) -> Options {
        Options {
            aspect: self.view_aspect(ctx),
            goal: self.layout_goal,
            ..Options::default()
        }
    }

    pub fn open_path(&mut self, ctx: &Context, path: &Path) {
        let layout = self.layout_options(ctx);
        if let Some(f @ (Format::PlantUml | Format::Umple)) = Format::from_path(path) {
            return self.import_path(path, f, &layout);
        }
        match Session::open(path, &layout) {
            Ok(s) => self.install_session(s),
            Err(e) => {
                self.recent.retain(|p| p != path);
                self.storage.save_recent(&self.recent);
                self.error("Cannot open file", e);
            }
        }
    }

    /// Opens a PlantUML or Umple file as a new, unsaved diagram. A PlantUML
    /// file with several diagrams asks which one first.
    fn import_path(&mut self, path: &Path, format: Format, layout: &Options) {
        if format == Format::PlantUml {
            let src = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned());
            let list = src
                .map(|s| umpl_text::plantuml::diagrams(&s))
                .unwrap_or_default();
            if list.len() > 1 {
                self.dialog = Some(Dialog::PickDiagram(path.to_path_buf(), list));
                return;
            }
        }
        self.import_nth(path, format, 0, layout);
    }

    /// Opens diagram `n` of a PlantUML file (or the Umple file).
    pub fn import_nth(&mut self, path: &Path, format: Format, n: usize, layout: &Options) {
        match Session::import_nth(path, format, n, layout) {
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
                self.open_path(ctx, &path);
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
                    self.open_path(ctx, &path);
                }
            }
            Pending::Recover(file) => {
                // Read first: the copy may be the current diagram's own.
                let text = std::fs::read(&file)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                let origin = std::fs::read_to_string(file.with_extension("path"))
                    .ok()
                    .filter(|p| !p.trim().is_empty())
                    .map(PathBuf::from);
                self.clear_recovery();
                let mut s = Session::from_text(&text, &self.layout_options(ctx));
                s.path = origin;
                s.mark_dirty();
                self.install_session(s);
                // Only now that it is open is the copy no longer needed.
                App::discard_recovery(&file);
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
        self.save_as_path(&path)
    }

    /// Saves to `path`, the file chosen in Save As.
    pub fn save_as_path(&mut self, path: &Path) -> bool {
        // The recovery copy is named after the file it was written for:
        // the one written before this save has the old name.
        let before = self.recovery_file();
        let Some(s) = self.session.as_mut() else {
            return false;
        };
        match s.save_to(path) {
            Ok(()) => {
                if let Some(f) = before {
                    App::discard_recovery(&f);
                }
                self.clear_recovery();
                push_recent(&mut self.recent, path);
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

    pub(crate) fn recovery_file(&self) -> Option<PathBuf> {
        use std::hash::{Hash, Hasher};
        let s = self.session.as_ref()?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.path.hash(&mut h);
        // Untitled diagrams of two windows must not share one copy.
        if s.path.is_none() {
            std::process::id().hash(&mut h);
        }
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

    pub(crate) fn write_recovery(&mut self) {
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

    /// Opens a recovery copy, after asking about unsaved changes. The copy
    /// is deleted only once it is open, so cancelling keeps it.
    pub fn recover(&mut self, ctx: &Context, file: &Path) {
        self.request(ctx, Pending::Recover(file.to_path_buf()));
    }

    pub fn discard_recovery(file: &Path) {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(file.with_extension("path"));
    }

    // ----- layout ----------------------------------------------------------

    pub fn start_tidy(&mut self) {
        self.start_tidy_with(false);
    }

    /// Lays everything out on a background thread; `initial` for the first
    /// layout of a diagram opened without positions.
    fn start_tidy_with(&mut self, initial: bool) {
        let initial = initial || self.tidy.as_ref().is_some_and(|t| t.initial);
        self.cancel_tidy();
        let Some(s) = &self.session else { return };
        let doc = s.editor.doc.clone();
        let revision = s.editor.revision;
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let c = cancel.clone();
        let r = self.canvas.rect;
        let aspect = if r.is_positive() && r.width() > 50.0 && r.height() > 50.0 {
            (r.width() / r.height()).clamp(0.4, 4.0) as f64
        } else {
            umpl_layout::DEFAULT_ASPECT
        };
        let goal = self.layout_goal;
        std::thread::spawn(move || {
            let opts = Options {
                cancel: Some(c),
                aspect,
                goal,
                ..Options::default()
            };
            let _ = tx.send(Diagram::tidy(&doc, &opts));
        });
        self.tidy = Some(TidyJob {
            revision,
            rx,
            cancel,
            initial,
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
                if job.initial {
                    // Edits made meanwhile keep their places; new elements
                    // are placed as usual.
                    s.settle_layout(&diagram.positions());
                    if s.editor.revision == job.revision + 1 {
                        self.view.install(diagram, s.editor.revision);
                    }
                    self.fit_pending = true;
                    ctx.request_repaint();
                    return;
                }
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
        // Text typed a moment ago is part of the diagram the command sees.
        self.source_apply(ctx, true);
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
            Cmd::Find => self.open_find(),
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
            Cmd::ToggleGrid => {
                self.show_grid = !self.show_grid;
                let v = if self.show_grid { "shown" } else { "hidden" };
                self.storage.save_setting("grid", v);
            }
            Cmd::LayoutGoal(goal) => {
                self.layout_goal = goal;
                self.storage
                    .save_setting("layout", layout_goal_setting(goal));
                if self.session.is_some() {
                    self.status = Some((
                        "Diagrams are laid out this way from now on; Tidy lays out this one again"
                            .into(),
                        Instant::now(),
                    ));
                }
            }
            Cmd::LeftToRight => {
                if let Some(s) = &mut self.session {
                    let on = !s.editor.doc.left_to_right;
                    s.editor.set_left_to_right(on);
                    self.start_tidy();
                }
            }
            Cmd::ToggleSource => self.toggle_source(ctx),
            Cmd::View(mode) => self.set_view(ctx, mode),
            Cmd::Problems => self.dialog = Some(Dialog::Problems),
            Cmd::Palette => self.palette = Some((String::new(), 0)),
            Cmd::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Cmd::About => self.dialog = Some(Dialog::About),
            Cmd::Settings => self.dialog = Some(Dialog::Settings),
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
            | Cmd::Settings
            | Cmd::LayoutGoal(_)
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
                if let Some(t) = &self.tidy {
                    if t.initial {
                        // Without a first layout nothing can be shown:
                        // stop early, keeping the first candidate.
                        t.cancel.store(true, Ordering::Relaxed);
                        self.notify("Finishing with the first layout found…");
                    } else {
                        self.cancel_tidy();
                        self.notify("Tidy cancelled");
                    }
                } else if self.find.open {
                    self.close_find();
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
        let label = self.command_label(cmd);
        self.menu_item_as(ui, cmd, label);
    }

    /// A menu entry for `cmd` with its own text, such as "SVG…" under
    /// File → Export where the palette says "Export SVG…".
    fn menu_item_as(&mut self, ui: &mut egui::Ui, cmd: Cmd, label: impl Into<String>) {
        let mut b = Button::new(label.into());
        if let Some(sc) = cmd.shortcut() {
            b = b.shortcut_text(ui.ctx().format_shortcut(&sc));
        }
        if ui.add_enabled(self.enabled(cmd), b).clicked() {
            ui.close();
            self.run(ui.ctx(), cmd);
        }
    }

    /// A menu entry for an on/off `cmd`, ticked while it is on.
    fn menu_check(&mut self, ui: &mut egui::Ui, cmd: Cmd, label: &str, on: bool) {
        let mut checked = on;
        let mut r = ui.add_enabled(self.enabled(cmd), egui::Checkbox::new(&mut checked, label));
        if let Some(sc) = cmd.shortcut() {
            r = r.on_hover_text(ui.ctx().format_shortcut(&sc));
        }
        if r.clicked() {
            ui.close();
            self.run(ui.ctx(), cmd);
        }
    }

    /// What the window shows now.
    pub fn view(&self) -> ViewMode {
        match (self.text_only, self.source.shown) {
            (true, _) => ViewMode::Text,
            (false, true) => ViewMode::Both,
            (false, false) => ViewMode::Canvas,
        }
    }

    /// Shows the text, the text and the drawing, or the drawing, and keeps
    /// the choice for the next run.
    pub fn set_view(&mut self, ctx: &Context, mode: ViewMode) {
        if (mode == ViewMode::Canvas) == self.source.shown {
            self.toggle_source(ctx);
        }
        self.text_only = mode == ViewMode::Text;
        self.storage
            .save_setting("view", &mode.name().to_ascii_lowercase());
    }

    /// The Text | Both | Canvas switch, centred over the top bar's `row`.
    fn view_switch(&mut self, ui: &mut egui::Ui, row: egui::Rect) {
        let width = 216.0;
        let rect = egui::Rect::from_center_size(
            egui::pos2(ui.max_rect().center().x, row.center().y),
            egui::vec2(width, row.height().max(24.0)),
        );
        let mut picked = None;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.scope(|ui| {
            Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(7)
                .inner_margin(Margin::same(2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for mode in ViewMode::ALL {
                            let on = self.view() == mode;
                            let b = Button::selectable(on, mode.name())
                                .min_size(egui::vec2(66.0, 20.0))
                                .corner_radius(5);
                            if ui.add(b).clicked() {
                                picked = Some(mode);
                            }
                        }
                    });
                });
        });
        if let Some(mode) = picked {
            self.run(ui.ctx(), Cmd::View(mode));
        }
    }

    fn menu_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            let bar = egui::MenuBar::new().ui(ui, |ui| {
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
                    ui.add_enabled_ui(self.session.is_some(), |ui| {
                        ui.menu_button("Export", |ui| {
                            for (cmd, label) in EXPORT_MENU {
                                match cmd {
                                    Some(cmd) => self.menu_item_as(ui, cmd, label),
                                    None => {
                                        ui.separator();
                                    }
                                }
                            }
                        });
                    });
                    ui.separator();
                    self.menu_item(ui, Cmd::Settings);
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
                    self.menu_item(ui, Cmd::Find);
                    ui.separator();
                    self.menu_item(ui, Cmd::Palette);
                });
                ui.menu_button("View", |ui| {
                    self.menu_item(ui, Cmd::Fit);
                    self.menu_item(ui, Cmd::ZoomIn);
                    self.menu_item(ui, Cmd::ZoomOut);
                    self.menu_item(ui, Cmd::ZoomReset);
                    ui.separator();
                    self.menu_check(ui, Cmd::ToggleSource, "Text", self.source.shown);
                    self.menu_check(ui, Cmd::ToggleGrid, "Grid", self.show_grid);
                    ui.separator();
                    self.menu_item(ui, Cmd::Theme(ThemePref::System));
                    self.menu_item(ui, Cmd::Theme(ThemePref::Light));
                    self.menu_item(ui, Cmd::Theme(ThemePref::Dark));
                    ui.separator();
                    self.menu_item(ui, Cmd::Problems);
                });
                ui.menu_button("Arrange", |ui| {
                    self.menu_item(ui, Cmd::Tidy);
                    self.menu_item(ui, Cmd::TidySelection);
                    ui.separator();
                    ui.label(RichText::new("Lay out diagrams for").weak());
                    for (goal, label) in [
                        (Goal::FewestCrossings, "Fewest line crossings"),
                        (Goal::FillView, "Filling the window"),
                    ] {
                        let cmd = Cmd::LayoutGoal(goal);
                        if ui.radio(self.layout_goal == goal, label).clicked() {
                            ui.close();
                            self.run(ui.ctx(), cmd);
                        }
                    }
                    ui.separator();
                    let on = self
                        .session
                        .as_ref()
                        .is_some_and(|s| s.editor.doc.left_to_right);
                    let mut lr = on;
                    let r = ui
                        .add_enabled(
                            self.session.is_some(),
                            egui::Checkbox::new(&mut lr, "Left to right"),
                        )
                        .on_hover_text(
                            "Lay this diagram out left to right instead of top to bottom \
                             (PlantUML's left to right direction)",
                        );
                    if r.clicked() {
                        ui.close();
                        self.run(ui.ctx(), Cmd::LeftToRight);
                    }
                });
                ui.menu_button("Help", |ui| {
                    self.menu_item(ui, Cmd::Shortcuts);
                    self.menu_item(ui, Cmd::About);
                });
                // Where we are: fastumpl / the diagram.
                if let Some(s) = &self.session {
                    ui.add_space(10.0);
                    ui.label(RichText::new("fastumpl  /").weak());
                    let dirty = if s.is_dirty() { " •" } else { "" };
                    ui.label(RichText::new(format!("{}{dirty}", s.display_name())).strong());
                }

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
                    ui.separator();
                    // Right to left: zoom in, the zoom level, zoom out.
                    let zoom = self.camera.zoom;
                    let can_zoom_in = zoom < crate::camera::MAX_ZOOM - 1e-4;
                    let can_zoom_out = zoom > crate::camera::MIN_ZOOM + 1e-4;
                    for (cmd, text, enabled) in [
                        (Cmd::ZoomIn, "+".to_string(), can_zoom_in),
                        (Cmd::ZoomReset, format!("{:.0}%", zoom * 100.0), true),
                        (Cmd::ZoomOut, "−".to_string(), can_zoom_out),
                    ] {
                        let mut tip = cmd.label();
                        if let Some(sc) = cmd.shortcut() {
                            tip = format!("{tip} ({})", ui.ctx().format_shortcut(&sc));
                        }
                        let button = if cmd == Cmd::ZoomReset {
                            Button::new(text)
                                .frame(false)
                                .min_size(egui::vec2(40.0, 0.0))
                        } else {
                            Button::new(RichText::new(text).size(15.0))
                                .min_size(egui::vec2(22.0, 0.0))
                        };
                        if ui.add_enabled(enabled, button).on_hover_text(tip).clicked() {
                            self.run(ui.ctx(), cmd);
                        }
                    }
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
            if self.session.is_some() {
                self.view_switch(ui, bar.response.rect);
            }
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
                if let Some(seq) = &doc.sequence {
                    ui.label(format!("{} participants", seq.participants.len()));
                    ui.label(format!("{} messages", seq.messages().count()));
                    return;
                }
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
                match &self.tidy {
                    Some(t) if t.initial => {
                        ui.separator();
                        ui.spinner();
                        ui.label(format!(
                            "Laying out {classes} classes… (Esc: keep the first result)"
                        ));
                    }
                    Some(_) => {
                        ui.separator();
                        ui.label("Tidying… (Esc to cancel)");
                    }
                    None => {}
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

    /// The tools, on a card floating over the canvas's top left corner.
    /// The texts the canvas draws, for tests.
    #[cfg(test)]
    pub fn view_scene_texts(&mut self) -> Vec<String> {
        let Some(s) = &self.session else {
            return Vec::new();
        };
        let theme = umpl_draw::Theme::light();
        let sel = self.sel.clone();
        self.view
            .scene(&s.editor, &sel, &theme, false)
            .shapes
            .iter()
            .filter_map(|sh| match sh {
                umpl_draw::Shape::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Is the open diagram a sequence diagram? It is drawn from its text
    /// and edited there; the canvas only shows it.
    pub fn is_sequence(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.editor.doc.sequence.is_some())
    }

    fn tools_panel(&mut self, ctx: &Context) {
        let theme = self.diagram_theme(ctx);
        let at = self.canvas.rect.min + egui::vec2(12.0, 12.0);
        if !self.canvas.rect.is_positive() || self.is_sequence() {
            return;
        }
        egui::Area::new(egui::Id::new("tools"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(ui.visuals().window_fill)
                    .stroke(ui.visuals().window_stroke)
                    .corner_radius(10)
                    .inner_margin(Margin::same(5))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 2],
                        blur: 8,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(40),
                    })
                    .show(ui, |ui| {
                        ui.set_width(36.0);
                        ui.vertical_centered(|ui| {
                            for t in Tool::ALL {
                                let sel = self.tool == t;
                                let r = ui.add_sized([34.0, 30.0], Button::selectable(sel, ""));
                                // The button has no text: name it for screen readers.
                                r.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::Button,
                                        true,
                                        sel,
                                        t.label(),
                                    )
                                });
                                let fg = ui.style().interact_selectable(&r, sel).fg_stroke.color;
                                let icon = egui::Rect::from_center_size(
                                    r.rect.center(),
                                    egui::vec2(22.0, 22.0),
                                );
                                crate::icons::paint_tool_icon(ui.painter(), icon, t, fg, &theme);
                                if r.on_hover_text(format!("{} ({:?})", t.label(), t.key()))
                                    .clicked()
                                {
                                    self.run(ctx, Cmd::Tool(t));
                                }
                                if t == Tool::Select
                                    || t == Tool::Element(umpl_core::ElementKind::Note)
                                {
                                    ui.separator();
                                }
                            }
                        });
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

impl App {
    /// Brings the layout up to date with the document, unless the first
    /// layout of a diagram is still being computed.
    pub fn sync_view(&mut self) {
        let laying_out = self.tidy.as_ref().is_some_and(|t| t.initial);
        if let (Some(s), false) = (&mut self.session, laying_out) {
            self.view.sync(&mut s.editor);
        }
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
        if let Some(f) = self.open_at_start.take() {
            self.open_path(ctx, &f);
            if let (Some(s), Ok(name)) = (&self.session, std::env::var("UMPL_SELECT")) {
                if let Some(id) = s.editor.doc.find(&name) {
                    self.sel = Selection::only_element(id);
                }
            }
        }
        self.handle_events(ctx);
        self.poll_tidy(ctx);
        self.periodic(ctx);
        self.source_apply(ctx, false);
        self.sync_view();
        self.source_refresh();
        self.keyboard(ctx);
        self.menu_bar(ctx);
        self.find_bar(ctx);
        self.status_bar(ctx);
        if self.session.is_some() && self.text_only {
            egui::CentralPanel::default()
                .frame(Frame::central_panel(&ctx.style()).inner_margin(Margin::same(12)))
                .show(ctx, |ui| self.source_ui(ui));
        } else if self.session.is_some() {
            if self.source.shown {
                egui::SidePanel::left("source")
                    .resizable(true)
                    .default_width(340.0)
                    .min_width(180.0)
                    .frame(Frame::side_top_panel(&ctx.style()).inner_margin(Margin::same(8)))
                    .show(ctx, |ui| self.source_ui(ui));
            }
            egui::SidePanel::right("inspector")
                .resizable(true)
                .default_width(270.0)
                .frame(Frame::side_top_panel(&ctx.style()).inner_margin(Margin::same(10)))
                .show(ctx, |ui| self.inspector_ui(ui));
            let bg = crate::paint::c32(self.diagram_theme(ctx).background);
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(bg))
                .show(ctx, |ui| self.canvas_ui(ui));
            self.tools_panel(ctx);
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
