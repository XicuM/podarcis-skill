"""Exercise the standalone script from outside the vault."""

import json
import subprocess
import sys
from pathlib import Path

import vault


def test_cli_serves_only_the_selected_vault(root, tmp_path):
    source = root / 'sources/testing/test_paper'
    source.mkdir(parents=True)
    for name in ('source.md', 'original.pdf'):
        (source / name).write_text('content\n')
    other = tmp_path / 'other'
    (other / 'sources').mkdir(parents=True)
    for selected, count in ((root, 1), (other, 0)):
        result = subprocess.run(
            [sys.executable, str(Path(vault.__file__)), '--root', str(selected), 'status'],
            cwd=other, capture_output=True, text=True, timeout=10,
        )
        assert result.returncode == 0, result.stderr
        assert len(json.loads(result.stdout)) == count


def test_missing_sources_directory_is_rejected(tmp_path):
    result = subprocess.run(
        [sys.executable, str(Path(vault.__file__)), '--root', str(tmp_path), 'status'],
        capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 2
    assert 'No sources/ directory' in result.stderr


def test_cli_ingestion_returns_only_json(root, metadata, pdf):
    result = subprocess.run(
        [sys.executable, str(Path(vault.__file__)), '--root', str(root),
         'ingest', str(metadata), 'cli_paper', 'testing', '--pdf-path', str(pdf)],
        capture_output=True, text=True, timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert (root / json.loads(result.stdout)['path']).is_file()


def test_cli_failure_is_nonzero_and_has_no_json(root, tmp_path):
    result = subprocess.run(
        [sys.executable, str(Path(vault.__file__)), '--root', str(root),
         'ingest', str(tmp_path / 'missing.json'), 'paper', 'testing',
         '--pdf-path', str(tmp_path / 'missing.pdf')],
        capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 1
    assert 'FileNotFoundError' in result.stderr
    assert result.stdout == ''
