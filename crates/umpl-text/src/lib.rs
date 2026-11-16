//! Reads and writes `.umpl` files.
//!
//! [`parse`] never fails: lines it does not understand are kept as raw text
//! (written back unchanged by [`print`]) and reported as [`Problem`]s.

mod fs;
mod parser;
mod printer;
mod scan;

pub use fs::write_atomic;
pub use parser::{parse, Parsed, Problem};
pub use printer::print;
