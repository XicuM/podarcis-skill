"""OpenAlex discovery and local, full-text paper ingestion."""

import asyncio
import os
import re
import shutil
import tempfile
from pathlib import Path
from typing import Literal
from urllib.parse import quote, urlsplit

import httpx
import yaml
from pdfminer.high_level import extract_text

ROOT = Path.cwd()


async def literature_search(query: str, limit: int = 5) -> list[dict[str, object]]:
    """Search OpenAlex. Results are candidates, not verified peer-reviewed evidence."""
    if not query.strip() or not 1 <= limit <= 20:
        raise ValueError('Provide a nonempty query and a limit between 1 and 20.')
    params = {'search': query, 'per_page': limit}
    key = os.environ.get('OPENALEX_API_KEY')
    headers = {'Authorization': f'Bearer {key}'} if key else {}
    async with httpx.AsyncClient(timeout=30, headers=headers) as client:
        response = await client.get('https://api.openalex.org/works', params=params)
        response.raise_for_status()
    return [
        {
            'paper_id': work['id'],
            'title': work['display_name'],
            'authors': [a['author']['display_name'] for a in work['authorships']],
            'year': work['publication_year'],
            'doi': work['doi'],
            'publication_type': work['type'],
            'pdf_url': (work.get('best_oa_location') or {}).get('pdf_url'),
        }
        for work in response.json()['results']
    ]


async def _resolve_metadata(paper_id: str) -> dict[str, object]:
    """Resolve an explicit OpenAlex work ID or DOI; never guess from a title."""
    identifier = paper_id.removeprefix('https://openalex.org/')
    identifier = identifier.removeprefix('openalex:')
    identifier = identifier.removeprefix('https://doi.org/').removeprefix('DOI:')
    if re.fullmatch(r'W[0-9]+', identifier):
        pass
    elif re.fullmatch(r'10\.\d{4,9}/\S+', identifier):
        identifier = 'https://doi.org/' + identifier
    else:
        raise ValueError('Use an OpenAlex W-number/URL or DOI, not a title.')
    key = os.environ.get('OPENALEX_API_KEY')
    headers = {'Authorization': f'Bearer {key}'} if key else {}
    async with httpx.AsyncClient(timeout=30, headers=headers) as client:
        response = await client.get(
            'https://api.openalex.org/works/' + quote(identifier, safe=':/'),
        )
        response.raise_for_status()
    return response.json()


async def _download_pdf(url: str, dest: Path) -> None:
    """Download a PDF over HTTPS with certificate validation and a 50 MiB limit."""
    if urlsplit(url).scheme != 'https':
        raise ValueError('The PDF URL must use HTTPS.')
    async with (
        httpx.AsyncClient(timeout=60, follow_redirects=True) as client,
        client.stream('GET', url) as response,
    ):
        response.raise_for_status()
        if response.url.scheme != 'https':
            raise ValueError('The PDF redirected to a non-HTTPS URL.')
        size = 0
        with dest.open('wb') as stream:
            async for chunk in response.aiter_bytes():
                size += len(chunk)
                if size > 50 * 1024 * 1024:
                    raise ValueError('PDF exceeds the 50 MiB download limit.')
                stream.write(chunk)
    with dest.open('rb') as stream:
        if stream.read(5) != b'%PDF-':
            raise ValueError('The server did not return a PDF.')


