#!/usr/bin/env python3
"""Write src/ui/avatar/pixelbot.rs from DiceBear's Pixelbot style.

    tools/gen-pixelbot.py <path to pixelbot.json>

pixelbot.json is the style's definition, in the dicebear-styles crate (or
the @dicebear/pixelbot npm package). Pixelbot is DiceBear's own style,
licensed CC0 1.0. It draws a 12 by 12 grid of rounded squares on a dark
background, a few of them lit in a glow color as eyes and a mouth. Mote
draws that grid itself (src/ui/avatar.rs), so only which cells each eye
and mouth lights, and the colors, are kept. Its animations are left out:
the eyes are drawn open and the mouth at rest.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src/ui/avatar/pixelbot.rs"

# Each cell's path starts at its top edge, after the rounded corner: one
# corner radius right of the square's left side.
RADIUS = 1.9
MARGIN = 0.42
STEP = 100 / 12


def starts(path, dx=0.0, dy=0.0):
    """The point each subpath starts at, moved by (dx, dy)."""
    points, here = [], (0.0, 0.0)
    for cmd, x, y in re.findall(r"([Mm])\s*(-?\d*\.?\d+)\s*,?\s*(-?\d*\.?\d+)", path):
        x, y = float(x), float(y)
        here = (x, y) if cmd == "M" else (here[0] + x, here[1] + y)
        points.append((here[0] + dx, here[1] + dy))
    return points


def cell(point):
    col = round((point[0] - RADIUS - MARGIN) / STEP)
    row = round((point[1] - MARGIN) / STEP)
    assert 0 <= col < 12 and 0 <= row < 12, point
    return col, row


def translate(element):
    numbers = re.findall(r"-?\d*\.?\d+", element["attributes"]["transform"])
    return float(numbers[0]), float(numbers[1])


def shapes(definition, name):
    canvas = {e["name"]: e for e in definition["canvas"]["elements"]}
    dx, dy = translate(canvas[name])
    out = []
    for variant, spec in definition["components"][name]["variants"].items():
        lit = set()
        for element in spec["elements"]:
            # Shown only while animated: the wide smile the mouth breaks into.
            if element["attributes"].get("opacity") == "0":
                continue
            lit.update(cell(p) for p in starts(element["attributes"]["d"], dx, dy))
        out.append((variant, sorted(lit, key=lambda c: (c[1], c[0]))))
    return out


def colors(values):
    return ", ".join("0x" + v.lstrip("#").lower() for v in values)


def main():
    definition = json.loads(Path(sys.argv[1]).read_text())
    license_name = definition["meta"]["license"]["name"]
    assert license_name == "CC0 1.0", license_name
    grid = starts(definition["canvas"]["elements"][0]["children"][0]["attributes"]["d"])
    assert len({cell(p) for p in grid}) == 144, "the grid is no longer 12 by 12"

    def table(name, rust):
        rows = []
        for variant, cells in shapes(definition, name):
            body = ", ".join(f"({c}, {r})" for c, r in cells)
            rows.append(f"    // {variant}\n    &[{body}],")
        return f"pub(super) const {rust}: &[&[(u8, u8)]] = &[\n" + "\n".join(rows) + "\n];\n"

    text = (
        "//! DiceBear's Pixelbot (https://www.dicebear.com), CC0 1.0: the cells\n"
        "//! each eye and mouth lights, as (column, row) on its 12 by 12 grid,\n"
        "//! and its colors. Written by tools/gen-pixelbot.py; don't edit.\n\n"
        f"pub(super) const BACKGROUNDS: &[u32] = &[{colors(definition['colors']['background']['values'])}];\n\n"
        f"pub(super) const GLOWS: &[u32] = &[{colors(definition['colors']['glow']['values'])}];\n\n"
        + table("eyes", "EYES")
        + "\n"
        + table("mouth", "MOUTHS")
    )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text)
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
