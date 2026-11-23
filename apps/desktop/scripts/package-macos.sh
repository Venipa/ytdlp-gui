#!/usr/bin/env bash
# Builds ytdlpd.app (+ dmg) for the host mac arch. Untested by the author: run on a mac, report problems.
# Usage: scripts/package-macos.sh [out-dir]
# Env: CODESIGN_IDENTITY (default "-" = ad-hoc; use "Developer ID Application: ..." for distribution),
#      NOTARY_PROFILE (optional notarytool keychain profile; submits and staples the dmg when set).
#      or APPLE_API_KEY (.p8 path) + APPLE_API_KEY_ID + APPLE_API_ISSUER (App Store Connect API key) to notarise.
# Layout: Contents/MacOS/ytdlpd, Contents/Frameworks/libpython3.14.dylib (exe rpath @executable_path/../Frameworks),
#         Contents/Resources/python/{lib/python3.14,site,...} (engine finds it via <exe>/../Resources/python).
set -euo pipefail
cd "$(dirname "$0")/.."
desk="$PWD"
repo="$(cd ../.. && pwd)"
case "$(uname -m)" in arm64) triple=aarch64-apple-darwin; arch=arm64 ;; *) triple=x86_64-apple-darwin; arch=x64 ;; esac
out="${1:-$desk/dist}"
id="${CODESIGN_IDENTITY:--}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="$out/ytdlpd.app"

bun scripts/setup.ts --triple "$triple"
cargo build -p ytdlpd --release --locked --target "$triple"
bin="${CARGO_TARGET_DIR:-$desk/target}/$triple/release/ytdlpd"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/ytdlpd"
cp -R "runtime/$triple/python" "$app/Contents/Resources/python"
# only the exe links libpython; the dylib id is already @rpath/libpython3.14.dylib (checked on PBS 20261110)
mv "$app/Contents/Resources/python/lib/libpython3.14.dylib" "$app/Contents/Frameworks/"
cp "$repo/THIRD-PARTY.md" "$repo/LICENSE" "$app/Contents/Resources/"
[ -f assets/macos/ytdlpd.icns ] && cp assets/macos/ytdlpd.icns "$app/Contents/Resources/"

# drop the dev rpath (absolute path into runtime/) that build.rs adds
dev_lib="$(sed -n 's/^lib_dir=//p' runtime/pyo3.cfg)"
install_name_tool -delete_rpath "$dev_lib" "$app/Contents/MacOS/ytdlpd" 2>/dev/null || true

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>ytdlpd</string>
<key>CFBundleDisplayName</key><string>ytdlpd</string>
<key>CFBundleIdentifier</key><string>dev.ytdlpd.app</string>
<key>CFBundleExecutable</key><string>ytdlpd</string>
<key>CFBundleIconFile</key><string>ytdlpd</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>ytdlpd</string><key>CFBundleURLSchemes</key><array><string>ytdlpd</string></array></dict></array>
</dict></plist>
PLIST

# sign every Mach-O inside out (notarisation wants each one signed, hardened runtime, no --deep)
opts=(--force --timestamp --options runtime --sign "$id")
[ "$id" = "-" ] && opts=(--force --sign -)
find "$app/Contents" -type f \( -name '*.so' -o -name '*.dylib' -o -path '*/python/bin/*' \) -print0 | xargs -0 -n1 codesign "${opts[@]}"
if [ "$id" != "-" ]; then
  # hardened runtime: embedded CPython (ctypes/libffi closures) needs executable memory
  ent="$(mktemp)"
  cat > "$ent" <<'ENT'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>com.apple.security.cs.allow-jit</key><true/>
<key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
</dict></plist>
ENT
  codesign "${opts[@]}" --entitlements "$ent" "$app"
  rm -f "$ent"
else
  codesign "${opts[@]}" "$app"
fi
codesign --verify --strict --verbose=2 "$app"

dmg="$out/ytdlpd-$version-macos-$arch.dmg"
rm -f "$dmg"
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname ytdlpd -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"
[ "$id" != "-" ] && codesign --force --timestamp --sign "$id" "$dmg"
if [ -n "${NOTARY_PROFILE:-}" ]; then
  xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg"
elif [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ "$id" != "-" ]; then
  # App Store Connect API key (.p8 file path, key id, issuer id), as used in CI
  xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" --wait
  xcrun stapler staple "$dmg"
fi
# smoke: runs python from the bundle, prints versions
"$app/Contents/MacOS/ytdlpd" --self-test
echo "dmg: $dmg"
