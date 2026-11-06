# Vault contract

```text
vault/
├── .agents/
│   ├── podarcis.yaml      # repository URLs, agent targets, MCP servers, extra skills
│   ├── .env               # credentials, private
│   └── skills/
├── sources/<optional/domain/folders>/<source_id>/
│   ├── source.md
│   ├── original.<ext>     # pdf, docx, pptx, xlsx, png, ...
│   └── revisions/<revision>/   # earlier snapshots of versioned sources
│       ├── source.md
│       └── original.<ext>
├── wiki/
└── workspace/
```

`sources/`, `wiki/`, and `workspace/` are each a local folder or a separate Git repository; relative links between them work in the vault checkout, not when one repository is browsed alone.

Each source ID is unique across domains. Sources are read-only snapshots: never edit or replace one. `revisions` is reserved and cannot be a domain folder. `source.md` is the entry point: YAML metadata followed by extracted paper text. Preserve the original paper's headings, abstract, and tables where extraction supports them. Do not invent an abstract or headings to disguise missing extraction. Generated summaries and interpretations belong in the wiki.

## Source

Every source has `type: source`, `id`, `kind`, and `title`. Legacy sources without `kind` are papers.

### Papers

The ingestion command writes these fields from a complete OpenAlex work record:

```yaml
---
type: source
id: smith_2024_gene_regulation
kind: paper
title: "Paper title"
authors:
  - "Jane Smith"
year: 2024
doi: null
openalex_id: W123
publication_type: article
---
```

These values illustrate the schema, not an actual publication. After the frontmatter come the title, relative link to `original.pdf`, OpenAlex link, DOI link when present, and `## Full text` with the extracted paper. Unknown DOI or year values remain null; do not infer missing bibliographic fields. Citation status is computed from wiki references and is not stored in source metadata.

### Other files

`add` writes these fields; `date`, `authors`, and `origin` appear only when known:

```yaml
---
type: source
id: lab_meeting_2026_10_14
kind: meeting-notes            # document, slides, dataset, media, other, or a custom kebab-case kind
title: "Lab meeting - field season planning"
date: 2026-10-14
revision: r42                  # origin revision, or the first 12 hex digits of sha256
sha256: 9f2c...
original: original.docx
origin:                        # only for files from an external provider
  provider: google-drive
  id: 1AbC...
  url: https://docs.google.com/document/d/1AbC...
  path: Meetings/2026/
  revision: r42
  retrieved: 2026-11-06
---
```

After the frontmatter come the title, a link to the original, a link to the live file when `origin.url` exists, and the extracted text with stable anchors: `## Page N`, `## Slide N` (title in bold, then text, tables, and `Speaker notes:`), `## Sheet <name>` (the first 200 rows), or the document's own headings one level below the title. Files without extractable text say so; cite only what the original shows.

A source with an `origin` is versioned. Importing changed content for the same provider and file moves the previous `source.md` and original unchanged into `revisions/<revision>/` and stores the new snapshot in place. Wiki pages record the revision they read, so `status` can report them as stale.

The PDF is the original evidence for checking figures, tables, equations, and extraction errors. Extraction is not a guarantee of faithful layout, complete tables, or OCR coverage. Read selectively using the main skill's guidance.

## Wiki and workspace

`wiki/` contains anonymous knowledge pages with `type: concept`. `workspace/` contains projects and deliverables, including slides, papers, reports, profiles, and protocols, with `type: deliverable`. Research claims in deliverables cite wiki pages. `_index.md` is exempt from frontmatter requirements.

Wiki and workspace pages require `type`, `title`, `status`, and `sources` in YAML. Each `sources` entry has an `id` and a `resource` path relative to the page. Named footnote references and definitions match those IDs. For example, `wiki/gene_regulation.md` can contain:

```markdown
---
type: concept
title: Gene regulation
status: draft
sources:
  - id: smith_2024_gene_regulation
    resource: ../sources/biology/genetics/smith_2024_gene_regulation/source.md
  - id: lab_meeting_2026_10_14
    resource: ../sources/team/meetings/lab_meeting_2026_10_14/source.md
    revision: r42
---

A claim supported by the sections actually reviewed.[^smith_2024_gene_regulation]

The team chose site B for the 2027 season.[^lab_meeting_2026_10_14]

[^smith_2024_gene_regulation]: [Paper](../sources/biology/genetics/smith_2024_gene_regulation/source.md)
[^lab_meeting_2026_10_14]: [Lab meeting 2026-10-14, Decisions](../sources/team/meetings/lab_meeting_2026_10_14/source.md#decisions)
```

Add `revision` to each `sources` entry for a versioned source, using the `revision` of the snapshot you read. A footnote may link a heading anchor for the part that supports the claim. When the source later changes, `status` reports it as stale; recheck the claim against `revisions/<revision>/source.md` and the current snapshot, then update the claim, link, and recorded revision together.

A workspace page uses the same structure with `type: deliverable`, and its resources and footnotes point to the relevant wiki pages. Keep evidence provenance visible without implying broader reading or stronger validation than performed.

Use relative Markdown links for internal files, snake_case for new filenames, and one line per paragraph. Images and figures use PNG files and relative links such as `![Figure](./figure.png)`; do not embed SVG or base64 blobs.

## Existing vaults

Older sources contain `metadata.md`, `original.pdf`, and `raw.md`. Status lists these without modifying them, and existing wiki citations remain usable. Read metadata first and relevant ranges of `raw.md` as needed. New imports use the two-file structure. Migration, removal of legacy files, and rewriting existing research links require explicit user authorization.
