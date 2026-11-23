# ytdlpd

A native desktop downloader for yt-dlp, written in Rust with GPUI. GPL-3.0-or-later.


## Develop
Layout: `apps/desktop` (workspace, crates in `crates/`), `packages/gpui` (vendored GPUI), `docs/` (plan, architecture, design).

```
bun install
bun run setup    # once: downloads the pinned CPython runtime + yt-dlp into apps/desktop/runtime (needs network)
bun run dev      # cargo run -p ytdlpd (debug build; first gpui build takes 10-20 minutes)
bun run check    # cargo check --workspace
cd apps/desktop && cargo test --workspace
```
`bun run setup` writes `runtime/pyo3.cfg`; `.cargo/config.toml` points `PYO3_CONFIG_FILE` at it, and `build.rs` makes libpython findable at run time (Windows: the Python DLLs are copied next to the exe in `target/<profile>/`; macOS/Linux: an rpath into `runtime/`). Re-run it after a `runtime.lock.json` change (`--force` rebuilds).

### Prerequisites
- All: [rustup](https://rustup.rs) (the toolchain in `apps/desktop/rust-toolchain.toml` installs itself), [bun](https://bun.sh), git, tar (bundled with Windows 10+ and macOS/Linux), network for `setup`.
- Windows 10/11: Visual Studio Build Tools (MSVC, "Desktop development with C++") and the Windows 11 SDK. PowerShell or Git Bash.
- macOS 13+ (Apple Silicon or Intel): Xcode Command Line Tools (`xcode-select --install`). Full Xcode is only needed if the Metal shader step asks for it (`sudo xcode-select -s /Applications/Xcode.app`, then `xcodebuild -downloadComponent MetalToolchain`).
- Linux x64/arm64 (Ubuntu/Debian names): `sudo apt install build-essential pkg-config cmake libfontconfig1-dev libfreetype-dev libx11-dev libxcb1-dev libxcursor-dev libxi-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libdbus-1-dev`, plus a Vulkan driver. The linker is the system default; to use mold set `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"`.

### Where things live
Debug builds use `ytdlpd.dev`, release builds `ytdlpd` as the folder name.
| | Windows | macOS | Linux |
|---|---|---|---|
| settings.json | `%APPDATA%\ytdlpd.dev` | `~/Library/Application Support/ytdlpd.dev` | `~/.config/ytdlpd.dev` |
| database, tools, logs (`logs/`) | `%LOCALAPPDATA%\ytdlpd.dev` | `~/Library/Application Support/ytdlpd.dev` | `~/.local/share/ytdlpd.dev` |
| cache | `%LOCALAPPDATA%\ytdlpd.dev\cache` | `~/Library/Caches/ytdlpd.dev` | `~/.cache/ytdlpd.dev` |

`--data-dir <dir>` puts everything under `<dir>/{config,data,cache}` (separate instance, no deep-link registration). Log level: `RUST_LOG=ytdlpd=debug,engine=debug bun run dev`.

### Demo scenes and self-test
```
cargo run -p ytdlpd -- --demo downloading      # also: empty, mixed (10k rows), failed, first-run; in-memory data, fake backend
cargo run -p ytdlpd -- --self-test             # headless: starts the embedded Python, prints versions, exit 0 when yt-dlp imports
cargo run -p ytdlpd -- --frame-stats           # frame time log
```

### Troubleshooting
- `PYO3_CONFIG_FILE` / "no such file runtime/pyo3.cfg" or pyo3 build errors: run `bun run setup`. In a git worktree `runtime/` is not shared: run setup there too (or junction/symlink `apps/desktop/runtime`).
- "python runtime not found" at start or `--self-test` fails: setup was skipped or the runtime moved; `bun run setup -- --force`. `YTDLPD_PYTHON_HOME=<dir>` overrides the location.
- Windows `python314.dll` not found when running an exe by hand: copy it from `runtime/x86_64-pc-windows-msvc/python/` next to the exe (cargo run/test do this for you).
- macOS "image not found libpython3.14.dylib": the dev rpath is absolute, so a moved `runtime/` breaks it; run setup again and `cargo clean -p ytdlpd -p engine`.
- Build without Python (fake backend, no setup needed, no real downloads): `cargo run -p ytdlpd --no-default-features`; the `engine` crate itself always needs the runtime, so use `-p ytdlpd` (and `-p state` etc.) instead of `--workspace` without setup.
- Linux `libxkbcommon` / `vulkan` link errors: install the packages above.

### Local release builds
From the repo root (output in `apps/desktop/dist/`):
- Windows x64: `bun run release:win` builds the release exe, stages it with the Python runtime, runs `--self-test` on the staged tree and writes `ytdlpd-<version>-windows-x64-setup.exe`. Needs Inno Setup 6 (`winget install JRSoftware.InnoSetup`). On the dev box set `CARGO_TARGET_DIR`, `TMP` and `TEMP` to a non-C: drive first.
- macOS arm64 (run on an Apple Silicon Mac): `bun run release:mac` builds `ytdlpd.app` and `ytdlpd-<version>-macos-arm64.dmg`, ad-hoc signed, and runs `--self-test`. Needs Xcode (command line tools) and bun. Set `CODESIGN_IDENTITY` / `NOTARY_PROFILE` for a signed, notarised build.

### Packaging
Per OS, producing the installer in `apps/desktop/dist/`: `pwsh scripts/package-windows.ps1` (needs Inno Setup 6), `bash scripts/package-macos.sh` (`.app` + dmg, ad-hoc signed; set `CODESIGN_IDENTITY` and `NOTARY_PROFILE` to ship), `bash scripts/package-linux.sh` (AppImage). A `v*` tag builds all six in `.github/workflows/release.yml`. Third-party licenses: `THIRD-PARTY.md` (regenerate with `python scripts/gen-third-party.py`).
