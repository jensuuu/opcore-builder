# Windows build: .\build.ps1   (needs rustup)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force dist | Out-Null
cargo build --release
Copy-Item target\release\opcore-simplify.exe dist\opcore-simplify-windows-x86_64.exe
