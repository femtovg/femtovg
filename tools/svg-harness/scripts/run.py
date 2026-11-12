#!/usr/bin/env python3
"""run.py BUILD... [group=G] [keys=FILE] [framings=z1,z2,z4,hd] [keep=diff|all|none] [workers=N]

Render every selected corpus file at every framing with each build and compare each frame with every reference
refs.py has made. One row per frame goes to OUT/results/<BUILD>.jsonl: the frame's pixel hash, the harness's
--stats, and per reference the share of pixels beyond 20/255 (px, %), the share 1..20/255 off (faint, %) and the
largest difference (max).

keep: which frames stay in OUT/frames/<BUILD>/<KEY>@<framing>.png - all, none, or (default) those whose hash
differs between the builds given, for compare.py and sheet.py to read without rendering again.

Resumable. Rows are tagged with the binary's id (rev + mtime): after a rebuild, the old rows are dropped rather
than mixed in. A frame is rendered again when a reference has been made since its row was written.
"""
import json, os, shutil, sys, threading, time
from concurrent.futures import ThreadPoolExecutor

from common import (DEFAULT_FRAMINGS, OUT, REFERENCES, build_info, cli, die, frame_png, frames_dir, framing_list, load, metrics,
                    pixel_hash, ref_png, render, temp_path)
from corpus import animated, select


def results_path(build):
    return os.path.join(OUT, 'results', f'{build}.jsonl')


def read_results(build, drop_stale=True):
    """{(key, framing): row} for the build's current binary. Rows and kept frames of an earlier binary are
    removed."""
    current = build_info(build)['id']
    frames_dir(build)
    path = results_path(build)
    if not os.path.exists(path):
        return {}
    rows, stale = {}, 0
    for line in open(path):
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if r.get('build_id') != current:
            stale += 1
            continue
        rows[(r['key'], r['framing'])] = r
    if stale and drop_stale:
        print(f'{build}: dropping {stale} rows of an earlier binary', file=sys.stderr)
        with open(path + '.tmp', 'w') as out:
            for r in rows.values():
                out.write(json.dumps(r) + '\n')
        os.replace(path + '.tmp', path)
    return rows


def reference_versions(key, framing):
    versions = {}
    for browser in REFERENCES:
        try:
            versions[browser] = os.stat(ref_png(browser, key, framing)).st_mtime_ns
        except FileNotFoundError:
            pass
    return versions


def main():
    pos, opts, _ = cli(__doc__, (1, None), ('group', 'keys', 'framings', 'keep', 'workers'))
    builds = pos
    keep = opts.get('keep', 'diff')
    if keep not in ('all', 'diff', 'none'):
        die('keep= is all, diff or none')
    framings = framing_list(opts.get('framings', DEFAULT_FRAMINGS))
    fs = select(opts)
    infos = {b: build_info(b) for b in builds}
    os.makedirs(os.path.join(OUT, 'results'), exist_ok=True)
    done = {b: read_results(b) for b in builds}
    outs = {b: open(results_path(b), 'a') for b in builds}
    lock = threading.Lock()
    counts = {'frames': 0, 'failed': 0, 'kept': 0}

    jobs = []
    for f in fs:
        for fr in framings:
            versions = reference_versions(f['key'], fr)
            rows = (done[b].get((f['key'], fr), {}) for b in builds)
            if any('hash' not in row or row.get('ref_mtimes') != versions for row in rows):
                jobs.append((f, fr, versions))

    print(f'{len(jobs)} of {len(fs) * len(framings)} frames to render with {", ".join(builds)}', file=sys.stderr,
          flush=True)

    def job(args):
        f, framing, versions = args
        refs = {b: load(ref_png(b, f['key'], framing)) for b in versions}
        rendered = {}
        for b in builds:
            png = temp_path('.png')
            ok, stats, err = render(b, f['path'], framing, png)
            row = {'key': f['key'], 'group': f['group'], 'framing': framing, 'build_id': infos[b]['id']}
            if animated(f['path']):
                row['animated'] = True
            if stats:
                row['stats'] = stats
            if not ok:
                row['error'] = err
            else:
                row['hash'] = pixel_hash(png)
                row['ref_mtimes'] = versions
                frame = load(png)
                row['refs'] = {r: metrics(frame, im) for r, im in refs.items()}
            with lock:
                outs[b].write(json.dumps(row) + '\n')
                outs[b].flush()
                counts['frames'] += 1
                counts['failed'] += not ok
            if ok:
                rendered[b] = (png, row['hash'])
        differ = len({h for _, h in rendered.values()}) > 1
        for b, (png, _) in rendered.items():
            dest = frame_png(b, f['key'], framing)
            if keep == 'all' or (keep == 'diff' and differ):
                os.makedirs(os.path.dirname(dest), exist_ok=True)
                shutil.move(png, dest)
                with lock:
                    counts['kept'] += 1
            else:
                os.remove(png)
                # A kept frame of an earlier binary would be read as this one's.
                if os.path.exists(dest):
                    os.remove(dest)

    workers = int(opts.get('workers', min(6, os.cpu_count() or 4)))
    started = time.time()
    with ThreadPoolExecutor(workers) as ex:
        for i, _ in enumerate(ex.map(job, jobs)):
            if (i + 1) % 100 == 0:
                print(f'{i + 1}/{len(jobs)} {counts} {time.time() - started:.0f} s', file=sys.stderr, flush=True)
    print(f'{counts["frames"]} frames rendered, {counts["failed"]} failed, {counts["kept"]} kept '
          f'in {time.time() - started:.0f} s; results in {os.path.join(OUT, "results")}')


if __name__ == '__main__':
    main()
