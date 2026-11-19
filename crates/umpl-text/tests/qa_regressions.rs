//! QA findings of 2026-11-19 (`docs/qa/2026-11-19-findings.md`), one test
//! each. Every test here failed on the code as found.

use umpl_core::*;
use umpl_text::{mermaid, parse, plantuml, print};

fn el<'a>(doc: &'a Document, name: &str) -> &'a Element {
    &doc.elements[&doc.find(name).unwrap_or_else(|| panic!("no {name}"))]
}

fn texts(el: &Element) -> Vec<&str> {
    el.members.iter().map(|m| m.text.as_str()).collect()
}

/// Notepad and many Windows editors start UTF-8 files with a byte order
/// mark. The first line must still be read.
#[test]
fn a_byte_order_mark_does_not_hide_the_first_line() {
    let p = parse("\u{feff}diagram \"Shop\"\n\nclass A\n");
    assert_eq!(p.problems, vec![]);
    assert_eq!(p.doc.title.as_deref(), Some("Shop"));
}

/// A class member that is just `}` (the last line of a method body typed
/// into a class) closes the class early when the file is read back: the
/// members after it, and the classes after the class, land elsewhere.
#[test]
fn a_member_that_is_a_closing_brace_survives_save_and_reopen() {
    let mut ed = Editor::new(Document::new());
    let pkg = ed.add_package("shop", None);
    let a = ed.add_element(ElementKind::Class, "Order", Some(pkg), Some(Pos::new(0, 0)));
    ed.add_element(ElementKind::Class, "Line", Some(pkg), Some(Pos::new(20, 0)));
    let mut order = ed.doc.elements[&a].clone();
    order.members = ["+run() {", "doIt()", "}", "+total(): Money"]
        .iter()
        .map(|l| Member::parse_line(l))
        .collect();
    ed.set_element(a, order);

    let reread = parse(&print(&ed.doc));
    assert_eq!(reread.problems, vec![]);
    assert_eq!(
        texts(el(&reread.doc, "shop.Order")),
        ["run() {", "doIt()", "}", "total(): Money"]
    );
    assert!(
        reread.doc.find("shop.Line").is_some(),
        "Line left its package"
    );
}

/// A relation name holding `\n` as text (a Windows path, a regex) is read
/// back with a line break in it.
#[test]
fn a_backslash_in_a_relation_name_survives_save_and_reopen() {
    let mut ed = Editor::new(Document::new());
    let a = ed.add_element(ElementKind::Class, "A", None, Some(Pos::new(0, 0)));
    let b = ed.add_element(ElementKind::Class, "B", None, Some(Pos::new(20, 0)));
    let r = ed.connect(a, b, RelationKind::Directed);
    let mut rel = ed.doc.relations[&r].clone();
    rel.name = Some(r"saves to C:\new\file".into());
    ed.set_relation(r, rel);

    let reread = parse(&print(&ed.doc)).doc;
    let name = reread.relations.values().next().unwrap().name.clone();
    assert_eq!(name.as_deref(), Some(r"saves to C:\new\file"));
}

/// PlantUML draws `RED(255, 0, 0)` as one enum row (checked with the
/// PlantUML jar, `-tutxt`); fastumpl splits it at every comma.
#[test]
fn plantuml_enum_rows_with_commas_stay_one_literal() {
    let src = "@startuml\nenum Color {\n  RED(255, 0, 0)\n  GREEN(0, 255, 0)\n}\n@enduml\n";
    let doc = plantuml::import(src).doc;
    assert_eq!(
        texts(el(&doc, "Color")),
        ["RED(255, 0, 0)", "GREEN(0, 255, 0)"]
    );
    // And the .umpl file keeps them so.
    let reread = parse(&print(&doc)).doc;
    assert_eq!(
        texts(el(&reread, "Color")),
        ["RED(255, 0, 0)", "GREEN(0, 255, 0)"]
    );
}

/// A relation whose second end is unknown is kept as raw text, but its
/// first end has already been created as a new class, which the next save
/// writes into the file.
#[test]
fn a_relation_kept_as_raw_text_creates_no_class() {
    let p = parse("class A\nNewOne --> nowhere.B\n");
    assert!(p.doc.relations.is_empty());
    assert!(p.doc.find("NewOne").is_none(), "{}", print(&p.doc));
}

/// `billing at 1, 2` (a package, without a size) is dropped on save, and
/// the problem calls the package an unknown element.
#[test]
fn a_package_position_without_a_size_is_not_called_unknown() {
    let p = parse(
        "package billing {\n  class A\n}\nlayout {\n  billing at 1, 2\n  billing.A at 3, 4\n}\n",
    );
    for problem in &p.problems {
        assert!(
            !problem.message.contains("unknown element"),
            "billing is a package, not unknown: {}",
            problem.message
        );
    }
}

fn doc_with_spaced_names() -> Document {
    parse(
        "package \"web app\" {\n  class Page\n  class \"Order line\"\n}\n\nnote \"my note\" \"hello\"\n\n\"my note\" .. \"web app\".Page\n\"web app\".Page --> \"web app\".\"Order line\"\n",
    )
    .doc
}

/// PlantUML rejects `package web app {` and `note "…" as my note`
/// ("Error line 2"; checked with the PlantUML jar): names with spaces must
/// be quoted. The exported file must read back with the same model.
#[test]
fn plantuml_export_of_spaced_names_reads_back() {
    let doc = doc_with_spaced_names();
    let out = plantuml::export(&doc);
    let back = plantuml::import(&out);
    assert_eq!(back.problems, vec![], "{out}");
    assert_eq!(back.doc.packages.len(), 1, "{out}");
    assert_eq!(back.doc.elements.len(), doc.elements.len(), "{out}");
}

