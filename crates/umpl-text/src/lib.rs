//! Reads and writes `.umpl` files, imports PlantUML and exports PlantUML
//! and Mermaid.
//!
//! [`parse`] never fails: lines it does not understand are kept as raw text
//! (written back unchanged by [`print`]) and reported as [`Problem`]s.

mod fs;
pub mod mermaid;
mod parser;
pub mod plantuml;
mod printer;
mod scan;

use std::path::Path;

pub use fs::write_atomic;
pub use parser::{parse, Parsed, Problem};
pub use printer::print;
use umpl_core::Document;

/// A text format fastumpl reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Umpl,
    PlantUml,
    Mermaid,
    /// Mermaid inside a Markdown code block.
    MermaidMarkdown,
}

/// File extensions of PlantUML files.
pub const PLANTUML_EXTENSIONS: [&str; 5] = ["puml", "plantuml", "pu", "iuml", "wsd"];

impl Format {
    /// The format a file name implies, from its extension.
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "umpl" => Some(Format::Umpl),
            "mmd" | "mermaid" => Some(Format::Mermaid),
            "md" | "markdown" => Some(Format::MermaidMarkdown),
            e if PLANTUML_EXTENSIONS.contains(&e) => Some(Format::PlantUml),
            _ => None,
        }
    }

    /// The format named on a command line: `umpl`, `plantuml`, `mermaid`,
    /// `markdown`, or a file extension.
    pub fn from_name(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "plantuml" => Some(Format::PlantUml),
            "mermaid" => Some(Format::Mermaid),
            "markdown" => Some(Format::MermaidMarkdown),
            other => Format::from_path(Path::new(&format!("x.{other}"))),
        }
    }

    /// Whether [`read_as`] can read this format.
    pub fn can_read(self) -> bool {
        matches!(self, Format::Umpl | Format::PlantUml)
    }
}

/// Reads `src` in `format`. Only formats that [`Format::can_read`].
///
/// # Panics
///
/// For a format that cannot be read.
pub fn read_as(format: Format, src: &str) -> Parsed {
    match format {
        Format::Umpl => parse(src),
        Format::PlantUml => plantuml::import(src),
        Format::Mermaid | Format::MermaidMarkdown => panic!("{format:?} cannot be read"),
    }
}

/// Writes `doc` in `format`.
pub fn write_as(format: Format, doc: &Document) -> String {
    match format {
        Format::Umpl => print(doc),
        Format::PlantUml => plantuml::export(doc),
        Format::Mermaid => mermaid::export(doc),
        Format::MermaidMarkdown => mermaid::export_markdown(doc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_from_paths_and_names() {
        assert_eq!(Format::from_path(Path::new("a.UMPL")), Some(Format::Umpl));
        assert_eq!(
            Format::from_path(Path::new("a.puml")),
            Some(Format::PlantUml)
        );
        assert_eq!(
            Format::from_path(Path::new("a.wsd")),
            Some(Format::PlantUml)
        );
        assert_eq!(Format::from_path(Path::new("a.mmd")), Some(Format::Mermaid));
        assert_eq!(
            Format::from_path(Path::new("README.md")),
            Some(Format::MermaidMarkdown)
        );
        assert_eq!(Format::from_path(Path::new("a.svg")), None);
        assert_eq!(Format::from_path(Path::new("a")), None);
        assert_eq!(Format::from_name("mermaid"), Some(Format::Mermaid));
        assert_eq!(Format::from_name("puml"), Some(Format::PlantUml));
        assert_eq!(Format::from_name("PlantUML"), Some(Format::PlantUml));
        assert_eq!(Format::from_name("md"), Some(Format::MermaidMarkdown));
        assert_eq!(Format::from_name("png"), None);
        assert!(Format::PlantUml.can_read() && !Format::Mermaid.can_read());
    }
}
