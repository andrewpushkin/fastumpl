# fastumpl: design

Status: version 1 implemented, 2026-11-16. Section 13 lists where the build differs from this design.

## 1. What this is

fastumpl is a desktop diagram editor written in Rust. It opens a diagram file, lets you edit the diagram with the mouse and keyboard, and saves it back. Its main promise is that diagrams look good without manual tidying: the app places the boxes and routes the lines itself, and it works to keep line crossings as few as possible.

### Assumptions made without asking

These were not confirmed. Each one can be changed, and section 12 says what a change would cost.

| Assumption | Why |
|---|---|
| "UMPL" means UML, and version 1 covers class diagrams | Class diagrams are the most common UML diagram and the hardest to lay out well, so they prove the layout engine |
| The file is the app's own text format with the extension `.umpl` | Text works with git, can be written by hand, and can be reviewed in a diff |
| Native desktop window, not a web page | The request says "editor on Rust" and the project name says "fast" |

### Goals

1. Open, edit and save `.umpl` files with no data loss.
2. One command ("Tidy") turns any diagram into a clean one: no overlapping boxes, no lines through boxes, few crossings, few bends.
3. Editing keeps the diagram clean: when you move a box, its lines reroute immediately.
4. The same file always produces the same picture, on every machine.
5. It feels instant: 60 frames per second while panning, and layout of a normal diagram in well under a second.

### Not in version 1

- Other UML diagram types (sequence, state, activity, use case). The architecture leaves room for them.
- Code generation or reverse engineering from source code.
- Real-time collaboration.
- Hand-drawn line routes. Lines are always routed by the app.
- Several diagrams in one file.

### What would make it a failure

- A saved file that reopens looking different.
- A small edit that reshuffles the whole diagram.
- Auto layout that the user must fix by hand most of the time.

## 2. Scope of version 1

Elements:

- Class, abstract class, interface, enum.
- Attributes and operations with visibility (`+ - # ~`), type, static and abstract markers.
- Stereotypes (`«entity»`).
- Packages that contain elements, nested to any depth.
- Notes attached to an element.

Relations:

| Relation | Drawn as |
|---|---|
| Generalization (extends) | Solid line, hollow triangle at the parent |
| Realization (implements) | Dashed line, hollow triangle at the interface |
| Association | Solid line, optional open arrow |
| Aggregation | Solid line, hollow diamond at the whole |
| Composition | Solid line, filled diamond at the whole |
| Dependency | Dashed line, open arrow |

Every relation can carry a name, and a role name and multiplicity at each end.

## 3. Technology

| Area | Choice | Reason |
|---|---|---|
| Language | Rust, stable toolchain | Given |
| Window and widgets | egui with eframe | Free-form drawing on a canvas is simple, it redraws fast, and it runs on Linux, Windows, macOS and WSL |
| File dialogs | Native dialogs through the `rfd` crate | Users expect the system dialog |
| Layout engine | Written for this project | See section 6.1 |
| Export | SVG written by the app; PNG produced by rendering that SVG | One drawing path for both formats, so they always match |

Alternatives considered for the window: iced (cleaner structure, but more work for a custom canvas with inline editing), Tauri (the interface would be web code, not Rust), Slint (good for forms, weaker for a free-form canvas). The builder should confirm the egui choice with a one-day test of text sharpness at several zoom levels before committing to it.

## 4. Architecture

The app is a Cargo workspace. Only the last two crates know about the screen. Everything else can be tested without a window.

```
                 ┌────────────┐
   .umpl file ⇄  │ umpl-text  │  parse and print
                 └─────┬──────┘
                       ▼
                 ┌────────────┐   edit operations, undo
                 │ umpl-core  │◄──────────────────────────┐
                 └─────┬──────┘                           │
                       ▼                                  │
                 ┌────────────┐                           │
                 │ umpl-layout│  positions and routes     │
                 └─────┬──────┘                           │
                       ▼                                  │
                 ┌────────────┐                           │
                 │ umpl-draw  │  list of shapes; SVG      │
                 └──┬──────┬──┘                           │
                    ▼      ▼                              │
            ┌──────────┐ ┌──────────┐                     │
            │ umpl-cli │ │ umpl-app │─────────────────────┘
            └──────────┘ └──────────┘
```