/// Mermaid class names are words; `class Order line` and
/// `namespace web app {` are not valid Mermaid. The exporter already writes
/// `id["Label"]` for clashing names and should do so here.
#[test]
fn mermaid_export_uses_word_ids_for_spaced_names() {
    let out = mermaid::export(&doc_with_spaced_names());
    for line in out.lines() {
        let t = line.trim();
        if let Some(rest) = t
            .strip_prefix("class ")
            .or_else(|| t.strip_prefix("namespace "))
        {
            let id: String = rest
                .chars()
                .take_while(|c| *c != '[' && *c != '{')
                .collect();
            let id = id.trim();
            assert!(
                id.chars().all(|c| c.is_alphanumeric() || c == '_'),
                "not a Mermaid id: `{id}` in\n{out}"
            );
        }
    }
}

/// Saving must not turn a symlinked diagram into a copy, nor open up a
/// private file's permissions (the GUI's Save uses the same function).
#[cfg(unix)]
#[test]
fn saving_keeps_symlinks_and_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = std::env::temp_dir().join(format!("umpl-qa-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("real.umpl");
    std::fs::write(&real, "class A\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.join("link.umpl");
    symlink(&real, &link).unwrap();

    umpl_text::write_atomic(&link, b"class B\n").unwrap();
    let still_link = std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink();
    let real_text = std::fs::read_to_string(&real).unwrap();
    umpl_text::write_atomic(&real, b"class C\n").unwrap();
    let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
    let _ = std::fs::remove_dir_all(&dir);

    assert!(still_link, "the symlink was replaced by a file");
    assert_eq!(real_text, "class B\n", "the link's target was not written");
    assert_eq!(mode, 0o600, "a private file became {mode:o}");
}

/// QA-11, QA-17: a position or size far out (a typo in a `layout` line)
/// made layout run for minutes and abort, or overflow; it is ignored with a
/// problem, and the box is placed as if it had no position.
#[test]
fn positions_far_out_are_ignored_with_a_problem() {
    let p = parse("class A\nclass B\nlayout {\n  A at 0, 0\n  B at 100000000, 0\n}\n");
    assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
    assert_eq!(p.doc.elements[&p.doc.find("B").unwrap()].pos, None);
    let p = parse("class A\nlayout {\n  A at 0, 0 size 2147483647, 5\n}\n");
    assert_eq!(p.problems.len(), 1, "{:?}", p.problems);
    let p = parse("class A\nlayout {\n  A at -1000000, 1000000 size 10000, 5\n}\n");
    assert_eq!(p.problems, vec![]);
}

/// QA-14: `!include` reads PlantUML files, not any file on the disk.
#[test]
fn includes_read_plantuml_files_only() {
    let dir = std::env::temp_dir().join(format!("umpl-qa-inc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("secret"), "root:x:0:0:root:/root:/bin/bash\n").unwrap();
    std::fs::write(dir.join("notes.cfg"), "class Leaked\n").unwrap();
    std::fs::write(dir.join("lib.iuml"), "class Lib\n").unwrap();
    std::fs::write(dir.join("defs"), "' shared\n!define X\nclass NoExt\n").unwrap();
    let main = dir.join("main.puml");
    std::fs::write(
        &main,
        "@startuml\n!include secret\n!include notes.cfg\n!include lib.iuml\n!include defs\nclass A\n@enduml\n",
    )
    .unwrap();
    let p = plantuml::import_path(&main).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let mut names: Vec<String> = p.doc.elements.values().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["A", "Lib", "NoExt"]);
    assert_eq!(p.problems.len(), 2, "{:?}", p.problems);
}

/// QA-15: renaming a duplicate does not take a name the file declares
/// further down, so lines to that name keep their element.
#[test]
fn a_renamed_duplicate_does_not_take_a_later_name() {
    let p = parse("class A {\n  +first\n}\nclass A\nclass A2 {\n  +mine\n}\nA2 --> C\n");
    let d = &p.doc;
    let a2 = d.find("A2").unwrap();
    assert_eq!(texts(&d.elements[&a2]), ["mine"]);
    let r = d.relations.values().next().unwrap();
    assert_eq!(r.from.element(), Some(a2));
    assert!(d.find("A3").is_some(), "{}", print(d));
}

/// QA-18: PlantUML reads a dot in a quoted name as a package separator, so
/// a class named `a.b` is exported under an alias and reads back as one
/// class showing `a.b`.
#[test]
fn a_dotted_name_exports_to_plantuml_as_one_class() {
    let doc = parse("class \"a.b\"\nclass C\n\"a.b\" --> C\n").doc;
    let out = plantuml::export(&doc);
    let back = plantuml::import(&out);
    assert_eq!(back.problems, vec![], "{out}");
    assert!(back.doc.packages.is_empty(), "{out}");
    assert_eq!(back.doc.elements.len(), 2, "{out}");
    assert!(
        back.doc.elements.values().any(|e| e.shown() == "a.b"),
        "{out}"
    );
    assert_eq!(back.doc.relations.len(), 1, "{out}");
}

/// QA-04 in the `.umpl` format itself: literals split only at commas
/// outside brackets.
#[test]
fn enum_literals_split_only_at_top_level_commas() {
    let d = parse("enum E { RED(1, 2), GREEN, \"a, b\" }\nenum F {\n  X(1, 2), Y\n}\n").doc;
    assert_eq!(texts(el(&d, "E")), ["RED(1, 2)", "GREEN", "\"a, b\""]);
    assert_eq!(texts(el(&d, "F")), ["X(1, 2)", "Y"]);
}
