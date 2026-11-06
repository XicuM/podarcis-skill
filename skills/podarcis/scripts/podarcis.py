# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "pyyaml>=6.0", "pymupdf>=2.0", "pymupdf4llm>=2.0",
#   "python-docx>=1.1", "python-pptx>=1.0", "openpyxl>=3.1",
# ]
# ///
"""Install Podarcis in a vault, set the vault up, import papers and other files, and report wiki citation status."""

import argparse
import contextlib
import csv
import datetime
import hashlib
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

VERSION = '2.3.0'  # Equal to metadata.version in SKILL.md; a script run from a URL has no SKILL.md.
ARCHIVE = f'https://github.com/XicuM/podarcis-skill/archive/refs/tags/v{VERSION}.tar.gz'
SKILL = Path(__file__).resolve().parents[1]
REPOS = ('sources', 'wiki', 'workspace')
AGENTS = ['claude', 'codex', 'opencode', 'vscode']  # Setup configures every one of them.
SETTINGS = Path('.agents', 'podarcis')
CONFIG = SETTINGS / 'config.yaml'  # Where each vault folder syncs to.
TOOLS = SETTINGS / 'tools.yaml'  # MCP servers and extra skills the agents get.
LIB = SETTINGS / 'lib'  # This skill's scripts and references, installed in the vault.
INSTRUCTIONS = SETTINGS / 'AGENTS.md'  # Generated from SKILL.md; the vault's AGENTS.md links to it.
LOCAL_INSTRUCTIONS = SETTINGS / 'AGENTS.local.md'  # The vault's own instructions, appended to INSTRUCTIONS.
LEGACY_SKILL = Path('.agents', 'skills', 'podarcis')  # Where 2.2 and older installed this skill.
ENV = SETTINGS / '.env'  # Credentials the tools read.
# Older layouts, newest first; setup moves them into SETTINGS.
LEGACY_CONFIG = (Path('.agents', 'podarcis.yaml'), Path('podarcis.yaml'))
LEGACY_ENV = (Path('.agents', '.env'), Path('.env'))
RETIRED = {'google-drive'}  # Recommended servers since removed; setup drops them.
HEADERS = {
    CONFIG: '# Podarcis vault settings. Each folder takes a Git URL, or null to stay a local folder.\n'
            '# Edit, then run: uv run .agents/podarcis/lib/scripts/podarcis.py setup\n',
    TOOLS: '# Tools for every agent in this vault (Claude Code, Codex, OpenCode, VS Code).\n'
           '# mcp: names from .agents/podarcis/lib/references/mcp-servers.yaml, or custom\n'
           '#   entries with name plus command and args, or url and headers.\n'
           '# skills: extra skills, each with name, source (owner/repo or git:<url>), and optional ref and path.\n'
           '# Credentials go in .agents/podarcis/.env. Edit, then run setup.\n',
}


