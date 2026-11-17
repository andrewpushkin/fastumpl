//! Interaction tests: drive the real app with pointer and key events.

use std::sync::Once;
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use umpl_core::{ElementId, ElementKind, Pos, RelationKind};
use umpl_draw::{RectF, GRID};

use crate::app::{App, Pending};
use crate::canvas::handles;
use crate::commands::Cmd;

static ENV: Once = Once::new();

/// Keeps the tests' recent files and recovery copies out of the real ones.
fn isolate() {
    ENV.call_once(|| {
        let dir = std::env::temp_dir().join(format!("fastumpl-tests-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        std::env::set_var("XDG_CACHE_HOME", dir.join("cache"));
        std::env::set_var("APPDATA", dir.join("appdata"));
    });
}

fn new_app() -> Harness<'static, App> {
    isolate();
    let mut h = Harness::builder()
        .with_size(Vec2::new(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(|cc| App::new(cc, None));
    let ctx = h.ctx.clone();
    h.state_mut().perform(&ctx, Pending::New);
    h.run_steps(3);
    h
}

fn add(h: &mut Harness<'static, App>, name: &str, x: i32, y: i32) -> ElementId {
    let s = h.state_mut().session.as_mut().unwrap();
    let id = s
        .editor
        .add_element(ElementKind::Class, name, None, Some(Pos::new(x, y)));
    h.run_steps(2);
    id
}

fn rect_of(h: &Harness<'static, App>, id: ElementId) -> RectF {
    let d = &h.state().view.diagram;
    let i = d.mapping.node_of(id).unwrap();
    let p = d.layout.nodes[i];
    let n = &d.graph.nodes[i];
    RectF::new(
        p.0 as f32 * GRID,
        p.1 as f32 * GRID,
        n.w as f32 * GRID,
        n.h as f32 * GRID,
    )
}

fn screen(h: &Harness<'static, App>, world: (f32, f32)) -> Pos2 {
    h.state().camera.to_screen(world)
}

fn press(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    press(h, pos, true);
    press(h, pos, false);
    h.run_steps(3);
}

fn double_click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    press(h, pos, true);
    press(h, pos, false);
    press(h, pos, true);
    press(h, pos, false);
    h.run_steps(3);
}

fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    press(h, from, true);
    for k in 1..=10 {
        let t = k as f32 / 10.0;
        h.hover_at(from + (to - from) * t);
    }
    press(h, to, false);
    h.run_steps(3);
}

#[test]
fn double_click_creates_class_and_inline_edit_renames_it() {
    let mut h = new_app();
    let at = h.state().canvas.rect.center();
    double_click(&mut h, at);
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(s.editor.doc.elements.len(), 1);
    assert!(h.state().canvas.edit.is_some());

    let edit = h.state_mut().canvas.edit.as_mut().unwrap();
    edit.name = "Order".into();
    edit.body = "+id: OrderId\n+total(): Money".into();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let s = h.state().session.as_ref().unwrap();
    let el = s.editor.doc.elements.values().next().unwrap();
    assert_eq!(el.name, "Order");
    assert_eq!(el.members.len(), 2);
    assert!(h.state().canvas.edit.is_none());
    assert!(s.is_dirty());
}

#[test]
fn invalid_name_keeps_editor_open() {
    let mut h = new_app();
    let at = h.state().canvas.rect.center();
    double_click(&mut h, at);
    h.state_mut().canvas.edit.as_mut().unwrap().name = "1 bad".into();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(2);
    let edit = h.state().canvas.edit.as_ref().expect("editor stays open");
    assert!(edit.error.is_some());
}

#[test]
fn dragging_from_edge_dot_connects_boxes() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 40, 0);
    let ra = rect_of(&h, a);
    let rb = rect_of(&h, b);
    // Hover A so its dots show, then drag from the right dot onto B.
    h.hover_at(screen(&h, ra.center()));
    h.run_steps(2);
    let dot = screen(&h, handles(ra)[1]);
    let target = screen(&h, rb.center());
    drag(&mut h, dot, target);
    assert!(h.state().canvas.connect.is_some(), "kind menu should open");
    h.key_press(Key::Num3);
    h.run_steps(3);
    let s = h.state().session.as_ref().unwrap();
    let rel = s
        .editor
        .doc
        .relations
        .values()
        .next()
        .expect("relation created");
    assert_eq!(rel.kind, RelationKind::Association);
    assert_eq!((rel.from.element, rel.to.element), (a, b));
    assert!(h.state().view.diagram.layout.routes[0].points.len() >= 2);
}

#[test]
fn dragging_a_box_moves_it_and_undo_restores() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let r = rect_of(&h, a);
    let from = screen(&h, r.center());
    let zoom = h.state().camera.zoom;
    drag(
        &mut h,
        from,
        from + Vec2::new(10.0 * GRID * zoom, 5.0 * GRID * zoom),
    );
    let pos = |h: &Harness<'static, App>| {
        h.state().session.as_ref().unwrap().editor.doc.elements[&a]
            .pos
            .unwrap()
    };
    assert_eq!(pos(&h), Pos::new(10, 5));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(pos(&h), Pos::new(0, 0));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    assert_eq!(pos(&h), Pos::new(10, 5));
}

