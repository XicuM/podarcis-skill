"""Typed, versioned sources from files other than papers."""

import json

import pytest
import yaml

import podarcis


def meta(path):
    return yaml.safe_load(path.read_text().split('---', 2)[1])


@pytest.fixture
def docx_file(tmp_path):
    import docx

    document = docx.Document()
    document.add_heading('Decisions', level=1)
    document.add_paragraph('Site B is chosen for 2027.')
    document.add_paragraph('Book the van', style='List Bullet')
    grid = document.add_table(rows=2, cols=2)
    for r, row in enumerate([['Site', 'Votes'], ['B', '4']]):
        for c, value in enumerate(row):
            grid.cell(r, c).text = value
    path = tmp_path / 'lab_meeting.docx'
    document.save(path)
    return path


@pytest.fixture
def pptx_file(tmp_path):
    import pptx

    deck = pptx.Presentation()
    for title, body in [('Field plan', 'Sample 40 lizards'), ('Budget', 'Pending')]:
        slide = deck.slides.add_slide(deck.slide_layouts[1])
        slide.shapes.title.text = title
        slide.placeholders[1].text = body
    deck.slides[0].notes_slide.notes_text_frame.text = 'Ask about permits'
    path = tmp_path / 'deck.pptx'
    deck.save(path)
    return path


@pytest.fixture
def xlsx_file(tmp_path):
    import openpyxl

    book = openpyxl.Workbook()
    book.active.title = 'Counts'
    book.active.append(['site', 'n'])
    book.active.append(['A', 12])
    path = tmp_path / 'counts.xlsx'
    book.save(path)
    return path


def test_docx_keeps_headings_lists_and_tables(root, docx_file):
    result = podarcis.add(root, docx_file, 'lab_meeting_2026_10_14', 'team/meetings',
                          kind='meeting-notes', date='2026-10-14')
    entry = root / result['path']
    data = meta(entry)
    assert data['kind'] == 'meeting-notes' and data['date'] == '2026-10-14'
    assert data['original'] == 'original.docx' and 'authors' not in data
    assert (entry.parent / 'original.docx').read_bytes() == docx_file.read_bytes()
    text = entry.read_text()
    assert '## Decisions' in text and '- Book the van' in text and '| B | 4 |' in text
    assert podarcis.status(root) == [{
        'id': 'lab_meeting_2026_10_14', 'domain': 'team/meetings', 'path': result['path'],
        'kind': 'meeting-notes', 'status': 'uncited', 'revision': result['revision'],
    }]


def test_slides_get_numbered_anchors_and_notes(root, pptx_file):
    text = (root / podarcis.add(root, pptx_file, 'deck', 'team')['path']).read_text()
    assert meta(root / 'sources/team/deck/source.md')['kind'] == 'slides'
    assert '## Slide 1\n\n**Field plan**' in text and '## Slide 2' in text
    assert 'Speaker notes:\n\nAsk about permits' in text


def test_sheets_and_pdf_pages(root, xlsx_file, pdf):
    sheet = (root / podarcis.add(root, xlsx_file, 'counts', 'data')['path']).read_text()
    assert '## Sheet Counts' in sheet and '| A | 12 |' in sheet
    page = (root / podarcis.add(root, pdf, 'scan', 'data')['path']).read_text()
    assert '## Page 1' in page and 'Synthetic research workflow test.' in page


def test_unknown_files_are_citable_without_text(root, tmp_path):
    image = tmp_path / 'whiteboard.png'
    image.write_bytes(b'\x89PNG fixture')
    entry = root / podarcis.add(root, image, 'whiteboard', 'team')['path']
    assert meta(entry)['kind'] == 'media'
    assert 'No text could be extracted' in entry.read_text()
    assert podarcis.status(root)[0]['kind'] == 'media'


def test_local_sources_are_never_replaced(root, docx_file):
    podarcis.add(root, docx_file, 'notes', 'team')
    with pytest.raises(FileExistsError):
        podarcis.add(root, docx_file, 'notes', 'elsewhere')


def test_external_revisions_keep_cited_snapshots_and_flag_stale(root, tmp_path):
    origin = {'provider': 'google-drive', 'id': 'abc', 'url': 'https://drive.example/abc',
              'revision': 'r1', 'path': 'Meetings/'}
    notes = tmp_path / 'notes.md'
    notes.write_text('# Plan\n\nSite A.\n')
    first = podarcis.add(root, notes, 'plan', 'team', origin=origin)
    assert first['revision'] == 'r1' and first['changed'] is True
    assert meta(root / first['path'])['origin']['provider'] == 'google-drive'
    assert podarcis.add(root, notes, 'plan', 'team', origin=origin)['changed'] is False

    page = root / 'wiki/plan.md'
    page.parent.mkdir()
    page.write_text(
        '---\ntype: concept\ntitle: Plan\nstatus: draft\nsources:\n'
        '  - id: plan\n    resource: ../sources/team/plan/source.md\n    revision: r1\n---\n\n'
        'Site A.[^plan]\n\n[^plan]: [Plan](../sources/team/plan/source.md#plan)\n'
    )
    assert podarcis.status(root)[0]['status'] == 'cited'

    notes.write_text('# Plan\n\nSite B.\n')
    with pytest.raises(FileExistsError):
        podarcis.add(root, notes, 'plan', 'team', origin=origin | {'id': 'other', 'url': None})
    second = podarcis.add(root, notes, 'plan', 'team', origin=origin | {'revision': 'r2'})
    assert second['revision'] == 'r2'
    archived = root / 'sources/team/plan/revisions/r1'
    assert 'Site A.' in (archived / 'source.md').read_text()
    assert (archived / 'original.md').is_file()
    assert 'Site B.' in (root / second['path']).read_text()
    [item] = podarcis.status(root, 'stale')
    assert item['stale_in'] == ['wiki/plan.md'] and item['revision'] == 'r2'
    assert podarcis.status(root, 'cited') == [item]
    assert len(podarcis.status(root)) == 1  # archived snapshots are not separate sources

    page.write_text(page.read_text().replace('revision: r1', 'revision: r2'))
    assert podarcis.status(root)[0]['status'] == 'cited'


