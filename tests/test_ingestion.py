"""Ingestion uses real PDF extraction; network responses are controlled fixtures."""

import asyncio

import httpx
import pytest
import yaml

from podarcis.tools import literature


@pytest.fixture
def metadata(monkeypatch):
    work = {
        'id': 'https://openalex.org/W123',
        'display_name': 'Synthetic fixture',
        'authorships': [{'author': {'display_name': 'Test Author'}}],
        'publication_year': 2026,
        'doi': None,
        'type': 'article',
        'best_oa_location': {'pdf_url': 'https://example.org/test.pdf'},
    }

    async def resolve(paper_id):
        return work

    monkeypatch.setattr(literature, '_resolve_metadata', resolve)
    return work


def test_real_pdf_ingestion_citation_and_duplicate_guard(
    root, pdf, metadata
):
    result = asyncio.run(
        literature.literature_download('W123', 'test_paper', 'testing', str(pdf))
    )
    assert result == {'id': 'test_paper', 'path': 'sources/testing/test_paper'}
    source = root / result['path']
    assert (source / 'original.pdf').read_bytes() == pdf.read_bytes()
    assert 'Synthetic research workflow test.' in (source / 'raw.md').read_text()
    header = (source / 'metadata.md').read_text().split('---')[1]
    assert yaml.safe_load(header)['title'] == 'Synthetic fixture'
    assert asyncio.run(literature.literature_status())[0]['status'] == 'uncited'
    with pytest.raises(FileExistsError):
        asyncio.run(
            literature.literature_download(
                'W123', 'test_paper', 'other/nested', str(pdf)
            )
        )
    page = root / 'wiki/page.md'
    page.parent.mkdir()
    page.write_text(
        'Evidence[^test_paper].\n\n'
        '[^test_paper]: [Paper](../sources/testing/test_paper/original.pdf)\n'
    )
    assert asyncio.run(literature.literature_status('uncited')) == []
    assert (
        asyncio.run(literature.literature_status('cited'))[0]['id'] == 'test_paper'
    )


def test_extraction_failure_leaves_no_partial_source(root, pdf, metadata, monkeypatch):
    monkeypatch.setattr(
        literature,
        'extract_text',
        lambda _: '\x00\n',
    )
    with pytest.raises(ValueError, match='no text'):
        asyncio.run(
            literature.literature_download('W123', 'empty', 'testing', str(pdf))
        )
    assert not (root / 'sources/testing/empty').exists()
    assert asyncio.run(literature.literature_status()) == []


def test_nested_domains_are_ingested_and_listed(root, pdf, metadata):
    domain = 'literature/biology/genetics'
    result = asyncio.run(
        literature.literature_download('W123', 'deep_paper', domain, str(pdf))
    )
    assert result['path'] == f'sources/{domain}/deep_paper'
    assert asyncio.run(literature.literature_status()) == [
        {
            'id': 'deep_paper',
            'domain': domain,
            'path': f'sources/{domain}/deep_paper/metadata.md',
            'status': 'uncited',
        }
    ]


@pytest.mark.parametrize(
    'source_id, domain',
    [('bad/name', 'testing'), ('ok', '../outside'), ('..', 'testing'), ('ok', 'a//b')],
)
def test_ingestion_rejects_path_traversal(root, source_id, domain):
    with pytest.raises(ValueError, match='snake_case'):
        asyncio.run(literature.literature_download('W123', source_id, domain))


def test_openalex_queries_are_encoded_and_failures_are_visible(monkeypatch):
    requests = []

    async def handle(request):
        requests.append(request)
        if request.url.path == '/works':
            return httpx.Response(200, json={'results': []})
        return httpx.Response(429, text='Rate limited')

    original = httpx.AsyncClient
    monkeypatch.setattr(
        literature.httpx,
        'AsyncClient',
        lambda **kw: original(
            transport=httpx.MockTransport(handle),
            **kw,
        ),
    )
    monkeypatch.setenv('OPENALEX_API_KEY', 'test-key')
    assert asyncio.run(literature.literature_search('A & B?')) == []
    assert requests[0].url.params['search'] == 'A & B?'
    assert requests[0].headers['Authorization'] == 'Bearer test-key'
    assert 'test-key' not in str(requests[0].url)
    with pytest.raises(httpx.HTTPStatusError):
        asyncio.run(literature._resolve_metadata('W123'))
    with pytest.raises(ValueError, match='not a title'):
        asyncio.run(literature._resolve_metadata('Guess a paper title'))


def test_download_rejects_html_and_keeps_tls_verification(monkeypatch, tmp_path):
    original = httpx.AsyncClient

    def client(**kwargs):
        assert kwargs.get('verify', True) is True
        return original(
            transport=httpx.MockTransport(
                lambda request: httpx.Response(200, text='<html>Not a PDF</html>'),
            ),
            **kwargs,
        )

    monkeypatch.setattr(literature.httpx, 'AsyncClient', client)
    with pytest.raises(ValueError, match='did not return a PDF'):
        asyncio.run(
            literature._download_pdf('https://example.org/paper', tmp_path / 'file')
        )
    with pytest.raises(ValueError, match='HTTPS'):
        asyncio.run(
            literature._download_pdf('http://example.org/paper', tmp_path / 'file')
        )


def test_remote_import_uses_recorded_location_and_cleans_up_failures(
    root,
    pdf,
    metadata,
    monkeypatch,
):
    metadata['best_oa_location'] = {'pdf_url': None}
    metadata['locations'] = [{'pdf_url': 'https://example.org/repository.pdf'}]

    async def download(url, dest):
        assert url == 'https://example.org/repository.pdf'
        dest.write_bytes(pdf.read_bytes())

    monkeypatch.setattr(literature, '_download_pdf', download)
    result = asyncio.run(
        literature.literature_download('W123', 'remote', 'testing')
    )
    assert (root / result['path'] / 'raw.md').is_file()

    async def fail(url, dest):
        dest.write_bytes(b'partial download')
        raise RuntimeError('Download interrupted')

    monkeypatch.setattr(literature, '_download_pdf', fail)
    with pytest.raises(RuntimeError, match='interrupted'):
        asyncio.run(literature.literature_download('W123', 'failed', 'testing'))
    assert sorted(path.name for path in (root / 'sources/testing').iterdir()) == [
        'remote'
    ]
