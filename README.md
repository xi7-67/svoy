<div align="center">
  <h1 align="center">Sakura🌸</h1>
  ![](resources/glimpse.gif)
  <p>A simple, modern, fast and chromeless image viewer with native support for tiling window managers. Built in Rust with eframe, egui, native to linux. The project is still under development and <b>HEAVILY</b> vibe-coded.</p>
</div>

## Todos

- [x] Add localsend sending option
- [x] make it downloadable via a package manager/sharing only the executable
- [ ] make a great README.md file.

## Installation

### Manual Installation (Any Linux)

To install `sakura` and integrate it with your desktop environment:

```bash
./install.sh
```

This will:

1. Build the release binary.
2. Install `sakura` to `~/.local/bin/`.
3. Install the desktop file to `~/.local/share/applications/`.

### Portable usage

You can simply share the binary found in `target/release/sakura` after building. It is self-contained (fonts are embedded).

### Arch Linux (PKGBUILD)

A template `PKGBUILD` is provided in `packaging/PKGBUILD` for creating an Arch Linux package.
