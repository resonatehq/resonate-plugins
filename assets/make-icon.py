#!/usr/bin/env python3
"""Normalize a provider logo into the repo's icon format.

    python3 assets/make-icon.py <scheme> <source.svg|source.png> [#hex]

Output: assets/<scheme>.png — 128x128 RGBA, transparent, the mark trimmed
to its alpha bounding box and centred with 8% padding, so every icon in
the catalog carries the same optical weight.

The optional #hex fills an unpainted single-path mark (a Simple Icons
silhouette) with the provider's brand colour.

The script then reports what share of the mark is distinguishable from a
light page (#ffffff) and from a dark one (#0d1117). Distinguishable, not
readable: these are colour fields, not text, so the measure is colour
distance rather than the luminance contrast used for type — a yellow
badge on white is perfectly visible at 1.7:1 luminance. Below 60% on
either background the mark needs two files, <scheme>-lb.png and
<scheme>-db.png, and a <picture> element at the use site.

Requires: pillow, cairosvg.
"""

import io
import math
import os
import re
import sys

import cairosvg
from PIL import Image

SIZE = 128
PAD = 0.08
LIGHT = (255, 255, 255)
DARK = (13, 17, 23)
DISTINCT = 60.0   # RGB euclidean distance, on a 0..441 scale
SHARE = 0.60      # of opaque pixels that must clear it


def load(path, recolor=None):
    if not path.endswith(".svg"):
        return Image.open(path).convert("RGBA")
    svg = open(path).read()
    # Illustrator exports carry a DOCTYPE with custom entities that the
    # hardened XML parser refuses.
    svg = re.sub(r"<!DOCTYPE.*?\]>", "", svg, flags=re.S)
    if recolor:
        svg = svg.replace("<path ", f'<path fill="{recolor}" ')
    png = cairosvg.svg2png(bytestring=svg.encode(), output_width=768, output_height=768)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def normalize(im):
    box = im.getchannel("A").getbbox()
    if box:
        im = im.crop(box)
    inner = int(SIZE * (1 - 2 * PAD))
    w, h = im.size
    scale = inner / max(w, h)
    im = im.resize((max(1, round(w * scale)), max(1, round(h * scale))), Image.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    canvas.paste(im, ((SIZE - im.width) // 2, (SIZE - im.height) // 2), im)
    return canvas


def visible(im, bg):
    px = [p for p in im.get_flattened_data() if p[3] > 128]
    clear = sum(
        math.dist(p[:3], bg) >= DISTINCT for p in px
    )
    return clear / len(px)


def main(scheme, source, recolor=None):
    im = normalize(load(source, recolor))
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), f"{scheme}.png")
    im.save(out)

    light, dark = visible(im, LIGHT), visible(im, DARK)
    verdict = "ok" if min(light, dark) >= SHARE else "SPLIT INTO -lb/-db"
    print(f"{out}  visible on light {light:.0%}  on dark {dark:.0%}  -> {verdict}")


if __name__ == "__main__":
    main(*sys.argv[1:])
