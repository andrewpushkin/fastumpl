# QA GUI session, 2026-11-19

Exploratory testing of the running app (SBTM), not the code. Build: release `fastumpl` at `17056a9`, running in the dev image on a virtual X display (Xvfb 1600×980, openbox), driven with xdotool and read from screenshots. HOME was a scratch folder, so config and recovery files were isolated. Test file: `examples/shop.umpl`. Screenshots are in [2026-11-19-gui/](2026-11-19-gui/).

## Charters

| # | Charter | Coverage |
|---|---|---|
| C1 | Explore canvas editing (add, edit in place, drag, delete, undo/redo) with mouse and keyboard to discover canvas/model desyncs | covered |
| C2 | Explore the Both view's text panel with ordinary typing to discover text↔drawing sync breakage | partial: Return, rename, unclosed `{` |
| C3 | Explore the file lifecycle (close with changes, external change, crash recovery, save) to discover data loss | covered |
| C4 | Explore menus, Find, command palette, Tidy, export to discover broken commands | partial: PlantUML export only; PNG/SVG/Mermaid/Umple, Open, Settings not tried |

## Findings

Severity as in [2026-11-19-findings.md](2026-11-19-findings.md). Numbering continues from it.

| ID | Sev | Finding |
|---|---|---|
| QA-20 | High | After Undo, Redo or Delete the canvas keeps drawing the previous state until the selection changes |
| QA-21 | High | Return in the text panel moves the cursor to the start of the selected box's line; the next keystrokes corrupt that line |
| QA-22 | Low | A new class's name is not selected in the in-place editor, so typing appends (`ClassFoo`) |
| QA-23 | Low | The inspector's Name field lags edits made in the text panel |
| QA-24 | Low | Undoing back to the saved state keeps the unsaved dot in the title |
| QA-25 | Low | The find bar's three buttons draw as empty boxes (missing glyphs) |
| QA-26 | Low | A class added by double-click near the window edge is centred on the click and hidden under the menu bar and tool palette |

