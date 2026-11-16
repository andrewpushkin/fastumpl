# fastumpl

A desktop editor for UML class diagrams that lays them out for you. It places the boxes and routes every line around them, keeping line crossings to a minimum. Written in Rust.

![The fastumpl editor with a class diagram](docs/images/editor.png)

- **Tidy in one key.** Ctrl+T lays out the whole diagram: parents above children, related classes side by side, no overlapping boxes, no lines through boxes.
- **Few crossings.** Crossings are removed at several stages, from box order to line routing, and several candidate layouts compete. The status bar shows the crossing count.
- **Lines follow you.** Drag a class and its lines re-route as you move.
- **Type, don't click.** Double-click a class and type its members: `+total(): Money`.
- **Plain text files.** Diagrams are `.umpl` text files that diff well. The model sits at the top and positions at the bottom.
- **Command line too.** `umpl` renders SVG and PNG, formats and checks files.

## Download

From the [latest release](../../releases/latest):

| Platform | File |
|---|---|
| Linux x86-64 (Debian, Ubuntu) | `fastumpl-linux-x86_64.deb`, then `sudo apt install ./fastumpl-linux-x86_64.deb` |
| Windows x64 | `fastumpl-windows-x86_64-portable.zip`: unzip and run `fastumpl.exe` |
| Windows ARM64 | `fastumpl-windows-arm64-portable.zip` |

The Windows builds are portable. Recent files and recovery copies live in a `data` folder next to the program.

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

## Command line

```
umpl render diagram.umpl -o diagram.svg      # or .png; --tidy, --dark, --scale N
umpl tidy diagram.umpl                       # lay out again and save positions
umpl fmt [--check] diagram.umpl              # rewrite in standard style
umpl check diagram.umpl                      # list problems, exit 1 if any
umpl stats diagram.umpl [--tidy]             # crossings, bends, score
```

## Keys

| Key | Action |
|---|---|
| Double-click empty space | New class |
| Double-click a box | Edit it in place |
| Drag from a dot on a box edge | Connect (pick the relation kind) |
| V C A I E N P R | Tools: select, class, abstract, interface, enum, note, package, relation |
| Ctrl+T | Tidy |
| F | Fit diagram in window |
| Ctrl+K | Command palette |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |
| Right- or middle-drag, scroll | Pan |
| Ctrl+scroll | Zoom |

## Building

Everything builds in a container, so nothing needs installing on your machine except Docker:

```
scripts/dev.sh cargo test --workspace
scripts/dev.sh cargo run -p umpl-app -- examples/library.umpl
scripts/dev.sh cargo deb -p umpl-app        # after cargo build --release -p umpl-app -p umpl-cli
```

`scripts/dev.sh` builds the Ubuntu 26.04 development image (Rust 1.91), mounts the repository, and forwards the display (X11, including WSLg on Windows), so the editor window opens on your desktop.

| Crate | Role |
|---|---|
| `umpl-core` | Document model and undoable edits |
| `umpl-text` | `.umpl` parser and printer |
| `umpl-layout` | Layered placement, orthogonal routing, crossing minimisation |
| `umpl-draw` | Box measuring, shapes, SVG and PNG |
| `umpl-cli` | The `umpl` command |
| `umpl-app` | The editor (egui) |

The design is in [docs/design.md](docs/design.md). Layout quality is guarded by a corpus test (`tests/corpus/scores.txt`): a change that adds crossings or worsens the score fails the build.

Releases: push a tag `vX.Y.Z` and GitHub Actions builds the `.deb` and both Windows zips. The website in `site/` is published to GitHub Pages from `main` (set Pages to "GitHub Actions" once in the repository settings).
