# QA session: big and extreme diagrams, 2026-11-20

Exploratory testing (SBTM) of how fastumpl opens, draws and keeps `.umpl` and PlantUML files with big values: many elements, long text, far coordinates, large numbers. Build: release `fastumpl` and `umpl` at `db08a2d`. The GUI ran in the `fastumpl-qa` image on Xvfb (1600×980, openbox, software OpenGL), driven with xdotool and read from screenshots, with HOME in a scratch folder. Memory was capped per container (2.5 GB for the CLI, 3.5 GB for the GUI) so a runaway allocation fails in the container, not the VM. Reference: PlantUML 1.2026.9 in the same image. Screenshots are in [2026-11-20-big/](2026-11-20-big/).

## Test files

[`scripts/big-files.py`](../../scripts/big-files.py) writes them into [`tests/big/`](../../tests/big/) with a fixed seed (`scripts/dev.sh python3 scripts/big-files.py`).

| File | Pushes |
|---|---|
| `class-many.umpl` | 220 classes, interfaces and enums, 460 relations of every kind, packages two deep, 6 notes (above the 150-class background layout threshold) |
| `class-crossings.umpl` | the same at 40 classes and 80 relations: small enough for a test, with crossings |
| `class-huge.umpl` | 1500 classes in 30 packages, 3000 relations |
| `class-long-text.umpl` | a 140-character class name, a 400-character member, 200 members, a 300-literal enum, CJK and combining characters, multiplicities of 20–70 digits, a 10-line relation name, a 40-line note, a 1100-character one-line note |
| `class-far-coords.umpl` | positions at ±1,000,000 (the limit) and past it, sizes 10,000 and 10,001 |
| `seq-many.puml` | 40 participants of all kinds in 4 boxes, 1530 messages, nested fragments, notes, refs, dividers, delays, activations |
| `seq-long-text.puml` | long and 8-line participant names, a 500-character label, a 25-line label, a 30-line note, fragments 30 deep |
| `seq-big-numbers.puml` | `autonumber` at and past `i64::MAX`, a 40-digit format, `resume` with a step of 10^12, `||100000||` |
| `seq-huge-space.puml` | `||4294967295||` (u32 max) followed by 12 messages and a note |
| `seq-arrows.puml` | every PlantUML message arrow, one per line |

## Charters

| # | Charter | Coverage |
|---|---|---|
| C1 | Explore the CLI (`check`, `stats`, `render` SVG/PNG, `fmt`, `tidy`, `convert`) with the big files to discover crashes, hangs and unbounded memory | covered |
| C2 | Explore the PlantUML sequence reader with big and unusual but valid input, against PlantUML itself, to discover misreadings | covered: arrows, fragment labels, numbers, spaces |
| C3 | Explore the canvas with the big files (open, Fit, zoom, Find, scroll) to discover drawing errors and slowness | covered for every file; light theme not tried |
| C4 | Explore save → reopen and export → import of the big files to discover what a file does not keep | covered for class files; PlantUML import of a class diagram only through our own export |

## Session log

Times are local, from the screenshots and outputs.

