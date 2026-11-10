#!/usr/bin/env python3
"""timing.py A B [pairs=14] [frames=200] [framing=hd] FILE...
timing.py instr A B [lo=200] [hi=1200] [repeats=3] [framing=hd] FILE...
timing.py soak A B [LIST] [group=G] [passes=3] [runs=2] [zooms=1,2,4] [framing=z1]

Frame cost of build B against build A. FILE is a path, a corpus key or a unique key substring.

default  Paired runs of `svg-harness time`, alternating A B, B A, ... so drift and background load fall on both
         builds alike. Per pair, B's median over frames 2..N divided by A's (frame 1 compiles pipelines and
         creates atlases, so it is reported apart); the change is the median of those ratios and the spread their
         range. Wall time runs to GPU completion; encode is the CPU's scene walk and flush.
instr    Instructions retired and cycles per frame, as a long run minus a short run so start-up cancels out
         (/usr/bin/time -l on macOS, perf stat on Linux): steadier than time when a change is small.
soak     `svg-harness soak` of every file in LIST (one path per line; default: the selected corpus files) for each
         build, `runs` times each in the order A B B A, summarized: CPU and wall time per frame (p50/p95/p99,
         spread over the runs' passes) and peak memory.

Bold marks a change beyond 3 % whose spread stays on one side of zero, and memory changes of 10 % or more.
"""
import json, os, platform, re, statistics, subprocess, sys

from common import (FRAMINGS, MAJOR_MEMORY_PCT, MAJOR_TIMING_PCT, OUT, bold, build_binary, build_info, cli, die,
                    framing_list, table)
from corpus import resolve, select

MiB = 1048576.0


def warn_if_loaded():
    load = os.getloadavg()[0]
    if load > max(1.0, (os.cpu_count() or 4) / 4):
        print(f'warning: load average {load:.1f}; background work biases timings - wait for it or compare with '
              f'`timing.py instr`', file=sys.stderr)


def change(ratios):
    """(median change %, lowest %, highest %, major?)"""
    pcts = [100 * (r - 1) for r in ratios]
    med, lo, hi = statistics.median(pcts), min(pcts), max(pcts)
    return med, lo, hi, abs(med) > MAJOR_TIMING_PCT and (lo > 0 or hi < 0)


def change_cells(ratios):
    med, lo, hi, major = change(ratios)
    return bold(f'{med:+.1f}', major), f'{lo:+.1f} .. {hi:+.1f}'