async def literature_download(
    paper_id: str,
    source_id: str,
    domain: str,
    pdf_path: str | None = None,
) -> dict[str, object]:
    """Ingest a paper into sources/<domain>/<source_id>; optionally use a supplied PDF.

    domain is any relative folder under sources/, such as biology/genetics.

    Metadata comes from OpenAlex. Existing source IDs are never overwritten.
    Failed extraction leaves no source directory. Scanned PDFs require OCR elsewhere.
    """
    if not re.fullmatch(r'[a-z][a-z0-9_]*', source_id) or not re.fullmatch(
        r'[a-z][a-z0-9_]*(/[a-z][a-z0-9_]*)*', domain
    ):
        raise ValueError(
            'source_id must be snake_case; domain must be snake_case folders.'
        )
    target = (ROOT / 'sources' / domain / source_id).resolve()
    if not target.is_relative_to(ROOT / 'sources'):
        raise ValueError('Source path leaves sources/.')
    if any(p.is_dir() for p in (ROOT / 'sources').rglob(source_id)):
        raise FileExistsError(f'Source ID already exists: {source_id}.')
    work = await _resolve_metadata(paper_id)
    pdf_url = next(
        (
            location['pdf_url']
            for location in [
                work.get('best_oa_location'),
                work.get('primary_location'),
                *work.get('locations', []),
            ]
            if location and location.get('pdf_url')
        ),
        None,
    )
    if not pdf_path and not pdf_url:
        raise ValueError(
            'No PDF URL in OpenAlex. Supply pdf_path to an accessible PDF.'
        )
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.ingest_', dir=target.parent) as scratch:
        stage = Path(scratch) / source_id
        stage.mkdir()
        pdf = stage / 'original.pdf'
        if pdf_path:
            local = Path(pdf_path).expanduser()
            if not local.is_absolute():
                raise ValueError('pdf_path must be an absolute path.')
            shutil.copyfile(local, pdf)
            with pdf.open('rb') as stream:
                if stream.read(5) != b'%PDF-':
                    raise ValueError('pdf_path does not contain a PDF.')
        else:
            await _download_pdf(pdf_url, pdf)
        text = await asyncio.to_thread(extract_text, str(pdf))
        text = re.sub(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]', '', text)
        text = text.encode('utf-8', errors='replace').decode('utf-8').strip()
        if not text:
            raise ValueError('PDF extraction returned no text; no source was stored.')
        (stage / 'raw.md').write_text(text + '\n', encoding='utf-8')
        metadata = {
            'id': source_id,
            'paper_id': work['id'],
            'title': work['display_name'],
            'authors': [a['author']['display_name'] for a in work['authorships']],
            'year': work['publication_year'],
            'doi': work['doi'],
            'publication_type': work['type'],
        }
        if not pdf_path:
            metadata['pdf_url'] = pdf_url
        (stage / 'metadata.md').write_text(
            '---\n'
            + yaml.safe_dump(metadata, sort_keys=False, allow_unicode=True)
            + '---\n\n[Full text](raw.md) · [Original PDF](original.pdf)\n',
            encoding='utf-8',
        )
        if target.exists() or any(
            p.is_dir() and p != stage for p in (ROOT / 'sources').rglob(source_id)
        ):
            raise FileExistsError(f'Source ID already exists: {source_id}.')
        stage.rename(target)
    return {'id': source_id, 'path': target.relative_to(ROOT).as_posix()}


async def literature_status(
    status: Literal['uncited', 'cited', 'all'] = 'all',
) -> list[dict[str, object]]:
    """List complete local sources; cited means referenced in wiki, not validated."""
    if status not in ('uncited', 'cited', 'all'):
        raise ValueError('Unknown citation status.')
    cited = set()
    for path in (ROOT / 'wiki').rglob('*.md'):
        if path.resolve().is_relative_to(ROOT / 'wiki'):
            text = path.read_text(encoding='utf-8')
            text = re.sub(
                r'^(`{3,}|~{3,}).*?^\1[^\n]*$', '', text, flags=re.MULTILINE | re.DOTALL
            )
            cited.update(re.findall(r'^\[\^([a-z][a-z0-9_]*)\]:', text, re.MULTILINE))
    items = []
    for metadata in sorted((ROOT / 'sources').rglob('metadata.md')):
        paper = metadata.parent
        if not paper.resolve().is_relative_to(ROOT / 'sources'):
            continue
        if not all(
            (paper / name).is_file() and (paper / name).stat().st_size
            for name in ('original.pdf', 'raw.md')
        ):
            continue
        state = 'cited' if paper.name in cited else 'uncited'
        if status in ('all', state):
            items.append(
                {
                    'id': paper.name,
                    'domain': paper.parent.relative_to(ROOT / 'sources').as_posix(),
                    'path': metadata.relative_to(ROOT).as_posix(),
                    'status': state,
                }
            )
    return items
