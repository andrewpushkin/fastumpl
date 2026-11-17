//! Every user command, with its label and shortcut, so menus, the command
//! palette and the keyboard all go through one place.

use egui::{Key, KeyboardShortcut, Modifiers};
use umpl_core::ElementKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Element(ElementKind),
    Package,
    Relation,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Element(ElementKind::Class),
        Tool::Element(ElementKind::Abstract),
        Tool::Element(ElementKind::Interface),
        Tool::Element(ElementKind::Enum),
        Tool::Element(ElementKind::Note),
        Tool::Package,
        Tool::Relation,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Element(k) => k.label(),
            Tool::Package => "Package",
            Tool::Relation => "Relation",
        }
    }

    pub fn key(self) -> Key {
        match self {
            Tool::Select => Key::V,
            Tool::Element(ElementKind::Class) => Key::C,
            Tool::Element(ElementKind::Abstract) => Key::A,
            Tool::Element(ElementKind::Interface) => Key::I,
            Tool::Element(ElementKind::Enum) => Key::E,
            Tool::Element(ElementKind::Note) => Key::N,
            Tool::Package => Key::P,
            Tool::Relation => Key::R,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    ExportSvg,
    ExportPng,
    ExportPlantUml,
    ExportMermaid,
    ExportUmple,
    CopyImage,
    CopyMermaid,
    Quit,
    Undo,
    Redo,
    Delete,
    SelectAll,
    Tidy,
    TidySelection,
    Fit,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Theme(ThemePref),
    ToggleGrid,
    ToggleSource,
    Problems,
    Palette,
    Shortcuts,
    About,
    Tool(Tool),
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

impl Cmd {
    /// Commands listed in the command palette, in order.
    pub fn all() -> Vec<Cmd> {
        let mut v = vec![
            Cmd::New,
            Cmd::Open,
            Cmd::Save,
            Cmd::SaveAs,
            Cmd::ExportSvg,
            Cmd::ExportPng,
            Cmd::ExportPlantUml,
            Cmd::ExportMermaid,
            Cmd::ExportUmple,
            Cmd::CopyImage,
            Cmd::CopyMermaid,
            Cmd::Undo,
            Cmd::Redo,
            Cmd::Delete,
            Cmd::SelectAll,
            Cmd::Tidy,
            Cmd::TidySelection,
            Cmd::Fit,
            Cmd::ZoomIn,
            Cmd::ZoomOut,
            Cmd::ZoomReset,
            Cmd::Theme(ThemePref::System),
            Cmd::Theme(ThemePref::Light),
            Cmd::Theme(ThemePref::Dark),
            Cmd::ToggleGrid,
            Cmd::ToggleSource,
            Cmd::Problems,
            Cmd::Shortcuts,
            Cmd::About,
            Cmd::Quit,
        ];
        v.extend(Tool::ALL.iter().map(|t| Cmd::Tool(*t)));
        v
    }

    pub fn label(self) -> String {
        match self {
            Cmd::New => "New diagram".into(),
            Cmd::Open => "Open…".into(),
            Cmd::Save => "Save".into(),
            Cmd::SaveAs => "Save as…".into(),
            Cmd::ExportSvg => "Export SVG…".into(),
            Cmd::ExportPng => "Export PNG…".into(),
            Cmd::ExportPlantUml => "Export PlantUML…".into(),
            Cmd::ExportMermaid => "Export Mermaid…".into(),
            Cmd::ExportUmple => "Export Umple…".into(),
            Cmd::CopyImage => "Copy as image".into(),
            Cmd::CopyMermaid => "Copy as Mermaid".into(),
            Cmd::Quit => "Quit".into(),
            Cmd::Undo => "Undo".into(),
            Cmd::Redo => "Redo".into(),
            Cmd::Delete => "Delete".into(),
            Cmd::SelectAll => "Select all".into(),
            Cmd::Tidy => "Tidy (lay out again)".into(),
            Cmd::TidySelection => "Tidy selection".into(),
            Cmd::Fit => "Fit diagram in window".into(),
            Cmd::ZoomIn => "Zoom in".into(),
            Cmd::ZoomOut => "Zoom out".into(),
            Cmd::ZoomReset => "Actual size".into(),
            Cmd::Theme(ThemePref::System) => "Theme: follow system".into(),
            Cmd::Theme(ThemePref::Light) => "Theme: light".into(),
            Cmd::Theme(ThemePref::Dark) => "Theme: dark".into(),
            Cmd::ToggleGrid => "Show or hide grid".into(),
            Cmd::ToggleSource => "Show or hide text".into(),
            Cmd::Problems => "Show problems in file".into(),
            Cmd::Palette => "Command palette".into(),
            Cmd::Shortcuts => "Keyboard shortcuts".into(),
            Cmd::About => "About fastumpl".into(),
            Cmd::Tool(t) => format!("Tool: {}", t.label()),
        }
    }

    /// Shortcuts that work from anywhere (with a modifier).
    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let k = |m, key| Some(KeyboardShortcut::new(m, key));
        match self {
            Cmd::New => k(CMD, Key::N),
            Cmd::Open => k(CMD, Key::O),
            Cmd::Save => k(CMD, Key::S),
            Cmd::SaveAs => k(CMD_SHIFT, Key::S),
            Cmd::ExportSvg => k(CMD, Key::E),
            Cmd::ExportPng => k(CMD_SHIFT, Key::E),
            Cmd::CopyImage => k(CMD_SHIFT, Key::C),
            Cmd::Quit => k(CMD, Key::Q),
            Cmd::Undo => k(CMD, Key::Z),
            Cmd::Redo => k(CMD_SHIFT, Key::Z),
            Cmd::SelectAll => k(CMD, Key::A),
            Cmd::Tidy => k(CMD, Key::T),
            Cmd::Fit => k(Modifiers::NONE, Key::F),
            Cmd::ZoomIn => k(CMD, Key::Plus),
            Cmd::ZoomOut => k(CMD, Key::Minus),
            Cmd::ZoomReset => k(CMD, Key::Num0),
            Cmd::Palette => k(CMD, Key::K),
            Cmd::ToggleSource => k(CMD, Key::B),
            Cmd::Delete => k(Modifiers::NONE, Key::Delete),
            Cmd::Tool(t) => k(Modifiers::NONE, t.key()),
            _ => None,
        }
    }

    /// Commands checked against the keyboard each frame, most specific
    /// (more modifiers) first so Ctrl+Shift+S is not read as Ctrl+S.
    pub fn keyboard_order() -> Vec<Cmd> {
        let mut v: Vec<Cmd> = Cmd::all()
            .into_iter()
            .chain([Cmd::Palette])
            .filter(|c| c.shortcut().is_some())
            .collect();
        v.sort_by_key(|c| {
            let m = c.shortcut().unwrap().modifiers;
            std::cmp::Reverse(m.shift as u8 + m.command as u8 + m.alt as u8)
        });
        v
    }
}
