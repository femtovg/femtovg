#!/usr/bin/env python3
"""refs.py BROWSER [group=G] [keys=FILE] [framings=z1,z2,z4,hd] [workers=N]

Browser references: OUT/refs/<BROWSER>/<KEY>@<framing>.png, each file on the page page.py makes for the framing.
Resumable: existing references are kept (delete the directory after a browser update). BROWSER is one of
  chromium      headless Chromium rasterizing in software (--disable-gpu): deterministic
  chromium-gpu  Chromium rasterizing on the GPU (ANGLE on Metal on macOS), as desktop Chrome draws. The software
                rasterizer snaps path edges to quarter pixels; the GPU one does not, so edges differ between them.
  firefox       headless Firefox
  webkit        macOS system WebKit in WKWebView via wk_shot.swift
  webkitgtk     Linux WebKitGTK 4.1 via wkgtk_shot.c (use xvfb-run on a headless host)
  area          supersampled coverage estimate from Chromium GPU, at 8× (4× for wider frames)
"""
import os, platform, shlex, shutil, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor

import numpy as np
from PIL import Image

from common import (OUT, REFERENCES, DEFAULT_FRAMINGS, FRAMINGS, SCRIPTS, chromium, cli, die, firefox, framing_list,
                    ref_png, temp_path)
from corpus import animated, select
from page import page

Image.MAX_IMAGE_PIXELS = None
WORKERS = {'chromium': 6, 'chromium-gpu': 4, 'firefox': 3, 'webkit': 4, 'webkitgtk': 2, 'area': 3}
AREA_SCALE = 8


def gpu_flags():
    angle = 'metal' if platform.system() == 'Darwin' else 'vulkan'
    return ['--enable-gpu', f'--use-angle={angle}', '--enable-gpu-rasterization', '--ignore-gpu-blocklist']


def write_page(f, framing):
    html = page(f['path'], FRAMINGS[framing])
    if html is None:
        return None
    path = temp_path('.html')
    open(path, 'w', encoding='utf-8').write(html)
    return path


def crop_to_frame(png, framing):
    w, h = FRAMINGS[framing].frame
    im = Image.open(png).convert('RGB')
    if im.size != (w, h):
        im = im.crop((0, 0, w, h))
    im.save(png)


def shoot_chromium(html, png, framing, flags, scale=1):
    w, h = FRAMINGS[framing].frame
    # A profile of its own per launch: back-to-back launches sharing one come back blank.
    with tempfile.TemporaryDirectory(dir=os.path.dirname(html)) as profile:
        try:
            r = subprocess.run([chromium(), '--headless', *flags, f'--user-data-dir={profile}', '--hide-scrollbars',
                                f'--force-device-scale-factor={scale}', f'--window-size={w},{h}',
                                '--default-background-color=FFFFFFFF', f'--screenshot={png}', f'file://{html}'],
                               capture_output=True, text=True, timeout=300)
            if not os.path.exists(png):
                report_failure('Chromium', r.stderr)
        except subprocess.TimeoutExpired:
            pass


_reported = set()


def report_failure(browser, stderr):
    """The browser's last error line, once per browser: a launch that fails every time (no usable sandbox, missing
    libraries) otherwise shows only as a count of failed references."""
    lines = [l for l in (stderr or '').splitlines() if 'ERROR' in l or 'rror' in l] or (stderr or '').splitlines()
    if browser not in _reported and lines:
        _reported.add(browser)
        print(f'{browser}: {lines[-1].strip()[:400]}', file=sys.stderr)


def shoot_firefox(html, png, framing):
    w, h = FRAMINGS[framing].frame
    for _attempt in range(2):
        with tempfile.TemporaryDirectory(dir=os.path.dirname(html)) as profile:
            try:
                r = subprocess.run([firefox(), '--headless', '--no-remote', '--profile', profile,
                                    f'--window-size={w},{h}', '--screenshot', png, f'file://{html}'],
                                   capture_output=True, text=True, timeout=120)
            except subprocess.TimeoutExpired:
                r = None
        if os.path.exists(png):
            return
    if r is not None:
        report_failure('Firefox', r.stderr)


def wk_shot():
    """wk_shot compiled once into OUT/bin, again when the source is newer."""
    if platform.system() != 'Darwin':
        die('webkit references need macOS')
    src, binary = os.path.join(SCRIPTS, 'wk_shot.swift'), os.path.join(OUT, 'bin', 'wk_shot')
    if not os.path.exists(binary) or os.path.getmtime(binary) < os.path.getmtime(src):
        os.makedirs(os.path.dirname(binary), exist_ok=True)
        r = subprocess.run(['swiftc', '-O', src, '-o', binary], capture_output=True, text=True)
        if r.returncode != 0:
            die(f'swiftc wk_shot.swift failed: {r.stderr.strip()[-400:]}')
    return binary


def wkgtk_shot():
    if platform.system() != 'Linux':
        die('webkitgtk references need Linux')
    src, binary = os.path.join(SCRIPTS, 'wkgtk_shot.c'), os.path.join(OUT, 'bin', 'wkgtk_shot')
    if not os.path.exists(binary) or os.path.getmtime(binary) < os.path.getmtime(src):
        os.makedirs(os.path.dirname(binary), exist_ok=True)
        try:
            flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'webkit2gtk-4.1'], text=True)
            subprocess.run(['cc', '-O2', src, '-o', binary, *shlex.split(flags)], check=True)
        except (OSError, subprocess.CalledProcessError) as e:
            die(f'cannot build wkgtk_shot (needs cc, pkg-config and WebKitGTK 4.1 development files): {e}')
    return binary


