//! Interaction tests: drive the real app with pointer and key events.

use std::sync::Once;
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::{NodeT as _, Queryable};
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
    // Settings a test changes go to a folder of its own: the shared one
    // stays empty, so every app starts from the defaults, whatever the
    // tests running beside it do.
    static APPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = APPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    own_settings(&mut h, &format!("app{n}"));
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
    h.state_mut().canvas.edit.as_mut().unwrap().name = "a\tb".into();
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
    assert_eq!(
        (rel.from.element().unwrap(), rel.to.element().unwrap()),
        (a, b)
    );
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
fn dragging_a_corner_resizes_the_box_and_undo_restores() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    h.state_mut().sel = crate::view::Selection::only_element(a);
    h.run_steps(2);
    let r = rect_of(&h, a);
    let natural = ((r.w / GRID).round() as i32, (r.h / GRID).round() as i32);
    let zoom = h.state().camera.zoom;
    let corner = screen(&h, (r.right(), r.bottom()));
    drag(
        &mut h,
        corner,
        corner + Vec2::new(8.0 * GRID * zoom, 6.0 * GRID * zoom),
    );
    let el = |h: &Harness<'static, App>| {
        h.state().session.as_ref().unwrap().editor.doc.elements[&a].clone()
    };
    assert_eq!(el(&h).size, Some((natural.0 + 8, natural.1 + 6)));
    assert_eq!(el(&h).pos, Some(Pos::new(0, 0)));
    let r2 = rect_of(&h, a);
    assert!((r2.w - r.w - 8.0 * GRID).abs() < 0.5, "{r:?} {r2:?}");
    // Smaller than its text needs: the box stops at that size, and keeps
    // no size of its own.
    let corner = screen(&h, (r2.right(), r2.bottom()));
    drag(
        &mut h,
        corner,
        corner - Vec2::new(40.0 * GRID * zoom, 40.0 * GRID * zoom),
    );
    assert_eq!(el(&h).size, None);
    // One drag is one undo step.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(el(&h).size, Some((natural.0 + 8, natural.1 + 6)));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(el(&h).size, None);
}

#[test]
fn right_click_colours_a_box() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    let r = rect_of(&h, a);
    let p = screen(&h, r.center());
    h.hover_at(p);
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    // The box under the pointer is selected, and the menu is about it.
    assert_eq!(h.state().sel.elements, vec![a]);
    h.get_by_label("Transparent").hover();
    h.run_steps(1);
    h.get_by_label("Transparent").click();
    h.run_steps(3);
    let color = |h: &Harness<'static, App>, id| {
        h.state().session.as_ref().unwrap().editor.doc.elements[&id]
            .color
            .clone()
    };
    assert_eq!(color(&h, a).as_deref(), Some("transparent"));
    assert_eq!(color(&h, b), None);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(color(&h, a), None);
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
    // From the bottom right: the top left corner of the canvas is under
    // the floating tools.
    let from = screen(&h, (rb.right() + 10.0, rb.bottom() + 10.0));
    let to = screen(&h, (ra.x - 20.0, ra.y - 20.0));
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
    let reopened = crate::session::Session::open(&path, &umpl_layout::Options::default()).unwrap();
    assert_eq!(reopened.editor.doc.elements.len(), 2);
    assert_eq!(reopened.editor.doc.relations.len(), 1);
    assert!(!reopened.is_dirty());
    let _ = std::fs::remove_file(path);
}

#[test]
fn opening_file_without_layout_places_everything_without_dirtying() {
    let s = crate::session::Session::from_text(
        "class A\nclass B\nB --|> A\n",
        &umpl_layout::Options::default(),
    );
    assert!(s.editor.doc.elements.values().all(|e| e.pos.is_some()));
    assert!(!s.is_dirty());
    assert!(!s.editor.can_undo());
}

