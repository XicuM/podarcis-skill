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
    assert yaml.safe_load((root / '.agents/podarcis/config.yaml').read_text()) == {
        'repos': {'sources': None, 'wiki': wiki, 'workspace': None}}
    assert yaml.safe_load((root / '.agents/podarcis/tools.yaml').read_text()) == {'mcp': config['mcp'], 'skills': []}
    assert (root / '.agents/podarcis/.env').stat().st_mode & 0o777 == 0o600
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
    (root / '.agents/podarcis/config.yaml').write_text(f'repos:\n  wiki: {moved}\n')
    (root / '.agents/podarcis/tools.yaml').write_text('# mine\nmcp: []\n')
    rerun = run(root, 'setup', script=str(root / '.agents/skills/podarcis/scripts/podarcis.py'))
    assert rerun.returncode == 0, rerun.stderr
    assert json.loads(rerun.stdout)['actions'] == {'sources': 'unchanged', 'wiki': 'remote updated', 'workspace': 'unchanged'}
    url = subprocess.run(['git', '-C', str(root / 'wiki'), 'remote', 'get-url', 'origin'],
                         capture_output=True, text=True).stdout.strip()
    assert url == moved
    # Every agent follows tools.yaml, and setup leaves the user's own edits to it alone.
    assert tomllib.loads((root / '.codex/config.toml').read_text()) == {'model': 'o3', 'profiles': {'fast': {'model': 'mini'}}}
    assert json.loads((root / '.mcp.json').read_text())['mcpServers'] == {}
    assert (root / '.agents/podarcis/tools.yaml').read_text() == '# mine\nmcp: []\n'


def test_first_setup_writes_default_config(tmp_path):
    result = run(tmp_path, 'setup')
    assert result.returncode == 0, result.stderr
    config = json.loads(result.stdout)['config']
    assert yaml.safe_load((tmp_path / '.agents/podarcis/config.yaml').read_text()) == {'repos': config['repos']}
    assert yaml.safe_load((tmp_path / '.agents/podarcis/tools.yaml').read_text()) == {'mcp': [], 'skills': []}
    assert (tmp_path / '.agents/podarcis/tools.yaml').read_text().startswith('# Tools for every agent')
    assert not (tmp_path / 'podarcis.yaml').exists() and not (tmp_path / '.env').exists()
    assert all((tmp_path / name).is_dir() for name in ('sources', 'wiki', 'workspace'))
    assert json.loads((tmp_path / '.vscode/mcp.json').read_text()) == {'servers': {}}
    assert json.loads((tmp_path / '.mcp.json').read_text()) == {'mcpServers': {}}
    assert (tmp_path / '.codex/config.toml').is_file() and (tmp_path / '.opencode/opencode.jsonc').is_file()


def test_extra_skills_are_fetched_at_their_ref_and_linked_for_claude(tmp_path):
    url, commit = skill_repo(tmp_path / 'skills')
    config = {'skills': [{'name': 'extra', 'source': f'git:{url}', 'ref': commit, 'path': 'sub/skill'}]}
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
    assert not (root / '.agents/skills').exists() and not (root / '.mcp.json').exists()


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
    assert not (root / '.agents/podarcis').exists()


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
    assert state['version'] == podarcis.VERSION and state['env'] == []
    assert state['config'] == {
        'repos': {'sources': None, 'wiki': None, 'workspace': None},
        'mcp': [],
        'skills': [],
    }
    assert set(state['catalogue']) == {'semantic-scholar', 'overleaf-suggest'}
    assert state['repos']['wiki'] == {'state': 'local', 'url': None, 'empty': True}
    assert state['repos']['sources'] == {'state': 'missing', 'url': None}


@pytest.mark.parametrize('folder', ['', '.agents/'])
def test_setup_splits_an_older_configuration_and_moves_credentials(tmp_path, folder):
    # 2.0 kept podarcis.yaml and .env in the root, 2.1 in .agents/; both chose agents and could use google-drive.
    (tmp_path / '.agents').mkdir()
    (tmp_path / f'{folder}podarcis.yaml').write_text(
        'agents: [claude]\nrepos:\n  wiki: null\nmcp: [semantic-scholar, google-drive]\n')
    (tmp_path / f'{folder}.env').write_text('SEMANTIC_SCHOLAR_API_KEY=old\n')
    (tmp_path / f'{folder}.env').chmod(0o644)
    before = json.loads(run(tmp_path, 'config').stdout)
    assert before['config']['mcp'] == ['semantic-scholar'] and before['env'] == ['SEMANTIC_SCHOLAR_API_KEY']
    result = run(tmp_path, 'setup')
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)['env'] == ['SEMANTIC_SCHOLAR_API_KEY']
    assert not (tmp_path / f'{folder}podarcis.yaml').exists() and not (tmp_path / f'{folder}.env').exists()
    assert yaml.safe_load((tmp_path / '.agents/podarcis/tools.yaml').read_text()) == {
        'mcp': ['semantic-scholar'], 'skills': []}
    assert 'agents' not in yaml.safe_load((tmp_path / '.agents/podarcis/config.yaml').read_text())
    assert (tmp_path / '.agents/podarcis/.env').read_text() == 'SEMANTIC_SCHOLAR_API_KEY=old\n'
    assert (tmp_path / '.agents/podarcis/.env').stat().st_mode & 0o777 == 0o600
    args = json.loads((tmp_path / '.mcp.json').read_text())['mcpServers']['semantic-scholar']['args']
    assert args[:3] == ['run', '--env-file', '.agents/podarcis/.env']
    assert 'semantic-scholar' in tomllib.loads((tmp_path / '.codex/config.toml').read_text())['mcp_servers']


def test_invalid_older_configuration_is_left_in_place(tmp_path):
    (tmp_path / 'podarcis.yaml').write_text('mcp: [unknown-server]\n')
    result = run(tmp_path, 'setup')
    assert result.returncode == 1 and 'Not a recommended MCP server' in result.stderr
    assert (tmp_path / 'podarcis.yaml').exists() and not (tmp_path / '.agents').exists()


def test_setup_sets_credentials_and_reports_only_their_names(tmp_path):
    (tmp_path / '.agents/podarcis').mkdir(parents=True)
    (tmp_path / '.agents/podarcis/.env').write_text('# mine\nKEEP=1\nexport SEMANTIC_SCHOLAR_API_KEY=old\nEMPTY=\n')
    # Sidebars before 2.2.0 still send agents; it is accepted and ignored.
    config = {'agents': ['claude'], 'env': {'SEMANTIC_SCHOLAR_API_KEY': 'n3w-key', 'OTHER_TOKEN': 'id.apps'}}
    result = run(tmp_path, 'setup', '--stdin', stdin=json.dumps(config))
    assert result.returncode == 0, result.stderr
    assert 'n3w-key' not in result.stdout
    assert json.loads(result.stdout)['env'] == ['KEEP', 'OTHER_TOKEN', 'SEMANTIC_SCHOLAR_API_KEY']
    assert (tmp_path / '.agents/podarcis/.env').read_text() == (
        '# mine\nKEEP=1\nEMPTY=\nSEMANTIC_SCHOLAR_API_KEY=n3w-key\nOTHER_TOKEN=id.apps\n')
    for name in ('config.yaml', 'tools.yaml'):
        assert not {'env', 'agents'} & set(yaml.safe_load((tmp_path / '.agents/podarcis' / name).read_text()))
    bad = run(tmp_path, 'setup', '--stdin', stdin=json.dumps({'env': {'KEY': 'has space'}}))
    assert bad.returncode == 1 and 'Credential KEY' in bad.stderr
