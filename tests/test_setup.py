"""Vault setup with real Git repositories, installed skills, and each agent's configuration."""

import json
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path

import pytest
import yaml

import podarcis

SCRIPT = str(Path(podarcis.__file__))
SKILL = podarcis.SKILL


def bare_repo(path):
    subprocess.run(['git', 'init', '-q', '--bare', str(path)], check=True)
    return str(path)


def skill_repo(path):
    """A Git repository with a skill in sub/skill; returns its file:// URL and commit."""
    (path / 'sub/skill').mkdir(parents=True)
    (path / 'sub/skill/SKILL.md').write_text('---\nname: extra\n---\n')
    git = ['git', '-C', str(path), '-c', 'user.name=t', '-c', 'user.email=t@t']
    subprocess.run(['git', 'init', '-q', str(path)], check=True)
    subprocess.run([*git, 'add', '.'], check=True)
    subprocess.run([*git, 'commit', '-q', '-m', 'skill'], check=True)
    commit = subprocess.run([*git, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
    return f'file://{path}', commit


def run(root, *args, stdin=None, script=SCRIPT):
    return subprocess.run(
        [sys.executable, script, '--root', str(root), *args],
        input=stdin, capture_output=True, text=True, timeout=60,
    )


def test_setup_clones_installs_and_configures_each_agent(tmp_path):
    root = tmp_path / 'vault'
    (root / '.codex').mkdir(parents=True)
    (root / '.codex/config.toml').write_text(
        'model = "o3"\n\n[mcp_servers.old]\ncommand = "old"\n\n[mcp_servers.old.env]\nA = "1"\n\n'
        '[profiles.fast]\nmodel = "mini"\n'
    )
    (root / '.mcp.json').write_text('{"other": true, "mcpServers": {"old": {"command": "old"}}}')
    wiki = bare_repo(tmp_path / 'wiki.git')
    config = {
        'repos': {'wiki': wiki},
        'mcp': ['semantic-scholar', {'name': 'custom', 'url': 'https://example.org/mcp', 'headers': {'X-Key': 'v'}}],
    }
    result = run(root, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 0, result.stderr
    state = json.loads(result.stdout)
    assert state['actions'] == {
        'sources': 'unchanged', 'wiki': 'cloned', 'workspace': 'unchanged', 'podarcis': f'installed {podarcis.VERSION}',
    }
    assert state['repos']['wiki'] == {'state': 'repo', 'url': wiki}
    assert yaml.safe_load((root / 'podarcis.yaml').read_text())['repos']['wiki'] == wiki
    assert (root / '.env').stat().st_mode & 0o777 == 0o600
    assert (root / '.agents/skills/podarcis/scripts/podarcis.py.lock').is_file()
    assert (root / '.claude/skills/podarcis/SKILL.md').is_file()

    s2_args = podarcis.load_catalogue()['semantic-scholar']['mcp']['args']
    claude = json.loads((root / '.mcp.json').read_text())
    assert claude == {'other': True, 'mcpServers': {
        'semantic-scholar': {'command': 'uv', 'args': s2_args},
        'custom': {'type': 'http', 'url': 'https://example.org/mcp', 'headers': {'X-Key': 'v'}},
    }}
    codex = tomllib.loads((root / '.codex/config.toml').read_text())
    assert codex == {
        'model': 'o3',
        'profiles': {'fast': {'model': 'mini'}},
        'mcp_servers': {
            'semantic-scholar': {'command': 'uv', 'args': s2_args},
            'custom': {'url': 'https://example.org/mcp', 'http_headers': {'X-Key': 'v'}},
        },
    }
    assert json.loads((root / '.opencode/opencode.jsonc').read_text())['mcp'] == {
        'semantic-scholar': {'type': 'local', 'command': ['uv', *s2_args]},
        'custom': {'type': 'remote', 'url': 'https://example.org/mcp', 'headers': {'X-Key': 'v'}},
    }
    assert json.loads((root / '.vscode/mcp.json').read_text())['servers']['semantic-scholar'] == {
        'type': 'stdio', 'command': 'uv', 'args': s2_args,
    }

    # The installed copy reruns setup without reinstalling itself, and repoints a changed remote.
    moved = bare_repo(tmp_path / 'moved.git')
    (root / 'podarcis.yaml').write_text(f'agents: [codex]\nrepos:\n  wiki: {moved}\n')
    rerun = run(root, 'setup', script=str(root / '.agents/skills/podarcis/scripts/podarcis.py'))
    assert rerun.returncode == 0, rerun.stderr
    assert json.loads(rerun.stdout)['actions'] == {'sources': 'unchanged', 'wiki': 'remote updated', 'workspace': 'unchanged'}
    url = subprocess.run(['git', '-C', str(root / 'wiki'), 'remote', 'get-url', 'origin'],
                         capture_output=True, text=True).stdout.strip()
    assert url == moved
    assert tomllib.loads((root / '.codex/config.toml').read_text()) == {'model': 'o3', 'profiles': {'fast': {'model': 'mini'}}}
    assert 'custom' in json.loads((root / '.mcp.json').read_text())['mcpServers']  # unselected agents are left alone


def test_first_setup_writes_default_config(tmp_path):
    result = run(tmp_path, 'setup')
    assert result.returncode == 0, result.stderr
    assert yaml.safe_load((tmp_path / 'podarcis.yaml').read_text()) == json.loads(result.stdout)['config']
    assert all((tmp_path / name).is_dir() for name in ('sources', 'wiki', 'workspace'))
    assert json.loads((tmp_path / '.vscode/mcp.json').read_text()) == {'servers': {}}


def test_extra_skills_are_fetched_at_their_ref_and_linked_for_claude(tmp_path):
    url, commit = skill_repo(tmp_path / 'skills')
    config = {'agents': ['claude'], 'skills': [{'name': 'extra', 'source': f'git:{url}', 'ref': commit, 'path': 'sub/skill'}]}
    result = run(tmp_path / 'skills', 'setup', '--stdin', stdin=json.dumps(config))  # any folder works as a vault
    root = tmp_path / 'skills'
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)['actions']['extra'] == 'installed'
    assert (root / '.claude/skills/extra/SKILL.md').read_text().startswith('---\nname: extra')
    assert not (root / '.agents/skills/extra/.git').exists()
    rerun = run(root, 'setup')
    assert json.loads(rerun.stdout)['actions']['extra'] == 'unchanged'


def test_failed_skill_fetch_is_reported(tmp_path):
    config = {'skills': [{'name': 'extra', 'source': f'git:file://{tmp_path}/missing'}]}
    result = run(tmp_path, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 1
    assert 'Fetching skill extra failed' in result.stderr
    assert result.stdout == ''


def test_setup_never_clones_over_local_files(tmp_path):
    root = tmp_path / 'vault'
    (root / 'sources').mkdir(parents=True)
    (root / 'sources/note.md').write_text('local research\n')
    (root / 'podarcis.yaml').write_text(f'repos:\n  sources: {bare_repo(tmp_path / "s.git")}\n')
    result = run(root, 'setup')
    assert result.returncode == 1
    assert 'has files but no Git repository' in result.stderr
    assert (root / 'sources/note.md').read_text() == 'local research\n'
    assert not (root / '.agents').exists() and not (root / '.mcp.json').exists()


def test_setup_refuses_to_rewrite_commented_json(tmp_path):
    (tmp_path / '.vscode').mkdir()
    (tmp_path / '.vscode/mcp.json').write_text('// mine\n{"servers": {}}')
    result = run(tmp_path, 'setup')
    assert result.returncode == 1
    assert '.vscode/mcp.json is not plain JSON' in result.stderr
    assert (tmp_path / '.vscode/mcp.json').read_text() == '// mine\n{"servers": {}}'


@pytest.mark.parametrize('config, message', [
    ({'mcp': ['unknown-server']}, 'Not a recommended MCP server'),
    ({'mcp': ['semantic-scholar', 'semantic-scholar']}, 'unique names'),
    ({'mcp': [{'name': 'both', 'command': 'x', 'url': 'https://example.org'}]}, 'either command and args'),
    ({'agents': ['cursor']}, 'agents takes only claude, codex, opencode, vscode'),
    ({'repos': {'notes': 'x'}}, 'repos takes only'),
    ({'repos': {'wiki': '--upload-pack=touch'}}, 'Git URL'),
    ({'skills': [{'name': 'podarcis', 'source': 'x/y'}]}, 'other than podarcis'),
    ({'skills': [{'name': 'extra', 'source': '--upload-pack=touch'}]}, 'other than podarcis'),
    ({'extra': 1}, 'takes only'),
])
def test_invalid_config_is_rejected_before_changes(tmp_path, config, message):
    result = run(tmp_path, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 1
    assert message in result.stderr
    assert list(tmp_path.iterdir()) == []


def release_archive(tmp_path, with_skill=True):
    """A GitHub-style release archive of this repository, as a file:// URL."""
    archive = tmp_path / 'release.tar.gz'
    with tarfile.open(archive, 'w:gz') as tar:
        tar.add(SKILL if with_skill else SKILL / 'references',
                f'podarcis-skill-{podarcis.VERSION}/skills/podarcis' if with_skill else 'other')
    return archive.as_uri()


def test_script_run_alone_installs_its_release_first(tmp_path, monkeypatch, capsys):
    # As with `uv run <url>/podarcis.py`, the script has no skill folder around it.
    root = tmp_path / 'vault'
    root.mkdir()
    monkeypatch.setattr(podarcis, 'ARCHIVE', release_archive(tmp_path))
    monkeypatch.setattr(podarcis, 'SKILL', tmp_path / 'cache')
    monkeypatch.setattr(sys, 'argv', ['podarcis', '--root', str(root), 'config'])
    podarcis.main()
    assert 'semantic-scholar' in json.loads(capsys.readouterr().out)['catalogue']
    assert (root / '.agents/skills/podarcis/scripts/podarcis.py').is_file()
    assert not (root / 'podarcis.yaml').exists()


def test_release_without_the_skill_is_reported(tmp_path, monkeypatch):
    monkeypatch.setattr(podarcis, 'ARCHIVE', release_archive(tmp_path, with_skill=False))
    monkeypatch.setattr(podarcis, 'SKILL', tmp_path / 'cache')
    with pytest.raises(RuntimeError, match='does not contain skills/podarcis'):
        podarcis.install(tmp_path)
    assert not (tmp_path / '.agents/skills/podarcis').exists()


def test_script_version_matches_the_skill():
    metadata = yaml.safe_load((podarcis.SKILL / 'SKILL.md').read_text().split('---')[1])
    assert metadata['metadata']['version'] == podarcis.VERSION


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
