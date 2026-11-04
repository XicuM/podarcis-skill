"""Adapted upstream CLI behavior with controlled HTTP responses."""

import argparse
import json
import logging
from types import SimpleNamespace
from urllib.parse import parse_qs, urlsplit

import httpx
import pytest

import openalex_cli as openalex


def test_search_parameters_and_environment_key(monkeypatch, capsys):
    requests = []

    def fetch(url, **kwargs):
        requests.append(url)
        return SimpleNamespace(json=lambda: {'results': []})

    monkeypatch.setattr(openalex._API_CLIENT, 'fetch', fetch)
    monkeypatch.setenv('OPENALEX_API_KEY', 'test-key')
    openalex.main([
        'openalex', 'filter', 'works', '--search', 'A & B?',
        '--select', 'id,display_name', '--per-page', '5',
    ])
    assert json.loads(capsys.readouterr().out) == {'results': []}
    params = parse_qs(urlsplit(requests[0]).query)
    assert params['search'] == ['A & B?']
    assert params['api_key'] == ['test-key']
    assert params['select'] == ['id,display_name']
    assert params['per_page'] == ['5']


def test_plan_restriction_is_not_retried_and_credentials_are_redacted(
    monkeypatch, caplog
):
    calls = []

    def fetch(url, **kwargs):
        calls.append(url)
        raise openalex.http_client.HttpError(
            f'Failed {url}', status_code=429, body=b'Plan upgrade required'
        )

    monkeypatch.setattr(openalex._API_CLIENT, 'fetch', fetch)
    with pytest.raises(SystemExit) as error:
        openalex.fetch_with_retry(
            openalex.BASE_URL + '/works', {}, api_key='secret-test-key'
        )
    assert error.value.code == 1
    assert len(calls) == 1
    assert 'paid plan' in caplog.text
    assert 'secret-test-key' not in caplog.text


def test_api_error_is_nonzero_and_does_not_expose_key(monkeypatch, caplog):
    def fetch(url, **kwargs):
        raise openalex.http_client.HttpError(f'Failed {url}', status_code=401)

    monkeypatch.setattr(openalex._API_CLIENT, 'fetch', fetch)
    with pytest.raises(SystemExit):
        openalex.fetch_with_retry(
            openalex.BASE_URL + '/works', {}, api_key='secret-test-key'
        )
    assert '401' in caplog.text
    assert 'secret-test-key' not in caplog.text


def test_pdf_download_is_atomic_and_never_overwrites(monkeypatch, tmp_path, pdf):
    requests = []
    client = httpx.Client

    def respond(request):
        requests.append(request)
        return httpx.Response(200, content=pdf.read_bytes())

    def connection(**kwargs):
        assert kwargs.get('verify', True) is True
        return client(transport=httpx.MockTransport(respond), **kwargs)

    monkeypatch.setattr(openalex.httpx, 'Client', connection)
    output = tmp_path / 'download.pdf'
    assert openalex._try_download_url('https://example.org/paper', str(output))
    assert output.read_bytes() == pdf.read_bytes()
    with pytest.raises(FileExistsError):
        openalex._try_download_url('https://example.org/paper', str(output))
    assert len(requests) == 1
    assert not list(tmp_path.glob('.download_*'))


@pytest.mark.parametrize('response_type', ['html', 'oversized', 'interrupted', 'http'])
def test_failed_download_leaves_no_partial_file(
    monkeypatch, tmp_path, response_type, caplog
):
    class Stream(httpx.SyncByteStream):
        def __iter__(self):
            yield b'%PDF-'
            if response_type == 'interrupted':
                raise httpx.ReadError('Interrupted')
            for _ in range(51):
                yield b'x' * (1024 * 1024)

    def respond(request):
        if response_type == 'http':
            return httpx.Response(403)
        if response_type == 'html':
            return httpx.Response(200, text='<html>Not a PDF</html>')
        return httpx.Response(200, stream=Stream())

    client = httpx.Client
    monkeypatch.setattr(
        openalex.httpx, 'Client',
        lambda **kwargs: client(transport=httpx.MockTransport(respond), **kwargs),
    )
    output = tmp_path / 'download.pdf'
    assert not openalex._try_download_url('https://example.org/paper', str(output))
    assert list(tmp_path.iterdir()) == []
    assert caplog.text


def test_https_and_redirect_boundaries(monkeypatch, tmp_path):
    client = httpx.Client

    def respond(request):
        if request.url.scheme == 'https':
            return httpx.Response(302, headers={'Location': 'http://example.org/pdf'})
        return httpx.Response(200, content=b'%PDF-fixture')

    monkeypatch.setattr(
        openalex.httpx, 'Client',
        lambda **kwargs: client(transport=httpx.MockTransport(respond), **kwargs),
    )
    output = tmp_path / 'paper.pdf'
    assert not openalex._try_download_url('http://example.org/pdf', str(output))
    assert not openalex._try_download_url('https://example.org/pdf', str(output))
    assert list(tmp_path.iterdir()) == []


def test_fallback_tries_all_recorded_locations_without_duplicates(monkeypatch, tmp_path):
    attempted = []
    work = {
        'best_oa_location': {'pdf_url': 'https://example.org/blocked.pdf'},
        'primary_location': {'pdf_url': None},
        'locations': [
            {'pdf_url': 'https://example.org/blocked.pdf'},
            {'pdf_url': 'https://example.org/working.pdf'},
        ],
    }
    monkeypatch.setattr(openalex, 'fetch_with_retry', lambda *a, **kw: work)

    def download(url, output):
        attempted.append(url)
        return url.endswith('working.pdf')

    monkeypatch.setattr(openalex, '_try_download_url', download)
    assert openalex._try_fallback_locations('W123', str(tmp_path / 'paper.pdf'), None)
    assert attempted == [
        'https://example.org/blocked.pdf', 'https://example.org/working.pdf',
    ]


def test_keyless_download_uses_recorded_locations(monkeypatch, tmp_path):
    calls = []
    monkeypatch.setattr(
        openalex, '_try_fallback_locations',
        lambda *args: calls.append(args) or True,
    )
    monkeypatch.setattr(
        openalex, '_try_download_url',
        lambda *args: pytest.fail('Keyless requests must not hit the content server'),
    )
    openalex.handle_download_pdf(argparse.Namespace(
        id='W123', output=str(tmp_path / 'paper.pdf'), api_key=None,
    ))
    assert len(calls) == 1


def test_download_failure_is_nonzero(monkeypatch, tmp_path):
    monkeypatch.setattr(openalex, '_try_fallback_locations', lambda *args: False)
    with pytest.raises(SystemExit) as error:
        openalex.handle_download_pdf(argparse.Namespace(
            id='W123', output=str(tmp_path / 'paper.pdf'), api_key=None,
        ))
    assert error.value.code == 1


def test_authenticated_download_does_not_log_key(monkeypatch, tmp_path, caplog):
    client = httpx.Client
    monkeypatch.setattr(
        openalex.httpx, 'Client',
        lambda **kwargs: client(transport=httpx.MockTransport(
            lambda request: httpx.Response(200, content=b'%PDF-fixture')
        ), **kwargs),
    )
    caplog.set_level(logging.INFO)
    openalex.handle_download_pdf(argparse.Namespace(
        id='W123', output=str(tmp_path / 'paper.pdf'), api_key='secret-test-key',
    ))
    assert 'secret-test-key' not in caplog.text
    assert 'Successfully downloaded' in caplog.text
