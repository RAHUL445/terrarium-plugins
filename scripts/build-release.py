#!/usr/bin/env python3
"""Build/check every plugin with an explicit host CLI, then export a digest-locked bundle."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
API = 'terrarium:plugin@0.1.0'

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def verify_sdk():
    lock = json.loads((ROOT / 'sdk.lock.json').read_text())
    base = ROOT / 'vendor/terrarium-sdk'
    actual = {f.relative_to(base).as_posix(): digest(f) for f in base.rglob('*')
              if f.is_file() and 'target' not in f.relative_to(base).parts}
    if actual != lock['files']:
        raise ValueError('SDK snapshot differs from sdk.lock.json; refresh/review the SDK explicitly')
    return lock

def export(terra, output):
    sdk = verify_sdk()
    if output.exists():
        raise ValueError(f'Output exists; choose a new release directory: {output}')
    packages = sorted((ROOT / 'packages').glob('*/manifest.json'))
    if not packages:
        raise ValueError('No plugins found')
    # No output is published until every plugin builds and passes runtime checks.
    for manifest in packages:
        subprocess.run([str(terra), 'build', str(manifest.parent)], check=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.plugin-release-', dir=output.parent) as temp:
        stage = Path(temp) / 'bundle'
        stage.mkdir()
        lock = {'format': 1, 'api': API, 'plugins_commit': subprocess.check_output(
            ['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
            'source_dirty': bool(subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain'])),
            'sdk': sdk['source'], 'plugins': []}
        for source in packages:
            manifest = json.loads(source.read_text())
            plugin = manifest['id']
            if not re.fullmatch(r'[a-z][a-z0-9-]*', plugin) or plugin != source.parent.name:
                raise ValueError(f'Invalid package identity: {plugin}')
            if manifest['api'] != API:
                raise ValueError(f'Unsupported API: {manifest["api"]}')
            files = {'manifest.json', 'dist/plugin.wasm'}
            files.update(view['template'] for kind in ('pages', 'widgets') for view in manifest.get(kind, []))
            hashes = {}
            for name in sorted(files):
                relative = Path(name)
                if relative.is_absolute() or '..' in relative.parts or '\\' in name:
                    raise ValueError(f'Unsafe package path: {name}')
                src = source.parent / relative
                if not src.resolve().is_relative_to(source.parent.resolve()):
                    raise ValueError(f'Package symlink escapes: {name}')
                dest = stage / plugin / relative
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dest)
                hashes[name] = digest(dest)
            lock['plugins'].append({'id': plugin, 'version': manifest['version'], 'files': hashes})
        (stage / 'bundle.json').write_text(json.dumps(lock, indent=2) + '\n')
        stage.rename(output)
    print(f'Exported {len(packages)} plugins to {output}')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--terra', type=Path, required=True, help='Host terra CLI executable')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    export(args.terra.resolve(), args.output.resolve())