#[test]
fn detects_file_changed_on_disk() {
    let path = std::env::temp_dir().join(format!("fastumpl-disk-{}.umpl", std::process::id()));
    std::fs::write(&path, "class A\n").unwrap();
    let mut s = crate::session::Session::open(&path, &umpl_layout::Options::default()).unwrap();
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
    let src = "@startuml\nclass Order\nOrder *-- Line\nhide empty members\n!include missing.iuml\n@enduml\n";
    std::fs::write(&path, src).unwrap();

    let mut h = new_app();
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, &path);
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
    // Recent entries are canonical (on Windows: `\\?\` prefix, long names).
    assert!(app.recent.contains(&path.canonicalize().unwrap()));
    // The PlantUML file itself is never touched.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), src);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_umple_imports_an_unsaved_untitled_diagram() {
    let dir = std::env::temp_dir().join(format!("fastumpl-umple-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("school.ump");
    let src = "namespace school;\nclass Student { name; * -- 1 Mentor; }\n\
               class Mentor { status { Active {} } }\n\
               //$?[End_of_model]$?\nclass Student { position 10 20 109 45; }\n";
    std::fs::write(&path, src).unwrap();

    let mut h = new_app();
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, &path);
    h.run_steps(3);
    let app = h.state();
    let s = app.session.as_ref().unwrap();
    assert_eq!(s.path, None);
    assert!(s.is_dirty());
    assert_eq!(s.display_name(), "school.umpl");
    assert_eq!(s.editor.doc.elements.len(), 2);
    assert_eq!(s.editor.doc.relations.len(), 1);
    // Umple positions are not used: the diagram is laid out.
    assert!(s.editor.doc.elements.values().all(|e| e.pos.is_some()));
    assert_eq!(s.problems.len(), 1);
    assert!(app.status.as_ref().unwrap().0.contains("1 problem"));
    assert!(app.recent.contains(&path.canonicalize().unwrap()));
    // The Umple file itself is never touched.
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
    for cmd in [
        Cmd::ExportPlantUml,
        Cmd::ExportMermaid,
        Cmd::ExportUmple,
        Cmd::CopyMermaid,
    ] {
        assert!(Cmd::all().contains(&cmd));
    }
}

fn doc<'a>(h: &'a Harness<'static, App>) -> &'a umpl_core::Document {
    &h.state().session.as_ref().unwrap().editor.doc
}

#[test]
fn deleting_boxes_and_another_line_is_one_undo_step() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    let c = add(&mut h, "C", 0, 40);
    let ab = {
        let s = h.state_mut().session.as_mut().unwrap();
        s.editor.connect(b, c, RelationKind::Association);
        s.editor.connect(a, b, RelationKind::Association)
    };
    h.run_steps(2);
    let before = doc(&h).clone();
    // C (taking B--C with it) and the line A--B, which touches neither.
    h.state_mut().sel = crate::view::Selection {
        elements: vec![c],
        relations: vec![ab],
        packages: vec![],
    };
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(doc(&h).relations.is_empty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(*doc(&h), before);
}

#[test]
fn nudging_another_selection_is_a_new_undo_step() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    let b = add(&mut h, "B", 30, 0);
    let pos = |h: &Harness<'static, App>, id| doc(h).elements[&id].pos.unwrap();
    h.state_mut().sel = crate::view::Selection::only_element(a);
    h.key_press(Key::ArrowRight);
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    h.state_mut().sel = crate::view::Selection::only_element(b);
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert_eq!((pos(&h, a), pos(&h, b)), (Pos::new(2, 0), Pos::new(30, 1)));
    // Undo takes back B's nudge only, then both of A's at once.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!((pos(&h, a), pos(&h, b)), (Pos::new(2, 0), Pos::new(30, 0)));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(pos(&h, a), Pos::new(0, 0));
}

#[test]
fn undo_and_redo_name_the_step() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    {
        let s = h.state_mut().session.as_mut().unwrap();
        s.editor.move_elements(&[(a, Pos::new(4, 4))], None);
    }
    h.run_steps(2);
    assert_eq!(h.state().command_label(Cmd::Undo), "Undo Move A");
    assert_eq!(h.state().command_label(Cmd::Redo), "Redo");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().status.as_ref().unwrap().0, "Undone: Move A");
    assert_eq!(h.state().command_label(Cmd::Redo), "Redo Move A");
    assert_eq!(h.state().command_label(Cmd::Undo), "Undo Add class");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().status.as_ref().unwrap().0, "Redone: Move A");
}

#[test]
fn toolbar_buttons_undo_and_redo() {
    let mut h = new_app();
    let a = add(&mut h, "A", 0, 0);
    {
        let s = h.state_mut().session.as_mut().unwrap();
        s.editor.move_elements(&[(a, Pos::new(4, 4))], None);
    }
    h.run_steps(2);
    h.get_by_label("⟲").click();
    h.run_steps(2);
    assert_eq!(doc(&h).elements[&a].pos, Some(Pos::new(0, 0)));
    h.get_by_label("⟳").click();
    h.run_steps(2);
    assert_eq!(doc(&h).elements[&a].pos, Some(Pos::new(4, 4)));
}

/// A diagram wider and taller than the window, so both bars can scroll.
fn big_diagram() -> Harness<'static, App> {
    let mut h = new_app();
    add(&mut h, "A", 0, 0);
    add(&mut h, "B", 400, 300);
    h.state_mut().fit_pending = false;
    h.state_mut().camera = crate::camera::Camera::default();
    h.run_steps(3);
    h
}

