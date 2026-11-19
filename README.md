# fastumpl

A desktop editor for UML class diagrams that lays them out for you. It places the boxes and routes every line around them, keeping line crossings to a minimum. Written in Rust.

![The fastumpl editor with a class diagram](docs/images/editor.png)

- **Tidy in one key.** Ctrl+T lays out the whole diagram: parents above children, related classes side by side, no overlapping boxes, no lines through boxes.
- **Few crossings.** Crossings are removed at several stages, from box order to line routing, and several candidate layouts compete. The status bar shows the crossing count.
- **Lines follow you.** Drag a class and its lines re-route as you move.
- **Type, don't click.** Double-click a class and type its members: `+total(): Money`.
- **Looks like a drawing tool.** Component, deployment and use case boxes show an icon for their kind, the name and the lines under it (a database is a cylinder); each group gets its own colour; the title sits above the diagram. The window is quiet and dark or light with the system, with the tools floating on the canvas and a Text | Both | Canvas switch in the top bar.
- **Size and colour.** Drag a corner of a selected box to make it bigger than its text needs (it never gets smaller). A package (group) moves when you drag anywhere inside it that is not a box or a line, and its frame resizes by the corners once it is selected; the frame always keeps its content inside. Right-click a box or package for its colour: a palette, transparent, or the usual colour back, and to fit a resized box to its text again.
- **Text and drawing side by side.** The text panel shows the diagram as `.umpl` text. Edit either one and the other follows as you work: type a class or a relation and it appears, laid out; drag, rename or delete on the canvas and the text updates. Click a box and its lines in the text light up; put the cursor in a line and its box is selected. Choose Text, Both or Canvas in the middle of the top bar (Ctrl+B shows or hides the text); it starts with the canvas alone, and the choice is kept for the next run.
- **Find.** Ctrl+F finds classes, members, relations and packages; matches are outlined in the drawing and marked in the text, and Enter steps through them.
- **Plain text files.** Diagrams are `.umpl` text files that diff well. The model sits at the top and positions at the bottom.
- **Command line too.** `umpl` renders SVG and PNG, formats and checks files.
- **PlantUML and Mermaid.** Open `.puml` files directly, `!include`s and macros included, and big ones too: they open at once and are laid out to fill the window. Export to PlantUML or Mermaid, or copy Mermaid to paste into a GitHub README or issue.
- **Umple.** Open the class model of an [Umple](https://umple.org) `.ump` file and see it laid out; export a diagram as Umple for the Umple compiler to generate code from.

## Download

From the [latest release](../../releases/latest):

| Platform | Files |
|---|---|
| Windows x64 | `fastumpl-windows-x86_64-setup.exe` (installer) or `fastumpl-windows-x86_64-portable.zip` |
| Windows ARM64 | `fastumpl-windows-arm64-setup.exe` or `fastumpl-windows-arm64-portable.zip` |
| macOS 11+ (Apple Silicon and Intel) | `fastumpl-macos-universal.dmg`; first launch: right-click the app, then Open |
| Linux x86-64 | `fastumpl-linux-x86_64.deb` (`sudo apt install ./fastumpl-linux-x86_64.deb`) or `fastumpl-linux-x86_64.tar.gz` (glibc 2.35+) |

The installer adds a Start menu entry, the `.umpl` file type and, optionally, `umpl` on PATH. The portable zip installs nothing and keeps its data in a `data` folder next to the program.

## The `.umpl` format

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

enum Status { New, Paid, Shipped }

# an order owns its lines
Order "1" *-- "1..*" OrderLine : contains
billing.CardPayment --|> billing.Payment
Order --> billing.Payment : paid by

layout {
  Order           at 29, 17
  billing.Payment at 3, 5
}
```

| Arrow | Meaning |
|---|---|
| `A --\|> B` | A extends B |
| `A ..\|> B` | A implements B |
| `A *-- B` | A is composed of B |
| `A o-- B` | A aggregates B |
| `A -- B` | association |
| `A --> B` | directed association |
| `A ..> B` | A depends on B |
| `N .. A` | note N describes A |

Reversed arrows such as `<|--` are accepted too. Each end can carry a quoted label, `"1..*"` or `"0..1 owner"` (multiplicity, then role). Elements without a line in the `layout` block are placed automatically when the file opens. Lines the parser does not understand are kept and written back unchanged.

Enum literals are separated by commas outside brackets, so `enum Color { RED(255, 0, 0), GREEN }` has two. In a body, a member that is just `}`, or one starting with `\`, is written with a `\` in front (`\}`), so it does not close the body. In a relation's name `\n` is a line break and `\\` a backslash. A file may start with a UTF-8 byte order mark. Positions lie within 1,000,000 grid units of the origin and sizes are at most 10,000; a `layout` line beyond that is reported and ignored, and the box is placed anew.

A colour follows a declaration: `class Order #lightblue`, `package billing #transparent {`, `note N "text" #ffeeaa` (a CSS colour name, `#rgb` or `#rrggbb`, or `transparent`, which leaves only the outline). A box that was resized keeps its size in the `layout` block: `Order at 29, 17 size 30, 12` (grid units; the box is never smaller than its text needs).