QA-20 and QA-21 are also in `91e8ed9` (same steps, same result), so the latest commit did not introduce them. All seven are fixed; see [Fixes](#fixes).

### QA-20: drawing not refreshed after Undo, Redo, Delete

Steps (Canvas view): drag `Status` somewhere else, click empty space, press Ctrl+Z (or the toolbar's undo button).

- Expected: Status jumps back.
- Actual: the status bar says "Undone: Move Status" and the selection handles and inspector show Status at `70, 0`, but the box is still drawn where it was dragged ([undo-move-stale.png](2026-11-19-gui/undo-move-stale.png)). The drawn box cannot be grabbed; the real one is invisible at its old place.

Other cases, all on the same pattern:

- **Undo of Add class:** the box stays as a ghost. The status bar says 6 classes and the text has no such class, yet Fit includes the ghost in the view ([undo-add-ghost.png](2026-11-19-gui/undo-add-ghost.png)).
- **Undo of a rename:** the box keeps the new name.
- **Undo of Tidy:** the inspector's stats revert (9 bends, score 55.4), but the tidied layout is still drawn ([undo-tidy-stale.png](2026-11-19-gui/undo-tidy-stale.png)).
- **Delete:** the box stays drawn while its lines go; undoing the delete hides the box.

Selecting any box redraws everything correctly. Saving writes the right model (checked by diff). So nothing is lost on disk, but the user edits against a picture that is wrong.

### QA-21: Return in the text panel jumps the cursor

Steps (Both view): click at the end of `class OrderLine` (last line), press End, Return, type `class Zed`.

- Expected: a new line `class Zed`.
- Actual: the blank line is inserted, but the text goes to the start of the line above: `class Zedclass OrderLine`, reported as "line 30: not understood".

The same steps on `enum Status { … }` give `clenum Status { … }`, and the enum leaves the model ([return-clenum.png](2026-11-19-gui/return-clenum.png)). With OrderLine selected, Return on the comment line `# an order owns its lines` sends the typing to the start of `class OrderLine`, not to the comment ([return-jumps-to-selected.png](2026-11-19-gui/return-jumps-to-selected.png)). So after Return the cursor goes to the start of the line of the selected box, which clicking in a line selects.

It reproduced with keystrokes 120–500 ms apart. Space, letters and End do not cause it.

### Smaller ones

- **QA-22:** double-clicking empty space opens the in-place editor with `Class` and the cursor after it; typing `Foo` gives `ClassFoo`.
- **QA-23:** in the text panel, `class OrderLineItem` → `class OrderLineItemX`. The canvas and text show `OrderLineItemX`; the inspector still shows `OrderLineItem`.
- **QA-24:** move a box, then undo. The diagram equals the file, but the title keeps `•`.
- **QA-25:** Ctrl+F shows `1 of 3` followed by three hollow squares ([find-bar-glyphs.png](2026-11-19-gui/find-bar-glyphs.png)). The toolbar icons render, so the glyphs are probably missing from the bundled font. Check on a desktop build before fixing.
- **QA-26:** double-click at the top-left of the canvas: the new box sits at `-11, -20`, mostly under the menu bar and tool palette.

## Questions

- Cancel in the in-place editor of a class that was just added keeps the class. Should Cancel remove it, as one step?
- After Order was moved above Priced, the dashed `Order ..|> Priced` line routes with 9 bends around Order to enter Priced from below. Is this the "parents above" rule at work, or should the router accept a side entry?
- In Both view, a box created by text (`class Broken {` typed before `enum Status`) took Status's old place and overlapped `OrderLineItemX`.

## What held up

- Closing with unsaved changes asks Save / Don't save / Cancel; Cancel keeps the window. A clean diagram closes without asking.
- External change while dirty: "File changed on disk" warns that reloading loses local edits. "Keep my version" does not nag again, and Save writes the local version.
- Crash recovery: after `kill -9`, the next start offers Recover / Discard / Later. Recover restores the moved box, marks the diagram unsaved and deletes the recovery copy.
- Export PlantUML from the command palette opens the GTK save dialog with `shop.puml` and a PlantUML filter. The PlantUML jar reads the file (6 entities).
- Find outlines all matches and Enter steps through them. The palette filters commands. Tidy gives 0 crossings. Dragging re-routes lines live. The view choice and zoom are kept for the next run.

## Fixes

All seven are fixed. Each has a test that failed before its fix, and each fix was checked again in the running app with the same steps as above.

| ID | Cause | Fix | Test |
|---|---|---|---|
| QA-20 | The frame synced the layout before handling keys and menus, so Undo or Delete then built the scene from the old layout and cached it under the new revision | The canvas syncs the layout before it paints; the scene's cache key includes the layout's revision | `undoing_a_move_redraws_the_box_where_it_now_is`, `undoing_an_add_or_a_rename_redraws_the_canvas`, `deleting_a_box_removes_it_from_the_canvas` |
| QA-21 | Every parse of typed text cleared the "selection followed" mark so the lines would be tinted again; the next frame took that as a new selection and moved the cursor to the selected box's line | A parse only re-tints, through its own generation mark; the cursor moves only when the selection really changes | `typing_after_enter_goes_on_the_new_line`, `typing_on_a_comment_line_stays_on_it` |
| QA-22 | The name field took focus with the cursor at its end | Opening the in-place editor selects the whole name | `typing_in_a_new_class_replaces_its_default_name` |
| QA-23 | The inspector skipped reloading while any text field had the keyboard, the text panel included | It skips only while one of its own fields has the keyboard | `the_inspector_follows_a_rename_typed_in_the_text` |
| QA-24 | "Unsaved" compared the editor revision, which grows on undo too | The editor names each state of the history (`Editor::state`); undo returns to the earlier state's name, and the session compares states | `undo_and_redo_return_to_the_same_state`, `a_new_change_after_undo_is_a_new_state`, property test `one_state_is_one_document`, `undoing_back_to_the_saved_state_is_not_unsaved` |
| QA-25 | `▲ ▼ ✕` (find bar) and `→` (inspector hint) are in none of the interface's fonts | `⏶ ⏷ 🗙` from egui's symbol font; `>` in the hint | `button_symbols_are_in_the_fonts` (every symbol the interface uses) |
| QA-26 | A new box was placed at a fixed offset from the click | A new box is centred on the click by its measured size, then moved just enough to lie inside the view, clear of the tools | `a_new_box_stays_inside_the_view` |

A new box at the top edge pushes the diagram title above the view (it sits above the topmost box), until you scroll or press Fit.

## Next

- Remaining charters: PNG/SVG/Mermaid/Umple export, Open and Open recent, PlantUML import of a large corpus file in the GUI, Settings, resize corners, group drag, the connect tool, the light theme.
- The three questions above.
