//! The fastumpl document: elements, relations, packages and their stored
//! positions, plus undoable edit operations.
//!
//! Every change to a [`Document`] goes through [`Op`]s. An op replaces one
//! slot (an element, a relation, a package, the root declaration list or the
//! title) and returns the old value as its inverse, which is what undo uses.

mod editor;
mod model;
mod ops;

pub use editor::{valid_name, Editor};
pub use model::*;
pub use ops::{Op, Slot};
