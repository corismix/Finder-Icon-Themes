#!/usr/bin/env python3
"""Repack an existing Material archive; never fetch or change artwork bytes.
Usage: python3 tools/trim_theme.py INPUT OUTPUT [LICENSE]
Keep LICENSE/NOTICE/attribution entries, association rules and every reachable
resource, including root-folder associations. Output is deterministic.
"""
import gzip
import io
import json
import pathlib
import sys
import tarfile

with tarfile.open(sys.argv[1], 'r:gz') as archive:
    files = {member.name.removeprefix('./'): archive.extractfile(member).read()
             for member in archive if member.isfile() and not pathlib.PurePosixPath(member.name).name.startswith('._')}
if len(sys.argv) > 3:
    files['LICENSE'] = pathlib.Path(sys.argv[3]).read_bytes()
manifest = json.loads(files['material-icons.json'])
ids = {manifest[key] for key in ('file', 'folder', 'rootFolder') if manifest.get(key)}
for key in ('fileNames', 'fileExtensions', 'folderNames', 'rootFolderNames'):
    ids.update(manifest.get(key, {}).values())
manifest['iconDefinitions'] = {key: definition for key, definition in manifest['iconDefinitions'].items() if key in ids}
sources = {'icons/' + pathlib.PurePosixPath(d['iconPath']).name for d in manifest['iconDefinitions'].values()}
missing = sources - files.keys()
if missing:
    raise ValueError(f'Missing resources: {sorted(missing)}')
keep = {'material-icons.json'} | sources | {key for key in files if any(word in key.lower() for word in ('license', 'notice', 'copyright', 'attribution'))}
files['material-icons.json'] = json.dumps(manifest, separators=(',', ':'), ensure_ascii=False).encode()
with open(sys.argv[2], 'wb') as output:
    with gzip.GzipFile(filename='', mode='wb', fileobj=output, mtime=0, compresslevel=9) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            for key in sorted(keep):
                entry = tarfile.TarInfo(key)
                entry.size = len(files[key])
                entry.mode = 0o644
                archive.addfile(entry, io.BytesIO(files[key]))
print(f'{len(sources)} SVGs retained; output {pathlib.Path(sys.argv[2]).stat().st_size} bytes')
