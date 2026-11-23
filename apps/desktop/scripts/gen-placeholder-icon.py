"""Generates the placeholder app icon (D-107): assets/windows/ytdlpd.ico, assets/macos/ytdlpd.icns
and assets/tray/icon-64.rgba.

Rounded indigo square with a white download arrow. Standard library only:
    python scripts/gen-placeholder-icon.py
Replace with the real icon later (icon-creator); the file names are what the code references.
"""
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BG = (79, 70, 229)
FG = (255, 255, 255)
SS = 4  # supersampling


def inside(x, y):
    """Coverage test in unit coordinates 0..1: returns 'bg', 'fg' or None."""
    r = 0.22
    cx = min(max(x, r), 1 - r)
    cy = min(max(y, r), 1 - r)
    if (x - cx) ** 2 + (y - cy) ** 2 > r * r:
        return None
    # arrow shaft
    if 0.43 <= x <= 0.57 and 0.20 <= y <= 0.55:
        return "fg"
    # arrow head: triangle (0.27,0.50) (0.73,0.50) (0.5,0.74)
    if 0.50 <= y <= 0.74 and abs(x - 0.5) <= (0.74 - y) / 0.24 * 0.23:
        return "fg"
    # tray bar
    if 0.27 <= x <= 0.73 and 0.78 <= y <= 0.85:
        return "fg"
    return "bg"


def render(n, pad=0.0):
    """pad: transparent margin per side, as a fraction of n (macOS icons sit inside a 824/1024 grid)."""
    px = bytearray()
    for j in range(n):
        for i in range(n):
            a = r = g = b = 0
            for sj in range(SS):
                for si in range(SS):
                    u = ((i + (si + 0.5) / SS) / n - pad) / (1 - 2 * pad)
                    v = ((j + (sj + 0.5) / SS) / n - pad) / (1 - 2 * pad)
                    k = inside(u, v) if 0 <= u <= 1 and 0 <= v <= 1 else None
                    if k is None:
                        continue
                    c = FG if k == "fg" else BG
                    a += 255
                    r += c[0] * 255
                    g += c[1] * 255
                    b += c[2] * 255
            if a:
                px += bytes((r // a, g // a, b // a, a // (SS * SS)))
            else:
                px += b"\0\0\0\0"
    return bytes(px)


def icns():
    # PNG-backed types: ic11 16@2x, ic12 32@2x, ic07 128, ic13 128@2x, ic09 512
    body = b""
    for t, n in ((b"ic11", 32), (b"ic12", 64), (b"ic07", 128), (b"ic13", 256), (b"ic08", 256), (b"ic09", 512)):
        d = png(n, render(n, pad=100 / 1024))
        body += t + struct.pack(">I", 8 + len(d)) + d
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def png(n, rgba):
    raw = b"".join(b"\0" + rgba[y * n * 4:(y + 1) * n * 4] for y in range(n))

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d))

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(sizes):
    imgs = [png(n, render(n)) for n in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    off = 6 + 16 * len(sizes)
    ent = b""
    for n, d in zip(sizes, imgs):
        ent += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(d), off)
        off += len(d)
    return head + ent + b"".join(imgs)


(ROOT / "assets/windows").mkdir(parents=True, exist_ok=True)
(ROOT / "assets/tray").mkdir(parents=True, exist_ok=True)
(ROOT / "assets/macos").mkdir(parents=True, exist_ok=True)
(ROOT / "assets/macos/ytdlpd.icns").write_bytes(icns())
(ROOT / "assets/windows/ytdlpd.ico").write_bytes(ico([16, 32, 48, 256]))
(ROOT / "assets/tray/icon-64.rgba").write_bytes(render(64))
(ROOT / "assets/tray/icon-64.png").write_bytes(png(64, render(64)))
print("ok")
