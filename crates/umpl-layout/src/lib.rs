//! Diagram layout: places boxes and routes orthogonal lines between them,
//! keeping line crossings few.
//!
//! The crate knows nothing about UML. It sees sized boxes ([`Node`]), typed
//! edges ([`Edge`]) and nested groups ([`Group`]). All coordinates are whole
//! grid units. Every function is deterministic.
//!
//! Entry points:
//! - [`layout_all`]: lay out everything (the Tidy command).
//! - [`place_new`]: place only nodes without a position.
//! - [`route_only`]: keep every node where it is and route the lines.
//! - [`reroute`]: re-route some lines after a node moved.

// Index loops read more clearly than iterator chains in these algorithms.
#![allow(clippy::needless_range_loop)]

mod coords;
mod graph;
mod labels;
mod layering;
mod ordering;
mod place;
mod rng;
mod router;
mod score;

pub use graph::*;
pub use place::{
    layout_all, place_new, route_only, update_after_move, Goal, Options, DEFAULT_ASPECT,
};
pub use rng::Rng;
pub use router::{reroute, route_all};
pub use score::{crossing_points, measure, Stats};
