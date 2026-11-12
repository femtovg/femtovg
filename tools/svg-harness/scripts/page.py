#!/usr/bin/env python3
"""page.py FILE.svg [FRAMING] > page.html

The browser page for one SVG at one framing (default z1), placed exactly where svg-harness draws it: a frame of the
framing's size, a zoom about its pivot, then the SVG's own viewport - its width and height, read as usvg reads
them - fitted into the box. The nested <svg> keeps the file's viewBox and preserveAspectRatio, so a viewBox whose
aspect differs from the viewport's is letterboxed as usvg letterboxes it; sizing the viewBox itself to the box
misplaces files such as width="200" height="200" on a 1280x1024 viewBox by up to 20 px.
"""
import html as html_lib
import re

from common import FRAMINGS, cli

# Every presentation attribute the root can carry, by its full name. Dropping them from the nested <svg> loses
# e.g. a root fill="none" or round line caps on the browser side only.
PRES = ("fill|fill-opacity|fill-rule|stroke|stroke-width|stroke-linecap|stroke-linejoin|stroke-miterlimit|stroke-dasharray|"
        "stroke-dashoffset|stroke-opacity|opacity|style|color|clip-rule|color-interpolation|color-interpolation-filters|"
        "paint-order|shape-rendering|text-rendering|image-rendering|visibility|direction|writing-mode|dominant-baseline|"
        "font|font-family|font-size|font-style|font-weight|font-variant|font-stretch|text-anchor|letter-spacing|word-spacing|"
        "marker|marker-start|marker-mid|marker-end|stop-color|stop-opacity|flood-color|flood-opacity|lighting-color")
UNITS = {"": 1, "px": 1, "mm": 96 / 25.4, "cm": 96 / 2.54, "in": 96, "pt": 96 / 72, "pc": 16, "em": 12, "ex": 6}


def page(svg_path, framing):
    """The page's HTML, or None when the file has no <svg> root to frame."""
    t = open(svg_path, encoding='utf-8', errors='replace').read()
    # Source-viewer pages saved as .svg: recover the XML from their <code> blocks.
    if "<code" in t and "<svg" in t:
        t = html_lib.unescape(re.sub(r"<[^>]+>", "", "".join(re.findall(r"<code[^>]*>(.*?)</code>", t, re.S))))
        t = t[t.index("<svg"):t.rindex("</svg>") + 6]
    m = re.search(r"<svg\b([^>]*)>", t)
    if not m or "</svg>" not in t:
        return None
    attrs, inner = m.group(1), t[m.end():t.rindex("</svg>")]
    vb = re.search(r'viewBox="([^"]*)"', attrs)
    vb = vb.group(1) if vb else None
    try:
        vbw, vbh = [float(v) for v in vb.replace(",", " ").split()[2:4]] if vb else (None, None)
    except ValueError:
        return None

    def length(name):
        """The root width/height as usvg resolves it (usvg::parser::converter::convert_size): unitless or px as is,
        a percentage of the viewBox (100 x 100 without one), absolute units at 96 dpi, em/ex at the default 12 px
        font; None when the attribute is missing."""
        m = re.search(rf'\b{name}="\s*([\d.]+(?:[eE][-+]?\d+)?)\s*([a-zA-Z%]*)\s*"', attrs)
        if not m:
            return None
        n, u = float(m.group(1)), m.group(2).lower()
        if u == "%":
            return n / 100 * ((vbw if name == "width" else vbh) if vb else 100.0)
        return n * UNITS.get(u, 1)

    w, h = length("width"), length("height")
    # One missing: usvg derives it from the other through the viewBox aspect; both missing: the viewBox (100 x 100
    # without one).
    if w is None and h is None:
        w, h = (vbw, vbh) if vb else (100.0, 100.0)
    elif w is None:
        w = h * vbw / vbh if vb else 100.0
    elif h is None:
        h = w * vbh / vbw if vb else 100.0
    if not w or not h:
        return None
    if not vb:  # no viewBox: the viewport is the user space
        vb = f"0 0 {w:g} {h:g}"
    par = re.search(r'preserveAspectRatio="([^"]*)"', attrs)
    par = par.group(1) if par else "xMidYMid meet"
    keep = " ".join(m.group(0) for m in re.finditer(r'(?<![\w:-])(?:' + PRES + r')="[^"]*"', attrs))
    f = FRAMINGS[framing] if isinstance(framing, str) else framing
    (fw, fh), (box, bx, by), (px, py) = f.frame, f.box, f.pivot
    fit = box / max(w, h)
    # The charset: WebKit reads a file page without one as windows-1252 (emoji and CJK text come out garbled).
    return f'''<!doctype html><meta charset="utf-8"><body style="margin:0;background:#fff">
<svg width="{fw}" height="{fh}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
<g transform="translate({px:g},{py:g}) scale({f.zoom:g}) translate({-px:g},{-py:g})"><svg x="{bx}" y="{by}" width="{w * fit:.6g}" height="{h * fit:.6g}" viewBox="{vb}" preserveAspectRatio="{par}" {keep}>
{inner}
</svg></g></svg>
'''


if __name__ == '__main__':
    pos, _, _ = cli(__doc__, (1, 2))
    out = page(pos[0], pos[1] if len(pos) > 1 else 'z1')
    if out is None:
        raise SystemExit(f'{pos[0]}: no <svg> root to frame')
    print(out, end='')
