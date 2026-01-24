<div align="center">
  <h1>Sakura🌸</h1>
  <img src="resources/glimpse.gif" alt="Sakura Preview" width="457" height="459">
  <p>A simple, fast and chromeless image viewer with native support for tiling window managers. Its built in rust with egui and native to linux. The project is still under development and <b>HEAVILY</b> vibe-coded.</p>
</div>

## Features

- file sharing with localsend-rs
- image info
- image rotation in 90°
- converting image into png, jpg
- image editing

### Todos

- [x] Add localsend sending option
- [x] make it downloadable via a package manager/sharing only the executable
- [ ] make a great README.md file.

## Installation

### Manual Installation (Any Linux)

To install `sakura` and integrate it with your desktop environment:

clone it, then

```bash
./install.sh
```

This will:

1. Build the release binary.
2. Install `sakura` to `~/.local/bin/`.
3. Install the desktop file to `~/.local/share/applications/`.

### Portable usage

You can simply share the binary found in `target/release/sakura` after building. It is self-contained (fonts are embedded).

### Arch Linux

Download it with your favourite aur helper.
