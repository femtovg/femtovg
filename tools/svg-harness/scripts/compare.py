#!/usr/bin/env python3
"""compare.py BEFORE AFTER [refs=chromium,firefox,webkit,webkitgtk,area] [group=G] [worst=10]

Two builds' run.py results: which frames are identical (by pixel hash) and, over the frames that differ, whether
AFTER moved toward or away from each reference - per frame by the share of pixels beyond 20/255, per pixel by
whether the pixels the builds differ on ended nearer the reference. Then the groups, the frames that moved most,
the renderers against the area reference (when refs include area) and the harness's --stats. Frames that differ
are read from OUT/frames (run.py keeps them) or rendered now. Markdown tables; bold marks a change of at least
1 percentage point of a frame's pixels, and memory changes of 10 % or more.
"""
import collections, os, statistics, sys
from concurrent.futures import ThreadPoolExecutor

import numpy as np

from common import (DIFF_LEVEL, MAJOR_MEMORY_PCT, MAJOR_PIXEL_POINTS, REF_NAMES, bold, build_info, cli, delta, die,
                    frame, load, pct, ref_png, table)
from corpus import files
from run import read_results

LEVEL = 0.005  # a frame within this many points of where it was has not moved


def nonzero_bold(n):
    return bold(n, n > 0)


def in_groups(row, groups):
    return not groups or any(row['group'] == g or row['group'].startswith(g.rstrip('/') + '/') for g in groups)


def main():
    pos, opts, _ = cli(__doc__, (2, 2), ('refs', 'group', 'worst'))
    before, after = pos
    refs = opts.get('refs', 'chromium,firefox,webkit,webkitgtk,area').split(',')
    groups = opts['group'].split(',') if opts.get('group') else None
    worst = int(opts.get('worst', 10))
    rb, ra = read_results(before), read_results(after)
    lb, la = build_info(before)['label'], build_info(after)['label']
    present = {f['key'] for f in files()}  # rows of files since removed from the corpus are left out
    both = sorted(k for k in rb if k in ra and k[0] in present and in_groups(rb[k], groups))
    if not both:
        die(f'no frames with results for both {before} and {after}; run.py {before} {after} first')
    animated = [k for k in both if rb[k].get('animated')]
    failed = [k for k in both if 'error' in rb[k] or 'error' in ra[k]]
    usable = [k for k in both if k not in animated and k not in failed]
    differ = [k for k in usable if rb[k]['hash'] != ra[k]['hash']]
    print(f'## {lb} -> {la}\n')
    print(table(['frames', 'identical', 'differing', 'failed', 'animated (left out)'],
                [(len(both), len(usable) - len(differ), len(differ), len(failed), len(animated))], 'rrrrr'))
    for k in failed:
        print(f'\nfailed: {k[0]}@{k[1]}: {(rb[k].get("error") or ra[k].get("error"))[-200:]}')
    if not differ:
        stats_table(rb, ra, usable, lb, la)
        return

    by_key = {f['key']: f for f in files()}
    with ThreadPoolExecutor(6) as ex:
        paths = list(ex.map(lambda kb: frame(kb[1], by_key[kb[0][0]], kb[0][1]) if kb[0][0] in by_key else None,
                            [(k, b) for k in differ for b in (before, after)]))
    pictures = {(k, b): p for (k, b), p in zip([(k, b) for k in differ for b in (before, after)], paths)}

    # Per reference: frames toward / away by share, pixels nearer / farther, mean distance over the changed pixels.
    rows, moved = [], []
    for ref in refs:
        d = [(ra[k]['refs'][ref]['px'] - rb[k]['refs'][ref]['px'], k) for k in differ
             if ref in rb[k].get('refs', {}) and ref in ra[k].get('refs', {})]
        if not d:
            continue
        nearer = farther = same = 0
        far_b = far_a = 0.0
        for _, k in d:
            pb, pa = pictures[(k, before)], pictures[(k, after)]
            if not pb or not pa:
                continue
            b, a, r = load(pb), load(pa), load(ref_png(ref, *k))
            if not (b.shape == a.shape == r.shape):
                continue
            changed = (a != b).any(axis=2)
            db, da = np.abs(b - r).max(axis=2)[changed], np.abs(a - r).max(axis=2)[changed]
            nearer += int((da < db).sum()); farther += int((da > db).sum()); same += int((da == db).sum())
            far_b += float(db.sum()); far_a += float(da.sum())
        n = max(nearer + farther + same, 1)
        vals = [v for v, _ in d]
        rows.append((REF_NAMES[ref], len(d), sum(v < -LEVEL for v in vals), sum(v > LEVEL for v in vals),
                     sum(abs(v) <= LEVEL for v in vals), nonzero_bold(sum(v <= -MAJOR_PIXEL_POINTS for v in vals)),
                     nonzero_bold(sum(v >= MAJOR_PIXEL_POINTS for v in vals)), f'{statistics.median(vals):+.3f}',
                     nearer, farther, f'{far_b / n:.1f}', f'{far_a / n:.1f}'))
        moved += [(v, ref, k) for v, k in d]
    print(f'\n### Differing frames against each reference\n')
    print(table(['reference', 'frames', 'toward', 'away', 'level', '>= 1 pt toward', '>= 1 pt away',
                 'median change (pts)', 'pixels nearer', 'pixels farther', 'mean distance before (/255)',
                 'mean distance after (/255)'], rows))

    first = next((r for r in refs if any(r in rb[k].get('refs', {}) for k in usable)), None)
    if first:
        print(f'\n### Groups, against {REF_NAMES[first]} (% of pixels beyond {DIFF_LEVEL}/255, mean over frames)\n')
        g = collections.defaultdict(list)
        for k in usable:
            if first in rb[k].get('refs', {}) and first in ra[k].get('refs', {}):
                g[rb[k]['group']].append((k, rb[k]['refs'][first]['px'], ra[k]['refs'][first]['px']))
        grows = []
        for name, items in sorted(g.items()):
            mb, ma = statistics.mean(x[1] for x in items), statistics.mean(x[2] for x in items)
            ch = ma - mb
            grows.append((name, len(items), sum(x[0] in differ for x in items), f'{mb:.3f}', f'{ma:.3f}',
                          bold(f'{ch:+.3f}', abs(ch) >= MAJOR_PIXEL_POINTS)))
        print(table(['group', 'frames', 'differing', 'before (%)', 'after (%)', 'change (pts)'], grows))

    if moved:
        print(f'\n### Frames that moved most\n')
        moved.sort(key=lambda m: -abs(m[0]))
        print(table(['frame', 'reference', 'before (%)', 'after (%)', 'change (pts)', 'direction'],
                    [(f'{k[0]}@{k[1]}', REF_NAMES[ref], pct(rb[k]['refs'][ref]['px'], 3), pct(ra[k]['refs'][ref]['px'], 3),
                      bold(f'{v:+.3f}', abs(v) >= MAJOR_PIXEL_POINTS), 'away' if v > 0 else 'toward')
                     for v, ref, k in moved[:worst]], 'llrrrl'))

    if 'area' in refs:
        area_table(differ, pictures, before, after, lb, la)
    stats_table(rb, ra, usable, lb, la)


