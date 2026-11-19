//! One open diagram: the document with its history, its file, and whether
//! it has unsaved changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use umpl_core::{Document, Editor};
use umpl_draw::Diagram;
use umpl_text::{parse, print, read_path, write_as, write_atomic, Format, Parsed, Problem};

/// Diagrams with more elements than this (or twice as many relations)
/// open at once and are laid out in the background.
pub const DEFER_ABOVE: usize = 150;

pub struct Session {
    pub editor: Editor,
    pub path: Option<PathBuf>,
    /// Revision of the editor when last saved (or opened).
    saved_revision: u64,
    pub problems: Vec<Problem>,
    /// Modification time of the file when we last read or wrote it.
    disk_stamp: Option<SystemTime>,
    /// File name offered by Save for a diagram without a file, such as
    /// one imported from PlantUML or Umple.
    pub suggested_name: Option<String>,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Session {
    pub fn new_empty() -> Session {
        Session {
            editor: Editor::new(Document::new()),
            path: None,
            saved_revision: 0,
            problems: Vec::new(),
            disk_stamp: None,
            suggested_name: None,
        }
    }

    /// Reads a file. Elements without a stored position are placed (the
    /// whole diagram is laid out with `layout`, for the view's shape and
    /// the chosen goal, if the file has no `layout` block); that does not
    /// count as an unsaved change.
    pub fn open(path: &Path, layout: &umpl_layout::Options) -> Result<Session, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let src = String::from_utf8_lossy(&bytes);
        let mut s = Session::from_text(&src, layout);
        // Opened anyway, as `umpl check` reads it; Problems says what Save
        // will change.
        if std::str::from_utf8(&bytes).is_err() {
            s.problems.insert(
                0,
                Problem {
                    line: 1,
                    col: 1,
                    message: "the file is not UTF-8: bytes that are not were replaced by �, \
                              and Save writes UTF-8"
                        .into(),
                },
            );
        }
        s.path = Some(path.to_path_buf());
        s.disk_stamp = mtime(path);
        Ok(s)
    }

    /// Reads diagram `n` (from 0; see [`umpl_text::plantuml::diagrams`]) of
    /// a PlantUML file, or an Umple file, into a new, unsaved diagram without
    /// a file, so Save asks for a `.umpl` name and never overwrites the
    /// source. PlantUML `!include`s of files next to it are read too. A
    /// PlantUML diagram of a kind fastumpl does not read (sequence,
    /// activity, …) is an error.
    pub fn import_nth(
        path: &Path,
        format: Format,
        n: usize,
        layout: &umpl_layout::Options,
    ) -> Result<Session, String> {
        let read = match format {
            Format::PlantUml => umpl_text::plantuml::import_path_nth(path, n),
            _ => read_path(format, path),
        };
        let parsed = read.map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        if format == Format::PlantUml && parsed.doc.elements.is_empty() {
            if let Some(p) = parsed
                .problems
                .iter()
                .find(|p| p.message.starts_with("this is a"))
            {
                let mut m = p.message.clone();
                m[..1].make_ascii_uppercase();
                return Err(format!("{}: {m}.", path.display()));
            }
        }
        let mut s = Session::from_parsed(parsed, layout);
        let stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        s.suggested_name = Some(format!("{stem}.umpl"));
        s.mark_dirty();
        Ok(s)
    }

    pub fn from_text(src: &str, layout: &umpl_layout::Options) -> Session {
        Session::from_parsed(parse(src), layout)
    }

    pub fn from_parsed(parsed: Parsed, layout: &umpl_layout::Options) -> Session {
        let mut editor = Editor::new(parsed.doc);
        // A big diagram is laid out on a background thread instead
        // (see `App::install_session`), so the window does not freeze.
        let deferred = !parsed.has_layout
            && (editor.doc.elements.len() > DEFER_ABOVE
                || editor.doc.relations.len() > 2 * DEFER_ABOVE);
        if !deferred && editor.doc.elements.values().any(|e| e.pos.is_none()) {
            let diagram = if parsed.has_layout {
                Diagram::arrange(&editor.doc)
            } else {
                Diagram::tidy(&editor.doc, layout)
            };
            let ops = editor.position_changes(&diagram.positions());
            editor.apply_silently(ops);
        }
        let saved_revision = editor.revision;
        Session {
            editor,
            path: None,
            saved_revision,
            problems: parsed.problems,
            disk_stamp: None,
            suggested_name: None,
        }
    }

    /// Elements still waiting for the first layout, which the app starts
    /// in the background.
    pub fn needs_layout(&self) -> bool {
        self.editor.doc.elements.values().any(|e| e.pos.is_none())
    }

    /// Takes in the first layout of a diagram opened without one: no undo
    /// step, and a file that was unchanged stays unchanged.
    pub fn settle_layout(&mut self, positions: &[(umpl_core::ElementId, Option<umpl_core::Pos>)]) {
        let clean = !self.is_dirty();
        let known: Vec<_> = positions
            .iter()
            .filter(|(id, _)| self.editor.doc.elements.contains_key(id))
            .copied()
            .collect();
        let ops = self.editor.position_changes(&known);
        self.editor.apply_silently(ops);
        if clean {
            self.saved_revision = self.editor.revision;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.editor.revision != self.saved_revision
    }

    pub fn mark_dirty(&mut self) {
        self.saved_revision = u64::MAX;
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| self.suggested_name.clone())
            .unwrap_or_else(|| "Untitled.umpl".into())
    }

    /// The display name without its extension, for exported files.
    pub fn stem(&self) -> String {
        let name = self.display_name();
        match name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem.to_string(),
            _ => name,
        }
    }

    /// The diagram in another text format.
    pub fn export_text(&self, format: Format) -> String {
        write_as(format, &self.editor.doc)
    }

    pub fn text(&self) -> String {
        print(&self.editor.doc)
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        write_atomic(path, self.text().as_bytes())
            .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.saved_revision = self.editor.revision;
        self.disk_stamp = mtime(path);
        Ok(())
    }

    /// Has the file changed on disk since we read or wrote it?
    pub fn changed_on_disk(&self) -> bool {
        match (&self.path, self.disk_stamp) {
            (Some(p), Some(stamp)) => mtime(p).is_some_and(|m| m != stamp),
            _ => false,
        }
    }

    /// Accept the file on disk as it is now without reloading.
    pub fn acknowledge_disk(&mut self) {
        if let Some(p) = &self.path {
            self.disk_stamp = mtime(p);
        }
    }
}
