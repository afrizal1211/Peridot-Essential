#!/usr/bin/env python3
"""Regression: stale renamed Rust entry points must not become Cargo targets."""
import pathlib
import subprocess
import tempfile
import tomllib
root = pathlib.Path(__file__).resolve().parents[2]
config = tomllib.loads((root / 'runtime/daemon-rust/Cargo.toml').read_text())
assert config['package'].get('autobins') is False
assert {entry['name'] for entry in config['bin']} == {'peridot_daemon', 'peridot_ctl'}
with tempfile.TemporaryDirectory(prefix='peridot-bin-layout-') as directory:
    path = pathlib.Path(directory)
    lines = ['[package]', 'name = "peridot-bin-layout-fixture"', 'version = "0.0.0"', 'edition = "2024"', 'autobins = false']
    for entry in config['bin']:
        lines += ['[[bin]]', f'name = "{entry["name"]}"', f'path = "{entry["path"]}"']
        source = path / entry['path']
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_text('fn main() {}\n')
    (path / 'Cargo.toml').write_text('\n'.join(lines) + '\n')
    for old in ['rodin_essentiald.rs', 'rodin_essentialctl.rs']:
        (path / 'src/bin' / old).write_text('use rodin_essential_backend::removed_crate;\nfn main() {}\n')
    subprocess.run(['cargo', 'check', '--offline', '--bins', '--manifest-path', str(path / 'Cargo.toml')], check=True)
print('PERIDOT_DAEMON_BINARY_LAYOUT=PASS (stale sources ignored)')
with tempfile.TemporaryDirectory(prefix='peridot-source-cleanup-') as directory:
    path = pathlib.Path(directory)
    (path / 'tools').mkdir()
    cleanup = path / 'tools/cleanup-legacy-paths.sh'
    cleanup.write_bytes((root / 'tools/cleanup-legacy-paths.sh').read_bytes())
    old = path / 'runtime/daemon-rust/src/bin/rodin_essentiald.rs'
    new = path / 'runtime/daemon-rust/src/bin/peridot_daemon.rs'
    old.parent.mkdir(parents=True)
    old.write_text('obsolete entry point')
    new.write_text('new entry point')
    notice = path / 'NOTICE'
    notice.write_text('Rodin Essential by NEESCHAL-3')
    subprocess.run(['bash', str(cleanup)], check=True)
    assert not old.exists()
    assert new.read_text() == 'new entry point'
    assert notice.read_text() == 'Rodin Essential by NEESCHAL-3'
print('PERIDOT_LEGACY_CLEANUP=PASS (new code and attribution preserved)')
