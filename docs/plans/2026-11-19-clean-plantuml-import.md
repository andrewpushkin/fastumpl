# Every valid PlantUML file opens with no problem

Goal (asked 2026-11-19): "Opened with no problem at all" at 100 %. Before this
work: 7788 of 14,940 diagrams read (52 %).

## 1. What 100 % can mean

Some corpus files are broken in their own source. PlantUML 1.2026.9, now in the
dev image (`scripts/puml-check.sh`, `scripts/PumlCheck.java`: parses every
file with PlantUML's own reader, draws nothing, fetches web includes),
rejects or crashes on 1112 of the 14,940 files we read: missing includes,
syntax errors, unknown functions. Those must keep a problem.

The target: **every file PlantUML reads without error opens here with no
problem**, and "no problem" must be true: what is drawn has the content
PlantUML shows. 13,828 files; 7976 (57.7 %) clean at the start.

Two kinds of difference are not problems, by design: styling (colours,
fonts, themes, `skinparam`, `<style>`, sprites: fastumpl draws in its own
style), and layout hints (`left to right direction`, `-up->`, `together`,
`Lay_*`: fastumpl lays out itself). Anything else that changes what is
shown is a problem, or is fixed.

## 2. Work, by files affected (PlantUML-valid files)

| # | Cause | Files | Fix |
|---|---|---|---|
| 1 | styling and layout lines reported | ~3000 | silent; `hide`/`remove` of elements done for real; `set namespaceSeparator` honoured |
| 2 | type parameters dropped | ~1900 | `Element.type_params`; `class Box<T>` in `.umpl`, exports |
| 3 | arrow ends `+`, `x`, `#`, `^`, `-up|>` | ~1050 | `RelationKind::Nesting` (circled plus); others mapped |
| 4 | names rewritten silently (`"Order Line"` → `Order_Line`) | ~4000 | names as written; `.umpl` quotes names that are not identifiers |
| 5 | legend, header, footer, caption | ~600 | legend → note; the others kept as comments |
| 6 | several diagrams in a file, `newpage` | ~260 | the app asks which diagram to open |
| 7 | links to packages; class and package with one name | ~330 | relation ends may be packages; `package A {` after `class A` makes the class the package, as PlantUML does |
| 8 | C4 and other library calls, stdlib and web includes | ~350 | C4 built in; known libraries recognised |
| 9 | lines not understood, braces | ~500 | case by case |
| 10 | association classes, notes on links | ~45 | the A–B line is split at a diamond; a note on a link becomes the line's label |
| 11 | kind read differently from PlantUML | 47 | follow PlantUML |
| 12 | entity count differs from PlantUML's | ~1150 | names found as PlantUML finds them (see results) |

The corpus test reads target/puml-check.tsv when present and reports the
clean share among PlantUML-valid files, and entity-count differences.

## 3. Results

| Stage | PlantUML-valid files opening with no problem |
|---|---|
| Start | 7976 of 13,828 (57.7 %) |
| Styling, type parameters, arrow ends, names as written | 92.0 % |
| Known libraries, lines to packages, several diagrams | 96.9 % |
| Remaining forms, case by case | **13,806 of 13,836 (99.8 %)** |

The 30 files left: 6 include a style file from the web that is not in the
corpus; 6 need preprocessor loops, arrays and JSON data (`!foreach`,
`%splitstr`); 3 have quotes inside quoted names; the rest are one-off forms.

"No problem" must also be true, so names were compared with what PlantUML
made of each file (`PumlCheck.java --dump`: every package, element and line
by its code name). Fixes that came out of the comparison:

- **Names are found as PlantUML finds them.** The importer keeps PlantUML's
  code names (its quarks) apart from the shown text: `class "Order" as O1`
  and `class "Order" as O2` are two boxes, both labelled `Order`
  (`Element.label`). A dotted name is read inside the current package
  unless its first part names something at the top; a leading `.` names
  from the top. A plain name in a line is the element of the current
  package, else the only one of that name anywhere, else a new one in the
  current package (enclosing packages are not searched). `extends` and
  `implements` look in the current package only. Component-style
  declarations and `object` reuse an element of that name anywhere.
  `X::member` is `X`, split at the last `::`.
- **Lines are read when they come**, as PlantUML reads them, not at the
  end: `A --> B` before `package p { class B }` makes a `B` at the top.
- **Diagram kind as PlantUML decides it.** PlantUML tries the class reader
  first; a component diagram is one with a line only the component reader
  takes (`node N`, `[C]`, `package P` without `{`, unless `allowmixing`).
  Component diagrams do not split names at dots. Also: package bodies are
  statements, not members; byte order marks and old Mac line ends;
  separator lines must end as they start (`==Heading` is text).
- **Smaller forms:** a second `@startuml` starts the diagram again;
  `C-left->D` is a line from `C-left` (a name takes `-word` while an arrow
  still follows); a class on the way of a dotted name stays a class.

Of the PlantUML-valid files, 576 have another number of boxes than PlantUML
counts. 468 of them differ only by legends, which fastumpl shows as notes.
Of the rest, 52 differ in their names, all small:

- members' notes and lollipops (`A -() I`), which PlantUML counts as boxes;
- elements PlantUML hides but still counts;
- library sprites in the shown text;
- a handful of one-off syntaxes.
