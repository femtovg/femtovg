#!/usr/bin/env python3
"""zoom.py BUILD FILE [zooms=Z,...] [ref=chromium] [framing=z1] [scale=N] [crop=X,Y,W,H] [near=24] [ms=700] [out=PATH]

A zoom gesture on one canvas, as an application zooms: `svg-harness ladder` draws FILE at each zoom in turn
without recreating the canvas, so a step can inherit state from the zooms before it. Each step is compared with a
fresh canvas at that zoom and with the browser reference of that zoom (made now when missing), and written as an
animation of femtovg | reference | difference, enlarged `scale` times with nearest neighbour: OUT/zoom/<KEY>.gif
and a lossless animated .webp (or out=PATH.gif). The table has per step the pixels that differ from the fresh
canvas (state carried over), the pixels beyond 8/255 and the share beyond 20/255 of the reference; a step is near
when at most `near` pixels are beyond 8/255. FILE is a path, a corpus key or a unique key substring.
"""
import glob, os, shutil, subprocess, sys

import numpy as np
from PIL import Image, ImageDraw

from common import (DIFF_LEVEL, MAJOR_PIXEL_POINTS, OUT, REF_NAMES, bold, build_binary, build_info, cli, delta, die,
                    frame_image, framing_for_zoom, framing_list, load, ref_label, ref_png, table)
from corpus import resolve
from refs import reference
from sheet import font, heat

# In to 4x and back, then out to a quarter and back: display scale factors and the steps between.
LADDER = [1, 1.25, 1.5, 1.75, 2, 3, 4, 3, 2, 1.75, 1.5, 1.25, 1, 0.75, 0.5, 0.33, 0.25, 0.33, 0.5, 0.75, 1]
FAINT = 8  # the near test counts pixels beyond this, finer than DIFF_LEVEL: zoom artefacts are often faint


def ladder(build, f, base, zooms, prefix):
    for old in glob.glob(prefix + '.*.png'):
        os.remove(old)
    args = base.args()
    i = args.index('--zoom')
    del args[i:i + 2]
    r = subprocess.run([build_binary(build), 'ladder', *args, '--zooms', ','.join(f'{z:g}' for z in zooms), f['path'],
                        prefix], capture_output=True, text=True, timeout=1800)
    steps = [f'{prefix}.{i:02}.png' for i in range(len(zooms))]
    if r.returncode != 0 or not all(os.path.exists(s) for s in steps):
        die(f'svg-harness ladder failed: {r.stderr.strip()[-400:]}')
    return steps


def main():
    pos, opts, _ = cli(__doc__, (2, 2), ('zooms', 'ref', 'framing', 'scale', 'crop', 'near', 'ms', 'out'))
    build, f = pos[0], resolve(pos[1])
    zooms = [float(z) for z in opts['zooms'].split(',')] if opts.get('zooms') else LADDER
    browser = opts.get('ref', 'chromium')
    base = framing_list([opts.get('framing', 'z1')])[0]
    scale, near = int(opts.get('scale', 2)), int(opts.get('near', 24))
    crop = tuple(int(v) for v in opts['crop'].split(',')) if opts.get('crop') else None
    out = opts.get('out', os.path.join(OUT, 'zoom', f['key'] + '.gif'))
    work = os.path.join(OUT, 'zoom', f['key'])
    os.makedirs(work, exist_ok=True)
    steps = [load(p) for p in ladder(build, f, framing_for_zoom(1, base), zooms, os.path.join(work, 'step'))]

    fresh, refs = {}, {}
    for z in sorted(set(zooms)):
        fr = framing_for_zoom(z, base).name
        fresh[z] = frame_image(build, f, fr)
        if reference(browser, f, fr) == 'made' or os.path.exists(ref_png(browser, f['key'], fr)):
            refs[z] = load(ref_png(browser, f['key'], fr))

    rows, frames = [], []
    label = build_info(build)['label']
    for i, (z, a) in enumerate(zip(zooms, steps)):
        r = refs.get(z)
        leak = int((delta(a, fresh[z]) > 0).sum()) if fresh.get(z) is not None else None
        if r is not None:
            d = delta(a, r)
            n8, share, mx = int((d > FAINT).sum()), 100 * float((d > DIFF_LEVEL).mean()), int(d.max())
        else:
            n8 = share = mx = None
        rows.append((i, f'{z:g}', bold(leak, bool(leak)) if leak is not None else '-',
                     '-' if n8 is None else n8, '-' if share is None else bold(f'{share:.3f}', share >= MAJOR_PIXEL_POINTS),
                     '-' if mx is None else mx, '-' if n8 is None else ('near' if n8 <= near else 'failing')))
        frames.append(strip(a, r, z, i, len(zooms), crop, scale, label, browser))

    print(f'## Zoom gesture: {f["key"]}, {label}, against {ref_label(browser)}\n')
    print(table(['step', 'zoom', 'px differing from a fresh canvas', f'px beyond {FAINT}/255',
                 f'beyond {DIFF_LEVEL}/255 (%)', 'max (/255)', f'near (<= {near} px)'], rows, 'rrrrrrl'))
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    ms = int(opts.get('ms', 700))
    frames[0].save(out, save_all=True, append_images=frames[1:], duration=ms, loop=0, optimize=True)
    webp = os.path.splitext(out)[0] + '.webp'
    frames[0].save(webp, save_all=True, append_images=frames[1:], duration=ms, loop=0, lossless=True, method=4)
    print(f'\n{out}\n{webp}', file=sys.stderr)


def strip(a, r, z, i, n, crop, scale, label, browser):
    x, y, w, h = crop or (0, 0, a.shape[1], a.shape[0])
    tiles = [(f'{label}', a), (ref_label(browser), r)]
    if r is not None:
        d = delta(a, r)
        tiles.append((f'vs {REF_NAMES[browser]}: {int((d > DIFF_LEVEL).sum())} px beyond {DIFF_LEVEL}/255',
                      np.asarray(heat(d, (0, 0, d.shape[1], d.shape[0]), 1, 1, False), dtype=np.int16)))
    ims = [(t, Image.fromarray(np.clip(im[y:y + h, x:x + w], 0, 255).astype(np.uint8)).resize((w * scale, h * scale), Image.NEAREST)
            if im is not None else None) for t, im in tiles]
    pad, top = 8, 44
    canvas = Image.new('RGB', (len(ims) * (w * scale + pad) + pad, h * scale + top + pad), (40, 40, 40))
    draw = ImageDraw.Draw(canvas)
    draw.text((pad, 4), f'zoom {z:g}   step {i + 1} of {n}', fill=(255, 255, 255), font=font(14))
    for j, (t, im) in enumerate(ims):
        px = pad + j * (w * scale + pad)
        draw.text((px, 24), t if im is not None else f'{t}: no frame', fill=(220, 220, 220), font=font(12, t))
        if im is not None:
            canvas.paste(im, (px, top))
    return canvas


if __name__ == '__main__':
    main()