def area_table(differ, pictures, before, after, lb, la):
    """Every renderer against the area reference over the differing frames: what the browsers at one device
    pixel are themselves off by, next to the builds."""
    who = [(lb, lambda k: pictures[(k, before)]), (la, lambda k: pictures[(k, after)])]
    who += [(REF_NAMES[r], lambda k, r=r: ref_png(r, *k))
            for r in ('chromium', 'chromium-gpu', 'firefox', 'webkit', 'webkitgtk')]
    rows = []
    for k in differ:
        if not os.path.exists(ref_png('area', *k)):
            continue
        area = load(ref_png('area', *k))
        counts = {}
        for name, path in who:
            p = path(k)
            if p and os.path.exists(p):
                im = load(p)
                if im.shape == area.shape:
                    counts[name] = int((delta(im, area) > DIFF_LEVEL).sum())
        if lb in counts and la in counts:
            rows.append((area.shape[0] * area.shape[1], counts))
    if not rows:
        return
    print(f'\n### Renderers against the area reference ({len(rows)} differing frames)\n')
    out = []
    for name, _ in who:
        have = [(size, c) for size, c in rows if name in c]
        if not have:
            continue
        best = sum(c[name] == min(c.values()) for _, c in have)
        out.append((name, len(have), sum(c[name] for _, c in have),
                    f'{100 * statistics.mean(c[name] / size for size, c in have):.3f}', best))
    print(table(['renderer', 'frames', f'pixels beyond {DIFF_LEVEL}/255', 'mean share of a frame (%)',
                 'frames nearest'], out))


def stats_table(rb, ra, keys, lb, la):
    """The harness's --stats over the frames both builds rendered: counters summed, memory as the median frame's.
    Single-frame timings are left out: they include pipeline compiles (timing.py measures time)."""
    values = collections.OrderedDict()
    for k in keys:
        for name, vb, va in flat(rb[k].get('stats', {}), ra[k].get('stats', {})):
            if not name.startswith('timing.') and name != 'frames':
                values.setdefault(name, ([], []))
                values[name][0].append(vb)
                values[name][1].append(va)
    rows = []
    for name, (xb, xa) in values.items():
        memory = is_memory(name)
        vb, va = (statistics.median(xb), statistics.median(xa)) if memory else (sum(xb), sum(xa))
        if not vb and not va:
            continue
        change = 100 * (va - vb) / vb if vb else None
        major = change is not None and memory and abs(change) >= MAJOR_MEMORY_PCT
        fmt = (lambda v: f'{v / 1048576:.1f}') if memory else (lambda v: f'{v:.0f}')
        rows.append((f'{name} (MiB, median frame)' if memory else f'{name} (sum)', fmt(vb), fmt(va),
                     bold('-' if change is None else f'{change:+.1f}', major)))
    if rows:
        print(f'\n### Harness statistics over {len(keys)} frames\n')
        print(table(['statistic', lb, la, 'change (%)'], rows))


def flat(sb, sa, prefix=''):
    for name, vb in sb.items():
        va = sa.get(name)
        if isinstance(vb, dict) and isinstance(va, dict):
            yield from flat(vb, va, f'{prefix}{name}.')
        elif isinstance(vb, (int, float)) and isinstance(va, (int, float)) and not isinstance(vb, bool):
            yield f'{prefix}{name}', float(vb), float(va)


def is_memory(name):
    return any(w in name for w in ('mem', 'bytes', 'footprint', 'graphics', 'rss', 'mib'))


if __name__ == '__main__':
    main()
