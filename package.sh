#!/usr/bin/env bash
# Builds .deb, .rpm, .AppImage and .exe into ./dist
#   ./package.sh            everything
#   ./package.sh linux      .deb .rpm .AppImage only
#   ./package.sh windows    .exe only (needs mingw: Fedora `dnf install mingw64-gcc`, Debian `apt install mingw-w64`)
set -euo pipefail
cd "$(dirname "$0")"
WHAT="${1:-all}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
mkdir -p dist

linux() {
  cargo build --release
  cp target/release/opcore-simplify dist/opcore-simplify-linux-x86_64

  command -v cargo-deb >/dev/null || cargo install --locked cargo-deb
  cargo deb --no-build --output "dist/opcore-simplify_${VERSION}_amd64.deb"

  command -v cargo-generate-rpm >/dev/null || cargo install --locked cargo-generate-rpm
  cargo generate-rpm --output "dist/opcore-simplify-${VERSION}-1.x86_64.rpm"

  # AppImage
  local app=target/AppDir
  rm -rf "$app"
  mkdir -p "$app/usr/bin"
  cp target/release/opcore-simplify "$app/usr/bin/"
  cp packaging/opcore-simplify.desktop packaging/opcore-simplify.png "$app/"
  printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/opcore-simplify" "$@"\n' > "$app/AppRun"
  chmod +x "$app/AppRun"
  if [ ! -x target/appimagetool ]; then
    curl -fL -o target/appimagetool \
      https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x target/appimagetool
  fi
  ARCH=x86_64 target/appimagetool --appimage-extract-and-run "$app" "dist/opcore-simplify-${VERSION}-x86_64.AppImage"
}

windows() {
  if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
    echo "Windows skipped: install mingw64-gcc (Fedora) or mingw-w64 (Debian/Ubuntu)" >&2
    return 0
  fi
  rustup target add x86_64-pc-windows-gnu
  cargo build --release --target x86_64-pc-windows-gnu
  cp target/x86_64-pc-windows-gnu/release/opcore-simplify.exe dist/opcore-simplify-windows-x86_64.exe
}

case "$WHAT" in
  linux) linux ;;
  windows) windows ;;
  all) linux; windows ;;
  *) echo "usage: $0 [all|linux|windows]" >&2; exit 1 ;;
esac
ls -la dist
