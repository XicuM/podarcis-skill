# Vault contract

```text
vault/
├── .agents/
│   ├── podarcis.yaml      # repository URLs, agent targets, MCP servers, extra skills
│   ├── .env               # credentials, private
│   └── skills/
├── sources/<optional/domain/folders>/<source_id>/
│   ├── source.md
│   └── original.pdf
├── wiki/
└── workspace/
```

`sources/`, `wiki/`, and `workspace/` are each a local folder or a separate Git repository; relative links between them work in the vault checkout, not when one repository is browsed alone.

Each source ID is unique across domains. Never replace an existing source. `source.md` is the entry point: YAML metadata followed by extracted paper text. Preserve the original paper's headings, abstract, and tables where extraction supports them. Do not invent an abstract or headings to disguise missing extraction. Generated summaries and interpretations belong in the wiki.

## Source

The ingestion command writes these fields from a complete OpenAlex work record:

```yaml
---
type: source
id: smith_2024_gene_regulation
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
---

A claim supported by the sections actually reviewed.[^smith_2024_gene_regulation]

[^smith_2024_gene_regulation]: [Paper](../sources/biology/genetics/smith_2024_gene_regulation/source.md)
```

A workspace page uses the same structure with `type: deliverable`, and its resources and footnotes point to the relevant wiki pages. Keep evidence provenance visible without implying broader reading or stronger validation than performed.

Use relative Markdown links for internal files, snake_case for new filenames, and one line per paragraph. Images and figures use PNG files and relative links such as `![Figure](./figure.png)`; do not embed SVG or base64 blobs.

## Existing vaults

Older sources contain `metadata.md`, `original.pdf`, and `raw.md`. Status lists these without modifying them, and existing wiki citations remain usable. Read metadata first and relevant ranges of `raw.md` as needed. New imports use the two-file structure. Migration, removal of legacy files, and rewriting existing research links require explicit user authorization.
