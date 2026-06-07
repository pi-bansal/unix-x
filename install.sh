#!/usr/bin/env bash
# aitoolx installer — Linux and macOS
# Windows: use install.ps1 instead
set -euo pipefail

REPO="pi-bansal/aitoolx"
INSTALL_DIR="${AITOOLX_INSTALL_DIR:-/usr/local/bin}"
TOOLS=(lx px logx dx arcx envx netx jsonx procx idx diffx memx statx hashx termx astx dnsx)

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
  x86_64)        ARCH_TAG="x86_64" ;;
  arm64|aarch64) ARCH_TAG="aarch64" ;;
  *) echo "Unsupported arch: $ARCH" && exit 1 ;;
esac

case "$OS" in
  linux)  PLATFORM="linux-${ARCH_TAG}" ;;
  darwin) PLATFORM="macos-${ARCH_TAG}" ;;
  *) echo "Unsupported OS: $OS — on Windows, run install.ps1" && exit 1 ;;
esac

echo "aitoolx installer"
echo "Platform : $PLATFORM"
echo "Install  : $INSTALL_DIR"
echo ""

if [ "${BUILD_FROM_SOURCE:-0}" != "1" ]; then
  LATEST=$(curl -sf "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)".*/\1/')

  if [ -n "$LATEST" ]; then
    URL="https://github.com/${REPO}/releases/download/${LATEST}/aitoolx-${PLATFORM}.tar.gz"
    TMP=$(mktemp -d)
    trap 'rm -rf "$TMP"' EXIT

    echo "Downloading $URL ..."
    curl -fL "$URL" | tar -xz -C "$TMP"

    for tool in "${TOOLS[@]}"; do
      install -m 755 "$TMP/$tool" "$INSTALL_DIR/$tool"
      echo "  ✓ $tool"
    done

    echo ""
    echo "Done. Run 'lx --help' to verify."
    exit 0
  fi

  echo "No release found — building from source..."
fi

# Build from source fallback
command -v cargo &>/dev/null || { echo "Rust not found. Install from https://rustup.rs"; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
git clone --depth=1 "https://github.com/${REPO}.git" "$TMP/aitoolx"
cd "$TMP/aitoolx"
cargo build --workspace --release

for tool in "${TOOLS[@]}"; do
  install -m 755 "target/release/$tool" "$INSTALL_DIR/$tool"
  echo "  ✓ $tool"
done

echo ""
echo "Done. Run 'lx --help' to verify."
