//! Drawing: from a document plus layout to a flat list of shapes, which the
//! app paints on screen and [`svg::to_svg`] writes to a file. Both use the
//! same list, so an export is exactly what is on screen.
//!
//! Units: the layout works in grid units; shapes are in pixels
//! ([`GRID`] pixels per unit).

pub mod diagram;
pub mod fonts;
pub mod measure;
#[cfg(feature = "png")]
pub mod png;
pub mod scene;
pub mod svg;
pub mod theme;

pub use diagram::{build_graph, Diagram, Mapping, NodeRef};
pub use measure::{box_content, BoxContent};
pub use scene::{build_scene, RectF, Scene, SceneOptions, Shape, TextStyle};
pub use theme::{Color, Theme};

/// Pixels per grid unit.
pub const GRID: f32 = 8.0;