#[test]
fn dragging_a_scroll_thumb_pans_one_axis() {
    let mut h = big_diagram();
    let before = h.state().camera.origin;
    let thumb = h.state().canvas.scroll_thumbs[0].expect("no horizontal bar");
    let rect = h.state().canvas.rect;
    assert!(thumb.bottom() <= rect.bottom() + 0.5 && thumb.top() > rect.bottom() - 20.0);
    drag(
        &mut h,
        thumb.center(),
        thumb.center() + Vec2::new(60.0, 0.0),
    );
    let after = h.state().camera.origin;
    // The view moved right, so the diagram moved left; up and down stayed.
    assert!(after.x < before.x - 60.0, "{before:?} -> {after:?}");
    assert_eq!(after.y, before.y);
    // The thumb followed the pointer.
    let moved = h.state().canvas.scroll_thumbs[0].unwrap();
    assert!(
        (moved.center().x - thumb.center().x - 60.0).abs() < 2.0,
        "{thumb:?} -> {moved:?}"
    );
    assert!(h.state().sel.is_empty());

    let thumb = h.state().canvas.scroll_thumbs[1].expect("no vertical bar");
    drag(
        &mut h,
        thumb.center(),
        thumb.center() + Vec2::new(0.0, 40.0),
    );
    let down = h.state().camera.origin;
    assert!(
        down.y < after.y - 40.0 && down.x == after.x,
        "{after:?} -> {down:?}"
    );
}

#[test]
fn clicking_a_scroll_track_moves_a_page_and_creates_nothing() {
    let mut h = big_diagram();
    // Even with the class tool, a click on the bar only scrolls.
    h.state_mut().tool = crate::commands::Tool::Element(ElementKind::Class);
    let thumb = h.state().canvas.scroll_thumbs[1].unwrap();
    let rect = h.state().canvas.rect;
    let before = h.state().camera.origin;
    click(&mut h, Pos2::new(thumb.center().x, rect.bottom() - 30.0));
    let after = h.state().camera.origin;
    assert!(
        before.y - after.y > rect.height() * 0.5,
        "{before:?} -> {after:?}"
    );
    assert_eq!(doc(&h).elements.len(), 2);
}

#[test]
fn zoom_buttons_zoom_and_the_percentage_resets() {
    let mut h = new_app();
    h.get_by_label("+").click();
    h.run_steps(2);
    assert!((h.state().camera.zoom - 1.25).abs() < 1e-4);
    h.get_by_label("+").click();
    h.run_steps(2);
    h.get_by_label("−").click();
    h.run_steps(2);
    assert!((h.state().camera.zoom - 1.25).abs() < 1e-4);
    h.get_by_label("125%").click();
    h.run_steps(2);
    assert!((h.state().camera.zoom - 1.0).abs() < 1e-4);
}

#[test]
fn tool_buttons_are_named_for_their_tool() {
    let mut h = new_app();
    for tool in crate::commands::Tool::ALL {
        h.get_by_label(tool.label()).click();
        h.run_steps(2);
        assert_eq!(h.state().tool, tool);
    }
}

// ----- text panel ------------------------------------------------------------

fn doc_of<'a>(h: &'a Harness<'static, App>) -> &'a umpl_core::Document {
    &h.state().session.as_ref().unwrap().editor.doc
}

fn source_text(h: &Harness<'static, App>) -> String {
    h.state().source.text.clone()
}

/// Puts the cursor in the text panel, selects everything and types `text`
/// over it, then waits a little less than the typing pause.
fn retype(h: &mut Harness<'static, App>, text: &str) {
    let id = egui::Id::new(crate::source::EDITOR_ID);
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text(text.into()));
    h.run_steps(2);
}

/// Waits past the typing pause.
fn settle(h: &mut Harness<'static, App>) {
    let steps = (crate::source::SETTLE * 60.0) as usize + 10;
    h.run_steps(steps);
}

fn app_with_text() -> Harness<'static, App> {
    let mut h = new_app();
    h.state_mut().source.shown = true;
    h.run_steps(2);
    h
}

#[test]
fn diagram_changes_show_in_the_text_without_positions() {
    let mut h = app_with_text();
    let a = add(&mut h, "Order", 2, 2);
    assert_eq!(source_text(&h), "class Order\n");
    let b = add(&mut h, "Customer", 30, 2);
    let s = h.state_mut().session.as_mut().unwrap();
    s.editor.connect(a, b, RelationKind::Directed);
    h.run_steps(2);
    assert_eq!(
        source_text(&h),
        "class Order\n\nclass Customer\n\nOrder --> Customer\n"
    );
    // Moving a box leaves the text alone.
    let s = h.state_mut().session.as_mut().unwrap();
    s.editor.move_elements(&[(a, Pos::new(9, 9))], None);
    h.run_steps(2);
    assert!(!source_text(&h).contains("layout"));
}

