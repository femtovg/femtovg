"""Shared pieces of the svg-harness scripts: paths, framings, builds, browsers, rendering, metrics and tables.

Environment:
  SVG_HARNESS_OUT    builds, references, frames and results (default <repo>/target/svg-harness)
  CHROMIUM, FIREFOX  the reference browsers (defaults: the puppeteer headless shell, Firefox from /Applications
                     or PATH)
"""
import glob, hashlib, json, os, re, shutil, subprocess, sys, tempfile, threading, time

import numpy as np
from PIL import Image

SCRIPTS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(os.path.dirname(SCRIPTS)))
OUT = os.path.abspath(os.environ.get('SVG_HARNESS_OUT', os.path.join(REPO, 'target', 'svg-harness')))
TMP = os.path.join(OUT, 'tmp')

# ---- thresholds: the one place every report takes them from -------------------------------------------------
# A pixel differs when any channel is more than this many 255ths off; below it, antialiasing noise dominates.
DIFF_LEVEL = 20
# Major accuracy change: a frame or group moving by at least this many percentage points of its pixels.
MAJOR_PIXEL_POINTS = 1.0
# Major timing change: a median ratio this far from 1 (in %), with the pair spread on one side of zero.
MAJOR_TIMING_PCT = 3.0
# Major memory change, in %.
MAJOR_MEMORY_PCT = 10.0
# A reftest passes when test and reference differ on at most this share (%) of pixels beyond DIFF_LEVEL.
REFTEST_MAX_PCT = 0.15


# ---- framings ---------------------------------------------------------------------------------------------------
class Framing:
    """A frame of W x H pixels with the SVG's size fitted into a square box, zoomed about a pivot.
    The browser pages (page.py) and the harness (its --frame/--box/--zoom/--pivot flags) take the same numbers."""

    def __init__(self, name, frame=(460, 260), box=(200, 130, 30), zoom=1.0, pivot=None):
        self.name, self.frame, self.box, self.zoom = name, frame, box, zoom
        # Default pivot: the frame centre, which is also the box centre for every framing below.
        self.pivot = pivot or (frame[0] / 2, frame[1] / 2)

    def with_zoom(self, zoom, name=None):
        return Framing(name or f'{self.name}x{zoom:g}', self.frame, self.box, zoom, self.pivot)

    def args(self):
        (w, h), (s, x, y), (px, py) = self.frame, self.box, self.pivot
        return ['--frame', f'{w}x{h}', '--box', f'{s}@{x},{y}', '--zoom', f'{self.zoom:g}', '--pivot', f'{px:g},{py:g}']


def _zoomed(name, zoom, **kw):
    return Framing(name, zoom=zoom, **kw)


FRAMINGS = {f.name: f for f in [
    # The 200-unit box at sub-fit zooms (natural size for a 20 px icon and up), the fit, display scale factors and
    # the steps up to 4x.
    *(_zoomed(n, z) for n, z in [('z01', 0.1), ('z025', 0.25), ('z033', 0.33), ('z05', 0.5), ('z075', 0.75),
                                 ('z1', 1.0), ('z125', 1.25), ('z15', 1.5), ('z175', 1.75), ('z2', 2.0),
                                 ('z3', 3.0), ('z4', 4.0)]),
    # About the top-centre band of the box, for wide artwork that a centre pivot zooms past.
    _zoomed('zt2', 2.0, pivot=(230, 48)), _zoomed('zt4', 4.0, pivot=(230, 48)),
    # 1080p and 240p: the box as tall as the frame, centred.
    Framing('hd', frame=(1920, 1080), box=(1080, 420, 0)),
    Framing('p240', frame=(426, 240), box=(240, 93, 0)),
]}
DEFAULT_FRAMINGS = ['z1', 'z2', 'z4', 'hd']


def framing_for_zoom(zoom, base='z1'):
    """The named framing of `base` at `zoom`, or a new one named like the others (1.3 -> z1p3)."""
    b = FRAMINGS[base]
    for f in FRAMINGS.values():
        if f.frame == b.frame and f.box == b.box and f.pivot == b.pivot and abs(f.zoom - zoom) < 1e-9:
            return f
    f = b.with_zoom(zoom, f'z{zoom:g}'.replace('.', 'p') if base == 'z1' else f'{base}x{zoom:g}'.replace('.', 'p'))
    FRAMINGS[f.name] = f
    return f


def framing_list(value):
    names = value.split(',') if isinstance(value, str) else list(value)
    for n in names:
        if n not in FRAMINGS:
            die(f'unknown framing {n!r}; known: {", ".join(FRAMINGS)}')
    return names


# ---- builds -----------------------------------------------------------------------------------------------------
def build_dir(name):
    return os.path.join(OUT, 'builds', name)