| Time | Tag | Note |
|---|---|---|
| 07:23 | NOTE | Generator written; eight files, 300 B to 190 KB |
| 07:24 | BUG | `render -o .png` of `class-far-coords` aborts: `memory allocation of 4096101376557568 bytes failed`; `seq-huge-space` the same with 14 TB (QA-34) |
| 07:24 | QUESTION | `seq-big-numbers.puml` opens as a class diagram. Cause: `autonumber -9223…` reads as a line `-`. PlantUML reads `autonumber -5` as a class diagram too, so this matches |
| 07:26 | NOTE | PlantUML rejects autonumber and `||N||` values above `int` (`NumberFormatException`); its own numbering wraps at 2147483647 |
| 07:28 | BUG | `loop - every second -` (valid, drawn by PlantUML) makes the file a class diagram (QA-27) |
| 07:29 | BUG | `class-many` PNG killed at the 2.5 GB cap: 18080×29224 px at the default scale 2 (QA-34) |
| 07:29 | BUG | `seq-many.puml` opens as a class diagram; arrow matrix against PlantUML: `-\`, `-\\`, `-/`, `-//`, `//--`, `o->`, `o->o` (QA-28) |
| 07:31 | BUG | Rows after `||268435456||` or more fall on one line; PlantUML keeps them apart (QA-29) |
| 07:31 | NOTE | Long names, members, 70-digit multiplicities, 300 literals, 200 members: all inside their boxes, labels beside their lines ([clt-bottomleft.png](2026-11-20-big/clt-bottomleft.png)) |
| 07:32 | NOTE | GUI, `class-many`: background layout ≈18 s, same crossings and score as the CLI; readable at 54 % ([many-100.png](2026-11-20-big/many-100.png)) |
| 07:32 | QUESTION | While it lays out, the status bar says `0 crossings` ([many-02s-status.png](2026-11-20-big/many-02s-status.png)) |
| 07:33 | BUG | `class-far-coords`: a draft line runs on top of another and through a box, so `FarCorner ..> NegCorner` cannot be seen (QA-30) |
| 07:33 | NOTE | Find reaches boxes a million units away; at 400 % they draw sharp ([far-east-zoom.png](2026-11-20-big/far-east-zoom.png)) |
| 07:34 | NOTE | GUI, `seq-many`: 40 participants, 1530 messages, opens in about a second |
| 07:35 | BUG | Find on a sequence diagram says "No matches" for a participant that is drawn (QA-36) |
| 07:36 | BUG | Opening `seq-huge-space.puml` grows the app ≈1 GB/s until it is killed; `||2147483647||`, valid in PlantUML, too (QA-31) |
| 07:37 | BUG | The recovery copy of `seq-many` (= what Save writes) has lost `->o` and `x->` (QA-32) |
| 07:37 | QUESTION | The recovery dialog names an imported diagram "Untitled diagram" though it has a title and a source file |
| 07:39 | NOTE | `seq-long-text`: 30 nested frames, 25-line label, long self-call: no overlaps ([seqlong-nest.png](2026-11-20-big/seqlong-nest.png), [slt-head.png](2026-11-20-big/slt-head.png)) |
| 07:41 | BUG | `class-huge`: Fit stops at the 5 % floor and the drawing does not fit (QA-37) |
| 07:42 | BUG | Idle, `class-huge` uses 42 % of a core; 3 % for `shop.umpl` (QA-38) |
| 07:44 | BUG | `autonumber` past `i64::MAX` wraps to a negative number (release) or panics (debug) (QA-35) |
| 07:46 | BUG | `class-many` saved and reopened: 4301 crossings instead of 3815 (QA-33); confirmed in the GUI at 07:48 |
| 07:50 | NOTE | Regression tests written; all six fail before any fix |

## Findings

