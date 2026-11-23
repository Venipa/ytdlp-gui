#!/usr/bin/env bash
# Builds an AppImage for the host arch. Untested by the author: run on Linux, report problems.
# Usage: scripts/package-linux.sh [out-dir]    Needs: bun, rustup, curl, a FUSE-less run uses --appimage-extract-and-run.
# Layout: usr/bin/ytdlpd, usr/lib/ytdlpd/python (exe rpath $ORIGIN/../lib/ytdlpd/python/lib).
set -euo pipefail
cd "$(dirname "$0")/.."
desk="$PWD"
repo="$(cd ../.. && pwd)"
case "$(uname -m)" in aarch64) triple=aarch64-unknown-linux-gnu; march=aarch64; arch=arm64 ;; *) triple=x86_64-unknown-linux-gnu; march=x86_64; arch=x64 ;; esac
out="${1:-$desk/dist}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
appdir="$out/ytdlpd.AppDir"

bun scripts/setup.ts --triple "$triple"
cargo build -p ytdlpd --release --locked
bin="${CARGO_TARGET_DIR:-$desk/target}/release/ytdlpd"

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib/ytdlpd" "$appdir/usr/share/doc/ytdlpd"
cp "$bin" "$appdir/usr/bin/ytdlpd"
cp -R "runtime/$triple/python" "$appdir/usr/lib/ytdlpd/python"
cp "$repo/THIRD-PARTY.md" "$repo/LICENSE" "$appdir/usr/share/doc/ytdlpd/"
icon=assets/linux/ytdlpd.png
[ -f "$icon" ] || icon=assets/tray/icon-64.png
cp "$icon" "$appdir/ytdlpd.png"
cat > "$appdir/ytdlpd.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=ytdlpd
Comment=Native yt-dlp downloader
Exec=ytdlpd %u
Icon=ytdlpd
Terminal=false
Categories=Network;
MimeType=x-scheme-handler/ytdlpd;
DESKTOP
printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/ytdlpd" "$@"\n' > "$appdir/AppRun"
chmod +x "$appdir/AppRun"

tool="$out/appimagetool-$march.AppImage"
[ -x "$tool" ] || { curl -fsSL -o "$tool" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$march.AppImage"; chmod +x "$tool"; }
img="$out/ytdlpd-$version-linux-$arch.AppImage"
ARCH="$march" "$tool" --appimage-extract-and-run "$appdir" "$img"
"$appdir/usr/bin/ytdlpd" --self-test
echo "appimage: $img"
