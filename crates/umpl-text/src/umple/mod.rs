//! Umple (`.ump`) class models: import into a [`Document`] and export from
//! one.
//!
//! Umple (umple.org) is a modelling language: classes, attributes,
//! associations and generalizations are written as text, next to state
//! machines and method code that fastumpl does not draw. Import reads the
//! class model and reports everything else it skips as a [`Problem`]; export
//! writes a class model the Umple compiler can read. See
//! `docs/plans/2026-11-17-umple.md` for the mapping.
//!
//! [`Problem`]: crate::Problem

mod export;
mod import;
mod lexer;

pub use export::export;
pub use import::import;
