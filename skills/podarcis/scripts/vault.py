# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml>=6.0", "pymupdf>=2.0", "pymupdf4llm>=2.0"]
# ///
"""Set up a vault, import papers into it, and report wiki citation status."""

import argparse
import contextlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import yaml

SKILL = Path(__file__).resolve().parents[1]
REPOS = ('sources', 'wiki', 'workspace')
AGENTS = ['claude', 'codex', 'opencode', 'vscode']
DOTAGENTS = ['npx', '--yes', '@sentry/dotagents@3.4.0', '--project', 'install']


def ingest(
    root: Path, metadata_path: Path, pdf_path: Path, source_id: str, domain: str
) -> dict[str, str]:
    """Store OpenAlex metadata and extracted PDF text as one source.md."""
    if not re.fullmatch(r'[a-z][a-z0-9_]*', source_id) or not re.fullmatch(
        r'[a-z][a-z0-9_]*(/[a-z][a-z0-9_]*)*', domain
    ):
        raise ValueError('source_id and domain folders must be snake_case.')
    sources = root / 'sources'
    target = (sources / domain / source_id).resolve()
    if not target.is_relative_to(sources):
        raise ValueError('Source path leaves sources/.')
    if any(p.is_dir() for p in sources.rglob(source_id)):
        raise FileExistsError(f'Source ID already exists: {source_id}.')
    work = json.loads(metadata_path.read_text(encoding='utf-8'))
    if not re.fullmatch(r'https://openalex.org/W[0-9]+', work['id']):
        raise ValueError('Metadata must be a complete OpenAlex work record.')
    title = ' '.join(work['display_name'].splitlines()).strip()
    if not title:
        raise ValueError('The OpenAlex work has no title.')
    metadata = {
        'type': 'source',
        'id': source_id,
        'title': title,
        'authors': [a['author']['display_name'] for a in work['authorships']],
        'year': work['publication_year'],
        'doi': work['doi'],
        'openalex_id': work['id'].rsplit('/', 1)[-1],
        'publication_type': work['type'],
    }
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.ingest_', dir=target.parent) as scratch:
        stage = Path(scratch) / source_id
        stage.mkdir()
        pdf = stage / 'original.pdf'
        shutil.copyfile(pdf_path.expanduser(), pdf)
        with pdf.open('rb') as stream:
            if stream.read(5) != b'%PDF-':
                raise ValueError('pdf_path does not contain a PDF.')
        # Converter diagnostics must not corrupt the command's JSON output.
        with contextlib.redirect_stdout(sys.stderr):
            import pymupdf4llm

            text = pymupdf4llm.to_markdown(str(pdf), use_ocr=False)
        text = re.sub(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', '', text)
        text = text.encode('utf-8', errors='replace').decode('utf-8').strip()
        if not text:
            raise ValueError('PDF extraction returned no text; no source was stored.')
        links = '[Original PDF](./original.pdf)'
        links += f' · [OpenAlex]({work["id"]})'
        if work['doi']:
            links += f' · [Publisher]({work["doi"]})'
        (stage / 'source.md').write_text(
            '---\n'
            + yaml.safe_dump(metadata, sort_keys=False, allow_unicode=True)
            + f'---\n\n# {title}\n\n{links}\n\n## Full text\n\n{text}\n',
            encoding='utf-8',
        )
        if target.exists() or any(
            p.is_dir() and p != stage for p in sources.rglob(source_id)
        ):
            raise FileExistsError(f'Source ID already exists: {source_id}.')
        stage.rename(target)
    return {'id': source_id, 'path': (target / 'source.md').relative_to(root).as_posix()}


def status(root: Path, state: str = 'all') -> list[dict[str, str]]:
    """List complete sources; cited means a wiki footnote, not validation."""
    if state not in ('uncited', 'cited', 'all'):
        raise ValueError('Unknown citation status.')
    cited = set()
    for path in (root / 'wiki').rglob('*.md'):
        if path.resolve().is_relative_to(root / 'wiki'):
            text = path.read_text(encoding='utf-8')
            text = re.sub(
                r'^(`{3,}|~{3,}).*?^\1[^\n]*$', '', text, flags=re.MULTILINE | re.DOTALL
            )
            cited.update(re.findall(r'^\[\^([a-z][a-z0-9_]*)\]:', text, re.MULTILINE))
    items = []
    for entry in sorted((root / 'sources').rglob('*.md')):
        if entry.name not in ('source.md', 'metadata.md'):
            continue
        paper = entry.parent
        if not entry.resolve().is_relative_to(root / 'sources'):
            continue
        # Legacy sources remain readable without rewriting research data.
        if entry.name == 'metadata.md' and (paper / 'source.md').exists():
            continue
        required = ('original.pdf', 'source.md')
        if entry.name == 'metadata.md':
            required = ('original.pdf', 'metadata.md', 'raw.md')
        if not all(
            (paper / name).resolve().is_relative_to(root / 'sources')
            and (paper / name).is_file()
            and (paper / name).stat().st_size
            for name in required
        ):
            continue
        citation = 'cited' if paper.name in cited else 'uncited'
        if state in ('all', citation):
            items.append({
                'id': paper.name,
                'domain': paper.parent.relative_to(root / 'sources').as_posix(),
                'path': entry.relative_to(root).as_posix(),
                'status': citation,
            })
    return items


def check_config(config: object) -> dict:
    """Validate podarcis.yaml content and fill its defaults."""
    if not isinstance(config, dict) or set(config) - {'agents', 'repos', 'mcp', 'skills'}:
        raise ValueError('podarcis.yaml takes only agents, repos, mcp, and skills.')
    agents = config.get('agents', AGENTS)
    repos = config.get('repos') or {}
    mcp = config.get('mcp') or []
    skills = config.get('skills') or []
    if not isinstance(agents, list) or not all(isinstance(a, str) for a in agents):
        raise ValueError('agents must be a list of dotagents target names.')
    if not isinstance(repos, dict) or set(repos) - set(REPOS):
        raise ValueError(f'repos takes only {", ".join(REPOS)}.')
    repos = {name: repos.get(name) or None for name in REPOS}
    if any(not isinstance(u, (str, type(None))) or (u or '').startswith('-') for u in repos.values()):
        raise ValueError('Each repo is a Git URL or empty for a local folder.')
    catalogue = load_catalogue()
    if not isinstance(mcp, list):
        raise ValueError('mcp must be a list.')
    names = [m if isinstance(m, str) else m.get('name') if isinstance(m, dict) else None for m in mcp]
    if None in names or len(set(names)) != len(names):
        raise ValueError('mcp entries need unique names.')
    unknown = [m for m in mcp if isinstance(m, str) and m not in catalogue]
    if unknown:
        raise ValueError(f'Not a recommended MCP server: {", ".join(unknown)}.')
    if not isinstance(skills, list) or not all(
        isinstance(s, dict) and s.get('name') not in (None, 'podarcis') and s.get('source')
        for s in skills
    ):
        raise ValueError('skills entries need a name other than podarcis and a source.')
    return {'agents': agents, 'repos': repos, 'mcp': mcp, 'skills': skills}


def load_catalogue() -> dict:
    """Read the recommended MCP servers shipped with this skill."""
    path = SKILL / 'references' / 'mcp-servers.yaml'
    return yaml.safe_load(path.read_text(encoding='utf-8'))


def git(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(['git', *args], capture_output=True, text=True)


def repo_state(path: Path) -> dict:
    """Report whether a vault folder is missing, a local folder, or its own clone."""
    if not path.is_dir():
        return {'state': 'missing', 'url': None}
    if not (path / '.git').exists():
        return {'state': 'local', 'url': None, 'empty': not any(path.iterdir())}
    url = git('-C', str(path), 'config', '--get', 'remote.origin.url').stdout.strip()
    return {'state': 'repo', 'url': url or None}


def config_state(root: Path) -> dict:
    """Return the vault's configuration, the catalogue, and each folder's Git state."""
    path = root / 'podarcis.yaml'
    config = yaml.safe_load(path.read_text(encoding='utf-8')) if path.exists() else {}
    return {
        'config': check_config(config or {}),
        'catalogue': load_catalogue(),
        'repos': {name: repo_state(root / name) for name in REPOS},
    }


def toml_key(key: object) -> str:
    return key if re.fullmatch(r'[A-Za-z0-9_-]+', str(key)) else json.dumps(str(key))


def toml(value: object) -> str:
    """Write the TOML values that dotagents entries use; JSON strings are TOML strings."""
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, (str, int)):
        return json.dumps(value)
    if isinstance(value, list):
        return '[' + ', '.join(toml(v) for v in value) + ']'
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{toml_key(k)} = {toml(v)}' for k, v in value.items()) + ' }'
    raise ValueError(f'Unsupported agents.toml value: {value!r}.')


