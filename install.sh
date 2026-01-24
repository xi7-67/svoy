#!/bin/bash

# Build release binary
echo "Building release binary..."
cargo build --release

# Install binary
echo "Installing binary to ~/.local/bin/..."
mkdir -p ~/.local/bin
cp target/release/sakura ~/.local/bin/

# Install desktop file
echo "Installing desktop entry to ~/.local/share/applications/..."
mkdir -p ~/.local/share/applications
cp packaging/sakura.desktop ~/.local/share/applications/

# Update desktop database
update-desktop-database ~/.local/share/applications/

echo "Done! You can now run 'sakura' from the terminal or find it in your app launcher."