@pytest.mark.parametrize('kwargs, match', [
    ({'kind': 'Bad Kind'}, 'kebab-case'),
    ({'date': '14/10/2026'}, 'YYYY-MM-DD'),
    ({'origin': {'provider': 'google-drive'}}, 'id or url'),
    ({'origin': {'provider': 'x', 'id': '1', 'revision': '../r'}}, 'revision'),
])
def test_add_validates_metadata(root, tmp_path, kwargs, match):
    notes = tmp_path / 'n.txt'
    notes.write_text('text')
    with pytest.raises(ValueError, match=match):
        podarcis.add(root, notes, 'n', 'team', **kwargs)
    assert list((root / 'sources').iterdir()) == []


def test_revisions_folder_name_is_reserved(root, tmp_path):
    notes = tmp_path / 'n.txt'
    notes.write_text('text')
    with pytest.raises(ValueError, match='reserved'):
        podarcis.add(root, notes, 'n', 'team/revisions')


def test_text_from_another_rendering_and_attachments(root, pptx_file, pdf, tmp_path):
    tab = tmp_path / 'counts.csv'
    tab.write_text('site,n\nA,12\n')
    result = podarcis.add(root, pdf, 'deck', 'team', kind='slides', text_from=pptx_file,
                          attachments=[tab])
    folder = (root / result['path']).parent
    assert sorted(p.name for p in folder.iterdir()) == ['counts.csv', 'original.pdf', 'source.md']
    text = (folder / 'source.md').read_text()
    assert '## Slide 1' in text and '[counts.csv](./counts.csv)' in text
    assert meta(folder / 'source.md')['attachments'] == ['counts.csv']


@pytest.mark.parametrize('name', ['original.csv', 'notes.md', 'Bad Name.csv'])
def test_attachment_names_are_checked(root, pdf, tmp_path, name):
    bad = tmp_path / name
    bad.write_text('x')
    with pytest.raises(ValueError, match='Attachment'):
        podarcis.add(root, pdf, 'x', 'team', attachments=[bad])


def test_reexport_with_the_same_text_is_not_a_new_revision(root, tmp_path):
    origin = {'provider': 'google-drive', 'id': 'doc'}
    first, second, third = (tmp_path / f'{n}.pdf' for n in 'abc')
    import pymupdf

    for path, text, stamp in [(first, 'Same text', '1'), (second, 'Same text', '2'), (third, 'New text', '3')]:
        pdf = pymupdf.open()
        pdf.new_page().insert_text((72, 72), text)
        pdf.set_metadata({'creationDate': f'D:2026010100000{stamp}'})
        pdf.save(path)
    assert first.read_bytes() != second.read_bytes()
    podarcis.add(root, first, 'doc', 'team', origin=origin | {'revision': 'v1'})
    assert podarcis.add(root, second, 'doc', 'team', origin=origin | {'revision': 'v2'}) == {
        'id': 'doc', 'path': 'sources/team/doc/source.md', 'revision': 'v1', 'changed': False,
    }
    assert podarcis.add(root, third, 'doc', 'team', origin=origin | {'revision': 'v3'})['changed'] is True
    assert (root / 'sources/team/doc/revisions/v1/source.md').is_file()


def test_changed_text_from_with_identical_file_is_a_new_revision(root, pdf, docx_file, tmp_path):
    import docx

    origin = {'provider': 'google-drive', 'id': 'doc'}
    podarcis.add(root, pdf, 'doc', 'team', origin=origin | {'revision': 'v1'}, text_from=docx_file)
    edited = tmp_path / 'edited.docx'
    document = docx.Document()
    document.add_paragraph('Site C replaces site B.')
    document.save(edited)
    result = podarcis.add(root, pdf, 'doc', 'team', origin=origin | {'revision': 'v2'}, text_from=edited)
    assert result['changed'] is True
    assert 'Site C' in (root / result['path']).read_text()


def test_sources_can_sit_directly_in_sources(root, tmp_path):
    notes = tmp_path / 'n.md'
    notes.write_text('# Notes\n\nText.\n')
    result = podarcis.add(root, notes, 'top_notes', '')
    assert result['path'] == 'sources/top_notes/source.md'
    assert podarcis.status(root)[0]['domain'] == ''
