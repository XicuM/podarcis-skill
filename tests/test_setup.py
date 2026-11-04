"""Vault setup with real Git repositories and a recorded dotagents call."""

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest
import yaml

import vault

SCRIPT = str(Path(vault.__file__))


@pytest.fixture
def npx(tmp_path, monkeypatch):
    """Put a fake npx on PATH that records its arguments, cwd, and agents.toml."""
    bin_dir, log = tmp_path / 'bin', tmp_path / 'npx.json'
    bin_dir.mkdir()
    fake = bin_dir / 'npx'
    fake.write_text(
        f'#!{sys.executable}\nimport json, os, sys\n'
        f'json.dump({{"args": sys.argv[1:], "cwd": os.getcwd(), '
        f'"toml": open("agents.toml").read()}}, open({str(log)!r}, "w"))\n'
    )
    fake.chmod(0o755)
    monkeypatch.setenv('PATH', f'{bin_dir}{os.pathsep}{os.environ["PATH"]}')
    return log


def bare_repo(path):
    subprocess.run(['git', 'init', '-q', '--bare', str(path)], check=True)
    return str(path)


def run(root, *args, stdin=None):
    return subprocess.run(
        [sys.executable, SCRIPT, '--root', str(root), *args],
        input=stdin, capture_output=True, text=True, timeout=60,
    )


def test_setup_clones_links_and_generates_dotagents_config(tmp_path, npx):
    root = tmp_path / 'vault'
    root.mkdir()
    wiki = bare_repo(tmp_path / 'wiki.git')
    config = {
        'repos': {'wiki': wiki},
        'mcp': ['semantic-scholar', {'name': 'custom', 'url': 'https://example.org/mcp',
                                     'headers': {'X-Key': 'value'}}],
        'skills': [{'name': 'extra', 'source': 'owner/repo', 'ref': 'abc'}],
    }
    result = run(root, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 0, result.stderr
    state = json.loads(result.stdout)
    assert state['actions'] == {'sources': 'unchanged', 'wiki': 'cloned', 'workspace': 'unchanged'}
    assert state['repos']['wiki'] == {'state': 'repo', 'url': wiki}
    assert state['repos']['sources']['state'] == 'local'
    assert yaml.safe_load((root / 'podarcis.yaml').read_text())['repos']['wiki'] == wiki
    assert (root / '.env').stat().st_mode & 0o777 == 0o600
    assert (root / '.agents/skills/podarcis/SKILL.md').is_file()
    call = json.loads(npx.read_text())
    assert call['args'] == ['--yes', '@sentry/dotagents@3.4.0', '--project', 'install']
    assert call['cwd'] == str(root)

    generated = tomllib.loads(call['toml'])
    assert generated['agents'] == ['claude', 'codex', 'opencode', 'vscode']
    assert generated['skills'] == [{'name': 'extra', 'source': 'owner/repo', 'ref': 'abc'}]
    assert [m['name'] for m in generated['mcp']] == ['semantic-scholar', 'custom']
    assert generated['mcp'][0]['args'][-1] == 's2-mcp-server'
    assert generated['mcp'][1]['headers'] == {'X-Key': 'value'}

    # Changing a URL repoints the existing clone; the installed copy can rerun setup.
    moved = bare_repo(tmp_path / 'moved.git')
    (root / 'podarcis.yaml').write_text(f'repos:\n  wiki: {moved}\n')
    rerun = subprocess.run(
        [sys.executable, str(root / '.agents/skills/podarcis/scripts/vault.py'),
         '--root', str(root), 'setup'],
        capture_output=True, text=True, timeout=60,
    )
    assert rerun.returncode == 0, rerun.stderr
    assert json.loads(rerun.stdout)['actions']['wiki'] == 'remote updated'
    url = subprocess.run(['git', '-C', str(root / 'wiki'), 'remote', 'get-url', 'origin'],
                         capture_output=True, text=True).stdout.strip()
    assert url == moved


def test_first_setup_writes_default_config(tmp_path, npx):
    result = run(tmp_path, 'setup')
    assert result.returncode == 0, result.stderr
    assert yaml.safe_load((tmp_path / 'podarcis.yaml').read_text()) == json.loads(result.stdout)['config']
    assert all((tmp_path / name).is_dir() for name in ('sources', 'wiki', 'workspace'))


def test_setup_never_clones_over_local_files(tmp_path, npx):
    root = tmp_path / 'vault'
    (root / 'sources').mkdir(parents=True)
    (root / 'sources/note.md').write_text('local research\n')
    (root / 'podarcis.yaml').write_text(f'repos:\n  sources: {bare_repo(tmp_path / "s.git")}\n')
    result = run(root, 'setup')
    assert result.returncode == 1
    assert 'has files but no Git repository' in result.stderr
    assert (root / 'sources/note.md').read_text() == 'local research\n'
    assert not npx.exists()


@pytest.mark.parametrize('config, message', [
    ({'mcp': ['unknown-server']}, 'Not a recommended MCP server'),
    ({'mcp': ['semantic-scholar', 'semantic-scholar']}, 'unique names'),
    ({'repos': {'notes': 'x'}}, 'repos takes only'),
    ({'repos': {'wiki': '--upload-pack=touch'}}, 'Git URL'),
    ({'skills': [{'name': 'podarcis', 'source': 'x'}]}, 'other than podarcis'),
    ({'extra': 1}, 'takes only'),
])
def test_invalid_config_is_rejected_before_changes(tmp_path, npx, config, message):
    result = run(tmp_path, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 1
    assert message in result.stderr
    assert not (tmp_path / 'podarcis.yaml').exists()
    assert not npx.exists()


def test_failed_install_is_reported(tmp_path, monkeypatch):
    bin_dir = tmp_path / 'bin'
    bin_dir.mkdir()
    (bin_dir / 'npx').write_text('#!/bin/sh\necho registry unreachable >&2\nexit 3\n')
    (bin_dir / 'npx').chmod(0o755)
    monkeypatch.setenv('PATH', f'{bin_dir}{os.pathsep}{os.environ["PATH"]}')
    root = tmp_path / 'vault'
    root.mkdir()
    result = run(root, 'setup')
    assert result.returncode == 1
    assert 'dotagents install failed: registry unreachable' in result.stderr
    assert result.stdout == ''


def test_config_reports_defaults_catalogue_and_folders(tmp_path):
    (tmp_path / 'wiki').mkdir()
    result = run(tmp_path, 'config')
    assert result.returncode == 0, result.stderr
    state = json.loads(result.stdout)
    assert state['config'] == {
        'agents': ['claude', 'codex', 'opencode', 'vscode'],
        'repos': {'sources': None, 'wiki': None, 'workspace': None},
        'mcp': [],
        'skills': [],
    }
    assert set(state['catalogue']) == {'semantic-scholar', 'google-drive', 'overleaf-suggest'}
    assert state['repos']['wiki'] == {'state': 'local', 'url': None, 'empty': True}
    assert state['repos']['sources'] == {'state': 'missing', 'url': None}
