# Real PlantUML files, big diagrams, text linked to the drawing, Find

Status: implemented 2026-11-18.

## 1. Why

The PlantUML importer of 2026-11-17 was written against hand-made samples. Files people actually keep are bigger and use more of PlantUML: `!include` of shared files, `!define` macros for database tables, C++ namespaces, generated `[[links]]`, component and use case diagrams next to class diagrams, C4 models built from library procedures. Opening big ones showed a second problem: a diagram of many loosely connected classes, or one class with many subclasses, opened as a single long row, shown as a thin strip with a long horizontal scroll bar.

Asked for at the same time:

- A large set of real `.puml` files over 5 KB, and a check that each opens without errors; includes followed.
- Big diagrams laid out to fill the window, and opened zoomed to fill it.
- The text panel linked to the drawing both ways: clicking a box marks its text and moves the cursor there; moving the cursor in the text marks its box. The text stays an ordinary editor (copy, paste).
- Three sample repositories in the dev image to test on: spiegel-im-spiegel/plantuml-sample, okunev-e/plantuml_examples, plantuml-stdlib/Azure-PlantUML.
- Find, marking matches in the drawing and in the text.

## 2. The corpus

`scripts/puml-corpus.py search` runs GitHub code search (`extension:puml size:5121..7000 "class"` and so on: ten class-diagram words, five size bands, five extensions) and records each hit with its repository, the commit it was found at and its path in `tests/puml-corpus/manifest.tsv`. `fetch` downloads each file at that commit into `target/puml-corpus/<owner>/<repo>/<path>`, keeping folders so relative includes resolve, and follows `!include`s of relative paths, adding each included file to the manifest with role `include`.

| | Files |
|---|---|
| Diagrams found (over 5 KB) | 7996, from 4517 repositories |
| Files they include, found | 6172 (3791 not found: outside the repository, generated, or named by a variable) |
| Files holding a diagram (`@start…`), with the samples | 15,573 |

The files belong to their authors and are not committed; the manifest pins them, so the same corpus can be downloaded again. The three sample repositories are cloned into the dev image at `/opt/puml-samples`, each at a pinned commit.

`crates/umpl-draw/tests/puml_corpus.rs` (ignored; `PUML_CORPUS=folders`) opens every file as the editor does: import with includes, then the same layout as on open. It fails on a panic, on overlapping boxes, and on a file taking over a minute, and reports the rest: kinds of diagram, problems by kind with an example each, the width : height of the drawings, the slowest layouts, and a table of every file in `target/puml-corpus-report.tsv`.

## 3. Import: what the corpus showed and what changed

First run, import only: 11,429 diagrams, no panic, but 174,217 lines not understood in 1562 files (15 %), and 57,032 `}` without a matching `{`. Lines were clustered by shape (words → `W`, strings → `Q`); most were one cause repeated through a class body.