def reference(browser, f, framing):
    """Make one reference unless it exists. Returns 'kept', 'made', 'unframed' or 'failed'."""
    png = ref_png(browser, f['key'], framing)
    if os.path.exists(png):
        return 'kept'
    os.makedirs(os.path.dirname(png), exist_ok=True)
    html = write_page(f, framing)
    if html is None:
        return 'unframed'
    shot = temp_path('.png')
    try:
        if browser == 'chromium':
            shoot_chromium(html, shot, framing, ['--disable-gpu'])
        elif browser == 'chromium-gpu':
            shoot_chromium(html, shot, framing, gpu_flags())
        elif browser == 'firefox':
            shoot_firefox(html, shot, framing)
        elif browser == 'webkit':
            w, h = FRAMINGS[framing].frame
            try:
                subprocess.run([wk_shot(), html, shot, str(w), str(h)], capture_output=True, timeout=60)
            except subprocess.TimeoutExpired:
                pass
        elif browser == 'webkitgtk':
            w, h = FRAMINGS[framing].frame
            env = {**os.environ, 'GDK_SCALE': '1', 'GDK_DPI_SCALE': '1'}
            try:
                subprocess.run([wkgtk_shot(), html, shot, str(w), str(h)], env=env, check=True, timeout=60)
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
                return 'failed'
            if os.path.exists(shot):
                with Image.open(shot) as im:
                    if im.size != (w, h):
                        print(f'{f["key"]}@{framing}: WebKitGTK produced {im.size}, expected {(w, h)}',
                              file=sys.stderr)
                        return 'failed'
        elif browser == 'area':
            w, h = FRAMINGS[framing].frame
            scale = min(AREA_SCALE, 4) if w > 1000 else AREA_SCALE
            shoot_chromium(html, shot, framing, gpu_flags(), scale)
            if os.path.exists(shot):
                im = np.asarray(Image.open(shot).convert('RGB'), dtype=np.float64)
                os.remove(shot)
                if im.shape[0] < h * scale or im.shape[1] < w * scale:
                    return 'failed'
                im = im[:h * scale, :w * scale].reshape(h, scale, w, scale, 3).mean(axis=(1, 3))
                Image.fromarray(np.rint(im).astype(np.uint8)).save(shot)
        else:
            die(f'unknown browser {browser!r}; one of {", ".join(REFERENCES)}')
        if not os.path.exists(shot):
            return 'failed'
        crop_to_frame(shot, framing)
        shutil.move(shot, png)
        return 'made'
    finally:
        os.remove(html)
        if os.path.exists(shot):
            os.remove(shot)


def record_version(browser):
    path = os.path.join(OUT, 'refs', browser, 'version.txt')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        if browser == 'webkit':
            v = subprocess.run(['defaults', 'read', '/Applications/Safari.app/Contents/Info', 'CFBundleShortVersionString'],
                               capture_output=True, text=True).stdout.strip()
            v = f'{platform.mac_ver()[0]} WKWebView' + (f' (system Safari {v})' if v else '')
        elif browser == 'webkitgtk':
            v = subprocess.check_output([wkgtk_shot(), '--version'], text=True).strip()
        else:
            exe = firefox() if browser == 'firefox' else chromium()
            out = subprocess.run([exe, '--version'], capture_output=True, text=True, timeout=60).stdout
            v = next((w for w in out.split() if w[:1].isdigit()), '')
    except (OSError, subprocess.SubprocessError):
        v = ''
    open(path, 'w').write(v + '\n')


def main():
    pos, opts, _ = cli(__doc__, (1, 1), ('group', 'keys', 'framings', 'workers'))
    browser = pos[0]
    if browser not in REFERENCES:
        die(f'unknown browser {browser!r}; one of {", ".join(REFERENCES)}')
    if browser == 'webkit':
        wk_shot()
    elif browser == 'webkitgtk':
        wkgtk_shot()
        if not (os.environ.get('DISPLAY') or os.environ.get('WAYLAND_DISPLAY')):
            die('webkitgtk needs a display; on a headless host use xvfb-run')
    fs = select(opts)
    framings = framing_list(opts.get('framings', DEFAULT_FRAMINGS))
    record_version(browser)
    jobs = [(f, fr) for f in fs for fr in framings]
    counts = {}
    with ThreadPoolExecutor(int(opts.get('workers', WORKERS[browser]))) as ex:
        for i, r in enumerate(ex.map(lambda j: reference(browser, *j), jobs)):
            counts[r] = counts.get(r, 0) + 1
            if (i + 1) % 200 == 0:
                print(f'{browser}: {i + 1}/{len(jobs)} {counts}', file=sys.stderr, flush=True)
    moving = sum(animated(f['path']) for f in fs)
    print(f'{browser}: {len(jobs)} references of {len(fs)} files; ' + ', '.join(f'{v} {k}' for k, v in sorted(counts.items()))
          + (f'; {moving} files are SMIL-animated (left out of metrics)' if moving else ''))
    if counts.get('failed'):
        sys.exit(1)


if __name__ == '__main__':
    main()
