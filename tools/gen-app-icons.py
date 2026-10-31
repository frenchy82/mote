#!/usr/bin/env python3
"""Regenerate the app icon from its master, data/icons/src/default.png.

The master is a 1024² PNG drawn edge to edge. GNOME's app icon grid puts a
square icon in 104 of its 128 pixels, so each size is the master shrunk to
that box and centered; without the margin Mote would stand larger than the
icons beside it in the dash and the overview.

Writes the hicolor icons (data/icons/hicolor/<n>x<n>/apps/co.hyprlab.Mote.png,
which data/mote.gresource.xml also embeds in the binary) and the README
artwork, data/repo/logo.png.

Requires ImageMagick.
"""
import pathlib, subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
MASTER = ROOT / "data/icons/src/default.png"
HICOLOR = ROOT / "data/icons/hicolor"
APP_ID = "co.hyprlab.Mote"
SIZES = (32, 48, 64, 128, 256, 512)


def render(png: pathlib.Path, size: int) -> None:
    png.parent.mkdir(parents=True, exist_ok=True)
    body = round(size * 104 / 128)
    subprocess.run(
        ["magick", str(MASTER), "-filter", "Lanczos", "-resize", f"{body}x{body}",
         "-background", "none", "-gravity", "center", "-extent", f"{size}x{size}",
         "-strip", str(png)],
        check=True,
    )


for size in SIZES:
    render(HICOLOR / f"{size}x{size}/apps/{APP_ID}.png", size)
render(ROOT / "data/repo/logo.png", 512)
