# /// script
# requires-python = ">=3.11"
# dependencies = ["pyyaml>=6.0", "pymupdf>=2.0", "pymupdf4llm>=2.0"]
# ///
"""Import a paper into a local vault and report wiki citation status."""

import argparse
import contextlib
import json
import re
import shutil
import sys
import tempfile
from pathlib import Path

import yaml


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


def main() -> None:
    """Run a vault command, returning JSON on stdout and errors on stderr."""
    parser = argparse.ArgumentParser(prog='podarcis')
    parser.add_argument('--root', default='.', help='Vault path; defaults to cwd')
    commands = parser.add_subparsers(dest='command', required=True)
    add = commands.add_parser('ingest', help='Import OpenAlex JSON and a local PDF')
    add.add_argument('metadata', type=Path, help='Complete OpenAlex get works JSON')
    add.add_argument('source_id')
    add.add_argument('domain', help='Relative snake_case folders under sources/')
    add.add_argument('--pdf-path', required=True, type=Path)
    listing = commands.add_parser('status', help='List cited or uncited sources')
    listing.add_argument('--status', choices=('all', 'cited', 'uncited'), default='all')
    args = parser.parse_args()
    root = Path(args.root).expanduser().resolve()
    if not (root / 'sources').is_dir():
        parser.error(f'No sources/ directory in {root}.')
    try:
        result = (
            ingest(root, args.metadata, args.pdf_path, args.source_id, args.domain)
            if args.command == 'ingest'
            else status(root, args.status)
        )
    except Exception as error:
        parser.exit(1, f'{type(error).__name__}: {error}\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