#[test]
fn typing_in_the_text_changes_the_diagram_once_typing_pauses() {
    let mut h = app_with_text();
    let a = add(&mut h, "Order", 2, 2);
    retype(&mut h, "class Order\nclass Zed\nOrder --> Zed\n");
    assert!(doc_of(&h).find("Zed").is_none(), "applied before the pause");
    assert!(h.state().source.pending());
    settle(&mut h);
    let doc = doc_of(&h);
    let z = doc.find("Zed").expect("Zed added");
    assert_eq!(doc.find("Order"), Some(a), "Order kept its id");
    assert_eq!(doc.elements[&a].pos, Some(Pos::new(2, 2)));
    assert!(doc.elements[&z].pos.is_some(), "the new class was placed");
    assert_eq!(doc.relations.len(), 1);
    assert!(h.state().view.diagram.mapping.node_of(z).is_some());
    let s = h.state().session.as_ref().unwrap();
    assert_eq!(s.editor.undo_label(), Some("Edit text"));
    // The text is left as typed, not reformatted.
    assert_eq!(source_text(&h), "class Order\nclass Zed\nOrder --> Zed\n");
}

#[test]
fn renaming_in_the_text_keeps_the_box_and_selection() {
    let mut h = app_with_text();
    let a = add(&mut h, "Order", 2, 2);
    add(&mut h, "Customer", 30, 2);
    h.state_mut().sel = crate::view::Selection::only_element(a);
    let text = source_text(&h).replace("Order", "Invoice");
    retype(&mut h, &text);
    settle(&mut h);
    let doc = doc_of(&h);
    assert_eq!(doc.find("Invoice"), Some(a));
    assert_eq!(doc.elements[&a].pos, Some(Pos::new(2, 2)));
    assert_eq!(h.state().sel.elements, vec![a]);
}

#[test]
fn an_open_brace_holds_the_text_back() {
    let mut h = app_with_text();
    add(&mut h, "Order", 2, 2);
    add(&mut h, "Customer", 30, 2);
    retype(&mut h, "class Order {\n  +id: int\n\nclass Customer\n");
    settle(&mut h);
    assert!(h.state().source.waiting_for_brace());
    let doc = doc_of(&h);
    assert!(doc.find("Customer").is_some(), "Customer was not swallowed");
    let order = doc.find("Order").unwrap();
    assert!(doc.elements[&order].members.is_empty());

    retype(&mut h, "class Order {\n  +id: int\n}\n\nclass Customer\n");
    settle(&mut h);
    assert!(!h.state().source.waiting_for_brace());
    let doc = doc_of(&h);
    assert_eq!(doc.elements[&order].members.len(), 1);
}

#[test]
fn undo_of_a_text_edit_reloads_the_text() {
    let mut h = app_with_text();
    add(&mut h, "Order", 2, 2);
    retype(&mut h, "class Order\nclass Zed\n");
    settle(&mut h);
    assert!(doc_of(&h).find("Zed").is_some());
    let ctx = h.ctx.clone();
    h.ctx
        .memory_mut(|m| m.surrender_focus(egui::Id::new(crate::source::EDITOR_ID)));
    h.run_steps(2);
    h.state_mut().run(&ctx, Cmd::Undo);
    h.run_steps(2);
    assert!(doc_of(&h).find("Zed").is_none());
    assert_eq!(source_text(&h), "class Order\n");
}

#[test]
fn a_command_applies_text_typed_just_before_it() {
    let mut h = app_with_text();
    add(&mut h, "Order", 2, 2);
    retype(&mut h, "class Order\nclass Zed\n");
    assert!(h.state().source.pending());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::SelectAll);
    assert!(doc_of(&h).find("Zed").is_some());
    assert_eq!(h.state().sel.elements.len(), 2);
}

#[test]
fn the_text_panel_can_be_hidden_and_shown() {
    let mut h = app_with_text();
    add(&mut h, "Order", 2, 2);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::ToggleSource);
    h.run_steps(2);
    assert!(!h.state().source.shown);
    add(&mut h, "Hidden", 30, 2);
    h.state_mut().run(&ctx, Cmd::ToggleSource);
    h.run_steps(2);
    assert!(h.state().source.shown);
    assert!(
        source_text(&h).contains("class Hidden"),
        "caught up on show"
    );
    assert!(Cmd::all().contains(&Cmd::ToggleSource));
    assert!(Cmd::ToggleSource.shortcut().is_some());
}

