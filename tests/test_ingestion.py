"""Vault behavior with real PDF extraction and temporary research data."""

import json

import pymupdf4llm
import pytest
import yaml

import vault


def test_real_pdf_ingestion_citation_and_duplicate_guard(root, pdf, metadata):
    result = vault.ingest(root, metadata, pdf, 'test_paper', 'testing')
    assert result == {
        'id': 'test_paper',
        'path': 'sources/testing/test_paper/source.md',
    }
    entry = root / result['path']
    assert sorted(p.name for p in entry.parent.iterdir()) == ['original.pdf', 'source.md']
    assert (entry.parent / 'original.pdf').read_bytes() == pdf.read_bytes()
    _, header, body = entry.read_text().split('---', 2)
    assert yaml.safe_load(header) == {
        'type': 'source',
        'id': 'test_paper',
        'title': 'Synthetic fixture',
        'authors': ['Test Author'],
        'year': 2026,
        'doi': 'https://doi.org/10.1234/test',
        'openalex_id': 'W123',
        'publication_type': 'article',
    }
    assert 'Synthetic research workflow test.' in body
    assert '[Original PDF](./original.pdf)' in body
    assert '[Publisher](https://doi.org/10.1234/test)' in body
    assert vault.status(root)[0]['status'] == 'uncited'
    before = entry.read_bytes()
    with pytest.raises(FileExistsError):
        vault.ingest(root, metadata, pdf, 'test_paper', 'other/nested')
    assert entry.read_bytes() == before
    page = root / 'wiki/page.md'
    page.parent.mkdir()
    page.write_text(
        'Evidence[^test_paper].\n\n'
        '[^test_paper]: [Paper](../sources/testing/test_paper/source.md)\n'
    )
    assert vault.status(root, 'uncited') == []
    assert vault.status(root, 'cited')[0]['id'] == 'test_paper'


@pytest.mark.parametrize('failure', ['empty', 'exception'])
def test_extraction_failure_leaves_no_partial_source(
    root, pdf, metadata, monkeypatch, failure
):
    def extract(*args, **kwargs):
        if failure == 'exception':
            raise RuntimeError('Extraction failed')
        return '\x00\n'

    monkeypatch.setattr(pymupdf4llm, 'to_markdown', extract)
    with pytest.raises((ValueError, RuntimeError)):
        vault.ingest(root, metadata, pdf, 'empty', 'testing')
    assert list((root / 'sources/testing').iterdir()) == []
    assert vault.status(root) == []


def test_nested_domains_are_ingested_and_listed(root, pdf, metadata):
    domain = 'literature/biology/genetics'
    result = vault.ingest(root, metadata, pdf, 'deep_paper', domain)
    assert vault.status(root) == [{
        'id': 'deep_paper',
        'domain': domain,
        'path': result['path'],
        'status': 'uncited',
    }]


@pytest.mark.parametrize(
    'source_id, domain',
    [('bad/name', 'testing'), ('ok', '../outside'), ('..', 'testing'), ('ok', 'a//b')],
)
def test_ingestion_rejects_path_traversal(root, pdf, metadata, source_id, domain):
    with pytest.raises(ValueError, match='snake_case'):
        vault.ingest(root, metadata, pdf, source_id, domain)


def test_symlink_escape_is_rejected(root, pdf, metadata, tmp_path):
    (root / 'sources/escape').symlink_to(tmp_path, target_is_directory=True)
    with pytest.raises(ValueError, match='leaves sources'):
        vault.ingest(root, metadata, pdf, 'paper', 'escape')
    assert not (tmp_path / 'paper').exists()


def test_invalid_pdf_leaves_no_source(root, metadata, tmp_path):
    pdf = tmp_path / 'not_pdf'
    pdf.write_text('<html>Not a PDF</html>')
    with pytest.raises(ValueError, match='does not contain a PDF'):
        vault.ingest(root, metadata, pdf, 'bad', 'testing')
    assert list((root / 'sources/testing').iterdir()) == []


def test_search_results_cannot_be_imported_as_metadata(root, pdf, metadata):
    metadata.write_text(json.dumps({'results': []}))
    with pytest.raises(KeyError):
        vault.ingest(root, metadata, pdf, 'bad', 'testing')
    assert list((root / 'sources').iterdir()) == []


def test_legacy_sources_are_listed_without_modification(root):
    source = root / 'sources/testing/legacy'
    source.mkdir(parents=True)
    for name in ('metadata.md', 'original.pdf', 'raw.md'):
        (source / name).write_text('Legacy content\n')
    assert vault.status(root)[0]['path'].endswith('/metadata.md')
    assert sorted(p.name for p in source.iterdir()) == [
        'metadata.md', 'original.pdf', 'raw.md',
    ]


def test_incomplete_sources_and_fenced_citations_are_excluded(root):
    for source_id in ('complete', 'incomplete'):
        source = root / 'sources' / source_id
        source.mkdir()
        (source / 'source.md').write_text('Paper text\n')
    (root / 'sources/complete/original.pdf').write_bytes(b'%PDF-fixture')
    (root / 'wiki').mkdir()
    (root / 'wiki/page.md').write_text(
        '```markdown\n[^complete]: Example citation only\n```\n'
    )
    assert [item['id'] for item in vault.status(root, 'uncited')] == ['complete']
    assert vault.status(root, 'cited') == []