#[test]
fn rubber_band_selects_and_delete_removes() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    let c = add(&mut h, "C", 0, 40);
    {
        let s = h.state_mut().session.as_mut().unwrap();
        s.editor.connect(a, c, RelationKind::Dependency);
    }
    h.run_steps(2);
    let ra = rect_of(&h, a);
    let rb = rect_of(&h, b);
    let from = screen(&h, (ra.x - 20.0, ra.y - 20.0));
    let to = screen(&h, (rb.right() + 10.0, rb.bottom() + 10.0));
    drag(&mut h, from, to);
    let mut sel = h.state().sel.elements.clone();
    sel.sort();
    assert_eq!(sel, vec![a, b]);
    h.key_press(Key::Delete);
    h.run_steps(2);
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(
        s.editor.doc.elements.keys().copied().collect::<Vec<_>>(),
        vec![c]
    );
    assert!(s.editor.doc.relations.is_empty());
}

#[test]
fn click_selects_and_escape_clears() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let r = rect_of(&h, a);
    let at = screen(&h, r.center());
    click(&mut h, at);
    assert_eq!(h.state().sel.elements, vec![a]);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(h.state().sel.is_empty());
}

#[test]
fn tool_click_creates_interface() {
    let mut h = new_app();
    h.key_press(Key::I);
    h.run_steps(2);
    let at = h.state().canvas.rect.center();
    click(&mut h, at);
    let s = h.state().session.as_ref().unwrap();
    let el = s.editor.doc.elements.values().next().unwrap();
    assert_eq!(el.kind, ElementKind::Interface);
}

#[test]
fn tidy_runs_in_background_and_is_one_undo_step() {
    let mut h = new_app();
    // All piled up on one spot.
    let ids: Vec<ElementId> = (0..5)
        .map(|i| add(&mut h, &format!("C{i}"), 0, 0))
        .collect();
    {
        let s = h.state_mut().session.as_mut().unwrap();
        for i in 1..5 {
            s.editor
                .connect(ids[i], ids[0], RelationKind::Generalization);
        }
    }
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::Tidy);
    let start = Instant::now();
    while h.state().tidy.is_some() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
    let stats = h.state().view.diagram.layout.stats;
    assert_eq!(stats.overlaps, 0);
    assert_eq!(stats.upward_violations, 0);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    let s = h.state().session.as_ref().unwrap();
    assert!(ids
        .iter()
        .all(|id| s.editor.doc.elements[id].pos == Some(Pos::new(0, 0))));
}

#[test]
fn save_writes_file_that_reopens() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    {
        let s = h.state_mut().session.as_mut().unwrap();
        s.editor.connect(a, b, RelationKind::Composition);
    }
    h.run_steps(2);
    let path = std::env::temp_dir().join(format!("fastumpl-save-{}.umpl", std::process::id()));
    let ok = {
        let s = h.state_mut().session.as_mut().unwrap();
        s.save_to(&path)
    };
    assert!(ok.is_ok());
    assert!(!h.state().session.as_ref().unwrap().is_dirty());
    let reopened = crate::session::Session::open(&path).unwrap();
    assert_eq!(reopened.editor.doc.elements.len(), 2);
    assert_eq!(reopened.editor.doc.relations.len(), 1);
    assert!(!reopened.is_dirty());
    let _ = std::fs::remove_file(path);
}

#[test]
fn opening_file_without_layout_places_everything_without_dirtying() {
    let s = crate::session::Session::from_text("class A\nclass B\nB --|> A\n");
    assert!(s.editor.doc.elements.values().all(|e| e.pos.is_some()));
    assert!(!s.is_dirty());
    assert!(!s.editor.can_undo());
}

#[test]
fn detects_file_changed_on_disk() {
    let path = std::env::temp_dir().join(format!("fastumpl-disk-{}.umpl", std::process::id()));
    std::fs::write(&path, "class A\n").unwrap();
    let mut s = crate::session::Session::open(&path).unwrap();
    assert!(!s.changed_on_disk());
    // File systems may store modification times in whole seconds.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&path, "class A\nclass B\n").unwrap();
    assert!(s.changed_on_disk());
    s.acknowledge_disk();
    assert!(!s.changed_on_disk());
    let _ = std::fs::remove_file(path);
}

#[test]
fn opening_plantuml_imports_an_unsaved_untitled_diagram() {
    let dir = std::env::temp_dir().join(format!("fastumpl-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("model.puml");
    let src = "@startuml\nclass Order\nOrder *-- Line\nhide empty members\n@enduml\n";
    std::fs::write(&path, src).unwrap();

    let mut h = new_app();
    h.state_mut().open_path(&path);
    h.run_steps(3);
    let app = h.state();
    let s = app.session.as_ref().unwrap();
    assert_eq!(s.path, None);
    assert!(s.is_dirty());
    assert_eq!(s.display_name(), "model.umpl");
    assert_eq!(s.editor.doc.elements.len(), 2);
    assert!(s.editor.doc.elements.values().all(|e| e.pos.is_some()));
    assert_eq!(s.problems.len(), 1);
    assert!(app.status.as_ref().unwrap().0.contains("1 problem"));
    assert!(app.recent.contains(&path));
    // The PlantUML file itself is never touched.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), src);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn copy_as_mermaid_puts_text_on_the_clipboard() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    h.state_mut()
        .session
        .as_mut()
        .unwrap()
        .editor
        .connect(a, b, RelationKind::Directed);
    h.run_steps(2);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::CopyMermaid);
    h.run_steps(1);
    let out = h.output().platform_output.commands.clone();
    let text = out
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .expect("nothing copied");
    assert!(text.starts_with("classDiagram\n"), "{text}");
    assert!(text.contains("    A --> B\n"), "{text}");
    assert_eq!(
        h.state().status.as_ref().unwrap().0,
        "Copied Mermaid to clipboard"
    );
}

#[test]
fn export_and_copy_commands_are_in_the_palette() {
    for cmd in [Cmd::ExportPlantUml, Cmd::ExportMermaid, Cmd::CopyMermaid] {
        assert!(Cmd::all().contains(&cmd));
    }
}