/// A settings folder for one test: the tests run at once, and would
/// otherwise rewrite one another's settings.
fn own_settings(h: &mut Harness<'static, App>, name: &str) {
    let dir = std::env::temp_dir().join(format!("fastumpl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    h.state_mut().storage = crate::storage::Storage::at(&dir);
}

#[test]
fn settings_window_changes_and_saves_settings() {
    let mut h = new_app();
    own_settings(&mut h, "settings-window");
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::Settings);
    h.run_steps(3);
    assert!(matches!(
        h.state().dialog,
        Some(crate::app::Dialog::Settings)
    ));
    // A pointer moves onto a control before it clicks.
    h.get_by_label("Fill the window").hover();
    h.run_steps(1);
    h.get_by_label("Fill the window").click();
    h.run_steps(3);
    assert_eq!(h.state().layout_goal, umpl_layout::Goal::FillView);
    // A bigger text panel font, as the slider would set it.
    let mut prefs = h.state().prefs;
    prefs.text_size = 18.0;
    prefs.save(&h.state().storage);
    let storage = &h.state().storage;
    let loaded = crate::prefs::Prefs::load(storage);
    let layout = storage.load_setting("layout");
    // Back to the defaults, which the other tests expect.
    h.get_by_label("Reset to defaults").hover();
    h.run_steps(1);
    h.get_by_label("Reset to defaults").click();
    h.run_steps(3);
    crate::prefs::Prefs::default().save(&h.state().storage);
    assert_eq!(loaded.text_size, 18.0);
    assert_eq!(layout.as_deref(), Some("fill"));
    assert_eq!(h.state().layout_goal, umpl_layout::Goal::FewestCrossings);
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
}

#[test]
fn the_view_switch_shows_text_both_or_canvas_and_is_saved() {
    use crate::commands::ViewMode;
    let mut h = new_app();
    own_settings(&mut h, "view-switch");
    for (label, mode) in [
        ("Text", ViewMode::Text),
        ("Both", ViewMode::Both),
        ("Canvas", ViewMode::Canvas),
    ] {
        h.get_by_label(label).hover();
        h.run_steps(1);
        h.get_by_label(label).click();
        h.run_steps(3);
        assert_eq!(h.state().view(), mode, "{label}");
        let saved = h.state().storage.load_setting("view");
        assert_eq!(saved.as_deref(), Some(label.to_ascii_lowercase().as_str()));
    }
    // An older settings file, with only `text`, still says what to show.
    assert_eq!(crate::app::view_mode(None, Some("shown")), ViewMode::Both);
    assert_eq!(crate::app::view_mode(None, None), ViewMode::Canvas);
}

#[test]
fn saved_settings_are_read_back() {
    use crate::app::{grid_shown, layout_goal, layout_goal_setting};
    use umpl_layout::Goal;
    // The grid shows unless turned off; layouts favour few crossings
    // unless the user chose to fill the window.
    assert!(grid_shown(None) && grid_shown(Some("shown")));
    assert!(!grid_shown(Some("hidden")));
    assert_eq!(layout_goal(None), Goal::FewestCrossings);
    for goal in [Goal::FewestCrossings, Goal::FillView] {
        assert_eq!(layout_goal(Some(layout_goal_setting(goal))), goal);
    }
}

#[test]
fn layout_goal_is_chosen_in_the_arrange_menu_and_saved() {
    let mut h = new_app();
    own_settings(&mut h, "arrange-layout");
    assert_eq!(h.state().layout_goal, umpl_layout::Goal::FewestCrossings);
    h.get_by_label("Arrange").click();
    h.run_steps(2);
    h.get_by_label("Filling the window").click();
    h.run_steps(2);
    assert_eq!(h.state().layout_goal, umpl_layout::Goal::FillView);
    let saved = h.state().storage.load_setting("layout");
    // Back to the default, saved as well.
    let ctx = h.ctx.clone();
    h.state_mut()
        .run(&ctx, Cmd::LayoutGoal(umpl_layout::Goal::FewestCrossings));
    assert_eq!(saved.as_deref(), Some("fill"));
    assert_eq!(
        h.state().storage.load_setting("layout").as_deref(),
        Some("crossings")
    );
}

/// Arrange → Left to right lays the diagram out again with parents left
/// of their children; undo goes back step by step.
#[test]
fn left_to_right_is_ticked_in_the_arrange_menu() {
    let mut h = new_app();
    let ids: Vec<ElementId> = (0..4)
        .map(|i| add(&mut h, &format!("C{i}"), 0, 0))
        .collect();
    {
        let s = h.state_mut().session.as_mut().unwrap();
        for &kid in &ids[1..] {
            s.editor.connect(kid, ids[0], RelationKind::Generalization);
        }
    }
    h.run_steps(2);
    h.get_by_label("Arrange").click();
    h.run_steps(2);
    h.get_by_label("Left to right").hover();
    h.run_steps(1);
    h.get_by_label("Left to right").click();
    h.run_steps(2);
    let start = Instant::now();
    while h.state().tidy.is_some() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
    let s = h.state().session.as_ref().unwrap();
    assert!(s.editor.doc.left_to_right);
    let x = |id: ElementId| s.editor.doc.elements[&id].pos.unwrap().x;
    for &kid in &ids[1..] {
        assert!(x(ids[0]) < x(kid), "{} {}", x(ids[0]), x(kid));
    }
    // Undo the layout, then the direction.
    for _ in 0..2 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run_steps(2);
    }
    assert!(!h.state().session.as_ref().unwrap().editor.doc.left_to_right);
}