| Crate | Responsibility | Depends on |
|---|---|---|
| `umpl-core` | The document: elements, relations, packages, stored positions. All edit operations and their inverses. | nothing |
| `umpl-text` | Reads a `.umpl` file into a document and writes a document back. | core |
| `umpl-layout` | Takes sized boxes and typed edges, returns box positions, line routes, label positions and quality numbers. Pure and deterministic. | nothing (its own small graph types) |
| `umpl-draw` | Turns document plus layout result into a flat list of shapes (rectangles, lines, text, arrowheads). Writes SVG. | core, layout |
| `umpl-cli` | `umpl render in.umpl -o out.svg`, `umpl fmt`, `umpl check`. | text, layout, draw |
| `umpl-app` | The window: canvas, tools, inspector, file handling. Paints the shape list with egui. | all |

Two rules keep this clean:

- `umpl-layout` does not know what a class is. It sees boxes with sizes, edges with a kind and a weight, and groups. This is what lets other diagram types reuse it later.
- The screen and the SVG export paint the same shape list, so export is always exactly what is on screen.

Text measurement is the one place where layout needs help from outside: box sizes depend on font metrics. `umpl-draw` owns a text measurer backed by a font file that ships inside the app, so sizes are identical on every machine and in the CLI.

## 5. The document and the file

### 5.1 Document model

A document holds:

- Elements, each with a kind, a name, an optional stereotype, an ordered member list, and the package it belongs to.
- Relations, each with a kind, two ends (element, role name, multiplicity, navigability), and an optional name.
- Packages, forming a tree.
- Notes, each with text and the element it is attached to.
- A position for each element (top-left corner, in whole grid units). A position may be missing.

Box sizes and line routes are never stored. They are always computed, which is why the same file always gives the same picture.

Names are unique inside a package, and the full name (`billing.Invoice`) identifies an element in the file. Renaming an element updates every relation and position that refers to it.

### 5.2 File format

A small text format, deliberately close to PlantUML class syntax so it is familiar, but not a promise of PlantUML compatibility.

```
diagram "Shop"

package billing {
  abstract class Payment {
    +amount: Money
    +pay(): Receipt {abstract}
  }
  class CardPayment
}

class Order {
  +id: OrderId
  -lines: List<OrderLine>
  +total(): Money
}

interface Priced {
  +price(): Money
}

enum Status { New, Paid, Shipped }

# an order owns its lines
Order "1" *-- "1..*" OrderLine : contains
billing.CardPayment --|> billing.Payment
Order ..|> Priced
Order --> billing.Payment : paid by

layout {
  Order               at 24, 8
  OrderLine           at 24, 22
  billing.Payment     at 48, 8
  billing.CardPayment at 48, 22
}
```

Rules:

- Relation arrows: `--|>` generalization, `..|>` realization, `--` association, `-->` directed association, `o--` aggregation, `*--` composition, `..>` dependency.
- The `layout` block is optional and comes last. Its lines are sorted by name. Meaning stays at the top of the file and geometry at the bottom, so a diff shows clearly whether a change was to the model or only to the picture.
- An element with no line in the `layout` block gets placed automatically when the file opens. A file with no `layout` block at all gets a full Tidy. This makes hand-written files work.
- Comments start with `#` and are kept. A comment belongs to the declaration that follows it and moves with it.
- On save, the app writes the file in one standard style (as `rustfmt` does for Rust). Declarations keep the order they had.
- Anything the parser does not understand is kept as raw text and written back unchanged, and is listed in a Problems panel with line and column. A file with problems still opens.

### 5.3 Opening and saving

- Save writes to a temporary file in the same folder and then renames it over the original, so a crash never leaves a half-written file.
- Unsaved work is copied to a recovery file every 30 seconds and offered back after a crash.
- If the file changes on disk while open, the app says so and offers to reload or keep the current version. It never overwrites silently.
- The title bar shows the file name and an unsaved-changes mark. Closing with unsaved changes asks first.
- Recent files are listed on the start screen. Files can also be opened by drag and drop or as a command-line argument.