def build_binary(name):
    path = os.path.join(build_dir(name), 'svg-harness')
    if not os.path.exists(path):
        die(f'no build {name!r}: make it with build.py {name} [REV]')
    return path


def build_info(name):
    """build.json plus an id that changes whenever the binary is rebuilt, so results of an older binary are never
    reused for a new one."""
    path = build_binary(name)
    info = {}
    try:
        info = json.load(open(os.path.join(build_dir(name), 'build.json')))
    except (OSError, ValueError):
        pass
    rev = info.get('rev', '?')[:12] + ('+dirty' if info.get('dirty') else '')
    info['id'] = f'{rev}@{int(os.stat(path).st_mtime)}'
    info['label'] = f'{name} ({rev})'
    return info


def render(build, svg, framing, out_png, extra=(), timeout=600):
    """Render one frame with the build. Returns (ok, stats dict, error text)."""
    wait_for_disk()
    cmd = [build_binary(build), 'render', *FRAMINGS[framing].args(), '--stats', *extra, svg, out_png]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return False, {}, 'timeout'
    stats = last_json(r.stderr)
    if r.returncode != 0 or not os.path.exists(out_png):
        return False, stats, (r.stderr.strip()[-400:] or f'exit {r.returncode}')
    return True, stats, None


def last_json(text):
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line.startswith('{'):
            try:
                return json.loads(line)
            except ValueError:
                pass
    return {}


def temp_path(suffix):
    """A temporary file of its own for each worker: one shared name once mixed two workers' frames."""
    os.makedirs(TMP, exist_ok=True)
    fd, path = tempfile.mkstemp(suffix=suffix, dir=TMP)
    os.close(fd)
    os.remove(path)
    return path


def wait_for_disk(min_free=1.5e9):
    for _ in range(90):
        if shutil.disk_usage(OUT if os.path.exists(OUT) else REPO).free > min_free:
            return
        print('under 1.5 GB of disk free, waiting', file=sys.stderr, flush=True)
        time.sleep(20)


# ---- references ---------------------------------------------------------------------------------------------------
REFERENCES = ['chromium', 'chromium-gpu', 'firefox', 'webkit', 'webkitgtk', 'area']
REF_NAMES = {'chromium': 'Chromium', 'chromium-gpu': 'Chromium GPU', 'firefox': 'Firefox', 'webkit': 'macOS WebKit',
             'webkitgtk': 'WebKitGTK', 'area': 'Area estimate'}


def ref_png(browser, key, framing):
    return os.path.join(OUT, 'refs', browser, f'{key}@{framing}.png')


def frame_png(build, key, framing):
    return os.path.join(OUT, 'frames', build, f'{key}@{framing}.png')


def frames_dir(build):
    """OUT/frames/<build>, emptied when the build's binary has changed since its frames were kept."""
    d = os.path.join(OUT, 'frames', build)
    with _frames_lock:
        if build not in _frames_checked:
            current = build_info(build)['id']
            marker = os.path.join(d, 'build_id')
            if not os.path.exists(marker) or open(marker).read() != current:
                shutil.rmtree(d, ignore_errors=True)
                os.makedirs(d)
                open(marker, 'w').write(current)
            _frames_checked.add(build)
    return d


_frames_lock, _frames_checked = threading.Lock(), set()


def frame(build, f, framing):
    """The build's frame of corpus file f: the kept one, or rendered now and kept. Returns its path or None."""
    path = os.path.join(frames_dir(build), f'{f["key"]}@{framing}.png')
    if not os.path.exists(path):
        tmp = temp_path('.png')
        ok, _, err = render(build, f['path'], framing, tmp)
        if not ok:
            print(f'{build}: no frame of {f["key"]} at {framing}: {err}', file=sys.stderr)
            return None
        shutil.move(tmp, path)
    return path


def frame_image(build, f, framing):
    """The build's frame as an array, from OUT/frames when kept there, else rendered and not kept."""
    path = os.path.join(frames_dir(build), f'{f["key"]}@{framing}.png')
    if os.path.exists(path):
        return load(path)
    tmp = temp_path('.png')
    ok, _, err = render(build, f['path'], framing, tmp)
    if not ok:
        print(f'{build}: no frame of {f["key"]} at {framing}: {err}', file=sys.stderr)
        return None
    try:
        return load(tmp)
    finally:
        os.remove(tmp)


def ref_label(browser):
    """The reference's name with the version refs.py recorded."""
    try:
        version = open(os.path.join(OUT, 'refs', browser, 'version.txt')).read().strip()
    except OSError:
        version = ''
    return f'{REF_NAMES.get(browser, browser)} {version}'.strip()


def _first(*candidates):
    for c in candidates:
        if c and os.path.exists(c):
            return c
    return None