#[test]
fn the_text_panel_is_off_unless_turned_on() {
    assert!(!crate::app::text_shown(None));
    assert!(!crate::app::text_shown(Some("hidden")));
    assert!(crate::app::text_shown(Some("shown")));
}

#[test]
fn view_menu_ticks_and_toggles_the_text() {
    let mut h = new_app();
    h.state_mut().source.shown = false;
    h.run_steps(2);
    h.get_by_label("View").click();
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Text")
        .click();
    h.run_steps(2);
    assert!(h.state().source.shown);
    h.get_by_label("View").click();
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Text")
        .click();
    h.run_steps(2);
    assert!(!h.state().source.shown);
}

#[test]
fn every_export_is_under_file_export() {
    use crate::app::EXPORT_MENU;
    let listed: Vec<Cmd> = EXPORT_MENU.iter().filter_map(|(c, _)| *c).collect();
    for cmd in Cmd::all() {
        let label = cmd.label();
        if label.starts_with("Export") || label.starts_with("Copy") {
            assert!(listed.contains(&cmd), "{label} missing from File → Export");
        }
    }
    let mut h = new_app();
    h.get_by_label("File").click();
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("Export SVG").is_none(),
        "exports are not at the top"
    );
    h.get_by_label("Export ⏵").hover();
    h.run_steps(3);
    for (_, label) in EXPORT_MENU.iter().filter(|(c, _)| c.is_some()) {
        assert!(
            h.query_by_label_contains(label).is_some(),
            "{label} not in the submenu"
        );
    }
}

// ----- text and drawing linked -------------------------------------------------

/// Puts the text cursor at the start of `line` (1-based) in the focused
/// text panel.
fn cursor_to_line(h: &mut Harness<'static, App>, line: usize) {
    use egui::text::{CCursor, CCursorRange};
    let id = egui::Id::new(crate::source::EDITOR_ID);
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(1);
    let c = crate::link::line_start_char(&source_text(h), line);
    let mut state = egui::TextEdit::load_state(&h.ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(c))));
    egui::TextEdit::store_state(&h.ctx, id, state);
    h.run_steps(2);
}

fn text_cursor(h: &Harness<'static, App>) -> Option<usize> {
    let id = egui::Id::new(crate::source::EDITOR_ID);
    egui::TextEdit::load_state(&h.ctx, id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| r.primary.index)
}

fn linked_app() -> (Harness<'static, App>, ElementId, ElementId) {
    let mut h = app_with_text();
    let a = add(&mut h, "Order", 2, 2);
    let b = add(&mut h, "Customer", 30, 2);
    let s = h.state_mut().session.as_mut().unwrap();
    let mut el = s.editor.doc.elements[&a].clone();
    el.members = vec![umpl_core::Member::parse_line("+id: int")];
    s.editor.set_element(a, el);
    s.editor.connect(a, b, RelationKind::Directed);
    h.run_steps(3);
    assert_eq!(
        source_text(&h),
        "class Order {\n  +id: int\n}\n\nclass Customer\n\nOrder --> Customer\n"
    );
    (h, a, b)
}

#[test]
fn clicking_a_box_marks_its_lines_and_puts_the_cursor_there() {
    let (mut h, _, b) = linked_app();
    let at = screen(&h, rect_of(&h, b).center());
    click(&mut h, at);
    assert_eq!(h.state().sel.elements, vec![b]);
    assert_eq!(h.state().source.highlight, vec![(5, 5)]);
    let start = crate::link::line_start_char(&source_text(&h), 5);
    assert_eq!(text_cursor(&h), Some(start));
    // The keyboard stays with the canvas: Delete deletes the box.
    let id = egui::Id::new(crate::source::EDITOR_ID);
    assert!(!h.ctx.memory(|m| m.has_focus(id)));

    // A box with members: its whole block is marked.
    let a = doc_of(&h).find("Order").unwrap();
    let at = screen(&h, rect_of(&h, a).center());
    click(&mut h, at);
    assert_eq!(h.state().source.highlight, vec![(1, 3)]);
    // Empty canvas: nothing marked.
    let empty = h.state().canvas.rect.right_bottom() - Vec2::new(60.0, 60.0);
    click(&mut h, empty);
    assert!(h.state().sel.is_empty());
    assert!(h.state().source.highlight.is_empty());
}