## 6. Layout: how diagrams stay beautiful

This is the hardest and most important part of the product.

### 6.1 Approaches considered

| Approach | Crossings | Fit for class diagrams | Effort |
|---|---|---|---|
| A. Layered placement plus obstacle-avoiding orthogonal router | Good | Strong: inheritance naturally forms layers | Medium |
| B. Planarization (make the graph planar first, then straighten and compact) | Best | Good, but box sizes and inheritance direction are hard to respect | High; needs min-cost-flow and planarity code |
| C. Force-directed (springs) | Poor | Weak: looks organic, ignores hierarchy | Low |
| D. Call an existing engine such as Graphviz | Good | No live rerouting while dragging; adds an external program | Low |

**Chosen: A.** It gives parents above children, which is what readers of class diagrams expect, and it splits cleanly into a placement step and a routing step. The routing step is also what runs while the user drags a box, so one router serves both auto layout and manual editing. B could replace the placement step later without touching anything else. I did not survey existing Rust layout crates in detail; the builder should check them before writing the layered step from scratch.

### 6.2 An honest limit

Finding the true minimum number of crossings is a problem with no known fast exact method. The app uses strong heuristics and tries several candidates. It gets the minimum on small diagrams in practice and close to it on large ones, but it cannot prove a result is minimal. Section 10 says how this is measured.

### 6.3 Where crossings are removed

Crossings are fought at five separate points, not one:

| Step | What it decides | How it reduces crossings |
|---|---|---|
| 1. Order of boxes in each layer | Left-to-right order | Repeated sweeps that move each box toward the average position of its neighbours, plus swaps of adjacent boxes whenever a swap lowers the count |
| 2. Side and order of attachment points | Where each line leaves its box | Lines leave in the same order as their targets lie, so lines from one box do not cross each other |
| 3. Path of each line | Route around boxes | Path search where crossing another line costs much more than a bend or extra length |
| 4. Reroute pass | Fixing early mistakes | Any line that still crosses another is removed and routed again with all other lines in place, until nothing improves |
| 5. Pick the best candidate | Which whole layout to keep | Several layouts are produced from different starting orders; the one with the best score wins |

### 6.4 Placement pipeline

Input: boxes with sizes, edges with kinds, groups, and optionally some fixed positions.

1. **Split.** Handle each connected part of the diagram separately and pack the parts together at the end. Lay out the content of each package first, then treat the package as one large box in its parent.
2. **Direct the edges.** Generalization and realization always point up to the parent. Composition and aggregation prefer the whole above the part. Associations and dependencies have no fixed direction.
3. **Assign layers.** Only the hierarchy edges force a box into a lower layer. Associations only pull their two ends close together, so associated classes tend to land side by side in the same layer. This gives the classic look: inheritance runs vertically, associations run horizontally. Layers are chosen to make the weighted total edge length as small as possible (network simplex method). If a layer becomes too wide for a target shape of about 16:10, some associations are promoted to span layers so the diagram grows downward.
4. **Order each layer.** Step 1 in the table above. Children of the same parent are kept next to each other so their inheritance lines can share one trunk.
5. **Assign coordinates.** Place boxes so long lines run straight and parents sit centred over their children (Brandes–Köpf method), respecting real box sizes and minimum gaps. Snap everything to the grid.
6. **Route.** Section 6.5.
7. **Place labels.** Multiplicities and role names sit next to their line end; a relation name sits on the longest segment of its line. Each label picks, from a few candidate spots, the first that overlaps nothing.

### 6.5 The router

All lines are orthogonal: horizontal and vertical segments only, with slightly rounded corners.

