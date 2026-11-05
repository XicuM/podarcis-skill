# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml>=6.0", "pymupdf>=2.0", "pymupdf4llm>=2.0"]
# ///
"""Install Podarcis in a vault, set the vault up, import papers, and report wiki citation status."""

import argparse
import contextlib
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

import yaml

VERSION = '2.1.0'  # Equal to metadata.version in SKILL.md; a script run from a URL has no SKILL.md.
ARCHIVE = f'https://github.com/XicuM/podarcis-skill/archive/refs/tags/v{VERSION}.tar.gz'
SKILL = Path(__file__).resolve().parents[1]
REPOS = ('sources', 'wiki', 'workspace')
AGENTS = ['claude', 'codex', 'opencode', 'vscode']
CONFIG = Path('.agents', 'podarcis.yaml')
ENV = Path('.agents', '.env')


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
    """Validate .agents/podarcis.yaml content and fill its defaults."""
    if not isinstance(config, dict) or set(config) - {'agents', 'repos', 'mcp', 'skills'}:
        raise ValueError('The configuration takes only agents, repos, mcp, and skills.')
    agents = config.get('agents', AGENTS)
    repos = config.get('repos') or {}
    mcp = config.get('mcp') or []
    skills = config.get('skills') or []
    if not isinstance(agents, list) or not set(agents) <= set(AGENTS) or len(set(agents)) != len(agents):
        raise ValueError(f'agents takes only {", ".join(AGENTS)}.')
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
    for m in mcp:
        if isinstance(m, dict) and not (
            set(m) <= {'name', 'command', 'args', 'url', 'headers'}
            and bool(m.get('command')) != bool(m.get('url'))
            and isinstance(m.get('args', []), list) and isinstance(m.get('headers', {}), dict)
        ):
            raise ValueError(f'MCP server {m["name"]} needs either command and args, or url and headers.')
    if not isinstance(skills, list) or not all(
        isinstance(s, dict) and set(s) <= {'name', 'source', 'ref', 'path'}
        and re.fullmatch(r'[a-z0-9][a-z0-9._-]*', str(s.get('name'))) and s['name'] != 'podarcis'
        and re.fullmatch(r'git:[^-].*|[\w.-]+/[\w.-]+', str(s.get('source')))
        for s in skills
    ):
        raise ValueError('skills entries need a name other than podarcis, a source (owner/repo or git:<url>), '
                         'and optionally ref and path.')
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
    """Return the configuration, the catalogue, each folder's Git state, and which credentials are set."""
    path = root / CONFIG
    if not path.exists():
        path = root / 'podarcis.yaml'  # Before 2.1.0; setup moves it.
    config = yaml.safe_load(path.read_text(encoding='utf-8')) if path.exists() else {}
    env = (root / ENV).read_text(encoding='utf-8') if (root / ENV).exists() else ''
    return {
        'version': VERSION,
        'config': check_config(config or {}),
        'catalogue': load_catalogue(),
        'repos': {name: repo_state(root / name) for name in REPOS},
        # Only the names: agents read this output, and credentials must not reach them.
        'env': sorted(set(re.findall(r'^[ \t]*(?:export[ \t]+)?([A-Za-z_]\w*)[ \t]*=[ \t]*[^\s#]', env, re.MULTILINE))),
    }


def toml(value: object) -> str:
    """Write a TOML value; JSON strings are TOML strings."""
    if isinstance(value, list):
        return '[' + ', '.join(toml(v) for v in value) + ']'
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{json.dumps(str(k))} = {toml(v)}' for k, v in value.items()) + ' }'
    return json.dumps(value)