#[test]
fn moving_the_text_cursor_selects_what_is_declared_there() {
    let (mut h, a, b) = linked_app();
    cursor_to_line(&mut h, 2);
    assert_eq!(h.state().sel.elements, vec![a]);
    assert_eq!(h.state().source.highlight, vec![(1, 3)]);
    cursor_to_line(&mut h, 5);
    assert_eq!(h.state().sel.elements, vec![b]);
    cursor_to_line(&mut h, 7);
    let r = *doc_of(&h).relations.keys().next().unwrap();
    assert_eq!(h.state().sel.relations, vec![r]);
    assert!(h.state().sel.elements.is_empty());
    // A blank line leaves the selection as it was.
    cursor_to_line(&mut h, 6);
    assert_eq!(h.state().sel.relations, vec![r]);
    // The panel keeps the keyboard while the cursor moves.
    let id = egui::Id::new(crate::source::EDITOR_ID);
    assert!(h.ctx.memory(|m| m.has_focus(id)));
}

#[test]
fn the_text_cursor_brings_its_box_into_view() {
    let (mut h, _, b) = linked_app();
    // Pan far away so Customer is off screen.
    h.state_mut().camera.pan(Vec2::new(-5000.0, 0.0));
    h.run_steps(2);
    let view = h.state().canvas.rect;
    assert!(!view.contains(screen(&h, rect_of(&h, b).center())));
    cursor_to_line(&mut h, 5);
    let c = screen(&h, rect_of(&h, b).center());
    assert!(view.contains(c), "{c:?} not in {view:?}");
}

#[test]
fn text_can_be_copied_cut_and_pasted() {
    let (mut h, _, _) = linked_app();
    let id = egui::Id::new(crate::source::EDITOR_ID);
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Copy);
    h.run_steps(1);
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .expect("nothing copied");
    assert_eq!(copied, source_text(&h));
    // Select-all in the text selects text, not boxes.
    assert!(h.state().sel.elements.len() <= 1);

    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Cut);
    h.run_steps(2);
    assert_eq!(source_text(&h), "");
    h.event(Event::Paste("class Pasted\n".into()));
    h.run_steps(2);
    h.event(Event::Paste(copied.clone()));
    settle(&mut h);
    assert_eq!(source_text(&h), format!("class Pasted\n{copied}"));
    let doc = doc_of(&h);
    assert!(doc.find("Pasted").is_some() && doc.find("Order").is_some());
    assert_eq!(doc.relations.len(), 1);
}

// ----- find --------------------------------------------------------------------

#[test]
fn find_steps_through_matches_and_marks_them() {
    let (mut h, a, b) = linked_app();
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    assert!(h.state().find.open);
    let field = egui::Id::new(crate::find::FIELD_ID);
    assert!(
        h.ctx.memory(|m| m.has_focus(field)),
        "the field takes the keyboard"
    );
    // "o" is in Order, Customer and the relation's ends: the first match is
    // selected and its declaration marked in the text.
    h.event(Event::Text("o".into()));
    h.run_steps(2);
    assert_eq!(h.state().find.matches.len(), 2, "Order and Customer");
    assert_eq!(h.state().sel.elements, vec![a]);
    assert_eq!(h.state().source.highlight, vec![(1, 3)]);
    // Every occurrence in the text is found.
    let ranges = h.state().find_text_ranges();
    assert_eq!(
        ranges.len(),
        source_text(&h).to_lowercase().matches('o').count()
    );
    // Enter steps on, wrapping round; the field keeps the keyboard.
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().sel.elements, vec![b]);
    assert!(h.ctx.memory(|m| m.has_focus(field)));
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().sel.elements, vec![a]);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().sel.elements, vec![b]);
    // A narrower query.
    h.event(Event::Text("rder".into()));
    h.run_steps(2);
    assert_eq!(h.state().find.matches.len(), 1);
    assert_eq!(h.state().sel.elements, vec![a]);
    // Esc closes the bar.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().find.open);
    assert!(h.state().find_text_ranges().is_empty());
}

#[test]
fn find_says_when_nothing_matches() {
    let (mut h, _, _) = linked_app();
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Cmd::Find);
    h.run_steps(2);
    h.event(Event::Text("zzz".into()));
    h.run_steps(2);
    assert!(h.state().find.matches.is_empty());
    assert!(h.query_by_label("No matches").is_some());
    assert!(Cmd::all().contains(&Cmd::Find));
}

// ----- big diagrams --------------------------------------------------------------

