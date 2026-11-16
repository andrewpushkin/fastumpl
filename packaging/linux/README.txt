fastumpl - portable Linux build
==============================

fastumpl is a UML class diagram editor that lays out your diagrams for you.

Run
---
    ./fastumpl                    open the editor
    ./fastumpl diagram.umpl       open a diagram
    ./umpl render d.umpl -o d.svg draw a diagram without a window

Needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later),
GTK 3 (for file dialogs) and an X11 or Wayland desktop.

Menu entry (optional)
---------------------
    mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
    cp fastumpl umpl ~/.local/bin/
    cp fastumpl.desktop ~/.local/share/applications/
    cp fastumpl.svg ~/.local/share/icons/hicolor/scalable/apps/

Settings and recovery copies live in ~/.config/fastumpl and ~/.cache/fastumpl.
For a system-wide install with file associations, use the .deb package.
