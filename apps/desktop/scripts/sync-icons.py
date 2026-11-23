"""Copies the app icon set (repo icons/ytdlpd, made with icon-creator) into assets/ and writes the
raw tray icons: assets/tray/icon-64.rgba (colour) and icon-template-64.rgba (macOS menu bar, black
plus alpha; macOS tints it). Standard library only:
    python scripts/sync-icons.py
"""
import shutil
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SET = ROOT.parent.parent / "icons" / "ytdlpd"
TRAY = ROOT.parent.parent / "icons" / "tray"


def rgba(png: bytes) -> tuple[int, int, bytes]:
    """Decodes an 8-bit RGBA, non-interlaced PNG (what the renderer writes) to raw RGBA rows."""
    assert png[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w = 8, b"", 0
    while pos < len(png):
        n, kind = struct.unpack(">I4s", png[pos:pos + 8])
        body = png[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, color, interlace) == (8, 6, 0), "needs an 8-bit RGBA non-interlaced PNG"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw, stride, out = zlib.decompress(idat), w * 4, bytearray()
    prev = bytearray(stride)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b, c = prev[i], prev[i - 4] if i >= 4 else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        out += line
        prev = line
    assert len(out) == w * h * 4
    return w, h, bytes(out)


def raw_tray(png: Path, dest: Path):
    w, h, data = rgba(png.read_bytes())
    assert (w, h) == (64, 64), f"{png} is {w}x{h}, the tray wants 64x64"
    dest.write_bytes(data)


assets = ROOT / "assets"
for sub in ("windows", "macos", "linux", "tray"):
    (assets / sub).mkdir(parents=True, exist_ok=True)
shutil.copyfile(SET / "windows/ytdlpd.ico", assets / "windows/ytdlpd.ico")
shutil.copyfile(SET / "macos/ytdlpd.icns", assets / "macos/ytdlpd.icns")
shutil.copyfile(SET / "linux/hicolor/512x512/apps/ytdlpd.png", assets / "linux/ytdlpd.png")
tray = SET / "linux/hicolor/64x64/apps/ytdlpd.png"
shutil.copyfile(tray, assets / "tray/icon-64.png")
raw_tray(tray, assets / "tray/icon-64.rgba")
raw_tray(TRAY / "tray-template-64.png", assets / "tray/icon-template-64.rgba")
print("ok")