def install(root: Path) -> Path:
    """Put this skill in the vault: copy it, or download its release when the script runs alone."""
    target = root / '.agents' / 'skills' / 'podarcis'
    staged = target.with_name('.podarcis_staging')
    shutil.rmtree(staged, ignore_errors=True)
    staged.parent.mkdir(parents=True, exist_ok=True)
    if (SKILL / 'SKILL.md').is_file():
        shutil.copytree(SKILL, staged, ignore=shutil.ignore_patterns('__pycache__'))
    else:
        with tempfile.TemporaryDirectory() as tmp:
            with urllib.request.urlopen(ARCHIVE, timeout=60) as response, tarfile.open(fileobj=response, mode='r|gz') as tar:
                tar.extractall(tmp, filter='data')
            found = list(Path(tmp).glob('*/skills/podarcis/SKILL.md'))
            if len(found) != 1:
                raise RuntimeError(f'{ARCHIVE} does not contain skills/podarcis.')
            shutil.copytree(found[0].parent, staged)
    shutil.rmtree(target, ignore_errors=True)
    staged.rename(target)
    return target


def fetch_skill(root: Path, skill: dict) -> str:
    """Install a skill from a Git repository at its ref, unless that ref is already installed."""
    target = root / '.agents' / 'skills' / skill['name']
    source, ref = skill['source'], skill.get('ref', 'HEAD')
    pin = f'{source}@{ref}:{skill.get("path", "")}\n'
    marker = target / '.podarcis-source'
    if marker.is_file() and marker.read_text(encoding='utf-8') == pin:
        return 'unchanged'
    url = source[4:] if source.startswith('git:') else f'https://github.com/{source}.git'
    with tempfile.TemporaryDirectory() as tmp:
        for args in (('init', '-q', tmp), ('-C', tmp, 'fetch', '-q', '--depth', '1', '--', url, ref),
                     ('-C', tmp, 'checkout', '-q', 'FETCH_HEAD')):
            result = git(*args)
            if result.returncode:
                raise RuntimeError(f'Fetching skill {skill["name"]} failed: {result.stderr.strip()}')
        folder = Path(tmp, skill.get('path', '')).resolve()
        if not folder.is_relative_to(Path(tmp).resolve()) or not (folder / 'SKILL.md').is_file():
            raise RuntimeError(f'Skill {skill["name"]} has no SKILL.md at {skill.get("path", "the repository root")}.')
        shutil.rmtree(target, ignore_errors=True)
        shutil.copytree(folder, target, ignore=shutil.ignore_patterns('.git'))
    marker.write_text(pin, encoding='utf-8')
    return 'installed'


def write_agents(root: Path, agents: list[str], servers: list[dict], skills: list[str]) -> None:
    """Write each selected agent's MCP servers, keeping its other settings, and link skills for Claude."""
    def stdio(m: dict) -> bool:
        return 'command' in m

    formats = {
        'claude': ('.mcp.json', 'mcpServers', lambda m: {'command': m['command'], 'args': m.get('args', [])}
                   if stdio(m) else {'type': 'http', 'url': m['url'], 'headers': m.get('headers', {})}),
        'vscode': ('.vscode/mcp.json', 'servers', lambda m: {'type': 'stdio', 'command': m['command'],
                   'args': m.get('args', [])} if stdio(m) else {'type': 'http', 'url': m['url'],
                   'headers': m.get('headers', {})}),
        'opencode': ('.opencode/opencode.jsonc', 'mcp', lambda m: {'type': 'local', 'command': [m['command'],
                     *m.get('args', [])]} if stdio(m) else {'type': 'remote', 'url': m['url'],
                     'headers': m.get('headers', {})}),
    }
    for agent in agents:
        if agent == 'codex':
            # Replace only the [mcp_servers.*] tables; the rest of config.toml is the user's.
            path = root / '.codex' / 'config.toml'
            kept, ours = [], False
            for line in path.read_text(encoding='utf-8').splitlines() if path.exists() else []:
                header = re.match(r'\s*\[\[?\s*([^\]]+?)\s*\]', line)
                if header:
                    ours = re.match(r'"?mcp_servers"?(\.|$)', header.group(1)) is not None
                if not ours:
                    kept.append(line)
            text = '\n'.join(kept).strip()
            for m in servers:
                text += f'\n\n[mcp_servers.{json.dumps(m["name"])}]\n'
                text += (f'command = {toml(m["command"])}\nargs = {toml(m.get("args", []))}\n' if stdio(m)
                         else f'url = {toml(m["url"])}\nhttp_headers = {toml(m.get("headers", {}))}\n')
            path.parent.mkdir(exist_ok=True)
            path.write_text(text.strip() + '\n', encoding='utf-8')
            continue
        name, key, entry = formats[agent]
        path = root / name
        try:
            data = json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}
        except json.JSONDecodeError as error:
            raise ValueError(f'{name} is not plain JSON ({error}); fix or remove it, then run setup again.')
        data[key] = {m['name']: entry(m) for m in servers}
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    if 'claude' in agents:
        # Claude Code reads skills only from .claude/skills; the others read .agents/skills.
        (root / '.claude' / 'skills').mkdir(parents=True, exist_ok=True)
        for skill in skills:
            link = root / '.claude' / 'skills' / skill
            if not link.exists() and not link.is_symlink():
                link.symlink_to(Path('..', '..', '.agents', 'skills', skill))