- The router searches a grid of candidate lanes that run along box edges and midway between boxes. It finds the cheapest path, where cost is length plus a penalty per bend plus a large penalty per crossing. Boxes and labels are obstacles.
- Lines are routed in a fixed order: inheritance first, then the rest from shortest to longest. Then the reroute pass (step 4 in the table) runs.
- Inheritance lines to the same parent merge into one tree: a single stem out of the parent's bottom edge, one horizontal bar, and a drop to each child.
- A final pass spreads apart lines that share a lane, ordered so they do not cross where they turn.
- Lines always meet a box edge at a right angle and never at a corner.
- Where a crossing cannot be avoided, one of the two lines gets a small gap so the eye follows each line correctly.

### 6.6 Layout while editing

Auto layout must not fight the user. Four modes cover it:

| Mode | When | What moves |
|---|---|---|
| Tidy all | The Tidy command; opening a file with no `layout` block | Everything. The current left-to-right order is the starting order, so a diagram that is already good changes little |
| Tidy selection | Tidy with boxes selected | Only the selected boxes; the rest are fixed obstacles |
| Place new | A new box is created, or a box has no stored position | Only the new box. It goes to the free spot near its neighbours that adds the fewest crossings |
| Reroute | A box is dragged or resized | No boxes. Lines touching the moved box are rerouted every frame; on drop, the spreading pass runs for the area around it |

After Tidy, boxes glide from old to new positions over about 200 ms so the user can follow what changed. Tidy is one undo step.

### 6.7 The quality score

One number, lower is better, used to pick between candidates and to test the engine.

Hard rules (a layout that breaks one is rejected):

- No two boxes overlap. Minimum gap between boxes is kept.
- No line passes through a box or a label.
- Every box sits inside its package.

Soft costs, with starting weights to be tuned on the test corpus:

| Cost | Weight |
|---|---|
| Each line crossing | 10 |
| Each inheritance line that points down or sideways | 20 |
| Each bend | 1 |
| Line length, per grid unit | 0.1 |
| Area and distance from the target shape | small |

The status bar shows the current crossing count, so the user can see whether Tidy would help.

## 7. Visual style

Clean layout is most of beauty. The rest is consistent drawing.

- **Grid.** Everything sits on an 8-pixel grid: box corners, box sizes, and line lanes.
- **Boxes.** 1.5-pixel border, 4-pixel corner radius, name centred and bold, members left-aligned in a monospace-friendly sans font. Abstract names are italic. Stereotypes sit above the name in a smaller size. Empty compartments are hidden.
- **Lines.** 1.5 pixels, corner radius 6 pixels, standard UML arrowheads at one fixed size.
- **Spacing.** At least 5 grid units between boxes and 10 between layers; a layer gap grows when many lines pass through it.
- **Colour.** A light and a dark theme. One accent colour for selection. Packages get a faint tint and a tab with the name. Interfaces and enums get a small coloured mark in the header rather than a different fill, so the diagram stays calm.
- **Zoom.** Text and lines are redrawn sharp at every zoom level, never scaled as an image. Below about 40% zoom, members are hidden and only names show.

## 8. The editor window

```
┌───────────────────────────────────────────────────────────────────────┐
│ File  Edit  View  Arrange                [Tidy] [Fit]   100%   Theme  │
├─────┬──────────────────────────────────────────────┬──────────────────┤
│     │                                              │ Order            │
│ Sel │      ┌─────────────┐        ┌───────────┐    │ ──────────────── │
│ Cls │      │   Payment   │        │ Customer  │    │ Kind    class    │
│ Ifc │      └──────△──────┘        └─────▲─────┘    │ Abstract  [ ]    │
│ Enm │             │                     │          │ Stereotype       │
│ Pkg │      ┌──────┴──────┐        ┌─────┴─────┐    │                  │
│ Rel │      │ CardPayment │        │   Order   │    │ Attributes       │
│ Note│      └─────────────┘        └───────────┘    │  +id: OrderId    │
│     │                                              │  -lines: List<…> │
│     │                                              │ Operations       │
│     │                                              │  +total(): Money │
├─────┴──────────────────────────────────────────────┴──────────────────┤
│ shop.umpl •   12 classes   9 relations   0 crossings   Problems: 0    │
└───────────────────────────────────────────────────────────────────────┘
  tools                    canvas                         inspector
```

Interaction:

- **Create.** Pick a tool and click, or double-click empty canvas for a class. The name is in edit mode at once.
- **Edit text in place.** Double-click a box and type members as text lines (`+total(): Money`), in the same syntax as the file. Enter adds a line, Escape leaves. The inspector on the right edits the same data as form fields.
- **Connect.** Hover a box to see handles on its edges; drag from a handle to another box, then choose the relation kind from a small popup. Dropping on empty canvas creates a new class there.
- **Select and move.** Click, shift-click, or drag a rectangle. Dragging snaps to the grid and shows guide lines when edges or centres line up with other boxes.
- **View.** Scroll to pan, Ctrl-scroll to zoom toward the pointer, a key to fit the whole diagram.
- **Undo.** Every change is undoable with no limit within a session. A drag is one step, not one per frame.
- **Keyboard.** Standard shortcuts for file and edit commands, single keys for tools, Ctrl-T for Tidy, and a command palette (Ctrl-K) that lists everything.
- **Export.** SVG and PNG to file; copy the selection to the clipboard as an image.

Undo is built on edit operations: each operation in `umpl-core` knows how to reverse itself, and a user action is a group of operations. The app never changes the document any other way, which also gives one place to mark the file as changed and to tell layout what needs redoing.

## 9. Speed

Targets, to be measured on a mid-range laptop:

| What | Target |
|---|---|
| Start to window visible | under 0.3 s |
| Pan and zoom, 500 boxes and 1000 lines | 60 frames per second |
| Tidy all, 100 boxes | under 0.1 s |
| Tidy all, 500 boxes | under 1 s |
| Reroute while dragging one box | under 4 ms per frame |
| Open and show a 500-box file with stored positions | under 0.2 s |

How:

- Layout runs on a background thread and can be cancelled. The window never freezes. Candidate layouts (step 5 in section 6.3) run in parallel and stop at a time budget.
- Measured text and box sizes are cached and recomputed only when the content of that box changes.
- The shape list is rebuilt only for what changed, and shapes outside the visible area are skipped when painting.
- Dragging reroutes only the lines attached to the moved box.

Measured on 2026-11-16 (release build, 32-core machine; layout candidates run in parallel):

| What | Target | Measured |
|---|---|---|
| Tidy, 30-class example (banking) | — | 50 ms |
| Tidy, 100 generated classes | under 0.1 s | 0.25–0.5 s |
| Tidy, 500 generated classes | under 1 s | 6–13 s |

Placement takes a few milliseconds; nearly all the time is line routing on the full grid. Meeting the targets needs a sparse routing graph (lanes only at box edges, ports and channel midpoints) instead of every grid point. Tidy runs on a background thread, so the editor stays responsive meanwhile.

## 10. Testing

- **File round trip.** Reading a file and writing it gives the same document. Writing twice gives identical bytes. Tested with generated random documents as well as examples.
- **Layout rules.** For random graphs: no overlaps, no line through a box, all lines orthogonal, everything on the grid, same input gives the same output.
- **Layout quality.** A corpus of real-looking diagrams (from 5 to 500 classes) with a recorded score for each. A change that makes any score worse fails the build until someone accepts the new numbers. For diagrams of up to about 10 classes, tests also search every possible layer order and check that the engine found the best crossing count.
- **Pictures.** SVG output of the corpus is stored and compared, so visual changes show up in review.
- **Speed.** Benchmarks for the targets in section 9.
- **Editor.** Edit operations and undo are tested without a window. The window layer stays thin.

## 10a. Distribution

| Platform | Package | Contents |
|---|---|---|
| Linux x86-64 | `.deb` | `fastumpl` (editor) and `umpl` (CLI) in `/usr/bin`, desktop entry, icon, `.umpl` file type |
| Windows x86-64 | Portable `.zip` | `fastumpl.exe`, `umpl.exe`, readme, examples. No installer |
| Windows ARM64 | Portable `.zip` | Same, built for ARM64 |