def ingest(
    root: Path, metadata_path: Path, pdf_path: Path, source_id: str, domain: str
) -> dict[str, str]:
    """Store OpenAlex metadata and extracted PDF text as one source.md."""
    if not re.fullmatch(r'[a-z][a-z0-9_]*', source_id) or not re.fullmatch(
        r'([a-z][a-z0-9_]*(/[a-z][a-z0-9_]*)*)?', domain
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
        'kind': 'paper',
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


SNAKE = r'[a-z][a-z0-9_]*'
KIND = r'[a-z][a-z0-9-]*'
DATE = r'\d{4}-\d{2}-\d{2}'
# Default kinds by extension; any kebab-case kind is accepted with --kind.
KINDS = {
    '.pdf': 'document', '.docx': 'document', '.md': 'document', '.txt': 'document',
    '.pptx': 'slides', '.xlsx': 'dataset', '.csv': 'dataset', '.tsv': 'dataset',
    '.png': 'media', '.jpg': 'media', '.jpeg': 'media', '.gif': 'media', '.webp': 'media',
    '.mp3': 'media', '.m4a': 'media', '.wav': 'media', '.mp4': 'media', '.webm': 'media',
}
TABLE_ROWS = 200  # Spreadsheet rows rendered per sheet; the original keeps the rest.


def clean(text: str) -> str:
    text = re.sub(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', '', text)
    return text.encode('utf-8', errors='replace').decode('utf-8').strip()


def table(rows: list[list[object]]) -> str:
    """Render rows as a Markdown table whose first row is the header."""
    cell = lambda v: '' if v is None else ' '.join(str(v).split()).replace('|', '\\|')
    rows = [[cell(v) for v in row] for row in rows]
    rows = [row for row in rows if any(row)]
    if not rows:
        return ''
    width = max(len(row) for row in rows)
    rows = [row + [''] * (width - len(row)) for row in rows]
    lines = ['| ' + ' | '.join(rows[0]) + ' |', '|' + ' --- |' * width]
    return '\n'.join(lines + ['| ' + ' | '.join(row) + ' |' for row in rows[1:]])


def rows_section(rows: list[list[object]], total: int) -> str:
    text = table(rows[: TABLE_ROWS + 1])
    if total > TABLE_ROWS + 1:
        text += f'\n\n_First {TABLE_ROWS} of {total - 1} data rows; see the original for the rest._'
    return text


def extract_pdf(path: Path) -> str:
    # Converter diagnostics must not corrupt the command's JSON output.
    with contextlib.redirect_stdout(sys.stderr):
        import pymupdf4llm

        pages = pymupdf4llm.to_markdown(str(path), use_ocr=False, page_chunks=True)
    return '\n\n'.join(
        f'## Page {page["metadata"]["page_number"]}\n\n{clean(page["text"])}'
        for page in pages if clean(page['text'])
    )


def extract_docx(path: Path) -> str:
    import docx
    from docx.table import Table
    from docx.text.paragraph import Paragraph

    document = docx.Document(str(path))
    parts = []
    for child in document.element.body.iterchildren():
        if child.tag.endswith('}tbl'):
            parts.append(table([[c.text for c in r.cells] for r in Table(child, document).rows]))
            continue
        if not child.tag.endswith('}p'):
            continue
        paragraph = Paragraph(child, document)
        text = paragraph.text.strip()
        style = paragraph.style.name if paragraph.style is not None else ''
        level = re.fullmatch(r'Heading (\d)', style)
        if not text:
            continue
        if style == 'Title' or level:
            # Document headings sit below the source title.
            parts.append('#' * min(int(level[1]) + 1 if level else 2, 6) + ' ' + text)
        elif style.startswith('List'):
            parts.append(f'- {text}')
        else:
            parts.append(text)
    return '\n\n'.join(p for p in parts if p)


def extract_pptx(path: Path) -> str:
    import pptx

    slides = []
    for number, slide in enumerate(pptx.Presentation(str(path)).slides, 1):
        title = slide.shapes.title.text.strip() if slide.shapes.title is not None else ''
        parts = [f'## Slide {number}'] + ([f'**{title}**'] if title else [])
        for shape in slide.shapes:
            if shape == slide.shapes.title:
                continue
            if shape.has_text_frame and shape.text_frame.text.strip():
                parts.append(shape.text_frame.text.strip())
            elif getattr(shape, 'has_table', False) and shape.has_table:
                parts.append(table([[c.text for c in r.cells] for r in shape.table.rows]))
        if slide.has_notes_slide and slide.notes_slide.notes_text_frame.text.strip():
            parts.append('Speaker notes:\n\n' + slide.notes_slide.notes_text_frame.text.strip())
        slides.append('\n\n'.join(parts))
    return '\n\n'.join(slides)


def extract_xlsx(path: Path) -> str:
    import openpyxl

    book = openpyxl.load_workbook(str(path), read_only=True, data_only=True)
    sheets = []
    for sheet in book.worksheets:
        rows = [list(r) for r in sheet.iter_rows(values_only=True)]
        sheets.append(f'## Sheet {sheet.title}\n\n' + rows_section(rows, len(rows)))
    book.close()
    return '\n\n'.join(sheets)


def extract_csv(path: Path) -> str:
    with path.open(encoding='utf-8', errors='replace', newline='') as stream:
        rows = list(csv.reader(stream, delimiter='\t' if path.suffix.lower() == '.tsv' else ','))
    return rows_section(rows, len(rows))


def extract_text(path: Path) -> str:
    return path.read_text(encoding='utf-8', errors='replace')


EXTRACTORS = {
    '.pdf': extract_pdf, '.docx': extract_docx, '.pptx': extract_pptx, '.xlsx': extract_xlsx,
    '.csv': extract_csv, '.tsv': extract_csv, '.md': extract_text, '.txt': extract_text,
}


def front_matter(path: Path) -> dict:
    """Return a Markdown file's YAML front matter, or {} when it has none or it is invalid."""
    text = path.read_text(encoding='utf-8', errors='replace')
    match = re.match(r'---\n(.*?)\n---\n', text, re.DOTALL)
    try:
        data = yaml.safe_load(match[1]) if match else None
    except yaml.YAMLError:
        return {}
    return data if isinstance(data, dict) else {}


def find_source(sources: Path, source_id: str) -> Path | None:
    for path in sources.rglob(source_id):
        if path.is_dir() and path.parent.name != 'revisions' and not path.parent.name.startswith('.ingest_'):
            return path
    return None


def same_origin(a: object, b: object) -> bool:
    keys = ('provider', 'id', 'url')
    return (
        isinstance(a, dict) and isinstance(b, dict) and a.get('provider') == b.get('provider')
        and any(a.get(k) and a.get(k) == b.get(k) for k in keys[1:])
    )


def add(
    root: Path, file_path: Path, source_id: str, domain: str, kind: str | None = None,
    title: str | None = None, date: str | None = None, authors: list[str] | None = None,
    origin: dict | None = None, text_from: Path | None = None,
    attachments: list[Path] | None = None,
) -> dict[str, object]:
    """Store any file as a typed, read-only source snapshot with extracted, citable text.

    A file from an external provider (origin with provider plus id or url) is versioned:
    adding changed content for the same origin keeps the previous snapshot under
    revisions/<revision>/ so existing citations still resolve to what was read.
    text_from extracts the text from another rendering of the same content (such as the .pptx
    export of a deck stored as PDF) without storing it; attachments are stored unchanged beside
    the original (such as one CSV per spreadsheet tab).
    """
    if not re.fullmatch(SNAKE, source_id) or not re.fullmatch(rf'({SNAKE}(/{SNAKE})*)?', domain):
        raise ValueError('source_id and domain folders must be snake_case.')
    if 'revisions' in domain.split('/'):
        raise ValueError('"revisions" is reserved for source snapshots.')
    file_path = file_path.expanduser()
    if not file_path.is_file():
        raise FileNotFoundError(f'No file at {file_path}.')
    suffix = file_path.suffix.lower()
    if not re.fullmatch(r'\.[a-z0-9]{1,8}', suffix or '.x'):
        raise ValueError('The file extension must be 1-8 letters or digits.')
    kind = kind or KINDS.get(suffix, 'other')
    if not re.fullmatch(KIND, kind):
        raise ValueError('kind must be lowercase kebab-case, such as meeting-notes.')
    if date is not None and not re.fullmatch(DATE, date):
        raise ValueError('date must be YYYY-MM-DD.')
    if origin is not None:
        if not isinstance(origin, dict) or not re.fullmatch(KIND, str(origin.get('provider', ''))):
            raise ValueError('origin needs a kebab-case provider.')
        if not (origin.get('id') or origin.get('url')):
            raise ValueError('origin needs an id or url identifying the external file.')
    attachments = [a.expanduser() for a in attachments or []]
    names = {a.name for a in attachments}
    for a in attachments:
        if not a.is_file():
            raise FileNotFoundError(f'No attachment at {a}.')
        if not re.fullmatch(r'[a-z0-9][a-z0-9_.-]{0,99}', a.name) or a.name.startswith('original.') \
                or a.name == 'source.md' or a.name.endswith('.md'):
            raise ValueError(f'Attachment names must be lowercase, not .md, and not original.*: {a.name}.')
    if len(names) != len(attachments):
        raise ValueError('Attachment names must be unique.')
    if text_from is not None and not text_from.expanduser().is_file():
        raise FileNotFoundError(f'No file at {text_from}.')
    digest = hashlib.sha256(file_path.read_bytes()).hexdigest()
    revision = str((origin or {}).get('revision') or digest[:12])
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,64}', revision):
        raise ValueError('origin revision must be 1-64 letters, digits, _ or -.')
    title = ' '.join((title or file_path.stem.replace('_', ' ')).split())
    sources = root / 'sources'
    existing = find_source(sources, source_id)
    if existing is not None:
        current = front_matter(existing / 'source.md')
        if origin is None or not same_origin(current.get('origin'), origin):
            raise FileExistsError(f'Source ID already exists: {source_id}.')
        target = existing
    else:
        target = (sources / domain / source_id).resolve()
    if not target.is_relative_to(sources.resolve()):
        raise ValueError('Source path leaves sources/.')

    original = f'original{suffix}'
    source = text_from.expanduser() if text_from is not None else file_path
    extractor = EXTRACTORS.get(source.suffix.lower())
    text = clean(extractor(source)) if extractor else ''
    # Exports such as PDFs differ byte for byte on every download; when the citable text and
    # attachments are the same, the content has not changed and no revision is recorded.
    content = hashlib.sha256(text.encode('utf-8'))
    for a in sorted(attachments, key=lambda a: a.name):
        content.update(a.name.encode('utf-8') + b'\0' + hashlib.sha256(a.read_bytes()).digest())
    text_digest = content.hexdigest()
    # The citable text decides when there is any (text_from may change while the file does not);
    # files without text, and sources stored before text_sha256 existed, compare file bytes.
    if existing is not None and (
        current.get('text_sha256') == text_digest if text and current.get('text_sha256')
        else current.get('sha256') == digest
    ):
        return {'id': source_id, 'path': (existing / 'source.md').relative_to(root).as_posix(),
                'revision': current.get('revision'), 'changed': False}
    metadata = {'type': 'source', 'id': source_id, 'kind': kind, 'title': title}
    if date:
        metadata['date'] = date
    if authors:
        metadata['authors'] = authors
    metadata |= {'revision': revision, 'sha256': digest, 'text_sha256': text_digest, 'original': original}
    if origin:
        metadata['origin'] = origin | {'retrieved': datetime.date.today().isoformat()}
    if attachments:
        metadata['attachments'] = sorted(names)
    links = f'[Original](./{original})'
    links += ''.join(f' · [{n}](./{n})' for n in sorted(names))
    if origin and origin.get('url'):
        links += f' · [Live file]({origin["url"]})'
    body = text or (
        '_No text could be extracted from this file; consult the original. '
        'Cite it only for what the original shows._'
    )

    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.ingest_', dir=target.parent) as scratch:
        stage = Path(scratch) / source_id
        stage.mkdir()
        shutil.copyfile(file_path, stage / original)
        for a in attachments:
            shutil.copyfile(a, stage / a.name)
        (stage / 'source.md').write_text(
            '---\n' + yaml.safe_dump(metadata, sort_keys=False, allow_unicode=True)
            + f'---\n\n# {title}\n\n{links}\n\n{body}\n',
            encoding='utf-8',
        )
        if existing is None:
            if find_source(sources, source_id) is not None:
                raise FileExistsError(f'Source ID already exists: {source_id}.')
            stage.rename(target)
        else:
            # Snapshots are immutable: the previous one moves aside, never changes.
            old = current.get('revision') or str(current.get('sha256', 'previous'))[:12]
            archive = target / 'revisions' / str(old)
            if archive.exists():
                raise FileExistsError(f'Revision {old} of {source_id} is already archived.')
            archive.mkdir(parents=True)
            moved = [p for p in target.iterdir() if p.is_file()]
            for p in moved:
                p.rename(archive / p.name)
            for p in stage.iterdir():
                p.rename(target / p.name)
    return {'id': source_id, 'path': (target / 'source.md').relative_to(root).as_posix(),
            'revision': revision, 'changed': True}


def citations(root: Path) -> tuple[set[str], dict[str, dict[str, set[str]]]]:
    """Return cited source IDs and, per ID, the pages that cite each recorded revision."""
    cited = set()
    revisions: dict[str, dict[str, set[str]]] = {}
    wiki = root / 'wiki'
    for path in wiki.rglob('*.md'):
        if not path.resolve().is_relative_to(wiki.resolve()):
            continue
        text = path.read_text(encoding='utf-8')
        page = path.relative_to(root).as_posix()
        entries = front_matter(path).get('sources')
        for entry in entries if isinstance(entries, list) else []:
            if not isinstance(entry, dict) or not isinstance(entry.get('id'), str):
                continue
            rev = entry.get('revision')
            found = re.search(r'/revisions/([A-Za-z0-9_-]+)/', str(entry.get('resource', '')))
            rev = str(rev) if rev is not None else (found[1] if found else None)
            if rev:
                revisions.setdefault(entry['id'], {}).setdefault(rev, set()).add(page)
        text = re.sub(r'^(`{3,}|~{3,}).*?^\1[^\n]*$', '', text, flags=re.MULTILINE | re.DOTALL)
        cited.update(re.findall(rf'^\[\^({SNAKE})\]:', text, re.MULTILINE))
    return cited, revisions


def status(root: Path, state: str = 'all') -> list[dict[str, object]]:
    """List complete sources; cited means a wiki footnote, not validation.

    A cited source is stale when a wiki page records a revision other than the current one;
    the cited snapshot stays readable under revisions/. The cited filter includes stale sources.
    """
    if state not in ('uncited', 'cited', 'stale', 'all'):
        raise ValueError('Unknown citation status.')
    cited, revisions = citations(root)
    sources = root / 'sources'
    items = []
    for entry in sorted(sources.rglob('*.md')):
        if entry.name not in ('source.md', 'metadata.md'):
            continue
        paper = entry.parent
        if not entry.resolve().is_relative_to(sources) or paper.parent.name == 'revisions':
            continue
        if any(part.startswith('.ingest_') for part in paper.relative_to(sources).parts):
            continue
        # Legacy sources remain readable without rewriting research data.
        if entry.name == 'metadata.md' and (paper / 'source.md').exists():
            continue
        meta = front_matter(entry) if entry.name == 'source.md' else {}
        original = meta.get('original', 'original.pdf')
        required = (str(original), 'source.md')
        if entry.name == 'metadata.md':
            required = ('original.pdf', 'metadata.md', 'raw.md')
        if not all(
            (paper / name).resolve().is_relative_to(sources)
            and (paper / name).is_file()
            and (paper / name).stat().st_size
            for name in required
        ):
            continue
        current = meta.get('revision')
        stale = sorted({
            page for rev, pages in revisions.get(paper.name, {}).items()
            if current is not None and rev != str(current) for page in pages
        })
        citation = 'stale' if stale else 'cited' if paper.name in cited else 'uncited'
        if state == 'all' or state == citation or (state == 'cited' and citation == 'stale'):
            item = {
                'id': paper.name,
                'domain': '' if paper.parent == sources else paper.parent.relative_to(sources).as_posix(),
                'path': entry.relative_to(root).as_posix(),
                'kind': meta.get('kind', 'paper'),
                'status': citation,
            }
            if current is not None:
                item['revision'] = str(current)
            if stale:
                item['stale_in'] = stale
            items.append(item)
    return items


def check_config(config: object) -> dict:
    """Validate the combined content of config.yaml and tools.yaml and fill its defaults."""
    # agents is from before 2.2.0, when each vault chose its agents; every agent is configured now.
    if not isinstance(config, dict) or set(config) - {'agents', 'repos', 'mcp', 'skills'}:
        raise ValueError('The configuration takes only repos, mcp, and skills.')
    repos = config.get('repos') or {}
    mcp = config.get('mcp') or []
    skills = config.get('skills') or []
    if not isinstance(repos, dict) or set(repos) - set(REPOS):
        raise ValueError(f'repos takes only {", ".join(REPOS)}.')
    repos = {name: repos.get(name) or None for name in REPOS}
    if any(not isinstance(u, (str, type(None))) or (u or '').startswith('-') for u in repos.values()):
        raise ValueError('Each repo is a Git URL or empty for a local folder.')
    catalogue = load_catalogue()
    if not isinstance(mcp, list):
        raise ValueError('mcp must be a list.')
    mcp = [m for m in mcp if not (isinstance(m, str) and m in RETIRED)]
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
    return {'repos': repos, 'mcp': mcp, 'skills': skills}


def read_config(root: Path) -> dict:
    """Read config.yaml and tools.yaml, or the single file of an older vault, without validating."""
    if (root / CONFIG).exists() or (root / TOOLS).exists():
        config = {}
        for path in (CONFIG, TOOLS):
            part = yaml.safe_load((root / path).read_text(encoding='utf-8')) if (root / path).exists() else None
            if part is not None and not isinstance(part, dict):
                raise ValueError(f'{path} must be a mapping.')
            config.update(part or {})
        return config
    for path in LEGACY_CONFIG:
        if (root / path).exists():
            return yaml.safe_load((root / path).read_text(encoding='utf-8')) or {}
    return {}


def write_config(root: Path, config: dict) -> None:
    """Write the repositories to config.yaml and the tools to tools.yaml."""
    (root / SETTINGS).mkdir(parents=True, exist_ok=True)
    for path, part in ((CONFIG, {'repos': config['repos']}),
                       (TOOLS, {'mcp': config['mcp'], 'skills': config['skills']})):
        body = yaml.safe_dump(part, sort_keys=False, allow_unicode=True)
        (root / path).write_text(HEADERS[path] + '\n' + body, encoding='utf-8')


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
    env_path = next((p for p in (ENV, *LEGACY_ENV) if (root / p).exists()), ENV)  # Setup moves an old one.
    env = (root / env_path).read_text(encoding='utf-8') if (root / env_path).exists() else ''
    return {
        'version': VERSION,
        'config': check_config(read_config(root)),
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
    target = root / LIB
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


def instructions(local: str = '') -> str:
    """This skill's SKILL.md as vault instructions, with paths into the installed copy, then the vault's own."""
    text = (SKILL / 'SKILL.md').read_text(encoding='utf-8')
    body = text.split('\n---\n', 1)[1].lstrip('\n') if text.startswith('---\n') else text
    for old, new in (
        ('](references/', f'](./{LIB.as_posix()}/references/'),
        ("Resolve `podarcis_skill` to this skill's actual installed directory; script paths are relative to this "
         "`SKILL.md`, not the vault or current directory. Set `vault_root` to the selected vault.",
         f"Set `vault_root` to this vault, the directory holding `AGENTS.md`; Podarcis is installed in its "
         f"`{LIB.as_posix()}`."),
        ('podarcis_skill=/absolute/path/to/installed/podarcis\nvault_root=/absolute/path/to/vault\n',
         f'vault_root=/absolute/path/to/vault\npodarcis_skill="$vault_root/{LIB.as_posix()}"\n'),
    ):
        if old not in body:
            raise RuntimeError(f'SKILL.md no longer contains {old!r}; update instructions().')
        body = body.replace(old, new)
    header = (f'<!-- Generated by Podarcis {VERSION} setup from its SKILL.md; setup overwrites this file. '
              f'Put this vault\'s own instructions in {LOCAL_INSTRUCTIONS.as_posix()}. -->\n\n')
    return header + body.rstrip('\n') + '\n' + (f'\n{local.strip()}\n' if local.strip() else '')


def write_instructions(root: Path) -> str:
    """Write the vault's instructions and link the root AGENTS.md to them, which every agent reads."""
    local = root / LOCAL_INSTRUCTIONS
    (root / INSTRUCTIONS).write_text(
        instructions(local.read_text(encoding='utf-8') if local.is_file() else ''), encoding='utf-8')
    link, target = root / 'AGENTS.md', INSTRUCTIONS
    if link.is_symlink():
        return 'linked' if Path(link.readlink()) == target else 'kept: AGENTS.md links elsewhere'
    if link.exists():
        return f'kept: AGENTS.md is a file; replace it with a link to {target.as_posix()} to use these instructions'
    link.symlink_to(target)
    return 'linked'


def write_agents(root: Path, servers: list[dict], skills: list[str]) -> None:
    """Write every agent's MCP servers, keeping its other settings, and link skills for Claude."""
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
    for agent in AGENTS:
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
    # Read before moving anything, so an invalid old configuration leaves the vault as it was.
    stored = check_config(read_config(root)) if config is None else config
    (root / SETTINGS).mkdir(parents=True, exist_ok=True)
    for old in LEGACY_ENV:  # .env in the root before 2.1.0, in .agents/ before 2.2.0.
        if (root / old).exists() and not (root / ENV).exists():
            (root / old).rename(root / ENV)
    if config is not None or not ((root / CONFIG).exists() and (root / TOOLS).exists()):
        write_config(root, stored)
    for old in LEGACY_CONFIG:
        (root / old).unlink(missing_ok=True)
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
    global SKILL
    if (root / LIB).resolve() != SKILL:
        install(root)
        actions['podarcis'] = f'installed {VERSION}'
    legacy = root / LEGACY_SKILL
    if legacy.is_symlink() or legacy.exists():  # Retire the 2.2 skill: its instructions now live in AGENTS.md.
        if legacy.resolve() == SKILL:
            SKILL = (root / LIB).resolve()
        legacy.unlink() if legacy.is_symlink() else shutil.rmtree(legacy)
        claude_link = root / '.claude' / 'skills' / 'podarcis'
        if claude_link.is_symlink() and claude_link.readlink() == Path('..', '..', LEGACY_SKILL):
            claude_link.unlink()
        actions['podarcis skill'] = 'removed'
    actions['AGENTS.md'] = write_instructions(root)
    servers, skills = [], list(config['skills'])
    for item in config['mcp']:
        if isinstance(item, str):
            servers.append({'name': item, **catalogue[item]['mcp']})
            skills += catalogue[item].get('skills', [])
        else:
            servers.append(item)
    for skill in skills:
        actions[skill['name']] = fetch_skill(root, skill)
    write_agents(root, servers, [s['name'] for s in skills])
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
    configure = commands.add_parser('setup', help='Apply .agents/podarcis/config.yaml and tools.yaml to the vault')
    configure.add_argument('--stdin', action='store_true', help='Replace the configuration with JSON from stdin first; its env key sets credentials')
    paper = commands.add_parser('ingest', help='Import OpenAlex JSON and a local PDF')
    paper.add_argument('metadata', type=Path, help='Complete OpenAlex get works JSON')
    paper.add_argument('source_id')
    paper.add_argument('domain', help='Relative snake_case folders under sources/, or "" for none')
    paper.add_argument('--pdf-path', required=True, type=Path)
    other = commands.add_parser('add', help='Import any file (notes, slides, documents, data, media) as a source')
    other.add_argument('file', type=Path, help='Local file to snapshot; the original is kept unchanged')
    other.add_argument('source_id')
    other.add_argument('domain', help='Relative snake_case folders under sources/, or "" for none')
    other.add_argument('--kind', help='paper, meeting-notes, slides, document, dataset, media, ...; guessed from the extension')
    other.add_argument('--title', help='Defaults to the file name')
    other.add_argument('--date', help='YYYY-MM-DD the content refers to, such as the meeting date')
    other.add_argument('--author', action='append', dest='authors', help='Repeat per author; omitted by default')
    other.add_argument('--origin', type=Path, help='JSON describing the external file: provider, id or url, revision, path')
    other.add_argument('--text-from', type=Path, help='Extract text from this rendering instead of the stored file')
    other.add_argument('--attach', type=Path, action='append', dest='attachments', help='Extra file stored unchanged beside the original; repeatable')
    listing = commands.add_parser('status', help='List cited, uncited, or stale sources')
    listing.add_argument('--status', choices=('all', 'cited', 'uncited', 'stale'), default='all')
    args = parser.parse_args()
    root = Path(args.root).expanduser().resolve()
    if not root.is_dir():
        parser.error(f'No vault directory at {root}.')
    if args.command in ('ingest', 'add', 'status') and not (root / 'sources').is_dir():
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
        elif args.command == 'add':
            origin = json.loads(args.origin.read_text(encoding='utf-8')) if args.origin else None
            result = add(root, args.file, args.source_id, args.domain, args.kind, args.title,
                         args.date, args.authors, origin, args.text_from, args.attachments)
        else:
            result = status(root, args.status)
    except Exception as error:
        parser.exit(1, f'{type(error).__name__}: {error}\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
