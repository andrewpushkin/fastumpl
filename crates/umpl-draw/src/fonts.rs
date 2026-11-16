//! The bundled font (DejaVu Sans Mono; license in `assets/fonts`).
//! Every face has the same advance width, so text width is
//! `chars × ADVANCE × size` everywhere.

pub const FAMILY: &str = "DejaVu Sans Mono";
pub const REGULAR: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");
pub const BOLD: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Bold.ttf");
pub const ITALIC: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono-Oblique.ttf");
pub const BOLD_ITALIC: &[u8] =
    include_bytes!("../../../assets/fonts/DejaVuSansMono-BoldOblique.ttf");

/// Advance width per character, in em (1233 / 2048).
pub const ADVANCE: f32 = 0.602;
/// Distance from the vertical centre of a line to its baseline, in em.
pub const CENTER_TO_BASELINE: f32 = 0.346;
