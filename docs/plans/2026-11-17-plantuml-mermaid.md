# PlantUML and Mermaid: import and export

Status: implemented 2026-11-17.

## 1. Why

fastumpl keeps diagrams in its own `.umpl` text format. That works well for its own files, but a diagram rarely stays in one tool. Two other text formats show up wherever class diagrams are written down:

- **Mermaid** renders directly inside Markdown on GitHub, GitLab, Notion, Obsidian and most documentation sites. A diagram that can be pasted into a README or an issue as a `mermaid` code block reaches readers who will never install fastumpl.
- **PlantUML** is the older standard. Many teams already have folders of `.puml` files, and a large part of what people find online is in PlantUML.

What people ask for in other tools:

| Source | What users ask for |
|---|---|
| Lucidchart community, "Implement proper integration with PlantUML" ([link](https://community.lucid.co/ideas/implement-proper-integration-with-plantuml-markup-language-7281)) | Import PlantUML into an editable diagram; one of the most voted UML ideas |
| Lucidchart community, "Export Lucidchart diagrams to Mermaid code" ([link](https://community.lucid.co/ideas/export-lucidchart-diagrams-to-mermaid-code-2677)) | Export a drawn diagram as Mermaid, at least for simple diagrams, to put it in Markdown |
| Miro community, "PlantUML support" ([link](https://community.miro.com/ideas/plantuml-support-in-miro-3722)) | Import PlantUML as real, editable shapes rather than a picture |
| IcePanel feedback, "Diagrams as code (PlantUML, Mermaid, GitHub)" ([link](https://feedback.icepanel.io/feature-requests/p/diagrams-as-code-plantuml-mermaid-github)) | Keep diagrams in text next to the code |
| Astah plug-ins ([link](https://astah.net/?p=462)) | Ship a Mermaid exporter and a PlantUML class-diagram importer, which shows the same demand |

Other features came up in the research and were considered:

| Feature | Seen in | Why not now |
|---|---|---|
| Layout direction (`left to right direction`) | PlantUML, Mermaid `direction LR` | Needs a rotated layering pass in `umpl-layout`; larger and riskier than the interop work |
| Per-class colours (`classDef`, `#color`) | Mermaid, PlantUML | Conflicts with the "calm diagram" style rule in design §7 |
| `hide empty members`, `hide @unlinked` | PlantUML | Empty compartments are already hidden |
| Mermaid import | Mermaid | Export is the common need (Markdown); import can reuse the PlantUML importer's structure later |

The design already expects this work: design §12 says "A PlantUML importer and exporter can be added in `umpl-text` without other changes".

## 2. What is built

Three features, all in `umpl-text` (no new dependencies), plus the CLI and the app on top.

1. **Export to Mermaid** (`.mmd`, or `.md` wrapped in a ```` ```mermaid ```` block), and **Copy as Mermaid** to the clipboard in the editor.
2. **Export to PlantUML** (`.puml`).
3. **Import from PlantUML**: `.puml`, `.plantuml`, `.pu`, `.iuml`, `.wsd` files open in the editor and in the CLI.

Positions are not exported: neither format can store them, since Mermaid and PlantUML lay diagrams out themselves. Import produces a document without positions, so the editor runs Tidy on it, as it does for any `.umpl` file without a `layout` block.

### API (`umpl-text`)

```rust
pub enum Format { Umpl, PlantUml, Mermaid, MermaidMarkdown }

impl Format {
    pub fn from_path(path: &Path) -> Option<Format>;   // by extension
    pub fn from_name(name: &str) -> Option<Format>;    // "umpl", "plantuml"/"puml", "mermaid"/"mmd", "markdown"/"md"
    pub fn can_read(self) -> bool;                     // Umpl, PlantUml
}

pub fn read_as(format: Format, src: &str) -> Parsed;   // panics for formats that cannot be read
pub fn write_as(format: Format, doc: &Document) -> String;

pub fn plantuml::import(src: &str) -> Parsed;
pub fn plantuml::export(doc: &Document) -> String;
pub fn mermaid::export(doc: &Document) -> String;
```

`import` never fails, like `parse`. Anything it skips or changes is reported as a `Problem` with the line and column in the PlantUML file.

## 3. Mermaid export

```
---
title: Shop
---
classDiagram
    namespace billing {
        class Payment {
            <<abstract>>
            +amount: Money
            +pay() Receipt*
        }
        class CardPayment
    }
    class Order {
        -lines: List~OrderLine~
        +total() Money
    }
    Order "1" *-- "1..*" OrderLine : contains
    CardPayment --|> Payment
    note for Order "an order owns its lines"
```

| `.umpl` | Mermaid |
|---|---|
| `diagram "T"` | Front matter `title: T` |
| Abstract, interface, enum | Annotation `<<abstract>>`, `<<interface>>`, `<<enumeration>>` as the first body line |
| Stereotype on a class | Annotation `<<stereo>>`. For other kinds the kind annotation wins (Mermaid shows one) |
| Operation `pay(): Receipt` | `pay() Receipt` (Mermaid puts the return type after a space) |
| `{static}` / `{abstract}` | `$` / `*` at the end of the member |
| Generic `List<T>` in a member | `List~T~` |
| Package | `namespace`. Mermaid does not nest namespaces, so `a.b` becomes `namespace a_b`; elements directly in `a` go in `namespace a` |
| Class names | The plain name when unique in the diagram; otherwise the full name with `_` for `.` and a label (`class billing_Item["Item"]`) |
| Note linked to classes | `note for X "text"` for each linked class; an unlinked note becomes `note "text"` |
| Relations | Same arrows (`--|>`, `..|>`, `--`, `-->`, `o--`, `*--`, `..>`, `..`); end labels and names are kept |
| Comments | `%% text` |
| Raw lines | `%% text`, so the output still parses |

## 4. PlantUML export

Close to `.umpl` itself: the format was designed to be similar (design §5.2).

- Wrapped in `@startuml` / `@enduml`; `title T` for the title.
- Elements are written inside their packages. Relations use the plain name when it is unique and the full dotted name otherwise.
- Notes become `note "text" as N1` (newlines as `\n`) and their links stay `N1 .. Order`.
- Members are written as in `.umpl`; PlantUML accepts `{static}` and `{abstract}` at the end of a line.
- Comments become `'` comments; raw lines are written as comments, so PlantUML does not trip over them.

## 5. PlantUML import

The importer builds a `Document` directly rather than translating to `.umpl` text. That way it can merge reopened packages, resolve names declared later in the file, and report problems at PlantUML line numbers.

Understood:

| PlantUML | Result |
|---|---|
| `@startuml` … `@enduml` | Only the first diagram; text outside is ignored |
| `title X` | Title |
| `'` and `/' … '/` comments | Comments, attached to the next declaration |
| `class`, `abstract [class]`, `interface`, `enum` | The same element kinds |
| `annotation`, `entity`, `struct`, `record`, `exception`, `metaclass`, `protocol`, `dataclass`, `object` | Class with that word as its stereotype (unless one is given) |
| `class "Long name" as L`, `class L as "Long name"` | One element; both names refer to it. A name that is not a valid `.umpl` name is cleaned up (`Long_name`) |
| `class Foo<T>` | Class `Foo`; the type parameters are dropped with a problem |
| `<<stereo>>`, `<<(S,#FF7700) stereo>>` | Stereotype (spot and colour dropped) |
| `#color`, `##[dashed]` after a name | Dropped silently |
| `extends A`, `implements B, C` on a declaration | Generalization and realization relations |
| Body `{ … }`, inline `{ a; b }`, `{field}` `{method}` `{classifier}` markers, separator lines `--` `..` `==` `__` | Members; separators dropped; `{classifier}` becomes `{static}` |
| `Name : member` outside a body | Member added to that class |
| `package` / `namespace` with plain, quoted or dotted names, `<<Folder>>`, colours | Packages, nested for dotted names; a package opened twice is merged |
| `together { … }` | Its contents, without grouping |
| Relations with any arrow length, direction words (`-up->`, `-l->`), styles (`-[#red,dashed]->`), heads `<|` `|>` `*` `o` `<` `>` on either side | The matching relation kind; `..` with no heads between two classes is the dashed line (`..`) |
| `"1"` / `"0..* items"` end labels, `: label >` | End labels; the direction marker on the name is dropped |
| `note "t" as N`, `note as N … end note`, `note left of X : t`, `note top of X … end note` | Note elements with note links |
| Names used only in relations | Created as classes in the package where they are used, as PlantUML does. A plain name found in exactly one package refers to that element |

Skipped, with a problem on the line: `skinparam` (and its `{ }` blocks), `hide`, `show`, `remove`, `restore`, `left to right direction`, `top to bottom direction`, `scale`, `set`, `!` preprocessor lines, `<style>` blocks, `legend`, `header`, `footer`, `caption`, `note on link`, arrow heads with no UML meaning here (`x`, `#`, `+`, `}`, `^`; drawn as a plain line), association classes `(A, B) .. C`.

Anything else is kept as a raw line with a problem, as the `.umpl` parser does, so no text is lost.

## 6. Command line

```
umpl convert diagram.puml -o diagram.umpl         # import
umpl convert diagram.umpl -o diagram.mmd          # or .md, .puml
umpl convert diagram.umpl --to mermaid            # write to standard output
umpl convert diagram.puml -o diagram.umpl --tidy  # also store a layout
umpl render diagram.puml -o diagram.svg           # every reading command accepts PlantUML
```

`render`, `check` and `stats` read PlantUML files directly. `fmt` and `tidy` refuse them, because they rewrite the input in `.umpl` style and that would destroy a PlantUML file.

## 7. Editor

- **Open** lists PlantUML files next to `.umpl`. Opening one (also by drag and drop, recent files or the command line) imports it as an unsaved, untitled diagram named after the file (`shop.puml` → `shop.umpl`), so Save asks where to write the `.umpl` file and never overwrites the PlantUML source.
- **File → Export PlantUML…**, **Export Mermaid…** (`.mmd` or `.md`) and **Copy as Mermaid**, which puts Mermaid text on the clipboard for pasting into an issue or a README. All three are in the command palette.

## 8. Tests

| Test | Where |
|---|---|
| Mermaid output for the design example: front matter, namespace, annotations, `$` `*` markers, `~T~` generics, cardinalities, `note for` | `umpl-text` unit tests |
| Mermaid ids for duplicate names across packages get labels; nested packages flatten | `umpl-text` unit tests |
| PlantUML export, then import, gives the same elements, kinds, members, stereotypes, packages and relations (design example and the `examples/` corpus) | `umpl-text/tests/interop.rs` |
| Import of a realistic PlantUML file with aliases, dotted packages, `extends`/`implements`, outside members, all note forms, styled and directional arrows, skinparam blocks | `umpl-text` unit tests |
| Skipped lines produce problems at the right line; unknown lines become raw lines; nothing panics on random input | `umpl-text` unit tests |
| `convert` in every direction, `--to` to standard output, `render` of a `.puml` file, `fmt` refuses `.puml` | `umpl-cli` integration test |
| Opening a `.puml` file gives an untitled, unsaved diagram named after the file, with every element placed | `umpl-app` interaction test |
