//! Runs the `umpl` binary on real files.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SHOP: &str = include_str!("../../../examples/shop.umpl");

const PUML: &str = "@startuml
title Imported
!include missing.iuml
class Order {
  +total() : Money
}
Order \"1\" *-- \"*\" Line
Order --> Customer
@enduml
";

const UMP: &str = "// A small school
namespace school;
class Student { name; Integer id; * -- 1 Mentor; }
class Mentor { status { Active {} Retired {} } }
";

/// A fresh, empty folder for one test.
fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("umpl-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn umpl(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_umpl"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn converts_umpl_to_mermaid_and_plantuml() {
    let dir = workdir("export");
    std::fs::write(dir.join("shop.umpl"), SHOP).unwrap();
    for (out, start) in [
        ("shop.mmd", "classDiagram"),
        ("shop.md", "```mermaid"),
        ("shop.puml", "@startuml"),
    ] {
        let r = umpl(&dir, &["convert", "shop.umpl", "-o", out]);
        assert!(r.status.success(), "{out}: {}", text(&r.stderr));
        let written = std::fs::read_to_string(dir.join(out)).unwrap();
        assert!(written.contains(start), "{out}:\n{written}");
    }
    let r = umpl(&dir, &["convert", "shop.umpl", "--to", "mermaid"]);
    assert!(r.status.success());
    assert_eq!(
        text(&r.stdout),
        std::fs::read_to_string(dir.join("shop.mmd")).unwrap()
    );
}

#[test]
fn reads_the_chosen_diagram_of_a_plantuml_file() {
    let dir = workdir("diagrams");
    std::fs::write(
        dir.join("two.puml"),
        "@startuml\nclass First\n@enduml\n@startuml\nclass Second\n@enduml\n",
    )
    .unwrap();
    let r = umpl(&dir, &["convert", "two.puml", "--to", "umpl"]);
    assert!(r.status.success());
    assert_eq!(text(&r.stdout), "class First\n");
    assert!(
        text(&r.stderr).contains("holds 2 diagrams"),
        "{}",
        text(&r.stderr)
    );
    let r = umpl(
        &dir,
        &["convert", "two.puml", "--to", "umpl", "--diagram", "2"],
    );
    assert_eq!(text(&r.stdout), "class Second\n");
    assert!(text(&r.stderr).is_empty(), "{}", text(&r.stderr));
    let r = umpl(&dir, &["check", "two.puml", "--diagram", "3"]);
    assert!(!r.status.success());
}

#[test]
fn imports_plantuml() {
    let dir = workdir("import");
    std::fs::write(dir.join("model.puml"), PUML).unwrap();

    let r = umpl(&dir, &["convert", "model.puml", "-o", "model.umpl"]);
    assert!(r.status.success(), "{}", text(&r.stderr));
    // The include that could not be read is reported with its line.
    assert!(
        text(&r.stderr).contains("model.puml:3:1: cannot read included file"),
        "{}",
        text(&r.stderr)
    );
    let umpl_text = std::fs::read_to_string(dir.join("model.umpl")).unwrap();
    assert!(
        umpl_text.starts_with("diagram \"Imported\"\n"),
        "{umpl_text}"
    );
    assert!(
        umpl_text.contains("Order \"1\" *-- \"*\" Line\n"),
        "{umpl_text}"
    );
    assert!(!umpl_text.contains("layout {"));

    // The converted file is clean and can be tidied and checked.
    assert!(umpl(&dir, &["check", "model.umpl"]).status.success());
    let r = umpl(
        &dir,
        &["convert", "model.puml", "-o", "tidy.umpl", "--tidy"],
    );
    assert!(r.status.success());
    let tidied = std::fs::read_to_string(dir.join("tidy.umpl")).unwrap();
    assert!(tidied.contains("layout {\n"), "{tidied}");

    // Reading commands accept PlantUML directly.
    let r = umpl(&dir, &["render", "model.puml", "-o", "model.svg"]);
    assert!(r.status.success(), "{}", text(&r.stderr));
    let svg = std::fs::read_to_string(dir.join("model.svg")).unwrap();
    assert!(svg.contains("Customer"));
    assert!(umpl(&dir, &["stats", "model.puml"]).status.success());
    // `check` fails because of the skipped line.
    assert!(!umpl(&dir, &["check", "model.puml"]).status.success());
}

#[test]
fn rewriting_commands_refuse_plantuml() {
    let dir = workdir("refuse");
    std::fs::write(dir.join("model.puml"), PUML).unwrap();
    for cmd in ["fmt", "tidy"] {
        let r = umpl(&dir, &[cmd, "model.puml"]);
        assert!(!r.status.success(), "{cmd}");
        assert!(text(&r.stderr).contains("umpl convert"), "{cmd}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("model.puml")).unwrap(),
        PUML
    );
}

#[test]
fn convert_needs_a_known_output() {
    let dir = workdir("bad");
    std::fs::write(dir.join("shop.umpl"), SHOP).unwrap();
    assert!(!umpl(&dir, &["convert", "shop.umpl"]).status.success());
    assert!(!umpl(&dir, &["convert", "shop.umpl", "-o", "x.txt"])
        .status
        .success());
    assert!(!umpl(&dir, &["convert", "shop.umpl", "--to", "png"])
        .status
        .success());
}

#[test]
fn imports_and_exports_umple() {
    let dir = workdir("umple");
    std::fs::write(dir.join("model.ump"), UMP).unwrap();

    let r = umpl(&dir, &["convert", "model.ump", "-o", "model.umpl"]);
    assert!(r.status.success(), "{}", text(&r.stderr));
    // The state machine is not drawn, and says so with its position.
    assert!(
        text(&r.stderr).contains("model.ump:4:16: state machine `status` of `Mentor` is not shown"),
        "{}",
        text(&r.stderr)
    );
    let umpl_text = std::fs::read_to_string(dir.join("model.umpl")).unwrap();
    assert!(umpl_text.contains("package school {\n"), "{umpl_text}");
    assert!(
        umpl_text.contains("school.Student \"*\" -- \"1\" school.Mentor\n"),
        "{umpl_text}"
    );

    // Reading commands accept Umple directly.
    let r = umpl(&dir, &["render", "model.ump", "-o", "model.svg"]);
    assert!(r.status.success(), "{}", text(&r.stderr));
    let svg = std::fs::read_to_string(dir.join("model.svg")).unwrap();
    assert!(svg.contains("Mentor"));

    // Export back to Umple, to a file or standard output.
    let r = umpl(&dir, &["convert", "model.umpl", "-o", "back.ump"]);
    assert!(r.status.success(), "{}", text(&r.stderr));
    let back = std::fs::read_to_string(dir.join("back.ump")).unwrap();
    assert!(back.contains("namespace school;\n"), "{back}");
    assert!(
        back.contains("class Student {\n  String name;\n  Integer id;\n  * -- 1 Mentor;\n}\n"),
        "{back}"
    );
    let r = umpl(&dir, &["convert", "model.umpl", "--to", "umple"]);
    assert!(r.status.success());
    assert_eq!(text(&r.stdout), back);

    // `fmt` and `tidy` would overwrite the Umple file with `.umpl` text.
    for cmd in ["fmt", "tidy"] {
        let r = umpl(&dir, &[cmd, "model.ump"]);
        assert!(!r.status.success(), "{cmd}");
    }
    assert_eq!(std::fs::read_to_string(dir.join("model.ump")).unwrap(), UMP);
}

#[test]
fn fill_lays_out_to_the_window_shape() {
    let dir = workdir("fill");
    std::fs::write(
        dir.join("library.umpl"),
        include_str!("../../../examples/library.umpl"),
    )
    .unwrap();
    // `size WxH` from `umpl stats`, as width : height.
    let shape = |extra: &[&str]| {
        let mut args = vec!["stats", "library.umpl", "--tidy"];
        args.extend_from_slice(extra);
        let out = text(&umpl(&dir, &args).stdout);
        let size = out
            .split("size ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let (w, h) = size.split_once('x').unwrap();
        w.trim().parse::<f64>().unwrap() / h.trim().parse::<f64>().unwrap()
    };
    let (usual, fill) = (shape(&[]), shape(&["--fill"]));
    // The library comes out much wider than 16:10 for the fewest
    // crossings, and close to it with --fill.
    let off = |r: f64| (r / 1.6).ln().abs();
    assert!(off(fill) < off(usual), "{usual:.2} {fill:.2}");
    assert!(off(fill) < 0.15, "{fill:.2}");
}