def chromium():
    found = os.environ.get('CHROMIUM') or _first(
        *sorted(glob.glob(os.path.expanduser('~/.cache/puppeteer/chrome-headless-shell/*/*/chrome-headless-shell')),
                key=_version_key, reverse=True),
        shutil.which('chrome-headless-shell'), shutil.which('chromium'), shutil.which('google-chrome'))
    if not found or not os.path.exists(found):
        die('no Chromium: set CHROMIUM, or install the headless shell with '
            '`npx @puppeteer/browsers install chrome-headless-shell@stable`')
    return found


def firefox():
    found = os.environ.get('FIREFOX') or _first(
        *(os.path.join(a, 'Contents/MacOS/firefox') for a in sorted(glob.glob('/Applications/Firefox*.app'))),
        shutil.which('firefox'), shutil.which('firefox-esr'))
    if not found or not os.path.exists(found):
        die('no Firefox: set FIREFOX to the firefox executable')
    return found


def _version_key(path):
    return [int(n) for n in re.findall(r'\d+', path.split('/chrome-headless-shell/')[1].split('/')[0])]


# ---- images and metrics -------------------------------------------------------------------------------------------
def load(path):
    return np.asarray(Image.open(path).convert('RGB'), dtype=np.int16)


def pixel_hash(path):
    im = Image.open(path).convert('RGBA')
    return hashlib.sha1(im.tobytes() + repr(im.size).encode()).hexdigest()[:20]


def delta(a, b):
    """Per-pixel difference (largest channel) over the area the two images share."""
    h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
    return np.abs(a[:h, :w] - b[:h, :w]).max(axis=2)


def metrics(a, b):
    """px: % of pixels beyond DIFF_LEVEL; faint: % 1..DIFF_LEVEL off; max: the largest difference."""
    d = delta(a, b)
    return {'px': round(100 * float((d > DIFF_LEVEL).mean()), 4),
            'faint': round(100 * float(((d > 0) & (d <= DIFF_LEVEL)).mean()), 4), 'max': int(d.max())}


def beyond(a, b):
    return 100 * float((delta(a, b) > DIFF_LEVEL).mean())


# ---- reports ----------------------------------------------------------------------------------------------------
def table(header, rows, align=None):
    """A Markdown table; align is a string of l/r per column (default: first left, the rest right)."""
    align = align or 'l' + 'r' * (len(header) - 1)
    out = ['| ' + ' | '.join(header) + ' |', '|' + '|'.join(':---' if a == 'l' else '---:' for a in align) + '|']
    out += ['| ' + ' | '.join(str(c) for c in row) + ' |' for row in rows]
    return '\n'.join(out)


def bold(text, major):
    return f'**{text}**' if major else str(text)


def pct(v, digits=2):
    return '-' if v is None else f'{v:.{digits}f}'


# ---- command lines ------------------------------------------------------------------------------------------------
def die(msg):
    raise SystemExit(f'{os.path.basename(sys.argv[0])}: {msg}')


def cli(doc, npos=(0, None), options=(), flags=(), argv=None):
    """Parse `POSITIONAL... key=value... flag...`. Returns (positionals, {option: value}, {flag}).
    -h/--help prints the script's docstring."""
    argv = sys.argv[1:] if argv is None else argv
    if any(a in ('-h', '--help') for a in argv):
        print(doc.strip())
        raise SystemExit(0)
    pos, opts, got = [], {}, set()
    for a in argv:
        m = re.match(r'^([a-z_]+)=(.*)$', a)
        if m and m.group(1) in options:
            opts[m.group(1)] = m.group(2)
        elif m:
            die(f'unknown option {m.group(1)}=; see -h')
        elif a in flags:
            got.add(a)
        else:
            pos.append(a)
    lo, hi = npos
    if len(pos) < lo or (hi is not None and len(pos) > hi):
        print(doc.strip().splitlines()[0], file=sys.stderr)
        die('wrong arguments; see -h')
    return pos, opts, got


if __name__ == '__main__':
    cli('common.py: shared definitions; prints the framings and the thresholds the reports use.', (0, 0))
    print(table(['framing', 'frame', 'box (size@x,y)', 'zoom', 'pivot', 'default'],
                [(f.name, '%dx%d' % f.frame, '%g@%g,%g' % f.box, f'{f.zoom:g}', '%g,%g' % f.pivot,
                  'yes' if f.name in DEFAULT_FRAMINGS else '') for f in FRAMINGS.values()], 'llllll'))
    print()
    print(table(['threshold', 'value'], [
        ('pixel differs (/255, any channel)', DIFF_LEVEL), ('major accuracy change (points of pixels)', MAJOR_PIXEL_POINTS),
        ('major timing change (%)', MAJOR_TIMING_PCT), ('major memory change (%)', MAJOR_MEMORY_PCT),
        ('reftest pass (% of pixels)', REFTEST_MAX_PCT)], 'lr'))
