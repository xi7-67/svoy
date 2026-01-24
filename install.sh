#!/bin/bash
set -e  # Exit on any error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Check for cargo
command -v cargo >/dev/null 2>&1 || error "Cargo not found. Please install Rust: https://rustup.rs"

# Get script directory (so it works from anywhere)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Determine binary name from Cargo.toml
BINARY_NAME=$(grep '^name = ' Cargo.toml | head -1 | sed 's/name = "\(.*\)"/\1/')
info "Building $BINARY_NAME..."

# Build release binary
if ! cargo build --release; then
    error "Build failed!"
fi

# Verify binary exists
BINARY_PATH="target/release/$BINARY_NAME"
if [[ ! -f "$BINARY_PATH" ]]; then
    error "Binary not found at $BINARY_PATH"
fi

# Install paths
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"

# Install binary
info "Installing binary to $BIN_DIR..."
mkdir -p "$BIN_DIR"
cp "$BINARY_PATH" "$BIN_DIR/"
chmod +x "$BIN_DIR/$BINARY_NAME"

# Find and install desktop file (prefer root, fallback to packaging/)
DESKTOP_FILE=""
if [[ -f "sakura.desktop" ]]; then
    DESKTOP_FILE="sakura.desktop"
elif [[ -f "packaging/sakura.desktop" ]]; then
    DESKTOP_FILE="packaging/sakura.desktop"
fi

if [[ -n "$DESKTOP_FILE" ]]; then
    info "Installing desktop entry to $APP_DIR..."
    mkdir -p "$APP_DIR"
    
    # Copy desktop file and fix Exec path to be absolute
    cp "$DESKTOP_FILE" "$APP_DIR/sakura.desktop"
    sed -i "s|Exec=sakura|Exec=$BIN_DIR/$BINARY_NAME|g" "$APP_DIR/sakura.desktop"
    
    # Update desktop database if available
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
    
    # Update MIME database so file managers recognize the app
    MIME_DIR="$HOME/.local/share/mime"
    if command -v update-mime-database >/dev/null 2>&1 && [[ -d "$MIME_DIR" ]]; then
        info "Updating MIME database..."
        update-mime-database "$MIME_DIR" 2>/dev/null || true
    fi
else
    warn "No desktop file found, skipping desktop entry installation."
fi

# Check if ~/.local/bin is in PATH
if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    warn "$BIN_DIR is not in your PATH!"
    echo ""
    echo "Add this to your shell config (~/.bashrc, ~/.zshrc, or ~/.config/fish/config.fish):"
    echo ""
    echo "  # For bash/zsh:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo "  # For fish:"
    echo "  fish_add_path ~/.local/bin"
    echo ""
fi

echo ""
info "✓ Installation complete!"
echo "  Run '$BINARY_NAME' or find it in your app launcher."
