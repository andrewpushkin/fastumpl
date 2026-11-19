# fastumpl: design

Status: version 1 implemented, 2026-11-16. Section 13 lists where the build differs from this design. PlantUML import and PlantUML/Mermaid export were added on 2026-11-17 (`docs/plans/2026-11-17-plantuml-mermaid.md`), and Umple import and export the same day (`docs/plans/2026-11-17-umple.md`). Real-world PlantUML files (includes, preprocessor, other diagram kinds), big diagrams that fill the window, the text panel linked to the selection, and Find were added on 2026-11-18 (`docs/plans/2026-11-18-big-plantuml.md`).

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

- Elements, each with a kind (class, abstract class, interface, enum, note, or the diamond of an association class), a name, optionally a label (the text the box shows when it differs from the name), type parameters and a stereotype, an ordered member list, and the package it belongs to.
- Relations, each with a kind, two ends (an element or a package; role name, multiplicity, navigability), and an optional name.
- Packages, forming a tree.
- Notes, each with text and the element it is attached to.
- A position for each element (top-left corner, in whole grid units). A position may be missing.

Line routes are never stored, and box sizes only as a minimum the user resized a box to (the layout block's `size`); otherwise both are always computed, which is why the same file always gives the same picture. Text that a font would draw wider than measured is drawn a little smaller, so it always stays inside its box.

Names are unique inside a package, and the full name (`billing.Invoice`) identifies an element in the file. A name may be any text: one that is not a plain word is quoted in paths (`billing."Order line"`). Renaming an element updates every relation and position that refers to it. Labels let several boxes show the same text (PlantUML's `class "Order" as O1` and `class "Order" as O2` are two classes): the names stay unique, the labels need not.

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

- Relation arrows: `--|>` generalization, `..|>` realization, `--` association, `-->` directed association, `o--` aggregation, `*--` composition, `..>` dependency, `+--` nesting (an inner class), `..` a note's or association class's dashed line. Either end may name a package.
- Declarations: `class "Order line"<T> as "Line of an order" <<entity>> #lightblue { … }`: a quoted name, type parameters, a label after `as`, a stereotype, a fill colour (a CSS name, `#rgb`, `#rrggbb` or `transparent`; packages and notes take one too). `diamond A` is the point an association class hangs from.
- A layout line may carry the size a box was resized to: `Order at 24, 8 size 30, 12`. Sizes, like positions, are layout: the box is at least that big, and still as big as its text needs. Box sizes are otherwise computed, never stored.
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
- A file without positions that holds more than 150 elements or 300 relations opens at once and is laid out on a background thread; the canvas stays empty and the status bar says "Laying out N classes…" until the layout arrives. That first layout is neither an undo step nor an unsaved change. Escape stops it early, keeping the first candidate (the layout cannot be skipped: there would be nothing to show).

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

Crossings are fought at six separate points, not one:

| Step | What it decides | How it reduces crossings |
|---|---|---|
| 1. Order of boxes in each layer | Left-to-right order | Repeated sweeps that move each box toward the average position of its neighbours, swaps of adjacent boxes whenever a swap lowers the count, sifting, and random restarts. Lines attach to a box at a *port*, a fraction of its width, so lines into one package box are ordered by the member they reach |
| 2. Packages and their outside lines | Order inside each package | A line that leaves a package becomes a fixed stand-in on the package side it leaves through (top, bottom, left, right), so the package's own order counts the crossings that line will make. See 6.4 |
| 3. Side and order of attachment points | Where each line leaves its box | Lines leave in the same order as their targets lie. A parent's other lines leave beside its inheritance bar, not through it |
| 4. Path of each line | Route around boxes | Path search where a crossing costs as much as 100 grid units of extra length, the same trade the quality score makes (6.7) |
| 5. Reroute pass | Fixing early mistakes | Any line that still crosses another is removed and routed again with all other lines in place. On the final layout this search also looks three times further out for a detour that avoids the crossing |
| 6. Pick the best candidate | Which whole layout to keep | Several layouts are produced from different starting orders and different layer widths; the one with the best score wins |

### 6.4 Placement pipeline

Input: boxes with sizes, edges with kinds, groups, and optionally some fixed positions.

1. **Split.** Handle each connected part of the diagram separately and pack the parts together at the end. Lay out the content of each package first, then treat the package as one large box in its parent. Laid out alone, a package cannot know where its outside lines go, which caused most crossings in diagrams with packages. So the whole placement runs again twice: each round, every package gets a stand-in for each outside line, placed where the line's other end was in the round before, and the parts of a package that are not connected to each other are placed left to right by where their outside lines go. Mathematically this optimises one package's order at a time with everything else held fixed. The parts at the top level are packed in rows to fill the view: of 48 row widths, the one whose drawing needs the least zoom to fit a view of the target shape, `max(w, h · aspect)`, then the smallest area. Packing for the smallest area alone, as before, put many small parts in one long row, since a second row costs a whole gap between rows; a diagram of 100 unconnected classes opened as a thin strip. Inside a package the parts still pack for a compact package box, unless that box would be a strip of more than 8 parts longer than 6:1 (58 classes implementing an interface outside their package came out 40:1); then they too pack to fill. Below 6:1 a strip is kept: the parts are ordered left to right by where their outside lines go, and wrapping them into rows makes those lines cross (wrapping at 3:1 raised the 200-class test diagram from 211 to 263 crossings).
2. **Direct the edges.** Generalization and realization always point up to the parent. Composition and aggregation prefer the whole above the part. Associations and dependencies have no fixed direction.
3. **Assign layers.** Only the hierarchy edges force a box into a lower layer. Associations only pull their two ends close together, so associated classes tend to land side by side in the same layer. This gives the classic look: inheritance runs vertically, associations run horizontally. Each box in turn moves to the layer its hard rules allow that makes its weighted edge length smallest, until nothing moves. If a layer becomes too wide for the target shape (the editor's canvas, width : height; 16:10 for the command line), some boxes are pushed down so the diagram grows downward. Boxes without a parent move first. When none can, children move too: inheritance only needs a child below its parent, not right below it, so sixty subclasses of one class fold into rows instead of one row as wide as all of them (width-bounded layering, as in Coffman and Graham's method). Moving a child stretches its inheritance line and splits its row of siblings, so inside a package, whose target comes from its own few boxes, children move only when a layer is twice too wide. Fewer, wider layers do not mean fewer crossings: removing this width limit cut long lines sharply but raised crossings by a third, because crowded layers cross more. How the boxes spread over layers changes the result more than any later step, so candidates try narrower and wider targets (0.6 to 1.7 times the usual).
4. **Order each layer.** Step 1 in the table above. Children of the same parent are kept next to each other so their inheritance lines can share one trunk.
5. **Assign coordinates.** Place boxes so long lines run straight and parents sit centred over their children (Brandes–Köpf method), respecting real box sizes and minimum gaps. Snap everything to the grid.
6. **Route.** Section 6.5.
7. **Place labels.** Multiplicities and role names sit next to their line end; a relation name sits on the longest segment of its line. Each label picks, from a few candidate spots, the first that overlaps nothing.
8. **Compete.** Several candidates (different starting orders and layer widths) go through steps 3–7 in parallel, and the best wins.
9. **Fit the view** (when chosen: Arrange → Filling the window, `umpl --fill`; the default favours the fewest crossings and skips this step). The drawing should fill the canvas it opens in, not leave part of it blank. Before the candidates, the layer width is calibrated: placed at the usual width, the drawing's width : height is measured, and the width is scaled by the square root of what is missing (with the area fixed, width : height grows as the square of the layer width), up to three times while it gets closer. The first candidate keeps the usual width, the next takes the calibrated one and the third the width halfway; the winner is chosen by score plus a cost for not filling the view: leaving half of it blank costs as much as 3 crossings, a third more than the fewest any candidate has, and one more per 24 lines. A package that is all there is at its level (a diagram drawn inside one package) fills the view too; other packages stay compact.

### 6.5 The router

All lines are orthogonal: horizontal and vertical segments only, with slightly rounded corners.

- The router searches a grid of candidate lanes that run along box edges and midway between boxes. It finds the cheapest path, where cost is length plus a penalty per bend plus a large penalty per crossing (as much as 100 grid units of length). Boxes and labels are obstacles.
- The search first looks within 14 grid units of the two boxes. Where that only offers a crossing route, the thorough mode looks three times as far, keeping only a route that is cheaper. The thorough mode runs on the winning Tidy layout, when a file opens, and when a drag ends; candidates and drag frames use the quick mode.
- Lines are routed in a fixed order: inheritance first, then the rest from shortest to longest. Then the reroute pass (step 5 in the table) runs.
- Inheritance lines to the same parent merge into one tree: a single stem out of the parent's bottom edge, one horizontal bar, and a drop to each child.
- A final pass spreads apart lines that share a lane, ordered so they do not cross where they turn.
- Lines always meet a box edge at a right angle and never at a corner.
- Where a crossing cannot be avoided, one of the two lines gets a small gap so the eye follows each line correctly.

The router's tables grow with the area of the drawing (about 90 bytes per grid point), and in a dense drawing its searches flood their window looking for a crossing-free detour that is not there: the estimate is the plain distance, while a crossing costs as much as 100 grid units. Measured on a 644-class, 1347-line C++ template diagram (15 million grid points), routing took 7 minutes. So:

- Every routing pass may spend 100 million search states (a few seconds) on exact searches; the final polish a quarter of that. Sparse drawings never come near it (the 972-class diagram below routes in about a second); in a dense one, once it is spent, the remaining searches are bounded as in a large drawing. A budget per search does not work: dense and sparse searches look alike until they have run, and a search cut short in a sparse drawing loses a good route (tried: shared lanes in the 972-class diagram went from 0 to 582).
- Above 3 million grid points a drawing is large: the search estimate is inflated three times (weighted A*: routes may be a little longer than the cheapest, the search goes straight at the target) and each search may visit 40,000 states in its window and 250,000 over the whole grid, then takes a draft line. The wider look and the final polish pass are skipped.
- Above 300 lines, only one candidate is routed: every candidate is placed (cheap), and the one whose straight centre-to-centre lines cross least is routed.
- Candidates route in parallel only as many at a time as fit in about 1.5 GB of tables.
- Above 12 million grid points lines are drafts: straight between boxes that face each other, otherwise one bend, meeting box edges at right angles but not going around boxes. `Stats::draft` says so. The dense diagram above opens in 0.6 s; routing it in the bounded mode took 40 s and crossed as much.

All limits count states and grid points, not time, so a layout stays the same on every machine.

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

### 6.8 What was measured

Changes to the engine were judged on 52 generated class diagrams of 40 to 150 classes, with and without packages, by their total crossing count, because single diagrams vary too much from small changes to show a real effect. Against the engine before these changes (November 2026), the total fell from 4942 to 3414 crossings (−31%), and banking from 3 to 0. Tidy of a 40-class diagram takes about 130 ms.

On 2026-11-18 the layout was made to fill the view (packing of loose parts, section 6.4 step 1, and children folding into rows, step 3). On the same 52 diagrams, extended into a test (section 10): 2808 → 2798 crossings, score 83161 → 83053, median width : height of the drawings 2.60 → 2.07, and drawings wider than 3:1 21 → 15. Sixty subclasses of one class went from a 34:1 strip to 4:1; 80 unconnected classes from one row to 1.6:1. The folded rows are still ragged (boxes of one row at a few heights, since compaction lets each rise until it meets a box above it); placing a large family of siblings as a grid would look cleaner.

On 2026-11-19 the drawing as a whole was made able to take the shape of the view (section 6.4, step 9; a setting, since it costs crossings: the default stays the fewest crossings, and the quality corpus records both, the second as `name+fill`). A drawing whose layers fill up to the old width limit, 1.8 times the side of a square of its area, comes out about 1.8² ≈ 3 times wider than the view; measured on 1725 PlantUML corpus files laid out for a 16:10 view, the median drawing was 1.96:1 and filled 70 % of the view, and only a third filled 80 % or more. Now: median 1.77:1 filling 77 %, 800 instead of 574 files filling 80 % or more (diagrams of four boxes or more: median 86–88 %), for 0.9 % more crossings in all. Most drawings still far from the view's shape have one to three boxes, which fit-to-view zooms in on only to 200 % anyway. In the quality corpus, banking and library now fill their views (1.03 → 1.48, 2.21 → 1.67) with the same crossings; the 200-class diagram goes from 2.38:1 to 1.53:1 at 211 → 257 crossings.

Tried and rejected, so they need not be tried again:

- **A truer cost for same-layer lines** (counting the lines of the boxes they pass instead of the boxes): no gain overall, and worse on small diagrams with packages.
- **Minimum total line length for layers** (fewer layers, fewer long lines): about a third more crossings, as layers get crowded.
- **More restarts for small diagrams:** lower ordering cost, but no fewer crossings once routed. The ordering's count is only a model of the drawing.
- **A smaller wider-search area, or a limit on its steps:** saved little time and lost much of its effect.
- **Packing every scope to fill the view**, including the parts inside packages: banking's score rose from 199 to 221 and the 200-class diagram gained crossings. A package box should stay compact; only the top level fills the view.
- **Lines from one box sharing their stand-ins in the layers between** (a trunk, so that a long row of children folds into rows without one stand-in per line in each row): 1 more point of median fill, but 3.7 % more crossings on the PlantUML corpus.
- **Weighing the fit more** (half the view blank worth half again the crossings instead of a third): 13 more of 1725 files filling 80 %, for 2.4 % more crossings.
- **Folding children inside packages at the usual width:** small families of three or four subclasses split into rows for no gain (banking 199 → 220, same crossings).
- **Bounded routing by line count (over 300 lines):** the 972-class diagram, which routes fully in 1.7 s, got 1692 shared lanes. Only the grid size predicts the cost.

## 7. Visual style

Clean layout is most of beauty. The rest is consistent drawing.

- **Grid.** Everything sits on an 8-pixel grid: box corners, box sizes, and line lanes.
- **Boxes.** 1.5-pixel border, 4-pixel corner radius, name centred and bold, members left-aligned in a monospace-friendly sans font. Abstract names are italic. Stereotypes sit above the name in a smaller size. Empty compartments are hidden.
- **Lines.** 1.5 pixels, corner radius 6 pixels, standard UML arrowheads at one fixed size.
- **Spacing.** At least 5 grid units between boxes and 10 between layers; a layer gap grows when many lines pass through it.
- **Colour.** A light and a dark theme. One accent colour for selection. Packages get a faint tint and a tab with the name. Each kind of box (class, abstract class, interface, enum) has its own colour: the box is filled with a pale tint of it (12% in the light theme, 18% in the dark), and a lettered badge in that colour sits in the header, so the kind does not depend on colour alone. The tint is blended into an opaque fill, so the canvas grid never shows through behind text.
- **Zoom.** Text and lines are redrawn sharp at every zoom level, never scaled as an image. Below about 40% zoom, members are hidden and only names show.

## 8. The editor window

```
┌───────────────────────────────────────────────────────────────────────┐
│ File  Edit  View  Arrange  Help      ⟲ ⟳ │ − 100% + │ [Fit] [Tidy] │
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
- **View.** Scroll to pan, Ctrl-scroll to zoom toward the pointer, a key to fit the whole diagram. Scroll bars along the bottom and right edges span the diagram plus a margin, extended to whatever is in view, since the canvas has no edge; the − and + buttons zoom around the centre, and clicking the percentage returns to 100 %.
- **Tools.** The tool buttons on the left are drawn icons of what they create: a class box with the coloured dot of its kind (as on the canvas), with an italic name for abstract, a lollipop for interface and a list for enum; a folded note; a folder for a package; a line with an arrowhead for relations.
- **Text panel.** An optional panel between the tools and the canvas (View → Text, Ctrl+B; off by default) shows the model as `.umpl` text without the `layout` block, and both sides edit the same document; see below.
- **Undo.** Every change is undoable with no limit within a session. A drag is one step, not one per frame.
- **Keyboard.** Standard shortcuts for file and edit commands, single keys for tools, Ctrl-T for Tidy, and a command palette (Ctrl-K) that lists everything.
- **Export.** File → Export holds every export: SVG, PNG, PlantUML, Mermaid and Umple to file, and copying as an image or as Mermaid.
- **Find.** Ctrl+F opens a bar under the menu. It matches, ignoring case, element names, stereotypes and members, relation names and end labels, and package names. Every match is outlined in orange on the canvas and every occurrence of the text is tinted in the text panel; Enter and Shift+Enter (F3, Shift+F3) select the next or previous match and bring it into view, which also scrolls the text to its declaration.
- **Fit.** Opening a diagram, and the F key, fit it to fill the canvas, zooming in up to 200 % for a small one and out to 5 % for a huge one.

Undo is built on edit operations: each operation in `umpl-core` knows how to reverse itself, and a user action is a group of operations. The app never changes the document any other way, which also gives one place to mark the file as changed and to tell layout what needs redoing.

### 8.1 Text and drawing in step

The text panel and the canvas edit one document; neither is a copy of the other.

- **Text to document.** The parser builds a fresh document with fresh ids. Swapping it in would lose positions, the selection and undo, which all refer to ids, so `umpl_core::reconcile` maps the new items onto the current ones and emits ordinary edit operations for the difference. Packages match by full name; elements by package and name, then by name alone (moved between packages); relations by ends and kind. Whatever is left over is paired in declaration order when both sides have the same number left, which reads a single changed name as a rename rather than a delete and an add. An element the text removed is remembered by full name with its id and position, so a class that vanishes while its name is retyped returns to the same place. The operations go in as one merged undo step ("Edit text"), sealed when typing pauses for two seconds or the panel loses focus.
- **When to apply.** The text is applied 0.25 s after the last keystroke, before any command runs, and when the panel loses focus. Text with an unclosed `{` is held back: the parser would read every following line as members of that class, and the diagram would lose those classes until the brace is typed.
- **Linked to the selection.** The parser records, for each element, relation and package, the lines that declare it (to its closing `}`). The text has ids of its own, so items are matched with the document's by key: elements and packages by full name, relations by their ends, kind and which of the relations with those is meant (`app/link.rs`). Selecting an item in the drawing tints its lines blue, scrolls them into view and moves the text cursor there without taking the keyboard, so Delete still deletes the box. Moving the cursor in the text selects the innermost item declared on that line and pans the canvas to it if it is out of view; a line that declares nothing leaves the selection as it is.
- **Document to text.** The panel shows `print_model`, the file without its `layout` block. After any revision the model text is printed again and compared with the one the panel last showed or applied; only when they differ is the panel's text replaced. Moving boxes, Tidy and placement of new boxes therefore never touch the text, and text the user typed stays as typed (not reformatted) until the model changes elsewhere.

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

Measured on 2026-11-18 on real PlantUML files (release build, same machine), with the bounds of section 6.5:

| File | Classes, lines | Before | After |
|---|---|---|---|
| bgembalczyk/F1 `uml_all_classes.puml` | 972, 536 | 1.7 s | 1.5 s, same layout |
| LeDatPhucAn/CS202 `game_diagram.puml` (dense C++ templates) | 644, 1347 | 427 s | 0.6 s, draft lines |
| lumin3000/rwsource `include.puml` | 5237, 11575 | out of memory | 36 s, draft lines |

## 10. Testing

- **File round trip.** Reading a file and writing it gives the same document. Writing twice gives identical bytes. Tested with generated random documents as well as examples.
- **Layout rules.** For random graphs: no overlaps, no line through a box, all lines orthogonal, everything on the grid, same input gives the same output.
- **Layout quality.** A corpus of real-looking diagrams (from 5 to 500 classes) with a recorded score for each. A change that makes any score worse fails the build until someone accepts the new numbers. For diagrams of up to about 10 classes, tests also search every possible layer order and check that the engine found the best crossing count.
- **Pictures.** SVG output of the corpus is stored and compared, so visual changes show up in review.
- **Speed.** Benchmarks for the targets in section 9.
- **Editor.** Edit operations and undo are tested without a window. The window layer stays thin.
- **Real PlantUML files.** `crates/umpl-draw/tests/puml_corpus.rs` opens every PlantUML file in given folders as the editor does and fails on a panic, an overlap or a file that takes more than a minute; it reports problems by kind, diagram kinds, drawing shapes and the slowest files. The corpus is about 8000 files over 5 KB from 4500 public GitHub repositories with the 6000 files they include (`scripts/puml-corpus.py`, pinned in `tests/puml-corpus/manifest.tsv`), plus three sample repositories in the dev image.
- **Layout benchmark.** `cargo test --release -p umpl-draw --test corpus -- --ignored benchmark`: the 52 generated diagrams of section 6.8, total crossings, score and drawing shapes.

## 10a. Distribution

| Platform | Packages | Built by |
|---|---|---|
| Linux x86-64 | `.deb` (`/usr/bin/fastumpl`, `/usr/bin/umpl`, desktop entry, icon, `.umpl` file type) and a portable `.tar.gz` | `make linux-x86_64-packages` (or `-deb`) in the Ubuntu 22.04 image (glibc 2.35) |
| Windows x64 | Inno Setup 7 installer and a portable `.zip` | `make windows-x86_64-packages` (MinGW; Inno Setup under Wine) |
| Windows ARM64 | Same as x64 | `make windows-arm-packages` (llvm-mingw) |
| macOS 11+ | Universal `.dmg` (Apple Silicon and Intel) | macOS runner in CI; `make mac` in Docker only with your own macOS SDK |

All `make` targets run inside the Docker image. A plain platform target (`make windows-x86_64`) only compiles; the `-packages` targets make the archives and installers; `make clean` removes all build output. The installer lets the user install per user or for all users, adds a Start menu entry, optionally a desktop icon, the `.umpl` file type and `umpl` on PATH. The portable zip ships a `data` folder; when that folder exists next to `fastumpl.exe`, recent files and recovery copies go there, otherwise to `%APPDATA%\fastumpl`. On Linux and macOS the app uses `~/.config/fastumpl` and `~/.cache/fastumpl`.

The macOS SDK may only be used on Apple hardware under Apple's license, so it is not part of the image; release `.dmg` files are built on GitHub's macOS runners.

Versions are `0.1.N`, where N is the number of commits on `main` (`scripts/version.sh`). Builds pass it as `UMPL_VERSION`, so the source tree is not modified.

GitHub Actions run only for commits on `main`; pull requests and other branches do not trigger them:

- **CI**: format check, clippy, tests on Linux; tests on Windows.
- **Release**: `make all` in Docker on a Linux runner, the `.dmg` on a macOS runner, then a GitHub release `v0.1.N` with all packages and checksums.
- **Website**: the page in `site/` with a gallery rendered from `examples/`, published to GitHub Pages.

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
| What "UMPL" means | UML class diagrams, with Umple as an import and export format | Done for the class model: `umpl-text` reads and writes `.ump` (`docs/plans/2026-11-17-umple.md`). `.umpl` stays the saved format, so Umple files are never overwritten. Umple state machines would need the state diagram type below |
| File format | Own text format | Done: `umpl-text` imports PlantUML and exports PlantUML and Mermaid (`docs/plans/2026-11-17-plantuml-mermaid.md`). A Mermaid importer would go in the same place |
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
- **Undo** uses set-slot operations: each op replaces one element, relation, package, the root list or the title, and returns the old value. Each step carries a name ("Move Order", "Delete 3 items", "Tidy") shown in the Edit menu, the palette, the ⟲ ⟳ toolbar tooltips and the status bar after undo or redo. A drag is one step; arrow-key nudges merge into one step while the selection stays the same; deleting a mixed selection is one step.
- **Lane spreading** is done by the router itself: sharing a lane costs as much as a crossing, except inheritance lines into one parent, which form a tree (stem, bar, drops) whenever that path is clear.
- **Labels** avoid boxes, lines and each other; lines do not avoid labels. Boxes joined by a labelled line in the same row get extra room for the labels.
- **Font**: DejaVu Sans Mono in four faces, bundled. Abstract names and operations are italic; static members are underlined.
- **Crossing reduction** adds sifting and random restarts to the barycenter sweeps. An exhaustive test checks that the true minimum is found on small two-layer graphs.
- **Package placement**: packages are laid out inside-out, and each finished package is one box in its parent. Lines leaving a package do not yet influence the order inside it.
- **PlantUML and Mermaid** (added 2026-11-17): PlantUML files open as unsaved, untitled diagrams so Save never overwrites them; export goes to PlantUML (`.puml`) and Mermaid (`.mmd`, `.md`). `umpl convert` converts between the formats. See `docs/plans/2026-11-17-plantuml-mermaid.md`.
- **Umple** (added 2026-11-17): `.ump` files open as unsaved, untitled diagrams, like PlantUML files, and File → Export → Umple… writes the class model back. Umple has no packages, notes, aggregation or dependencies, so export uses `namespace` statements and comments for them. See `docs/plans/2026-11-17-umple.md`.
- **Development** happens in a Docker image (Ubuntu 26.04, Rust 1.91) through `scripts/dev.sh`. Interaction tests drive the real editor with egui_kittest.
