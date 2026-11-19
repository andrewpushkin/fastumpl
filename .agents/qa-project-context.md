# QA project context: fastumpl

Recorded 2026-11-19 from the repository; nothing here was invented. Unknowns are listed at the end.

## Product

Desktop editor for UML class diagrams (Rust, egui) with automatic layout and line routing, plus the `umpl` command line tool. Files are `.umpl` text; PlantUML (`.puml`…) and Umple (`.ump`) are imported; PlantUML, Mermaid and Umple are exported. Releases for Windows (x64, ARM64), macOS and Linux.

## Stack and layout

| Crate | Role | Test surface |
|---|---|---|
| `umpl-core` | Document model, undoable edits, text↔model reconcile | unit tests; `tests/qa_undo_property.rs` |
| `umpl-text` | `.umpl` parser/printer, PlantUML/Umple import, PlantUML/Mermaid/Umple export | unit + `tests/` (round trip, interop, Umple corpus, QA tests) |
| `umpl-layout` | Layering, ordering, placement, orthogonal routing | unit tests; layout corpus via `umpl-draw/tests/corpus.rs` |
| `umpl-draw` | Measuring, scene, SVG/PNG | unit; `tests/corpus.rs` (score ratchet), `tests/puml_corpus.rs` (ignored, real PlantUML files) |
| `umpl-cli` | `umpl` command | `tests/cli.rs` |
| `umpl-app` | The window | `src/tests.rs`: 55 headless interaction tests (egui_kittest) |

## Commands (all inside the dev container, never on the host)

- Full suite: `scripts/dev.sh cargo test --workspace` (≈270 tests, a few minutes cold).
- As CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- QA regression tests (all run in the normal suite): `crates/umpl-text/tests/qa_*.rs`, `crates/umpl-core/tests/qa_undo_property.rs`, `crates/umpl-draw/tests/qa_layout.rs`, and the QA-12/13/16 tests at the end of `crates/umpl-app/src/tests.rs`. Known-failing ones, if any, are `#[ignore = "QA-NN: …"]`; list them with `-- --ignored`.
- PlantUML corpus: `PUML_CORPUS=/opt/puml-samples PUML_CORPUS_LAYOUT=4 scripts/dev.sh cargo test --release -p umpl-draw --test puml_corpus -- --ignored --nocapture` (samples: seconds; full 8000-file corpus: 30–40 min with `=4`).
- PlantUML reference: `java -jar /opt/plantuml/plantuml.jar -tutxt FILE` (needs Graphviz `dot`, not in the image, once a diagram has lines) and `java -cp /opt/plantuml/plantuml.jar scripts/PumlCheck.java --dump FILE` in the dev image.

## CI/CD

GitHub Actions on pushes to `main` only (no PR builds): Linux fmt + clippy + test, Windows test. Every push to `main` also publishes release `v0.1.N`, so a red test on `main` blocks a release.

## Environments and limits

WSL2 VM, ~7.5 GB RAM, 32 cores; GUI through X11/WSLg from the container. No Mermaid renderer in the image, so Mermaid output is checked by syntax rules only.

## Risk areas (from the design's own failure conditions)

1. Data loss: save → reopen must give the same diagram (`docs/design.md` §1 "What would make it a failure").
2. Interop: exported PlantUML/Mermaid must be readable by those tools.
3. Layout must never hang or crash on any file `umpl check` accepts.
4. Recovery files and external-change detection.

## Unresolved (not in the repo)

- Coverage goals, team/ownership, support matrix beyond the release targets, whether Mermaid output must be validated by a real Mermaid parser in CI.
