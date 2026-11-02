"""Vault and PDF fixtures for the ingestion workflow."""

import json

import pytest


@pytest.fixture
def root(tmp_path):
    vault = tmp_path / 'vault'
    (vault / 'sources').mkdir(parents=True)
    return vault


@pytest.fixture
def metadata(tmp_path):
    work = {
        'id': 'https://openalex.org/W123',
        'display_name': 'Synthetic fixture',
        'authorships': [{'author': {'display_name': 'Test Author'}}],
        'publication_year': 2026,
        'doi': 'https://doi.org/10.1234/test',
        'type': 'article',
    }
    path = tmp_path / 'work.json'
    path.write_text(json.dumps(work))
    return path


@pytest.fixture
def pdf(tmp_path):
    text = b'BT /F1 12 Tf 72 720 Td (Synthetic research workflow test.) Tj ET'
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        (
            b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] '
            b'/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>'
        ),
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
        b'<< /Length '
        + str(len(text)).encode()
        + b' >>\nstream\n'
        + text
        + b'\nendstream',
    ]
    body = b'%PDF-1.4\n'
    offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(body))
        body += f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n'
    xref = len(body)
    body += b'xref\n0 6\n0000000000 65535 f \n'
    body += b''.join(f'{offset:010d} 00000 n \n'.encode() for offset in offsets[1:])
    body += f'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF'.encode()
    path = tmp_path / 'sample.pdf'
    path.write_bytes(body)
    return path