Portable means nothing is written outside the unpacked folder: on Windows, recent files and recovery copies live in a `data` folder next to `fastumpl.exe`. If that folder cannot be written (for example under Program Files), the app falls back to `%APPDATA%\fastumpl`. On Linux the app uses `~/.config/fastumpl` and `~/.cache/fastumpl`.

Release asset names have no version in them, so the website can always link to `releases/latest/download/<name>`:
`fastumpl-linux-x86_64.deb`, `fastumpl-windows-x86_64-portable.zip`, `fastumpl-windows-arm64-portable.zip`.

GitHub Actions:

- **CI** on every push and pull request: format check, clippy, tests on Linux; a build on Windows.
- **Release** on a `v*` tag: builds the three packages (the ARM64 one is cross-compiled on an x86-64 Windows runner) and attaches them to a GitHub release.
- **Website** on pushes to `main`: a static page in `site/` published to GitHub Pages. The workflow renders the diagrams in `examples/` with the CLI, so the gallery on the site always shows the real current layout.

## 11. Build order

The riskiest part (layout quality) comes early, and it can be judged before any editor exists because the CLI can produce pictures.

1. **Document and file.** `umpl-core`, `umpl-text`, `umpl fmt` and `umpl check`.
2. **Layout and pictures.** `umpl-layout`, `umpl-draw`, `umpl render`. Build the corpus. Tune until the pictures are good. This milestone decides whether the product works.
3. **Viewer.** Window that opens a file, pans, zooms, and runs Tidy.
4. **Editor.** Create, edit, connect, move with live rerouting, undo, save.
5. **Finish.** Inspector, themes, export, recovery file, problems panel, command palette.

## 12. Open decisions

| Decision | Current assumption | If it changes |
|---|---|---|
| What "UMPL" means | UML class diagrams | If it means Umple (`.ump` files from umple.org), only `umpl-text` is replaced by an Umple reader and writer, and positions go in a side file because `.ump` belongs to another tool. Model, layout, drawing and editor stay |
| File format | Own text format | A PlantUML importer and exporter can be added in `umpl-text` without other changes |
| Next diagram type | Not decided | State diagrams reuse the layout engine as it is. Sequence diagrams need a different, much simpler layout and a new drawing module |
| Hand-adjusted line routes | Not supported | Would add waypoints to the `layout` block and a "must pass through" rule to the router |
| Source text pane beside the canvas | Later | Needs the parser to report positions for every element so text and canvas selection can follow each other |
| Routing speed | Grid A* (see section 9) | A sparse routing graph would make large diagrams 5–10× faster |

## 13. Left to the builder

- Exact crate versions and the parser library.
- The weights in the quality score, tuned on the corpus.
- The exact lane grid used by the router and its data structures.
- Colours and the bundled font.
- Shortcut keys beyond the ones named here.

## 13. Where the build differs

Decisions made while building, for the record:

- **Notes** are elements of kind Note (`note N1 "text"`), and attaching one is a relation `N1 .. Order`. This puts notes in the `layout` block like any box.
- **End labels** are one quoted string per end, `"1..* items"`: multiplicity first, then role.
- **Unknown plain names** used only in a relation create a class, as PlantUML does. An unknown dotted name keeps the line as raw text with a problem.
- **Undo** uses set-slot operations: each op replaces one element, relation, package, the root list or the title, and returns the old value.
- **Lane spreading** is done by the router itself: sharing a lane costs as much as a crossing, except inheritance lines into one parent, which form a tree (stem, bar, drops) whenever that path is clear.
- **Labels** avoid boxes, lines and each other; lines do not avoid labels. Boxes joined by a labelled line in the same row get extra room for the labels.
- **Font**: DejaVu Sans Mono in four faces, bundled. Abstract names and operations are italic; static members are underlined.
- **Crossing reduction** adds sifting and random restarts to the barycenter sweeps. An exhaustive test checks that the true minimum is found on small two-layer graphs.
- **Package placement**: packages are laid out inside-out, and each finished package is one box in its parent. Lines leaving a package do not yet influence the order inside it.
- **Development** happens in a Docker image (Ubuntu 26.04, Rust 1.91) through `scripts/dev.sh`. Interaction tests drive the real editor with egui_kittest.