def setup(root: Path, config: dict | None = None, env: dict | None = None) -> dict:
    """Clone or link the vault folders, install the skills, configure each agent's MCP servers, and store credentials."""
    if config is not None:
        config = check_config(config)
    for key, value in (env or {}).items():
        if not re.fullmatch(r'[A-Za-z_]\w*', key) or not re.fullmatch(r'[^\s\'"#\\]+', str(value)):
            raise ValueError(f'Credential {key} needs a name of letters, digits, and _, '
                             'and a value without spaces, quotes, #, or backslashes.')
    (root / '.agents').mkdir(exist_ok=True)
    for old, new in (('podarcis.yaml', CONFIG), ('.env', ENV)):
        if (root / old).exists() and not (root / new).exists():
            (root / old).rename(root / new)  # Vaults before 2.1.0 kept both in the root.
    if config is not None or not (root / CONFIG).exists():
        (root / CONFIG).write_text(
            yaml.safe_dump(config or check_config({}), sort_keys=False, allow_unicode=True), encoding='utf-8'
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
    if (root / '.agents' / 'skills' / 'podarcis').resolve() != SKILL:
        install(root)
        actions['podarcis'] = f'installed {VERSION}'
    servers, skills = [], list(config['skills'])
    for item in config['mcp']:
        if isinstance(item, str):
            servers.append({'name': item, **catalogue[item]['mcp']})
            skills += catalogue[item].get('skills', [])
        else:
            servers.append(item)
    for skill in skills:
        actions[skill['name']] = fetch_skill(root, skill)
    write_agents(root, config['agents'], servers, ['podarcis', *(s['name'] for s in skills)])
    (root / ENV).touch(mode=0o600)
    (root / ENV).chmod(0o600)  # Also a moved or hand-made file.
    if env:
        lines = [line for line in (root / ENV).read_text(encoding='utf-8').splitlines()
                 if not re.match(rf'[ \t]*(export[ \t]+)?({"|".join(env)})[ \t]*=', line)]
        (root / ENV).write_text('\n'.join([*lines, *(f'{k}={v}' for k, v in env.items())]) + '\n', encoding='utf-8')
    return {**config_state(root), 'actions': actions}


def main() -> None:
    """Run a vault command, returning JSON on stdout and errors on stderr."""
    parser = argparse.ArgumentParser(prog='podarcis')
    parser.add_argument('--root', default='.', help='Vault path; defaults to cwd')
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('config', help='Show the configuration, recommended servers, and repo state')
    configure = commands.add_parser('setup', help='Apply .agents/podarcis.yaml to the vault')
    configure.add_argument('--stdin', action='store_true', help='Replace the configuration with JSON from stdin first; its env key sets credentials')
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
    global SKILL
    try:
        if args.command in ('config', 'setup') and not (SKILL / 'SKILL.md').is_file():
            # Run from a URL: install this release in the vault and use its catalogue.
            SKILL = install(root)
        if args.command == 'config':
            result = config_state(root)
        elif args.command == 'setup':
            config = json.load(sys.stdin) if args.stdin else None
            env = config.pop('env', None) if isinstance(config, dict) else None
            result = setup(root, config, env)
        elif args.command == 'ingest':
            result = ingest(root, args.metadata, args.pdf_path, args.source_id, args.domain)
        else:
            result = status(root, args.status)
    except Exception as error:
        parser.exit(1, f'{type(error).__name__}: {error}\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
