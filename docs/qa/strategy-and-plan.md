# QA strategy and first plan

2026-11-19. Context: [.agents/qa-project-context.md](../../.agents/qa-project-context.md). Findings of the first pass: [2026-11-19-findings.md](2026-11-19-findings.md).

## Strategy

The product's promises are textual (a file that reopens identically, exports other tools read) and geometric (no overlaps, few crossings), and almost all logic sits below the window. So the weight goes to fast, deterministic tests below the GUI, with property tests where the input space is large.

| Layer | What it guards | Tool | Share |
|---|---|---|---|
| Property tests | save → reopen keeps every editor value; undo all / redo all restores every state; layout never panics | seeded generators in `tests/` (no new dependencies) | grows first |
| Unit and example tests | parser, printer, importers, exporters, layout pieces | `cargo test` | most tests |
| Reference checks | PlantUML reads our export as we meant it | PlantUML jar in the dev image | per export change |
| Corpus | real PlantUML files open without panic or overlap; layout score ratchet | `puml_corpus.rs`, `corpus.rs` | sampled per change, full weekly |
| Interaction | canvas, text panel, undo steps, dialogs | egui_kittest (`src/tests.rs`) | few, per user flow |

Gates: CI on `main` stays as it is (fmt, clippy, tests). A confirmed bug gets a failing test, `#[ignore = "QA-NN: …"]` if it cannot be fixed in the same change; the fix removes the `ignore`. Exit criterion for a release: no ignored QA test of severity High.

## First plan

| # | Area | Test | Status |
|---|---|---|---|
| 1 | Save/reopen fidelity | hostile-value property test, 2000 documents | passes with every hostile value |
| 2 | Save/reopen fidelity | QA-01…06, 15, 17, 19 targeted tests | fixed, pass |
| 3 | Interop | PlantUML and Mermaid export of non-plain names, QA-07/08/18 | fixed, pass; PlantUML jar parses the export |
| 4 | File safety | atomic save keeps symlinks and permissions, QA-09; includes, QA-14 | fixed, pass |
| 5 | Undo | undo/redo property test, 400 sequences | passes |
| 6 | Undo | unsealed drag after undo, QA-10 | fixed, passes |
| 7 | Layout robustness | far-apart boxes lay out in < 0.5 s; lane count equals the point count, QA-11 | fixed, pass |
| 8 | Recovery | Save As and cancelled recovery keep no stale copy and lose none, QA-12/13/16 | fixed, pass (Save As tested past its file dialog via `save_as_path`) |
| 9 | PlantUML corpus | full 8000-file run with `PUML_CORPUS_LAYOUT=4` | next (≈40 min) |
| 10 | Mermaid | parse exports with a real Mermaid parser in the dev image | open decision (adds Node to the image) |

Out of scope for this pass: visual regression of the canvas, performance profiling of the GUI, macOS/Windows-specific behaviour (CI covers Windows tests only).
