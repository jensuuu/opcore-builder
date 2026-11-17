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

## Contributing
Contributions are welcome: bug reports, hardware compatibility fixes, new kext or ACPI patch support, and documentation.

1. Fork the repository and create a branch: `git checkout -b feature/my-change`
2. Make your changes and make sure they build: `cargo build --release`
3. Run the checks before committing:
   - `cargo fmt`
   - `cargo clippy -- -D warnings`
   - `cargo test`
4. Commit with a clear message and open a pull request describing what changed and why.

When reporting a bug or compatibility problem, please include:
- your OS and the `opcore-builder` version
- the hardware report from the interactive menu (CPU, GPU, chipset, network/audio devices)
- the macOS version you targeted and the full error output

Hardware-specific fixes are easiest to review if you note which machine you tested on.

## Contributors
Thanks to everyone who has helped improve this project.

| Contributor | Role |
|-------------|------|
| [@jensuuu](https://github.com/jensuuu) | Author and maintainer |

Want to see your name here? Open a pull request.

## Credits

- [OpenCorePkg](https://github.com/acidanthera/OpenCorePkg) and the Acidanthera kexts it builds on