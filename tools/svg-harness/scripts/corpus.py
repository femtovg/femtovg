#!/usr/bin/env python3
"""corpus.py fetch [SET...] | list [group=G] [keys=FILE] [paths] | groups [group=G]

The SVG corpus: tests/svg/<category>/<feature>/*.svg in the repository, plus the third-party sets of
tests/svg/external.json, which are fetched on demand into target/svg-corpus/<set>/ and never committed.

  fetch   shallow, sparse git fetch of each set's pinned rev and paths (all sets by default); `local` sets are
          used in place
  list    one KEY<tab>path line per file (`paths`: paths only, e.g. for svg-harness soak)
  groups  a table of the groups and their file counts

A file's KEY is its path relative to tests/svg (external sets: external/<set>/<path in the set>) without .svg, with
/ as __. Its group is the first two components of that path (masking/clip-path, external/reddit); group=masking
selects every group under masking. Files with SMIL animation are flagged: their frames depend on time, so the
reports leave them out of their counts.
"""
import json, os, re, subprocess, sys

from common import REPO, OUT, cli, die, table

TESTS = os.path.join(REPO, 'tests', 'svg')
EXTERNAL = os.path.join(REPO, 'target', 'svg-corpus')
ANIMATION = re.compile(rb'<(?:animate|animateTransform|animateMotion|animateColor|set)\b')


def external_sets():
    return json.load(open(os.path.join(TESTS, 'external.json')))


def set_root(name, spec):
    if 'local' in spec:
        return os.path.join(REPO, spec['local'])
    return os.path.join(EXTERNAL, name)


def fetch(name, spec):
    if 'local' in spec:
        path = set_root(name, spec)
        return f'{name}: local, {path}' + ('' if os.path.exists(path) else ' (MISSING)')
    root = set_root(name, spec)
    git = lambda *a, **kw: subprocess.run(['git', '-C', root, *a], check=True, capture_output=True, text=True, **kw)
    os.makedirs(root, exist_ok=True)
    if not os.path.isdir(os.path.join(root, '.git')):
        git('init', '-q')
        git('remote', 'add', 'origin', spec['git'])
    # Non-cone patterns anchored at the root: an entry may be a directory or a single file. Set on every fetch, so
    # a change to `paths` applies even when the rev is unchanged.
    git('sparse-checkout', 'set', '--no-cone', *('/' + p.strip('/') for p in spec['paths']))
    try:
        if git('rev-parse', 'HEAD').stdout.strip() == spec['rev']:
            return f'{name}: at {spec["rev"][:12]} already'
    except subprocess.CalledProcessError:
        pass
    git('fetch', '-q', '--depth', '1', '--filter=blob:none', 'origin', spec['rev'])
    git('checkout', '-q', '--detach', 'FETCH_HEAD')
    n = sum(1 for f in walk(root) if f)
    return f'{name}: fetched {spec["rev"][:12]}, {n} SVG files'


def walk(root):
    if os.path.isfile(root):
        yield root
        return
    for d, dirs, names in os.walk(root):
        dirs[:] = sorted(x for x in dirs if x != '.git')
        for n in sorted(names):
            if n.endswith('.svg'):
                yield os.path.join(d, n)


def record(prefix, rel, path):
    rel = (prefix + rel[:-4]) if rel.endswith('.svg') else prefix + rel
    parts = rel.split('/')
    return {'key': rel.replace('/', '__'), 'path': path, 'group': '/'.join(parts[:2]) if len(parts) > 2 else parts[0]}


_animated = {}


def animated(path):
    if path not in _animated:
        try:
            _animated[path] = bool(ANIMATION.search(open(path, 'rb').read()))
        except OSError:
            _animated[path] = False
    return _animated[path]


def files(groups=None, keys=None):
    """Every corpus file present (fetched external sets included), optionally narrowed to groups (each a group or a
    group prefix) and to a set of keys."""
    out = [record('', os.path.relpath(p, TESTS), p) for p in walk(TESTS)]
    for name, spec in external_sets().items():
        root = set_root(name, spec)
        if not os.path.exists(root):
            continue
        prefix = f'external/{name}/'
        if os.path.isfile(root):
            out.append(record(prefix, os.path.basename(root), root))
        else:
            out += [record(prefix, os.path.relpath(p, root), p) for p in walk(root)]
    if groups:
        out = [f for f in out if any(f['group'] == g or f['group'].startswith(g.rstrip('/') + '/') or
                                     f['key'].startswith(g.replace('/', '__') + '__') for g in groups)]
    if keys is not None:
        out = [f for f in out if f['key'] in keys]
    seen = set()
    for f in out:
        if f['key'] in seen:
            die(f'two files map to the key {f["key"]}')
        seen.add(f['key'])
    return out


def select(opts):
    """files() narrowed by the common options group=G[,G...] and keys=FILE (one key per line, or a JSON list)."""
    groups = opts['group'].split(',') if opts.get('group') else None
    keys = None
    if opts.get('keys'):
        text = open(opts['keys']).read()
        keys = set(json.loads(text)) if text.lstrip().startswith('[') else set(text.split())
    fs = files(groups, keys)
    if not fs:
        die('no corpus files selected' + ('' if os.path.exists(EXTERNAL) else '; external sets: corpus.py fetch'))
    return fs


def resolve(arg):
    """A file given as a path, a key, or a unique key substring."""
    if os.path.exists(arg) and arg.endswith('.svg'):
        path = os.path.abspath(arg)
        for f in files():
            if os.path.abspath(f['path']) == path:
                return f
        return {'key': os.path.basename(arg)[:-4], 'path': path, 'group': 'other'}
    fs = files()
    exact = [f for f in fs if f['key'] == arg]
    if exact:
        return exact[0]
    hits = [f for f in fs if f['key'].endswith('__' + arg)] or [f for f in fs if arg in f['key']]
    if len(hits) != 1:
        die(f'{arg!r} matches {len(hits)} corpus files' + (': ' + ', '.join(f['key'] for f in hits[:5]) if hits else ''))
    return hits[0]


def main():
    pos, opts, flags = cli(__doc__, (1, None), ('group', 'keys'), ('paths',))
    cmd = pos[0]
    if cmd == 'fetch':
        sets = external_sets()
        for name in pos[1:] or sets:
            if name not in sets:
                die(f'no set {name!r} in tests/svg/external.json')
            try:
                print(fetch(name, sets[name]), flush=True)
            except subprocess.CalledProcessError as e:
                die(f'{name}: {" ".join(e.cmd[3:])} failed: {e.stderr.strip()}')
    elif cmd == 'list':
        for f in select(opts):
            print(f['path'] if 'paths' in flags else f'{f["key"]}\t{f["path"]}')
    elif cmd == 'groups':
        counts = {}
        for f in select(opts):
            c = counts.setdefault(f['group'], [0, 0, 0])
            c[0] += 1
            c[1] += f['key'].endswith('-ref')
            c[2] += animated(f['path'])
        rows = [(g, n, r, a) for g, (n, r, a) in sorted(counts.items())]
        rows.append(('**total**', *(sum(r[i] for r in rows) for i in (1, 2, 3))))
        print(table(['group', 'files', 'reftest references', 'animated (SMIL)'], rows))
    else:
        die(f'unknown command {cmd!r}; see -h')


if __name__ == '__main__':
    main()
