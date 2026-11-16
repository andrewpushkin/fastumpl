//! One open diagram: the document with its history, its file, and whether
//! it has unsaved changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use umpl_core::{Document, Editor};
use umpl_draw::Diagram;
use umpl_text::{parse, print, write_atomic, Problem};

pub struct Session {
    pub editor: Editor,
    pub path: Option<PathBuf>,
    /// Revision of the editor when last saved (or opened).
    saved_revision: u64,
    pub problems: Vec<Problem>,
    /// Modification time of the file when we last read or wrote it.
    disk_stamp: Option<SystemTime>,
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
        }
    }

    /// Reads a file. Elements without a stored position are placed (the
    /// whole diagram is laid out if the file has no `layout` block); that
    /// does not count as an unsaved change.
    pub fn open(path: &Path) -> Result<Session, String> {
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
        let mut s = Session::from_text(&src);
        s.path = Some(path.to_path_buf());
        s.disk_stamp = mtime(path);
        Ok(s)
    }

    pub fn from_text(src: &str) -> Session {
        let parsed = parse(src);
        let mut editor = Editor::new(parsed.doc);
        if editor.doc.elements.values().any(|e| e.pos.is_none()) {
            let diagram = if parsed.has_layout {
                Diagram::arrange(&editor.doc)
            } else {
                Diagram::tidy(&editor.doc, &umpl_layout::Options::default())
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
            .unwrap_or_else(|| "Untitled.umpl".into())
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
