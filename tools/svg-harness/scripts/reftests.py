#!/usr/bin/env python3
"""reftests.py BUILD [BUILD2] [threshold=0.15] [group=G] [framings=z1,z2,z4,hd] [engines=E,...] [workers=N]

The reftest pairs of the corpus, X.svg with X-ref.svg beside it: a pair passes when the build renders the two
alike at every framing - at most `threshold` % of pixels beyond 20/255 apart. Each pair is checked in every browser
engine too (default chromium, firefox and webkit on macOS or webkitgtk on Linux; references missing are made now).
A pair counts when at least one engine renders it alike; one that every engine renders apart is
reference-inconsistent and listed separately, not counted as a failure. With BUILD2, flips between builds are bold.
"""
import collections, os, platform
from concurrent.futures import ThreadPoolExecutor

from common import (DEFAULT_FRAMINGS, REF_NAMES, REFTEST_MAX_PCT, beyond, bold, build_info, cli, frame_image, framing_list, load,
                    ref_png, table)
from corpus import animated, select
from refs import reference


def pairs(fs):
    by_path = {f['path']: f for f in fs}
    out = []
    for f in fs:
        if f['key'].endswith('-ref'):
            continue
        ref = by_path.get(f['path'][:-4] + '-ref.svg')
        if ref and not animated(f['path']) and not animated(ref['path']):
            out.append((f, ref))
    return out


def worst(values):
    """The largest share over the framings, or None when a frame is missing."""
    return None if any(v is None for v in values) else max(values)


def main():
    pos, opts, _ = cli(__doc__, (1, 2), ('threshold', 'group', 'framings', 'engines', 'workers'))
    default_engines = ['chromium', 'firefox', 'webkit' if platform.system() == 'Darwin' else 'webkitgtk']
    engines = opts['engines'].split(',') if opts.get('engines') else default_engines
    builds = pos
    threshold = float(opts.get('threshold', REFTEST_MAX_PCT))
    framings = framing_list(opts.get('framings', DEFAULT_FRAMINGS))
    ps = pairs(select(opts))
    labels = [build_info(b)['label'] for b in builds]

    def check(pair):
        test, ref = pair
        row = {'pair': test['key'], 'group': test['group']}
        for b in builds:
            vals = []
            for fr in framings:
                a, r = frame_image(b, test, fr), frame_image(b, ref, fr)
                vals.append(beyond(a, r) if a is not None and r is not None and a.shape == r.shape else None)
            row[b] = worst(vals)
        for e in engines:
            vals = []
            for fr in framings:
                ok = reference(e, test, fr) != 'failed' and reference(e, ref, fr) != 'failed'
                pt, pr = ref_png(e, test['key'], fr), ref_png(e, ref['key'], fr)
                vals.append(beyond(load(pt), load(pr)) if ok and os.path.exists(pt) and os.path.exists(pr) else None)
            row[e] = worst(vals)
        return row

    with ThreadPoolExecutor(int(opts.get('workers', 4))) as ex:
        rows = list(ex.map(check, ps))

    passes = lambda v: v is not None and v <= threshold
    consistent = [r for r in rows if any(passes(r[e]) for e in engines)]
    inconsistent = [r for r in rows if not any(passes(r[e]) for e in engines)]
    names = [REF_NAMES[e] for e in engines]
    summary = []
    for b, label in zip(builds, labels):
        summary.append((label, len(consistent), sum(passes(r[b]) for r in consistent),
                        sum(not passes(r[b]) for r in consistent), len(inconsistent)))
    print(f'## Reftests: within {threshold:g} % of pixels beyond 20/255 at {", ".join(framings)}\n')
    print(table(['build', 'pairs', 'pass', 'fail', 'reference-inconsistent (left out)'], summary))

    groups = collections.defaultdict(list)
    for r in consistent:
        groups[r['group']].append(r)
    grows = []
    for g, rs in sorted(groups.items()):
        counts = [sum(passes(r[b]) for r in rs) for b in builds]
        grows.append((g, len(rs), *(bold(c, len(builds) > 1 and c != counts[0]) for c in counts)))
    print('\n### Pass counts by group\n')
    print(table(['group', 'pairs', *(f'pass: {l}' for l in labels)], grows))

    flips = lambda r: len(builds) > 1 and passes(r[builds[0]]) != passes(r[builds[1]])
    listed = [r for r in consistent if not all(passes(r[b]) for b in builds)]
    if listed:
        print('\n### Failing pairs (worst framing, % of pixels apart)\n')
        print(table(['pair', *labels, *names],
                    [(bold(r['pair'], flips(r)), *(bold(fmt(r[b], threshold), flips(r)) for b in builds),
                      *(fmt(r[e], threshold) for e in engines)) for r in sorted(listed, key=lambda r: r['pair'])],
                    'l' + 'r' * (len(builds) + len(engines))))
    split = [r for r in consistent if not all(passes(r[e]) for e in engines)]
    if split:
        print('\n### Pairs the engines disagree on (counted: at least one renders them alike)\n')
        print(table(['pair', *labels, *names],
                    [(r['pair'], *(fmt(r[b], threshold) for b in builds), *(fmt(r[e], threshold) for e in engines))
                     for r in sorted(split, key=lambda r: r['pair'])], 'l' + 'r' * (len(builds) + len(engines))))
    if inconsistent:
        print('\n### Reference-inconsistent pairs (every engine renders test and reference apart)\n')
        print(table(['pair', *labels, *names],
                    [(r['pair'], *(fmt(r[b], threshold) for b in builds), *(fmt(r[e], threshold) for e in engines))
                     for r in sorted(inconsistent, key=lambda r: r['pair'])], 'l' + 'r' * (len(builds) + len(engines))))


def fmt(v, threshold):
    return 'no frame' if v is None else f'{v:.3f}' + ('' if v <= threshold else ' (fail)')


if __name__ == '__main__':
    main()