Severity as in [2026-11-19-findings.md](2026-11-19-findings.md). Numbering continues from [2026-11-19-gui-session.md](2026-11-19-gui-session.md).

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| QA-33 | Medium | A saved diagram reopens with different lines: `class-many` 3815 → 4301 crossings, half its lines re-routed | test `a_saved_diagram_reopens_with_the_same_lines`; GUI |
| QA-28 | Medium | Seven valid sequence arrows (`-\`, `-\\`, `-/`, `-//`, `//--`, `o->`, `o->o`) make a sequence diagram open as a class diagram | test `every_sequence_arrow_keeps_a_sequence`; PlantUML reference |
| QA-27 | Medium | A fragment label starting with a dash (`loop - every second -`) makes it a class diagram | test `a_fragment_label_starting_with_a_dash_keeps_a_sequence` |
| QA-32 | Medium | `->o`, `<->o` and `x->` lose their circle or cross, in the drawing and on Save | test `message_decorations_survive_save`; PlantUML render of the saved text |
| QA-31 | Medium | A sequence diagram with a large space (`||2147483647||`, valid PlantUML) runs the app out of memory | GUI, exit 137; code [paint.rs:111-112](../../crates/umpl-app/src/paint.rs#L111-L112) |
| QA-34 | Medium | PNG rendering has no size limit: big drawings abort the process or run out of memory | CLI `umpl render -o .png`; code [png.rs:29-34](../../crates/umpl-draw/src/png.rs#L29-L34) |
| QA-29 | Low | Rows after a space of 2^28 px or more fall on one line | test `rows_after_a_large_space_stay_apart` |
| QA-30 | Low | Draft lines can lie on top of each other and cross boxes, hiding a relation | CLI SVG of `class-far-coords`; GUI |
| QA-35 | Low | `autonumber` past `i64::MAX` wraps (release) or panics (debug) | test `autonumber_at_the_largest_number_does_not_overflow` |
| QA-36 | Low | Find on a sequence diagram finds nothing and says "No matches" | GUI ([seqmany-find.png](2026-11-20-big/seqmany-find.png)) |
| QA-37 | Low | Fit cannot fit a very big diagram (5 % zoom floor) | GUI ([huge-90s.png](2026-11-20-big/huge-90s.png)) |
| QA-38 | Low | The once-a-second repaint costs 42 % of a core on a 1500-class diagram | GUI, CPU time from `/proc` |

None is High. QA-31 and QA-34 crash, but on drawings far bigger than usual; QA-33 loses no data (boxes and model are intact) but every diagram with crossings is affected.

### QA-33: a saved diagram reopens with different lines

Steps: open `tests/big/class-many.umpl` (no positions) and wait for the layout: 3815 crossings. Ctrl+S (positions are written), close, open the file again.

- Expected: the same drawing; positions are stored so that it comes back.
- Actual: the boxes are where they were, but 4301 crossings ([gui-many-before-status.png](2026-11-20-big/gui-many-before-status.png), [gui-many-after-status.png](2026-11-20-big/gui-many-after-status.png)). In the SVGs only 2494 of about 5000 line paths are the same. The CLI shows it too: `umpl stats` against `umpl tidy` then `umpl stats`.

It is not only big diagrams: 40 classes 88 → 81 crossings (13 of 86 lines differ), 100 classes 753 → 773. Diagrams without crossings (all four examples) come back identical.

Cause: opening lays out with `layout_all`, which routes the chosen candidate and then runs `polish` ([place.rs:1132-1133](../../crates/umpl-layout/src/place.rs#L1132-L1133)). Reopening, every box has a position, so `route_only` calls `finalize`, a different routing of the same positions ([place.rs:1342-1347](../../crates/umpl-layout/src/place.rs#L1342-L1347)). Lines are not stored, so the drawing is only reproducible if the routes are a function of the positions alone, the same function in both paths.

### QA-28 and QA-27: sequence diagrams read as class diagrams

Each arrow, in a file with two participants and one message, is drawn by PlantUML with lifelines; fastumpl reads `-\`, `-\\`, `-/`, `-//`, `//--`, `o->` and `o->o` as a class diagram. `->`, `-->`, `->>`, `->x`, `x->`, `<->`, `-[#red]>`, `->o`, `<->o`, `-->o`, `<-`, `<--` are fine. One such line anywhere makes the whole file a class diagram: `seq-many.puml` became 52 boxes and 656 lines, with every `box`, `autonumber` and fragment line "not understood". Saving it writes a class diagram with the messages as comments (the `.puml` itself is left alone).

Cause: [kind.rs `class_arrow`](../../crates/umpl-text/src/plantuml/kind.rs#L601) counts an arrow starting with `o` as aggregation and one without `<` or `>` as a plain line, both class-only. PlantUML tries its sequence reader first; asking the sequence reader's own `read_arrow` whether the arrow is a message would follow it.

QA-27 has the same effect by another rule: "a keyword followed by an arrow is a name" ([kind.rs:346](../../crates/umpl-text/src/plantuml/kind.rs#L346)) takes the `-` of `loop - every second -` for a line, so `loop` is read as a class named `loop`. (`autonumber -5` goes the same way, but PlantUML reads that file as a class diagram too.)

### QA-32: circles and crosses on messages are dropped

`A ->o B`, `A <->o B` and `A x-> B` are read as plain `->` and `<->`. The model has `Head::Circle`, but the reader never sets it. The drawing shows no circle, and Save writes `A -> B`; PlantUML's render of the saved text has 0 circles where the original had 2, and no cross. Found by diffing the recovery copy of `seq-many` with its source.

### QA-31: a large space runs the app out of memory

Steps: open `tests/big/seq-huge-space.puml` (`||4294967295||`), or the same with `||2147483647||`, which PlantUML accepts and draws.

- Expected: a tall drawing.
- Actual: the window never paints; memory grows ≈1 GB/s (287 → 2222 MB in 2.5 s) until the process is killed (exit 137, nothing logged). `||10000000||` opens at 272 MB.

Cause: a dashed path is skipped only when its whole bounding box is off screen; otherwise `Shape::dashed_line` makes one shape per dash along the whole length ([paint.rs:111-112](../../crates/umpl-app/src/paint.rs#L111-L112)). Lifelines of 2^31 px give 2×10^8 dashes each, every frame, whatever the zoom. Clipping the polyline to the visible rectangle before dashing (keeping the dash phase) bounds it by the window. The draft lines of `class-far-coords` (8 million px) cost the same way, only less.

### QA-34: PNG rendering has no size limit

`umpl render X -o X.png` (default scale 2) allocates the whole image: `class-far-coords` asks for 4 PB and aborts (`memory allocation of 4096101376557568 bytes failed`, SIGABRT), `seq-huge-space` 14 TB. `class-many` needs 18080×29224 px (2.1 GB of pixels) and was killed at the 2.5 GB cap; `seq-many` would need 7.4 GB. The `"image too large"` check only catches sizes that overflow tiny-skia ([png.rs:34](../../crates/umpl-draw/src/png.rs#L34)). File → Export → PNG… and the clipboard image use the same function at scale 2 ([app.rs:507](../../crates/umpl-app/src/app.rs#L507)); not tried in the GUI, as this session was about the files. A pixel limit with an error that says so (or a lower scale for big drawings) would turn the abort into a message.

### Smaller ones

- **QA-29:** with `||268435456||` (2^28) the rows after the space are at the same y (268435584), and with `||2147483647||` everything below, participants' feet included, sits at 2147483648. y is an `f32`; PlantUML, with the same input, keeps 29 px between rows. PlantUML rejects spaces above 2147483647; fastumpl takes any `u32`.
- **QA-30:** with boxes a million units apart all lines are draft lines (the plane is far past 12 million grid points). The dashed `FarCorner ..> NegCorner` leaves from the same port where `EastLimit --> FarCorner` arrives, runs exactly on it for a million units and through `EastLimit` ([far-find-east.png](2026-11-20-big/far-find-east.png)). Draft lines are documented not to go around boxes; that one line can hide another is not.
- **QA-35:** `autonumber 9223372036854775806` gives …806, …807, then `-9223372036854775808`; `autonumber 1 9223372036854775807` gives 1, then a negative number; a start too big for 64 bits silently restarts at 1. A debug build panics (`attempt to add with overflow`, [sequence.rs:145](../../crates/umpl-draw/src/sequence.rs#L145)). PlantUML rejects all of these (its numbers are `int`), so only contrived files reach it.
- **QA-36:** in a 1530-message diagram Find is the obvious way around, but it searches class diagrams only; "No matches" for `P00`, a participant on screen, reads as "not there".
- **QA-37:** README: big diagrams open "and the view zooms to fit". `class-huge` and `class-far-coords` do not fit at the 5 % floor; Fit leaves the drawing cut at the top and bottom.
- **QA-38:** `periodic()` asks for a repaint every second to check the disk ([app.rs:1096](../../crates/umpl-app/src/app.rs#L1096)), and each repaint draws the whole scene. Idle CPU over 10 s: `shop.umpl` 0.3 s, `class-many` 0.7 s, `seq-many` 0.7 s, `class-huge` 4.2 s. Under software OpenGL; a GPU would cost less, the pattern stays. Panning `class-huge`: about 110 ms per scroll step (≈9 frames per second). The app has no frame-rate display, so CPU time stood in for it.

## Questions

- During a background layout the status bar shows `0 crossings` beside "Laying out…": better to show nothing until the result is in?
- The recovery dialog calls an imported diagram "Untitled diagram". Its title or source file name would say which one it is.
- PlantUML's light box colours (`box "…" #f0f0ff`) are dimmed to mid-grey in the dark theme; light-grey message lines and dashed lifelines are hard to see on it ([seqmany-plus7.png](2026-11-20-big/seqmany-plus7.png)). Should lines on a coloured box take a contrasting colour?
- Emoji are not letters, so `class Emoji🚀Rocket` is "not understood"; CJK names work. Intended?

## What held up

- Every file opened; nothing hung. `check`, `stats`, `render -o .svg` take 0.01–18 s; `class-many` lays out in ≈18 s in the CLI and the GUI with the same result, `class-huge` in ≈85 s at a peak of 1 GB, then 300 MB.
- Long text: names, members, a 300-literal enum, 200 members, 70-digit multiplicities, 10-line relation names and 40-line notes stay inside their boxes; labels sit beside their lines without overlapping.
- Coordinates at ±1,000,000 draw sharp at 400 %; the three layout lines past the limits are reported under Problems and the boxes placed anew.
- Sequences: 1530 messages open in about a second; numbering, edge messages, 30 nested fragments, many-line labels and notes are drawn without text running into text.
- `umpl fmt` is idempotent on every class file and keeps its stats; PlantUML export of `class-many` re-imports with the same 226 boxes and 466 lines; Umple export turns notes, dependencies and aggregations into comments, as documented.
- A session ended by SIGTERM left a recovery copy of its imported diagram, offered at the next start. The OOM-killed sessions (QA-31) died before their first copy, so there was no crash loop.

## Regression tests

Each fails on `db08a2d` and is `#[ignore = "QA-NN: …"]` until fixed (`scripts/dev.sh cargo test --no-fail-fast -p umpl-text -p umpl-draw --test qa_big -- --ignored`).

| ID | Test |
|---|---|
| QA-27, 28, 32 | [crates/umpl-text/tests/qa_big.rs](../../crates/umpl-text/tests/qa_big.rs) |
| QA-29, 33, 35 | [crates/umpl-draw/tests/qa_big.rs](../../crates/umpl-draw/tests/qa_big.rs) |

QA-31 and QA-34 need a decision on the fix (clipping, a pixel limit) before a test can say what is right without running out of memory; QA-30, 36, 37, 38 are GUI or performance and were left as notes.

## Next

- Fix order by reach: QA-33 (every diagram with crossings), QA-28/27 (whole files misread), QA-32, QA-31, QA-34.
- Not covered: the light theme with the big files, Export PNG/SVG from the GUI, typing in the text panel of a 1500-line file (each pause re-reads it), PlantUML class diagrams written by people rather than by our export.
