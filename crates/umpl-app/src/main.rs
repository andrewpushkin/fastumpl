//! fastumpl: a UML class diagram editor.
//!
//! Usage: `fastumpl [file.umpl]`

// No console window on Windows in release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod camera;
mod canvas;
mod commands;
mod dialogs;
mod find;
mod icons;
mod inspector;
mod link;
mod paint;
mod prefs;
mod scrollbar;
mod session;
mod source;
mod storage;
mod view;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Release version (0.1.N, N = commits on main), set by the build as
/// UMPL_VERSION; plain builds show the Cargo version.
pub const VERSION: &str = match option_env!("UMPL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

fn icon() -> Option<egui::IconData> {
    let img = umpl_draw::png::svg_to_rgba(include_str!("../../../assets/icon.svg"), 1.0).ok()?;
    Some(egui::IconData {
        rgba: img.rgba,
        width: img.width,
        height: img.height,
    })
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("fastumpl {VERSION}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fastumpl [file.umpl]");
        return Ok(());
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("fastumpl")
        .with_app_id("fastumpl")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([640.0, 420.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "fastumpl",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, file)))),
    )
}
