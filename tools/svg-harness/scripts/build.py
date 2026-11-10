#!/usr/bin/env python3
"""build.py NAME [REV]

Build svg-harness as build NAME: OUT/builds/NAME/svg-harness plus build.json (rev, dirty flag, date).
Without REV: the library in the working tree. With REV: the library at REV, checked out in a temporary git
worktree with the working tree's tools/svg-harness copied in, so both sides of an A/B run the same harness and
only the library differs (a harness that differs between the sides moves frames the library change cannot reach).

Environment: CARGO_TARGET_DIR (default <repo>/target), shared by every build to reuse the dependencies.
"""
import datetime, glob, json, os, shutil, subprocess, sys, tempfile, time

from common import OUT, REPO, build_dir, cli, die

HARNESS = os.path.join(REPO, 'tools', 'svg-harness')
LIBRARY_PATHS = ['src', 'build.rs', 'Cargo.toml']


def git(*args, cwd=REPO):
    r = subprocess.run(['git', '-C', cwd, *args], capture_output=True, text=True)
    if r.returncode != 0:
        die(f'git {" ".join(args)}: {r.stderr.strip()}')
    return r.stdout.strip()


def prepare_worktree(rev):
    """A detached worktree at REV outside the repository (cargo would otherwise find this repository's workspace
    above it), with the current harness copied in."""
    tree = tempfile.mkdtemp(prefix='svg-harness-build-')
    os.rmdir(tree)
    git('worktree', 'add', '--detach', '-q', tree, rev)
    dest = os.path.join(tree, 'tools', 'svg-harness')
    shutil.rmtree(dest, ignore_errors=True)
    shutil.copytree(HARNESS, dest, ignore=shutil.ignore_patterns('target', '__pycache__', '*.pyc'))
    root_toml = open(os.path.join(tree, 'Cargo.toml')).read()
    lock_dir = tree
    if '[workspace]' not in root_toml:
        # The library at REV is no workspace: the harness becomes its own root.
        manifest = os.path.join(dest, 'Cargo.toml')
        text = open(manifest).read()
        if '[workspace]' not in text:
            open(manifest, 'a').write('\n[workspace]\n')
        lock_dir = dest
    # The working tree's lock file, so both sides resolve the same dependency versions where they can.
    if os.path.exists(os.path.join(REPO, 'Cargo.lock')):
        shutil.copy(os.path.join(REPO, 'Cargo.lock'), os.path.join(lock_dir, 'Cargo.lock'))
    return tree


def cargo_build(tree, target):
    manifest = os.path.join(tree, 'tools', 'svg-harness', 'Cargo.toml')
    env = {**os.environ, 'CARGO_TARGET_DIR': target, 'CARGO_PROFILE_RELEASE_DEBUG': '0'}
    # With one target directory for several trees, cargo can link a femtovg built from another tree's path:
    # clean both crates so this tree's sources are compiled.
    subprocess.run(['cargo', 'clean', '--release', '-q', '-p', 'femtovg', '-p', 'svg-harness', '--manifest-path', manifest],
                   env=env, cwd=tree, capture_output=True)
    r = subprocess.run(['cargo', 'build', '--release', '--manifest-path', manifest], env=env, cwd=tree)
    if r.returncode != 0:
        die('cargo build failed')
    return os.path.join(target, 'release', 'svg-harness')


def check_shaders(tree, binary):
    """The tree's wgpu shaders are embedded verbatim: each must be in the binary, or another tree's library was
    linked."""
    data = open(binary, 'rb').read()
    shaders = sorted(glob.glob(os.path.join(tree, 'src', 'renderer', 'wgpu', '*.wgsl')))
    missing = [os.path.basename(s) for s in shaders if open(s, 'rb').read() not in data]
    return len(shaders), missing


def main():
    pos, _, _ = cli(__doc__, (1, 2))
    name, rev = pos[0], (pos[1] if len(pos) > 1 else None)
    if '/' in name or name.startswith('.'):
        die('NAME must be a plain name')
    target = os.path.abspath(os.environ.get('CARGO_TARGET_DIR', os.path.join(REPO, 'target')))
    started = time.time()
    if rev:
        sha = git('rev-parse', '--verify', f'{rev}^{{commit}}')
        tree = prepare_worktree(sha)
        dirty = False
    else:
        sha, tree = git('rev-parse', 'HEAD'), REPO
        dirty = bool(git('status', '--porcelain', '--', *LIBRARY_PATHS))
    try:
        built = cargo_build(tree, target)
        if os.path.getmtime(built) < started - 1:
            die(f'{built} was not relinked')
        n, missing = check_shaders(tree, built)
        if missing:
            die(f'the binary lacks the shaders of {sha[:12]} ({", ".join(missing)}): a library from another tree was linked')
        out = build_dir(name)
        os.makedirs(out, exist_ok=True)
        shutil.copy2(built, os.path.join(out, 'svg-harness'))
        info = {'name': name, 'rev': sha, 'dirty': dirty, 'requested': rev or 'working tree',
                'date': datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='seconds')}
        json.dump(info, open(os.path.join(out, 'build.json'), 'w'), indent=1)
    finally:
        if rev:
            git('worktree', 'remove', '--force', tree)
    # Provenance: the recorded rev is the requested one, and the copy runs.
    recorded = json.load(open(os.path.join(out, 'build.json')))['rev']
    if recorded != sha:
        die(f'build.json records {recorded}, expected {sha}')
    r = subprocess.run([os.path.join(out, 'svg-harness'), 'render', '--help'], capture_output=True, text=True)
    if r.returncode != 0:
        die(f'the built binary does not run: {r.stderr.strip()[-300:]}')
    print(f'build {name}: {sha[:12]}{" + local changes" if dirty else ""}, {n} wgpu shaders checked, '
          f'{os.path.join(out, "svg-harness")}')


if __name__ == '__main__':
    main()
