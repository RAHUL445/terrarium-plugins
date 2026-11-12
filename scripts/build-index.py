#!/usr/bin/env python3
"""Build an unsigned format-one catalog from signed artifacts and reviewed metadata.

Metadata: {"plugins": {"id": {"name": "…", "description": "…", "icon": "inbox",
"versions": {"0.1.0": {"min_host": "0.1.0-alpha.1", "yanked": false}}}}}
Sign separately: terra sign-index index.json --origin https://plugins.example.org --key /private/registry-key
No private keys are loaded here. --previous is required after sequence one.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile


def strict(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('Duplicate metadata key')
        result[key] = value
    return result


def load(path):
    return json.loads(path.read_text(encoding='utf-8'), object_pairs_hook=strict)


def terra_json(terra, artifact):
    # Do not echo subprocess stderr or package-controlled output on failure.
    result = subprocess.run([str(terra), 'verify', str(artifact), '--json'],
                            capture_output=True, text=True)
    if result.returncode:
        raise ValueError('Package verification failed')
    return json.loads(result.stdout, object_pairs_hook=strict)


def build(args):
    if args.registry != 'terrarium-official':
        raise ValueError('Unsupported registry')
    if type(args.sequence) is not int or args.sequence < 1:
        raise ValueError('Invalid sequence')
    if args.sequence > 1 and not args.previous:
        raise ValueError('Supply --previous to preserve release identities')
    metadata = load(args.metadata)
    if set(metadata) != {'plugins'}:
        raise ValueError('Invalid catalog metadata')
    if args.previous:
        result = subprocess.run([str(args.terra), 'check-index', str(args.previous), '--origin', args.origin], capture_output=True)
        if result.returncode:
            raise ValueError('Previous catalog failed host validation')
    previous = load(args.previous) if args.previous else None
    if previous and (args.sequence <= previous['sequence'] or previous['registry'] != args.registry):
        raise ValueError('Sequence must increase in the same registry')
    plugins = {p['id']: copy.deepcopy(p) for p in previous['plugins']} if previous else {}
    seen = set()
    for artifact in args.artifacts:
        info = terra_json(args.terra, artifact)
        ident, version = info['id'], info['version']
        if (ident, version) in seen:
            raise ValueError('Duplicate package identity')
        seen.add((ident, version))
        if not info['signer']:
            raise ValueError('Unsigned package')
        review = metadata['plugins'][ident]
        if set(review) != {'name', 'description', 'icon', 'versions'}:
            raise ValueError('Invalid reviewed metadata')
        for field in ('name', 'description', 'icon'):
            expected = info[field] if info[field] is not None else 'apps-plus'
            if review[field] != expected:
                raise ValueError('Catalog metadata disagrees with the package')
        release = review['versions'][version]
        if set(release) != {'min_host', 'yanked'}:
            raise ValueError('Invalid release metadata')
        entry = {'version': version, 'api': info['api'], 'url':
                 f"{args.origin}/v1/packages/{ident}/{version}/{info['digest']}.terrapkg",
                 'size': artifact.stat().st_size, 'digest': info['digest'],
                 'capabilities': sorted(info['capabilities']), **release}
        plugin = plugins.setdefault(ident, {'id': ident, 'name': review['name'],
            'description': review['description'], 'icon': review['icon'],
            'publisher': info['signer'], 'versions': []})
        if plugin['publisher'] != info['signer']:
            raise ValueError('Publisher identity changed')
        old = next((v for v in plugin['versions'] if v['version'] == version), None)
        if old and any(old[k] != entry[k] for k in ('api', 'url', 'size', 'digest', 'capabilities', 'min_host')):
            raise ValueError('Published release identity changed')
        if old:
            old.update(entry)
        else:
            plugin['versions'].append(entry)
        plugin.update({k: review[k] for k in ('name', 'description', 'icon')})
    # Metadata can yank retained versions without repacking artifacts.
    for ident, plugin in plugins.items():
        review = metadata['plugins'].get(ident)
        if review:
            for entry in plugin['versions']:
                release = review['versions'].get(entry['version'])
                if release:
                    if set(release) != {'min_host', 'yanked'} or release['min_host'] != entry['min_host']:
                        raise ValueError('Published compatibility metadata changed')
                    entry['yanked'] = release['yanked']
        plugin['versions'].sort(key=lambda v: tuple(int(n) for n in v['version'].split('.')))
        for entry in plugin['versions']:
            entry['capabilities'].sort()
    index = {'format': 1, 'registry': args.registry, 'generated_ms': args.generated_ms,
             'expires_ms': args.expires_ms, 'sequence': args.sequence,
             'plugins': [plugins[k] for k in sorted(plugins)]}
    raw = (json.dumps(index, ensure_ascii=False, indent=2) + '\n').encode('utf-8')
    # The host owns wire validation; no second implementation of its security rules.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.registry-', dir=args.output.parent) as temp:
        staged = Path(temp) / 'index.json'
        staged.write_bytes(raw)
        result = subprocess.run([str(args.terra), 'check-index', str(staged), '--origin', args.origin], capture_output=True)
        if result.returncode:
            raise ValueError('Catalog failed host validation')
        staged.replace(args.output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', type=Path, nargs='*')
    parser.add_argument('--terra', type=Path, required=True)
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--previous', type=Path)
    parser.add_argument('--registry', required=True)
    parser.add_argument('--sequence', type=int, required=True)
    parser.add_argument('--generated-ms', type=int, required=True)
    parser.add_argument('--expires-ms', type=int, required=True)
    parser.add_argument('--origin', required=True)
    parser.add_argument('--output', type=Path, required=True)
    try:
        build(parser.parse_args())
    except (ValueError, KeyError, TypeError, OSError):
        parser.exit(1, 'Could not build registry index; check artifacts and reviewed metadata.\n')


if __name__ == '__main__':
    main()
