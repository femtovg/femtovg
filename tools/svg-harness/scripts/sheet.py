#!/usr/bin/env python3
"""sheet.py OUT.png BEFORE AFTER ROW... [refs=chromium] [zoom=N] [crop=X,Y,W,H] [full] [framings=z1] [labels=A,B]
               [others=R,...] [range=R,...] [diffs-only] [bold]

The evidence sheet: one row per frame of before | after | reference | before vs reference | after vs reference,
each panel labelled, with the share of pixels beyond 20/255 in the row's caption. A difference panel is white where
the frames agree, orange 1..20/255 off and red beyond. Frames and references missing are made now.
With BEFORE and AFTER the same build, each row is that build | reference | build vs reference.

ROW is KEY@FRAMING, or a key or unique key substring (then once per framing in framings=), followed by options
after colons and a note for the caption after a semicolon, e.g. 'gpt-6-astra@hd:at=960,400:zoom=8;the eye':
  box=X,Y,W,H         the window, in frame pixels
  at=CX,CY [size=WxH] or its centre and size (default 40x30)
  zoom=N | shrink=N   magnify (nearest neighbour) or reduce (pictures box-filtered, difference panels by the block
                      maximum, so one deviating pixel still shows)
  before=B | after=B  another build for this row
Sheet options: refs= the references (several give a set of three panels each); zoom= and crop= for every row;
without crop=, a zoomed row centres its window on where the builds differ, unless `full` keeps the whole frame. At
zoom 6 and up, panels get a faint pixel grid and the build panels outline the pixels beyond 20/255 of the first
reference. others= adds a panel per renderer against the first reference (what a browser is off by itself); range=
adds panels for how far each build lies outside the span of those references; diffs-only leaves the reference
pictures out; bold widens deviating pixels by one, so a one-pixel edge survives shrinking.
"""
import os, sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

from common import (DIFF_LEVEL, FRAMINGS, REF_NAMES, beyond, build_info, cli, die, frame, framing_list, load, ref_label,
                    ref_png)
from corpus import resolve
from refs import reference

GRID_ZOOM = 6
WINDOW = (320, 240)  # an auto window's size on the sheet, in sheet pixels


def font(size, text=''):
    wide = any(ord(c) > 0x2e7f for c in text)
    for name in (['Hiragino Sans GB.ttc', 'NotoSansCJK-Regular.ttc'] if wide else []) + \
                ['Helvetica.ttc', 'Arial.ttf', 'DejaVuSans.ttf', 'LiberationSans-Regular.ttf']:
        for d in ('/System/Library/Fonts', '/Library/Fonts', '/usr/share/fonts/truetype/dejavu',
                  '/usr/share/fonts/opentype/noto', '/usr/share/fonts/truetype/liberation', ''):
            try:
                return ImageFont.truetype(os.path.join(d, name) if d else name, size)
            except OSError:
                pass
    try:
        return ImageFont.load_default(size)
    except TypeError:
        return ImageFont.load_default()