def setup(root: Path, config: dict | None = None) -> dict:
    """Clone or link the vault folders, write agents.toml, and install with dotagents."""
    if config is not None or not (root / 'podarcis.yaml').exists():
        config = check_config(config or {})
        (root / 'podarcis.yaml').write_text(
            yaml.safe_dump(config, sort_keys=False, allow_unicode=True), encoding='utf-8'
        )
    state = config_state(root)
    config, catalogue = state['config'], state['catalogue']
    actions = {}
    for name, url in config['repos'].items():
        path, current = root / name, state['repos'][name]
        if current['state'] == 'repo' and url and current['url'] != url:
            verb = 'set-url' if current['url'] else 'add'
            result = git('-C', str(path), 'remote', verb, 'origin', url)
            actions[name] = 'remote updated'
        elif url and (current['state'] == 'missing' or current.get('empty')):
            result = git('clone', '--', url, str(path))
            actions[name] = 'cloned'
        elif url and current['state'] == 'local':
            raise ValueError(f'{name}/ has files but no Git repository; clone it elsewhere or remove its URL.')
        else:
            path.mkdir(exist_ok=True)
            result, actions[name] = None, 'unchanged'
        if result and result.returncode:
            raise RuntimeError(f'git failed for {name}/: {result.stderr.strip()}')
    mcp, skills = [], list(config['skills'])
    for item in config['mcp']:
        if isinstance(item, str):
            mcp.append({'name': item, **catalogue[item]['mcp']})
            skills += catalogue[item].get('skills', [])
        else:
            mcp.append(item)
    tables = [('skills', s) for s in skills] + [('mcp', m) for m in mcp]
    (root / 'agents.toml').write_text(
        '# Generated by Podarcis setup from podarcis.yaml; edit that file instead.\n'
        f'version = 1\nagents = {toml(config["agents"])}\n'
        + ''.join(
            f'\n[[{kind}]]\n' + ''.join(f'{toml_key(k)} = {toml(v)}\n' for k, v in entry.items())
            for kind, entry in tables
        ),
        encoding='utf-8',
    )
    (root / '.env').touch(mode=0o600)
    installed = root / '.agents' / 'skills' / 'podarcis'
    if installed.resolve() != SKILL:
        # dotagents only installs local skills from inside the vault, so copy this one in.
        staged = installed.with_name('.podarcis_staging')
        shutil.rmtree(staged, ignore_errors=True)
        shutil.copytree(SKILL, staged, ignore=shutil.ignore_patterns('__pycache__'))
        shutil.rmtree(installed, ignore_errors=True)
        staged.rename(installed)
    result = subprocess.run(DOTAGENTS, cwd=root, capture_output=True, text=True)
    sys.stderr.write(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f'dotagents install failed: {result.stderr.strip()}')
    return {**config_state(root), 'actions': actions}