def run_time(build, path, framing, frames):
    r = subprocess.run([build_binary(build), 'time', *FRAMINGS[framing].args(), '--frames', str(frames), path],
                       capture_output=True, text=True, timeout=3600)
    try:
        return json.loads(r.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        die(f'{build} time {path}: {r.stderr.strip()[-300:]}')


def paired(a, b, opts, paths):
    pairs, frames = int(opts.get('pairs', 14)), int(opts.get('frames', 200))
    framing = framing_list([opts.get('framing', 'hd')])[0]
    warn_if_loaded()
    rows = []
    for f in paths:
        runs = {a: [], b: []}
        for i in range(pairs):
            for build in ((a, b) if i % 2 == 0 else (b, a)):
                runs[build].append(run_time(build, f['path'], framing, frames))
        get = lambda build, k: [r[k]['median_ms'] for r in runs[build]]
        cpu = lambda build: [1e3 * (r['user_s'] + r['sys_s']) / r['frames'] for r in runs[build]]
        ratios = lambda xs, ys: [y / x for x, y in zip(xs, ys) if x > 0]
        wa, wb, ea, eb = get(a, 'wall'), get(b, 'wall'), get(a, 'encode'), get(b, 'encode')
        rows.append((f['key'], f'{statistics.median(wa):.3f}', f'{statistics.median(wb):.3f}', *change_cells(ratios(wa, wb)),
                     f'{statistics.median(ea):.3f}', f'{statistics.median(eb):.3f}', *change_cells(ratios(ea, eb)),
                     *change_cells(ratios(cpu(a), cpu(b))),
                     f'{statistics.median(r["first_ms"] for r in runs[a]):.1f} / {statistics.median(r["first_ms"] for r in runs[b]):.1f}'))
    la, lb = build_info(a)['label'], build_info(b)['label']
    print(f'## {lb} against {la}: {pairs} pairs of {frames} frames at {framing}\n')
    print(table(['file', f'wall A (ms)', f'wall B (ms)', 'wall change (%)', 'wall spread (%)', 'encode A (ms)',
                 'encode B (ms)', 'encode change (%)', 'encode spread (%)', 'CPU change (%)', 'CPU spread (%)',
                 'first frame A / B (ms)'], rows))
    print(f'\nA = {la}, B = {lb}; medians over frames 2..{frames}, CPU = user + system time per frame.')


def counters(build, path, framing, frames):
    cmd = [build_binary(build), 'time', *FRAMINGS[framing].args(), '--frames', str(frames), path]
    if platform.system() == 'Darwin':
        r = subprocess.run(['/usr/bin/time', '-l', *cmd], capture_output=True, text=True)
        g = lambda name: int(re.search(r'(\d+)\s+' + name, r.stderr).group(1))
        try:
            return g('instructions retired'), g('cycles elapsed')
        except AttributeError:
            die(f'/usr/bin/time -l reported no instruction counts: {r.stderr.strip()[-300:]}')
    r = subprocess.run(['perf', 'stat', '-x,', '-e', 'instructions,cycles', '--', *cmd], capture_output=True, text=True)
    found = {}
    for line in r.stderr.splitlines():
        parts = line.split(',')
        if len(parts) > 2 and parts[0].strip().isdigit():
            found[parts[2].split(':')[0]] = int(parts[0])
    if 'instructions' not in found:
        die(f'perf stat reported no instruction counts: {r.stderr.strip()[-300:]}')
    return found['instructions'], found.get('cycles', 0)


def instr(a, b, opts, paths):
    lo, hi, repeats = int(opts.get('lo', 200)), int(opts.get('hi', 1200)), int(opts.get('repeats', 3))
    framing = framing_list([opts.get('framing', 'hd')])[0]
    rows = []
    for f in paths:
        per = {a: [], b: []}
        for i in range(repeats):
            for build in ((a, b) if i % 2 == 0 else (b, a)):
                (i0, c0), (i1, c1) = counters(build, f['path'], framing, lo), counters(build, f['path'], framing, hi)
                per[build].append(((i1 - i0) / (hi - lo), (c1 - c0) / (hi - lo)))
        med = lambda build, j: statistics.median(x[j] for x in per[build])
        ri = [y[0] / x[0] for x, y in zip(per[a], per[b])]
        rc = [y[1] / x[1] for x, y in zip(per[a], per[b]) if x[1]]
        rows.append((f['key'], f'{med(a, 0) / 1e6:.3f}', f'{med(b, 0) / 1e6:.3f}', *change_cells(ri),
                     f'{med(a, 1) / 1e6:.3f}', f'{med(b, 1) / 1e6:.3f}', *(change_cells(rc) if rc else ('-', '-'))))
    la, lb = build_info(a)['label'], build_info(b)['label']
    print(f'## {lb} against {la}: per frame, {hi} frames minus {lo}, {repeats} repeats at {framing}\n')
    print(table(['file', 'instructions A (M)', 'instructions B (M)', 'change (%)', 'spread (%)', 'cycles A (M)',
                 'cycles B (M)', 'cycles change (%)', 'cycles spread (%)'], rows))


def read_soak(path):
    """Soak CSV rows; a file name may hold commas, so the eleven numeric fields are taken from the right."""
    lines = open(path).read().splitlines()
    header = lines[0].split(',')
    rows = []
    for line in lines[1:]:
        parts = line.split(',')
        rows.append(dict(zip(header, [parts[0], ','.join(parts[1:-11])] + parts[-11:])))
    return rows


def soak(a, b, opts, rest):
    framing = framing_list([opts.get('framing', 'z1')])[0]
    os.makedirs(os.path.join(OUT, 'soak'), exist_ok=True)
    if rest:
        listing = rest[0]
    else:
        listing = os.path.join(OUT, 'soak', 'list.txt')
        open(listing, 'w').write(''.join(f['path'] + '\n' for f in select(opts)))
    args = FRAMINGS[framing].args()
    i = args.index('--zoom')
    del args[i:i + 2]
    warn_if_loaded()
    # Runs in the order A B B A ...: drift and background load fall on both builds alike.
    runs = int(opts.get('runs', 2))
    data = {a: [], b: []}
    for i in range(runs):
        for build in ((a, b) if i % 2 == 0 else (b, a)):
            out = os.path.join(OUT, 'soak', f'{build}.{i}.csv')
            r = subprocess.run([build_binary(build), 'soak', *args, '--passes', opts.get('passes', '3'), '--zooms',
                                opts.get('zooms', '1,2,4'), listing, out], capture_output=True, text=True)
            if r.returncode != 0:
                die(f'{build} soak: {r.stderr.strip()[-300:]}')
            data[build] += [{**row, 'pass': f'{i}.{row["pass"]}'} for row in read_soak(out)]

    def pctl(values, q):
        s = sorted(values)
        return s[min(len(s) - 1, int(q * len(s)))]

    def col(build, name, scale=1.0):
        return [float(r[name]) / scale for r in data[build] if r.get(name) not in (None, '')]

    def pass_medians(build, name):
        by = {}
        for r in data[build]:
            by.setdefault(r['pass'], []).append(float(r[name]))
        return [statistics.median(v) for _, v in sorted(by.items())]

    rows = []
    for name, label in (('cpu_ms', 'CPU'), ('wall_ms', 'wall')):
        for q, ql in ((0.5, 'p50'), (0.95, 'p95'), (0.99, 'p99')):
            va, vb = pctl(col(a, name), q), pctl(col(b, name), q)
            if ql == 'p50':
                cell, spread = change_cells([y / x for x, y in zip(pass_medians(a, name), pass_medians(b, name)) if x])
            else:
                cell, spread = f'{100 * (vb / va - 1):+.1f}' if va else '-', '-'
            rows.append((f'{label} per frame {ql} (ms)', f'{va:.3f}', f'{vb:.3f}', cell, spread))
    for name, label in (('peak_footprint', 'peak footprint'), ('peak_graphics', 'peak graphics memory'),
                        ('maxrss', 'resident high-water mark'), ('footprint_after', 'held after the last frame')):
        va, vb = col(a, name, MiB), col(b, name, MiB)
        if not va or not vb:
            continue
        if name == 'maxrss' and platform.system() != 'Darwin':
            va, vb = [v * 1024 for v in va], [v * 1024 for v in vb]  # KiB there
        xa, xb = (va[-1], vb[-1]) if name == 'footprint_after' else (max(va), max(vb))
        ch = 100 * (xb / xa - 1) if xa else 0.0
        rows.append((f'{label} (MiB)', f'{xa:.1f}', f'{xb:.1f}', bold(f'{ch:+.1f}', abs(ch) >= MAJOR_MEMORY_PCT), '-'))
    la, lb = build_info(a)['label'], build_info(b)['label']
    print(f'## Soak: {len(data[a])} frames per build over {runs} runs ({listing}), {la} -> {lb}\n')
    print(table(['measure', 'A', 'B', 'change (%)', 'spread over runs and passes (%)'], rows))


def main():
    argv = sys.argv[1:]
    mode = argv[0] if argv and argv[0] in ('instr', 'soak') else 'paired'
    if mode != 'paired':
        argv = argv[1:]
    options = {'paired': ('pairs', 'frames', 'framing'), 'instr': ('lo', 'hi', 'repeats', 'framing'),
               'soak': ('passes', 'runs', 'zooms', 'framing', 'group', 'keys')}[mode]
    pos, opts, _ = cli(__doc__, (2, 3) if mode == 'soak' else (3, None), options, argv=argv)
    a, b, rest = pos[0], pos[1], pos[2:]
    build_binary(a), build_binary(b)
    if mode == 'soak':
        soak(a, b, opts, rest)
    else:
        (paired if mode == 'paired' else instr)(a, b, opts, [resolve(p) for p in rest])


if __name__ == '__main__':
    main()
