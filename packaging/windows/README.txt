fastumpl - portable edition
===========================

fastumpl is a UML class diagram editor that lays out your diagrams for you.

Run
---
Double-click fastumpl.exe. Nothing needs installing.

To open a diagram directly, drag a .umpl file onto fastumpl.exe, or run:

    fastumpl.exe path\to\diagram.umpl

The examples folder has diagrams to start from.

Portable
--------
Recent files and crash-recovery copies are kept in the "data" folder next to
fastumpl.exe. Copy the whole folder to move fastumpl to another computer or a
USB stick. If the folder is read-only, fastumpl uses %APPDATA%\fastumpl
instead.

Command line
------------
umpl.exe works without a window:

    umpl render diagram.umpl -o diagram.svg    draw a diagram to SVG or PNG
    umpl render diagram.umpl -o out.png --tidy re-lay it out first
    umpl fmt diagram.umpl                      rewrite a file in standard style
    umpl check diagram.umpl                    list problems in a file

Uninstall
---------
Delete the folder.