def main() -> None:
    """Run a vault command, returning JSON on stdout and errors on stderr."""
    parser = argparse.ArgumentParser(prog='podarcis')
    parser.add_argument('--root', default='.', help='Vault path; defaults to cwd')
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('config', help='Show podarcis.yaml, recommended servers, and repo state')
    configure = commands.add_parser('setup', help='Apply podarcis.yaml to the vault')
    configure.add_argument('--stdin', action='store_true', help='Replace podarcis.yaml with JSON from stdin first')
    add = commands.add_parser('ingest', help='Import OpenAlex JSON and a local PDF')
    add.add_argument('metadata', type=Path, help='Complete OpenAlex get works JSON')
    add.add_argument('source_id')
    add.add_argument('domain', help='Relative snake_case folders under sources/')
    add.add_argument('--pdf-path', required=True, type=Path)
    listing = commands.add_parser('status', help='List cited or uncited sources')
    listing.add_argument('--status', choices=('all', 'cited', 'uncited'), default='all')
    args = parser.parse_args()
    root = Path(args.root).expanduser().resolve()
    if not root.is_dir():
        parser.error(f'No vault directory at {root}.')
    if args.command in ('ingest', 'status') and not (root / 'sources').is_dir():
        parser.error(f'No sources/ directory in {root}; run setup first.')
    try:
        if args.command == 'config':
            result = config_state(root)
        elif args.command == 'setup':
            result = setup(root, json.load(sys.stdin) if args.stdin else None)
        elif args.command == 'ingest':
            result = ingest(root, args.metadata, args.pdf_path, args.source_id, args.domain)
        else:
            result = status(root, args.status)
    except Exception as error:
        parser.exit(1, f'{type(error).__name__}: {error}\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
