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
mod inspector;
mod paint;
mod session;
mod storage;
mod view;

use std::path::PathBuf;

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
        println!("fastumpl {}", env!("CARGO_PKG_VERSION"));
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
    eframe::run_native("fastumpl", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, file)))))
}