| Cause | Example | Files | Now |
|---|---|---|---|
| C++ namespaces | `class util::render::Camera {` | ~720 | `::` separates packages like `.`; `A::b` in a relation is a member of `A` when no `A.b` exists |
| Links | `class A [[A.html]] {`, `+x : int [[[url]]]` | ~400 | Removed outside quotes before reading; their `{tooltip}` braces no longer unbalance bodies |
| Other kinds of diagram | sequence diagrams with `box`, activity, state, Graphviz `digraph` | ~800 | Recognised (section 3.1) and refused with one clear message |
| Line styles after a relation | `A --> B #green;text:green : x` | ~110 | Skipped |
| Aligned header or footer | `right footer … endfooter` | 142 | Skipped like `footer` |
| clang-uml output | `package [src] as C_000… {`, `class "::std::pair<T1,T2>" as C_01` then `class C_01 {` | ~70 | Bracketed package names; a declaration by alias reopens the element (the CS202 diagram had doubled to 1291 boxes, half empty) |
| Component, deployment, use case | `node "Web" as W {`, `[Api]`, `(Login)`, `:Admin:` | ~650 | Containers with a body become packages, the rest boxes with their kind as stereotype |
| C4 and other libraries | `Person(user, "User")`, `Rel(a, b, "Uses")`, `System_Boundary(b, "Shop") {` | ~380 | Calls of procedures from a library that was not read become boxes, lines and packages; reported once per procedure |
| Keywords in other case | `Title`, `Note right of`, `end Note` | — | A line not understood is read again with its keyword in lower case (`Card --> Account` is a relation from `Card`, not the `card` keyword) |
| Windows paths | `!include ..\.c4s\C4.puml` | 143 | `\` is a folder separator |
| Crow's-foot ends of ER diagrams | `User \|\|--o{ Order` | (part of 963 with unknown ends) | Associations with multiplicities: `\|\|` 1, `\|o` 0..1, `}o` 0..*, `}\|` 1..*; other unknown ends are reported once per file, not per line |
| A macro naming itself | `!define PK <b>PK</b>` | — | Not expanded inside its own expansion (as in PlantUML and C); it had grown a member line to thousands of characters and an ERD of 14 tables to 10881 × 330 grid units (now 359 × 207) |
| Formatting tags | `<b>`, `<color:Gold>`, `<size:12>` in names and members | — | Dropped; only known tag names, so `List<String>` stays |
| Paragraphs in library calls | `Component(c, "Cache", "Angular", "…1173 characters…")` | — | Wrapped at 60 characters and at `\n`, as C4 wraps them (a 79-box diagram went from 11539 to 1907 units wide) |
| Smaller ones | stereotype before `as`, `class A <T>`, `+class Inner {`, `implements .a.b.C`, `{` on the line after `package`, notes on methods, names over several lines | — | Read |

### 3.1 Kinds of diagram

`plantuml::diagram_kind` follows PlantUML's own rule: its readers are tried in turn (sequence, class, activity, description, state, …) and the first that accepts every line wins. A file is a sequence diagram when it has sequence lines and nothing a sequence diagram cannot hold; a class diagram can never hold `state X`, `[*]` or `:action;`, so those decide even when arrows look like class arrows. Note, legend and style blocks are skipped, since their text can be anything (a JSON example in a note made one sequence diagram look like a class diagram). Sequence, activity, state, timing and non-UML diagrams (`@startmindmap`, `@startgantt`, …) give an empty document with one problem; the editor shows it as the reason the file cannot be opened. Class, object, component, deployment and use case diagrams are read.

### 3.2 Preprocessor

`plantuml/preprocess.rs` runs before reading. It follows `!include`, `!include_once`, `!include_many` (a file is included once unless asked otherwise), `!includesub file!PART` and `file!N` / `file!ID`; resolves includes relative to the including file; stops at 16 levels (a file including itself). It expands `!define` and `!definelong` macros (arguments without their quotes, `##` pasting), `!$variables`, `!procedure` and `!function` with `!return`, and evaluates `!if`, `!ifdef`, `!ifndef`, `!elseif`, `!else`, `!endif` (numbers, strings, `==`, `!=`, `!`, `&&`, `||`, `%true()`, `%defined()`; anything else counts as true). Every line keeps its line in the main file and, if included, its file and line there, so a problem in an included file reads "in `tables.iuml` line 3: …" at the `!include` line. Web and standard-library includes are reported, not fetched: the editor does not go on the network by itself.

## 4. Big diagrams fill the window

Two causes of one-row layouts, both in the model rather than in a parameter:

- **Loose parts were packed for the smallest area.** A second row costs a whole gap between rows, so many small parts always fit in less area as one row. The top level now packs for the view: the packing whose drawing needs the least zoom to fit a view of the target shape, `max(w, h · aspect)`, then the smallest area. Inside a package the old compact packing stays (packing inside packages for the view raised banking's score by 11 % and crossings elsewhere).
- **Inside a package, loose parts packed into a strip too.** Kept compact (their left-to-right order follows the lines leaving the package, and wrapping crosses those lines), unless it is a strip of more than 8 parts longer than 6:1 (two boxes side by side are not a strip): then they pack to fill. 3:1 cost crossings on the test diagrams; 6:1 and up leaves them unchanged.
- **A row of siblings could not be split.** The layer-width limit only moved boxes without a parent. Inheritance only requires a child below its parent, so when no parentless box can move, children now move down (width-bounded layering, as Coffman and Graham's); at the top level as soon as a layer is too wide, inside a package only when it is twice too wide.

The target shape is the canvas's width : height (`Options::aspect`), passed by the editor on open and on Tidy; the command line keeps 16:10. Fit fills the canvas, zooming in up to 200 % and out to 5 %.

Benchmark (52 generated diagrams of 40–150 classes, now `cargo test --release -p umpl-draw --test corpus -- --ignored benchmark`), engine before and after:

| | Before | After |
|---|---|---|
| Crossings | 2808 | 2798 |
| Score | 83,161 | 83,053 |
| Median width : height | 2.60 | 2.07 |
| Wider than 3:1 | 21 of 52 | 15 of 52 |

| Synthetic case | Before | After |
|---|---|---|
| 80 unconnected classes | one row | 188 × 116 grid units (1.6:1) |
| 60 subclasses of one class | 1065 × 31 (34:1) | 409 × 103 (4:1), 0 crossings, score 1859 → 1090 |

The quality corpus passes; gen-200-4 went from 246 to 211 crossings and its recorded score was updated.

## 5. Big diagrams open without freezing

Measured on real files: a 972-class diagram laid out in 1.7 s; a dense 644-class, 1347-line C++ template diagram took 427 s; a 5237-class, 11,575-line one ran the machine out of memory. Placement took 0.2 s of the 427; routing took the rest, on a grid of 15 million points (about 90 bytes each, for each of three candidates routed in parallel). In a dense drawing each A* search floods its window: the estimate is the plain distance while a crossing costs 100 grid units, and most lines must cross something.

- A routing pass may spend 100 million states on exact searches (the final polish a quarter); after that its searches are bounded. Sparse drawings never reach it. A budget per search was tried first and dropped: in a sparse 972-class diagram it cut good searches short (0 → 582 shared lanes), and in dense ones each failing search still spent its budget and then a whole-grid search.
- Above 3 million grid points: weighted A* (estimate × 3) and a budget of states per search; no wider look, no final polish.
- Above 300 lines: one candidate is routed, the one whose straight lines cross least.
- Routing candidates in parallel only as fit in 1.5 GB.
- Above 12 million grid points: draft lines (straight or one bend, meeting box edges square, not around boxes); `Stats::draft`.
- In the editor, a diagram of more than 150 elements without positions opens at once and is laid out in the background; the first layout is not an undo step and does not mark the file changed.

All limits count states and grid points, never time, so layouts stay deterministic. Result: 972 classes 1.3 s with the same layout; 644 dense classes 0.6 s with drafts (bounded routing took 40 s and crossed as much); 5237 classes 36 s with drafts; a 36-class diagram with 536 lines 40 s → 15 s, a 152-class one with 632 lines 66 s → 24 s. The editor lays out in the background any diagram of more than 150 classes or 300 relations. The 52-diagram benchmark and the quality corpus are unchanged by all of these.

## 6. Text and drawing linked

The `.umpl` parser records the lines declaring each element, relation and package (`Parsed::spans`, `item_at`, `span_of`). `app/link.rs` matches them with the document by name, since the parsed text has ids of its own. Selecting in the drawing tints the declaration, scrolls to it and sets the text cursor there without taking the keyboard; moving the text cursor selects the innermost item on its line and pans the canvas to it if needed; a line declaring nothing leaves the selection. Copy, cut, paste and select-all in the panel are covered by interaction tests.

## 7. Find

Ctrl+F (Edit → Find…) opens a bar under the menu. Case-insensitive matches in element names, stereotypes and members, relation names and end labels, package names; all outlined on the canvas, all occurrences tinted in the text panel; Enter / Shift+Enter or F3 / Shift+F3 step through them, selecting and revealing each, which also scrolls the text to its declaration; Esc closes.

## 8. Corpus results

Final run, 2026-11-19: all 15,573 files with a diagram imported, every fourth laid out (`PUML_CORPUS_LAYOUT=4`), 10 minutes on the 32-core dev machine. No panic, no overlapping boxes, no file over a minute.

| | First run (importer of 2026-11-17) | Final |
|---|---|---|
| Diagrams read (class, object, component, use case) | 10,704 | 14,940 |
| ... opened without any problem | 4318 (40 %) | 7788 (52 %) |
| ... with lines not understood | 1562 (15 %) | 660 (4.4 %) |
| Lines not understood | 174,217 | 19,459 |
| `}` without a matching `{` | 57,032 | 2326 |
| Other kinds, refused with a clear message | 725 | 368 sequence, activity, state, … and 105 non-UML (mind maps, Gantt, JSON, Graphviz, …) |

The problems left are mostly reports, not failures: styling skipped (`skinparam`, `hide`), type parameters dropped (`List<T>` as a class name), web and standard-library includes not fetched, a class renamed because a package has its name.

Drawings, laid out for a 16:10 view (the editor uses its canvas shape):

| Diagrams with | Drawings | Median width : height | Wider than 3:1 | Wider than 6:1 |
|---|---|---|---|---|
| 10 or more elements | 1598 | 1.89 | 21 % | 44 |
| 30 or more | 910 | 1.98 | 23 % | 29 |
| 100 or more | 176 | 2.20 | 30 % | 14 |

Layout time per diagram: median 4 ms, 90 % under 0.8 s, 99 % under 10 s, slowest 29 s (a dense 655-class model; in the editor such diagrams are laid out in the background).

## 9. Left open

- Dense graphs with hubs stay wide: a package dependency graph of 670 entities and 1280 arrows, most to a few hubs, comes out 38:1. The layer-width limit counts boxes, not the waypoints of long lines that pass through a layer (in a dense graph most of a layer's width).
- Below 10 elements, wide drawings are mostly one box with a very long name or member, or a short chain; nothing to fold.

- Folded rows of siblings are ragged: boxes of one row at a few heights. A large family of siblings placed as a grid would look cleaner.
- Dense diagrams over 12 million grid points get draft lines. A sparse routing graph (lanes only at box edges, ports and channel midpoints, design §9) would route them properly and make every layout faster.
- Includes from the web and PlantUML's standard library are not read; C4 diagrams come out as boxes named by their labels, without C4's own shapes.
- `+--` (nested class) is drawn as a plain association and reported.
