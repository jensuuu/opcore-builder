# opcore-builder

Generates an OpenCore EFI for the detected hardware (Linux and Windows).

    opcore-builder                 interactive menu (report, compatibility, kexts, ACPI, SMBIOS, build)
    opcore-builder --yes           build straight away with the recommended settings
    opcore-builder [--macos "Sonoma"|14] [--out EFI-Output] [--dvmt-60]

Set `GITHUB_TOKEN` to avoid GitHub API rate limits.

## Build for contributors
- Native: `cargo build --release`
- Packages (from Linux): `./package.sh` -> `dist/` with `.deb`, `.rpm`, `.AppImage`, `.exe`
  - `./package.sh linux` / `./package.sh windows` for one side only
  - `.exe` needs `mingw64-gcc` (Fedora) or `mingw-w64` (Debian/Ubuntu)
  - `.AppImage` downloads appimagetool on first run
- `just build` does the same; on Windows `just build` runs `build.ps1`
- CI: `.github/workflows/build.yml` builds all of them and attaches them to `v*` releases
