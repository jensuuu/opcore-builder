set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default: build

# .deb .rpm .AppImage + Windows .exe, built from Linux
[unix]
build:
    bash ./package.sh

[unix]
linux:
    bash ./package.sh linux

# Windows .exe only, cross-compiled from Linux (needs mingw-w64)
[unix]
win:
    mkdir -p dist
    rustup target add x86_64-pc-windows-gnu
    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/opcore-simplify.exe dist/opcore-simplify-windows-x86_64.exe

# Native build when running on Windows
[windows]
build:
    powershell -NoProfile -ExecutionPolicy Bypass -File ./build.ps1

[windows]
win: build