def grid(im, zoom):
    """A faint line between enlarged pixels."""
    over = Image.new('RGBA', im.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(over)
    for x in range(0, im.width, zoom):
        d.line([(x, 0), (x, im.height)], fill=(128, 128, 128, 48))
    for y in range(0, im.height, zoom):
        d.line([(0, y), (im.width, y)], fill=(128, 128, 128, 48))
    return Image.alpha_composite(im.convert('RGBA'), over).convert('RGB')


def outline(im, mask, zoom):
    d = ImageDraw.Draw(im)
    for y, x in zip(*np.nonzero(mask)):
        d.rectangle([x * zoom, y * zoom, x * zoom + zoom - 1, y * zoom + zoom - 1], outline=(255, 0, 0))
    return im


def picture(a, box, zoom, shrink, far=None):
    x, y, w, h = box
    im = Image.fromarray(np.clip(a[y:y + h, x:x + w], 0, 255).astype(np.uint8))
    if shrink > 1:
        return im.resize((w // shrink, h // shrink), Image.BOX)
    im = im.resize((w * zoom, h * zoom), Image.NEAREST)
    if zoom >= GRID_ZOOM:
        im = grid(im, zoom)
        if far is not None:
            im = outline(im, far[y:y + h, x:x + w], zoom)
    return im


def heat(d, box, zoom, shrink, widen):
    x, y, w, h = box
    d = d[y:y + h, x:x + w]
    if widen:
        p = np.pad(d, 1)
        d = np.maximum.reduce([p[1 + dy:1 + dy + d.shape[0], 1 + dx:1 + dx + d.shape[1]]
                               for dy in (-1, 0, 1) for dx in (-1, 0, 1)])
    if shrink > 1:
        hh, ww = h // shrink * shrink, w // shrink * shrink
        d = d[:hh, :ww].reshape(hh // shrink, shrink, ww // shrink, shrink).max(axis=(1, 3))
    out = np.full(d.shape + (3,), 255, np.uint8)
    out[d > 0] = [255, 160, 0]
    out[d > DIFF_LEVEL] = [255, 0, 0]
    im = Image.fromarray(out)
    if shrink > 1:
        return im
    im = im.resize((im.width * zoom, im.height * zoom), Image.NEAREST)
    return grid(im, zoom) if zoom >= GRID_ZOOM else im


def absdiff(a, b):
    return np.abs(a - b).max(axis=2)


def outside(a, refs):
    """How far a frame lies outside the range the references span, per pixel: 0 where it is between them."""
    low, high = np.minimum.reduce(refs), np.maximum.reduce(refs)
    return np.maximum(np.maximum(low - a, a - high), 0).max(axis=2)


def window(b, a, ref, opt, zoom, crop, full):
    H, W = b.shape[:2]
    if 'box' in opt:
        return tuple(int(v) for v in opt['box'].split(','))
    if crop:
        return crop
    if 'at' in opt or 'size' in opt:
        w, h = (int(v) for v in opt.get('size', '40x30').split('x'))
    elif zoom > 1 and not full:
        w, h = -(-WINDOW[0] // zoom), -(-WINDOW[1] // zoom)
    else:
        return (0, 0, W, H)
    if 'at' in opt:
        cx, cy = (int(v) for v in opt['at'].split(','))
    else:
        # Where the builds differ furthest toward the upper right (a clip's edge, typically); else where the
        # after frame is furthest from the reference.
        d = absdiff(b, a)
        if not d.any():
            d = absdiff(a, ref)
        ys, xs = np.nonzero(d)
        if len(xs):
            i = np.argmax(xs - ys)
            cx, cy = int(xs[i]), int(ys[i])
        else:
            cx, cy = W // 2, H // 2
    w, h = min(w, W), min(h, H)
    return (min(max(cx - w // 2, 0), W - w), min(max(cy - h // 2, 0), H - h), w, h)


def get_ref(browser, f, framing):
    if reference(browser, f, framing) in ('unframed', 'failed'):
        die(f'no {browser} reference for {f["key"]} at {framing}')
    return load(ref_png(browser, f['key'], framing))


def rows_of(specs, framings):
    for spec in specs:
        spec, _, note = spec.partition(';')
        name, *options = spec.split(':')
        opt = dict(o.split('=', 1) for o in options)
        if '@' in name:
            name, fr = name.rsplit('@', 1)
            yield resolve(name), framing_list([fr])[0], opt, note
        else:
            for fr in framings:
                yield resolve(name), fr, opt, note


def main():
    pos, opts, flags = cli(__doc__, (4, None), ('refs', 'zoom', 'crop', 'framings', 'labels', 'others', 'range'),
                           ('full', 'diffs-only', 'bold'))
    out, before, after, *specs = pos
    browsers = opts.get('refs', 'chromium').split(',')
    others = opts['others'].split(',') if opts.get('others') else []
    span = opts['range'].split(',') if opts.get('range') else []
    labels = opts['labels'].split(',') if opts.get('labels') else [build_info(before)['label'], build_info(after)['label']]
    crop = tuple(int(v) for v in opts['crop'].split(',')) if opts.get('crop') else None
    framings = framing_list(opts.get('framings', 'z1'))
    rows = []
    for f, framing, opt, note in rows_of(specs, framings):
        nb, na = opt.get('before', before), opt.get('after', after)
        lb = labels[0] if nb == before else build_info(nb)['label']
        la = labels[1] if na == after else build_info(na)['label']
        pb, pa = frame(nb, f, framing), frame(na, f, framing)
        if not pb or not pa:
            die(f'no frame of {f["key"]} at {framing}')
        b, a = load(pb), load(pa)
        refs = [(r, get_ref(r, f, framing)) for r in browsers]
        zoom, shrink = int(opt.get('zoom', opts.get('zoom', 1))), int(opt.get('shrink', 1))
        box = window(b, a, refs[0][1], opt, zoom, crop, 'full' in flags)
        far_b, far_a = absdiff(b, refs[0][1]) > DIFF_LEVEL, absdiff(a, refs[0][1]) > DIFF_LEVEL
        single = nb == na
        panels = [(lb, picture(b, box, zoom, shrink, far_b))]
        if not single:
            panels.append((la, picture(a, box, zoom, shrink, far_a)))
        caption = f'{f["key"]} at {framing}'
        if box != (0, 0, b.shape[1], b.shape[0]):
            caption += f', window {box[2]}x{box[3]} at {box[0]},{box[1]}'
        caption += f' (x{zoom})' if zoom > 1 else (f' (1/{shrink})' if shrink > 1 else '')
        caption += f': beyond {DIFF_LEVEL}/255 of the frame'
        widen = 'bold' in flags
        for r, ref in refs:
            short = REF_NAMES[r]
            if 'diffs-only' not in flags:
                panels.append((ref_label(r), picture(ref, box, zoom, shrink)))
            panels.append((f'{lb} vs {short}', heat(absdiff(b, ref), box, zoom, shrink, widen)))
            if single:
                caption += f', vs {short} {beyond(b, ref):.2f} %'
                continue
            panels.append((f'{la} vs {short}', heat(absdiff(a, ref), box, zoom, shrink, widen)))
            caption += f', vs {short} {beyond(b, ref):.2f} % -> {beyond(a, ref):.2f} %'
        for other in others:
            first, theirs = refs[0][1], get_ref(other, f, framing)
            panels.append((f'{REF_NAMES[other]} vs {REF_NAMES[browsers[0]]}', heat(absdiff(theirs, first), box, zoom, shrink, widen)))
            caption += f'; {REF_NAMES[other]} {beyond(theirs, first):.2f} %'
        if span:
            among = [get_ref(r, f, framing) for r in span]
            ob, oa = outside(b, among), outside(a, among)
            panels.append((f'{lb} outside the browsers', heat(ob, box, zoom, shrink, widen)))
            if not single:
                panels.append((f'{la} outside the browsers', heat(oa, box, zoom, shrink, widen)))
            caption += (f', outside all {len(span)} {100 * (ob > DIFF_LEVEL).mean():.2f} % -> '
                        f'{100 * (oa > DIFF_LEVEL).mean():.2f} %')
        caption += f'. {note}' if note else ''
        rows.append((caption, panels))
        print(caption)
    if not rows:
        die('no rows')

    gap, cap = 8, 40
    measure = ImageDraw.Draw(Image.new('RGB', (1, 1)))
    column = max(max(p.width, int(measure.textlength(label, font=font(11, label)))) for _, ps in rows
                 for label, p in ps) + gap
    width = max(max(len(ps) for _, ps in rows) * column + gap,
                max(int(measure.textlength(c, font=font(13, c))) for c, _ in rows) + 2 * gap)
    height = sum(max(p.height for _, p in ps) + cap + gap for _, ps in rows) + gap
    sheet = Image.new('RGB', (width, height), 'white')
    draw = ImageDraw.Draw(sheet)
    y = gap
    for caption, ps in rows:
        draw.text((gap, y), caption, fill=(0, 0, 0), font=font(13, caption))
        for i, (label, p) in enumerate(ps):
            x = gap + i * column
            draw.text((x, y + 21), label, fill=(90, 90, 90), font=font(11, label))
            sheet.paste(p, (x, y + cap))
            draw.rectangle([x - 1, y + cap - 1, x + p.width, y + cap + p.height], outline=(200, 200, 200))
        y += max(p.height for _, p in ps) + cap + gap
    sheet.save(out)
    print(f'{out}: {sheet.size[0]}x{sheet.size[1]}', file=sys.stderr)


if __name__ == '__main__':
    main()