#[test]
fn a_big_diagram_opens_at_once_and_is_laid_out_in_the_background() {
    let mut h = new_app();
    let n = crate::session::DEFER_ABOVE + 50;
    let mut text = String::new();
    for i in 0..n {
        text.push_str(&format!("class C{i}\n"));
    }
    for i in 1..n {
        text.push_str(&format!("C{i} --> C{}\n", (i * 7) % i));
    }
    let dir = std::env::temp_dir().join(format!("fastumpl-big-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.umpl");
    std::fs::write(&path, &text).unwrap();
    let ctx = h.ctx.clone();
    let started = Instant::now();
    h.state_mut().open_path(&ctx, &path);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "open waited for the layout"
    );
    assert!(h.state().tidy.as_ref().is_some_and(|t| t.initial));
    // The window keeps running meanwhile.
    h.run_steps(2);
    let deadline = Instant::now() + Duration::from_secs(120);
    while h.state().tidy.is_some() && Instant::now() < deadline {
        h.run_steps(5);
        std::thread::sleep(Duration::from_millis(20));
    }
    h.run_steps(3);
    let s = h.state().session.as_ref().unwrap();
    assert!(s.editor.doc.elements.values().all(|e| e.pos.is_some()));
    assert!(!s.is_dirty(), "the first layout is not a change");
    assert!(!s.editor.can_undo(), "nor an undo step");
    assert_eq!(h.state().view.diagram.mapping.nodes.len(), n);
    // Fitted to fill the canvas.
    let canvas = h.state().canvas.rect;
    let bounds = h
        .state()
        .view
        .diagram
        .layout
        .bounds(&h.state().view.diagram.graph);
    let shown = h.state().camera.rect_to_screen(RectF::new(
        bounds.x as f32 * GRID,
        bounds.y as f32 * GRID,
        bounds.w as f32 * GRID,
        bounds.h as f32 * GRID,
    ));
    assert!(
        canvas.expand(1.0).contains_rect(shown),
        "{shown:?} in {canvas:?}"
    );
    let fill = (shown.width() / canvas.width()).max(shown.height() / canvas.height());
    assert!(fill > 0.9, "fills {fill:.2} of the canvas");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_with_several_diagrams_asks_which_to_open() {
    let dir = std::env::temp_dir().join(format!("fastumpl-pick-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("many.puml");
    std::fs::write(
        &path,
        "@startuml model\nclass A\n@enduml\n@startuml\nAlice -> Bob : hi\n@enduml\n@startuml\nclass B\nnewpage\nclass C\n@enduml\n",
    )
    .unwrap();
    let mut h = new_app();
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, &path);
    h.run_steps(3);
    let list = match &h.state().dialog {
        Some(crate::app::Dialog::PickDiagram(_, list)) => list.clone(),
        _ => panic!("no choice of diagram"),
    };
    assert_eq!(list.len(), 4);
    // The sequence diagram is listed but cannot be opened.
    let seq = h.get_by_label_contains("sequence diagram (not read)");
    assert!(seq.accesskit_node().is_disabled());
    h.get_by_label_contains("Diagram 4").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let names: Vec<String> = doc_of(&h)
        .elements
        .values()
        .map(|e| e.name.clone())
        .collect();
    assert_eq!(names, ["C"]);
    assert!(h.state().session.as_ref().unwrap().problems.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_a_sequence_diagram_says_what_it_is() {
    let dir = std::env::temp_dir().join(format!("fastumpl-seq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.puml");
    std::fs::write(
        &path,
        "@startuml\nAlice -> Bob : hello\nactivate Bob\n@enduml\n",
    )
    .unwrap();
    let mut h = new_app();
    add(&mut h, "Kept", 0, 0);
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, &path);
    h.run_steps(3);
    match &h.state().dialog {
        Some(crate::app::Dialog::Message(_, m)) => {
            assert!(m.contains("This is a sequence diagram"), "{m}")
        }
        _ => panic!("no message"),
    }
    // The open diagram stays.
    assert!(doc_of(&h).find("Kept").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn plantuml_includes_next_to_the_file_are_read() {
    let dir = std::env::temp_dir().join(format!("fastumpl-inc-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("common")).unwrap();
    std::fs::write(dir.join("common/base.iuml"), "abstract class Base\n").unwrap();
    let path = dir.join("model.puml");
    std::fs::write(
        &path,
        "@startuml\n!include common\\base.iuml\nclass Order\nOrder --|> Base\n@enduml\n",
    )
    .unwrap();
    let mut h = new_app();
    let ctx = h.ctx.clone();
    h.state_mut().open_path(&ctx, &path);
    h.run_steps(3);
    let doc = doc_of(&h);
    let base = doc.find("Base").expect("Base from the include");
    assert_eq!(doc.elements[&base].kind, ElementKind::Abstract);
    assert_eq!(h.state().session.as_ref().unwrap().problems, vec![]);
    let _ = std::fs::remove_dir_all(dir);
}