## The text panel

The panel on the left shows the model part of the file (everything above `layout`) and is kept in step with the drawing both ways:

- **Text to drawing.** A quarter of a second after you stop typing, the text is read again and the diagram updated. A class you rename keeps its place and its selection; a new class is placed in a free spot; a class whose line you delete and retype gets its place back. While a `{` is still open the diagram waits (the panel says so), so a half-typed class body does not swallow the classes below it. Lines that are not understood get a red background and are listed under the text.
- **Drawing to text.** Changes on the canvas, in the inspector, and undo or redo rewrite the text. Moving boxes does not, since positions are not shown; a `layout` block typed into the panel is still applied.
- **Undo.** Typing is one undo step until you pause for two seconds or leave the panel. Inside the panel Ctrl+Z undoes typing; outside it, the step ("Undo Edit text").
- **Linked both ways.** Click a box, line or package and the lines that declare it get a blue background, scroll into view and get the text cursor; the keyboard stays with the drawing, so Delete still deletes the box. Move the cursor into a declaration (click, arrow keys) and that item is selected in the drawing and brought into view. A blank or comment line leaves the selection as it is.
- **Editing.** The panel is an ordinary text editor: select, cut, copy and paste (Ctrl+X, Ctrl+C, Ctrl+V) with other programs, select all with Ctrl+A, undo typing with Ctrl+Z.

## Settings

File → Settings… (Ctrl+,) opens the Settings window. Every change applies at once and is saved for the next run (in `~/.config/fastumpl/settings.txt`; on Windows `%APPDATA%\fastumpl`, or the `data` folder of the portable zip), as are the recent files:

- **Layout:** lay diagrams out for the **fewest line crossings** (the default; a drawing may come out much wider or taller than the window), or to **fill the window**: the drawing takes the window's shape, with no blank band beside or below it, for a few more crossings where need be. It applies to diagrams opened from then on and to Tidy (also in the Arrange menu).
- **Theme:** follow the system, light or dark (also in the View menu).
- **Interface text:** size, 80–160 %, and font: sans or monospace (DejaVu Sans Mono, the diagram's font).
- **Text panel:** shown or hidden (also View → Text), and its font size, 9–24 pt.
- **Canvas:** the grid shown or hidden (also View → Grid).

The diagram's own text keeps its size, since boxes are measured from it and a saved layout would no longer fit them; zoom instead. **Reset to defaults** puts everything back.

## Command line

```
umpl render diagram.umpl -o diagram.svg      # or .png; --tidy, --fill, --aspect 16:9, --dark, --scale N
umpl tidy diagram.umpl                       # lay out again and save positions; --fill
umpl fmt [--check] diagram.umpl              # rewrite in standard style
umpl check diagram.umpl                      # list problems, exit 1 if any
umpl stats diagram.umpl [--tidy]             # crossings, bends, score
umpl convert diagram.puml -o diagram.umpl    # import PlantUML; --tidy stores a layout
umpl convert model.ump -o model.umpl        # import Umple
umpl convert diagram.umpl -o diagram.mmd     # export: .mmd, .md (Mermaid block), .puml, .ump
umpl convert diagram.umpl --to mermaid       # write to standard output
```

`render`, `check` and `stats` also read PlantUML files (`.puml`, `.plantuml`, `.pu`, `.iuml`, `.wsd`) and Umple files (`.ump`).

## PlantUML and Mermaid

- **Import.** Open a PlantUML class diagram (File → Open, drag and drop, or `fastumpl model.puml`). It opens as a new, unsaved diagram and is laid out at once; Save writes a `.umpl` file and leaves the PlantUML file alone. Styling and layout hints (`skinparam`, `!theme`, `scale`, `-[hidden]-`) are skipped without a problem, since fastumpl draws and lays out in its own style; `left to right direction` is kept (Arrange → Left to right), and a label written over several lines (`: a\nb`) keeps its lines. `hide`/`remove`/`show`/`restore` of elements (by name, `*` pattern, `<<stereotype>>`, `$tag` or `@unlinked`) leave them out of the drawing, as PlantUML does. A file with several diagrams (`@startuml` … `@enduml` blocks, or pages split by `newpage`) asks which one to open; `umpl` takes `--diagram N`.
- **Sequence diagrams.** A PlantUML sequence diagram opens drawn as one: participants with their lifelines, messages (calls, replies, asynchronous, lost, to and from the edge), activations, notes, `alt`/`loop`/`group` fragments, refs, dividers, delays, `autonumber`, created and destroyed participants, boxes around participants. You change it in the text (View → Text or Both) and the drawing follows; Save keeps it as PlantUML text. Every row is as tall as its text and each gap between two lifelines as wide as what must fit between them, so no text runs into another.
- **Includes and macros.** `!include`, `!include_once`, `!include_many` and `!includesub` of other files are followed, relative to the including file (`\` works as a folder separator too). PlantUML's preprocessor is read as far as class diagrams use it: `!define` and `!definelong` macros, `!$variables`, `!procedure` and `!function`, and `!if`/`!ifdef`/`!else`/`!endif`. The standard library (`<C4/…>`, `<aws/…>`, …) and the well-known libraries on the web (C4-PlantUML, Azure, AWS icons and others) are recognised: their procedures (`Person(…)`, `Rel(…)`, `System_Boundary(…) {`) are read as boxes, lines and frames without a problem. Other includes from the web are not fetched; each is listed under Problems. An included file is read if it has a PlantUML extension (`.puml`, `.iuml`, `.pu`, `.wsd`, `.plantuml`, `.ipu`, `.theme`, `.sprite`, `.style`, `.inc`, `.def`, `.txt`) or, without one, starts like PlantUML (`@start…`, a `!` directive or a `'` comment); other files, such as `/etc/passwd`, are not read into a diagram.
- **Colours.** `#color` on boxes, groups and notes (`#LightBlue`, `#AABBCC`, `#transparent`, `#back:pink;line:red`, a gradient's first colour), and the background colours `skinparam` and `<style>` set for a kind (`skinparam packageBackgroundColor transparent`, `skinparam rectangle { BackgroundColor Gold }`), become the boxes' colours; they are exported to PlantUML and Mermaid too.
- **What is read.** Class and object diagrams; entity-relationship lines with crow's feet (`User ||--o{ Order` gets multiplicities `1` and `0..*`); type parameters (`class Repo<T>`); lines between packages; association classes (`(A, B) .. C`, drawn through a diamond); nested classes (`+--`); links (`[[url]]`) and line styles (`#red;line.dashed`) are dropped. Names are found as PlantUML finds them: `class "Purchase order" as PO` is known by `PO` only, so several boxes may show the same text; a dotted name such as `a.b.C` is read inside the current package unless its first part is something at the top (a leading `.` names from the top); a plain name in a line is the element of the current package, else the only one of that name anywhere, else a new one; `set namespaceSeparator` is followed; component diagrams do not split names at dots. Component, deployment and use case diagrams are read as boxes with their kind as stereotype: `node`, `rectangle`, `folder`, `cloud`, `database` and the like with a body become packages, `[Component]`, `(Use case)` and `:Actor:` become boxes. Calls of library procedures whose library could not be read, such as C4's `Person(…)`, `Container(…)` and `Rel(…)`, become boxes and lines. Sequence, activity, state, timing, mind map, Gantt and other diagrams are not class diagrams: opening one says which kind it is.
- **Big diagrams.** A diagram of more than 150 classes or 300 relations opens at once and is laid out in the background (the status bar says so; Esc keeps the first layout found). Loose parts go in rows that fill the window rather than one long row, and the view zooms to fit; with Arrange → Filling the window the whole drawing is laid out to the canvas's shape (see Settings). Drawings too large to route (over 12 million grid points, such as a 5000-class model) get draft lines that do not go around boxes.
- **Export.** File → Export → PlantUML… and Mermaid… (`.mmd`, or `.md` for a ready-made ```` ```mermaid ```` block). **Copy as Mermaid** puts the diagram on the clipboard for pasting into Markdown on GitHub, GitLab, Notion or Obsidian.
- Both formats lay diagrams out themselves, so positions are not exported.

## Umple

[Umple](https://umple.org) writes class models as text (classes, attributes, associations such as `1 -- * Student;`, `isA`) next to state machines and method code, and generates Java, Python and other languages from them.

- **Import.** Open a `.ump` file the same way as a PlantUML file. Classes, interfaces, enums, traits, attributes, methods (signatures only), associations, association classes, `isA` and namespaces (as packages) are read, and the diagram is laid out at once. State machines, constraints, requirements, mixsets, ports and other files named by `use` are not drawn; each is listed under Problems. UmpleOnline's `position` lines are ignored in favour of fastumpl's own layout.
- **Export.** File → Export → Umple… writes a `.ump` file the Umple compiler can read: packages become `namespace` statements, associations go into class bodies, and an association without a multiplicity gets `*`. Notes, dependencies, aggregation and anything else Umple cannot express become comments, so nothing is silently dropped.
## Keys

| Key | Action |
|---|---|
| Double-click empty space | New class |
| Double-click a box | Edit it in place |
| Drag from a dot on a box edge | Connect (pick the relation kind) |
| Drag a corner of a selected box | Resize it |
| Drag inside a package, off its boxes | Move the package with everything in it |
| Drag a corner of a selected package | Resize its frame |
| Right-click a box or package | Colour, fit size to text, delete (on empty space: Tidy, Fit) |
| V C A I E N P R | Tools: select, class, abstract, interface, enum, note, package, relation (also the icons on the left) |
| Ctrl+T | Tidy |
| Ctrl+, | Settings |
| F | Fit diagram in window (zooms in up to 200 % for a small diagram) |
| Ctrl+K | Command palette |
| Ctrl+F, then Enter / Shift+Enter (or F3 / Shift+F3) | Find, next and previous match; Esc closes |
| Ctrl+B | Show or hide the text panel (also Text / Both / Canvas in the top bar) |
| Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) | Undo / redo; also the ⟲ ⟳ buttons, and the Edit menu names the step ("Undo Move Order") |
| Right- or middle-drag, scroll, the scroll bars | Pan |
| Ctrl+scroll, or − and + in the top bar | Zoom (click the percentage for 100 %) |

## Building

Everything builds in Docker, so nothing else needs installing on your machine:

```
make linux-x86_64              # build (binaries for glibc 2.35+)
make linux-x86_64-deb          # dist/fastumpl-linux-x86_64.deb
make linux-x86_64-packages     # .tar.gz and .deb
make windows-x86_64            # build
make windows-x86_64-packages   # portable .zip and installer (Inno Setup 7)
make windows-arm               # build for ARM64
make windows-arm-packages      # portable .zip and installer for ARM64
make mac                       # dist/fastumpl-macos-universal.dmg (needs your macOS SDK)
make all                       # all packages except mac
make test
make clean                     # remove dist/, target/ and generated files
```

`make mac` needs Apple's macOS SDK, which may only be used on Apple hardware and is not included; see [packaging/macos/README.md](packaging/macos/README.md). Release `.dmg` files are built on GitHub's macOS runners.

For development, `scripts/dev.sh` runs any command in the same image with the repository mounted and the display forwarded (X11, including WSLg), so the editor window opens on your desktop:

```
scripts/dev.sh cargo test --workspace
scripts/dev.sh cargo run -p umpl-app -- examples/library.umpl
```

| Crate | Role |
|---|---|
| `umpl-core` | Document model and undoable edits |
| `umpl-text` | `.umpl` parser and printer |
| `umpl-layout` | Layered placement, orthogonal routing, crossing minimisation |
| `umpl-draw` | Box measuring, shapes, SVG and PNG |
| `umpl-cli` | The `umpl` command |
| `umpl-app` | The editor (egui) |

The design is in [docs/design.md](docs/design.md). Layout quality is guarded by a corpus test (`tests/corpus/scores.txt`): a change that adds crossings or worsens the score fails the build.

PlantUML import is checked against real files. The dev image holds three sample repositories in `/opt/puml-samples` (spiegel-im-spiegel/plantuml-sample, okunev-e/plantuml_examples, plantuml-stdlib/Azure-PlantUML, each pinned to a commit), and `scripts/puml-corpus.py` builds a corpus of about 8000 PlantUML files over 5 KB from public GitHub repositories, with what they include; [tests/puml-corpus/manifest.tsv](tests/puml-corpus/manifest.tsv) lists each file with its repository and commit, so the same corpus can be downloaded again:

```
python3 scripts/puml-corpus.py fetch            # download into target/puml-corpus
PUML_CORPUS=/opt/puml-samples:target/puml-corpus scripts/dev.sh \
    cargo test --release -p umpl-draw --test puml_corpus -- --ignored --nocapture
```

The test opens every file as the editor does, lays it out, fails on a panic, an overlap or a file that takes over a minute, and reports problems by kind, the shape of the drawings and the slowest files (`PUML_CORPUS_LAYOUT=0` imports only, `=4` lays out every fourth file).

PlantUML itself is the reference: the dev image holds its jar, and `scripts/dev.sh scripts/puml-check.sh` has it read every corpus file (without drawing) into `target/puml-check.tsv`. The corpus test then reports how many of the files PlantUML reads without error open here with no problem at all, which files PlantUML sees as another kind of diagram, and where it counts another number of elements. `java -cp /opt/plantuml/plantuml.jar scripts/PumlCheck.java --dump FILE` (in the dev image) prints the packages, elements and lines PlantUML made of a file, to check how it reads a construct.

Releases: every push to `main` builds all packages (with the `make` targets above, plus the `.dmg` on macOS) and publishes release `v0.1.N`, where N is the number of commits on `main`. CI runs only on `main`; pull requests do not trigger it. The website in `site/` is published to GitHub Pages from `main` (set Pages to "GitHub Actions" once in the repository settings).

## License

MIT; see [LICENSE](LICENSE). The bundled DejaVu Sans Mono fonts have their own permissive license in [assets/fonts/DejaVu-LICENSE.txt](assets/fonts/DejaVu-LICENSE.txt).
