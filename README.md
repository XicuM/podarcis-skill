# Podarcis

An MCP server that finds papers on OpenAlex and stores them as local sources in a research vault. Your agent reads, searches, and writes the vault's Markdown with its own file tools; Podarcis only does what those tools cannot.

```text
vault/
├── sources/<any/domain/folders>/<source_id>/
│   ├── metadata.md
│   ├── original.pdf
│   └── raw.md
├── wiki/
└── workspace/
```

Knowledge flows sources → wiki → workspace: wiki pages cite sources, and workspace deliverables cite wiki pages.

## Install

Requires Python 3.10 or newer and [uv](https://docs.astral.sh/uv/).

```bash
git clone https://github.com/XicuM/Podarcis.git
cd Podarcis
uv venv
uv pip install -e .
```

The vault only needs a `sources/` directory; the server refuses to start without one.

## Connect an MCP client

For clients using the `mcpServers` format:

```json
{
  "mcpServers": {
    "podarcis": {
      "command": "/absolute/path/to/Podarcis/.venv/bin/podarcis-mcp",
      "args": ["--root", "/absolute/path/to/vault"]
    }
  }
}
```

Without `--root`, the server uses the current directory. In Claude Code: `claude mcp add podarcis --scope user -- /absolute/path/to/Podarcis/.venv/bin/podarcis-mcp --root /absolute/path/to/vault`.

## Tools

| Tool | Behavior |
|---|---|
| `literature_search(query, limit=5)` | Search OpenAlex for paper candidates. Errors are surfaced rather than treated as empty results. |
| `literature_download(paper_id, source_id, domain, pdf_path=None)` | Resolve OpenAlex metadata, obtain a PDF, and extract full text into `sources/<domain>/<source_id>/`; `domain` may be nested, such as `biology/genetics`. |
| `literature_status(status="all")` | List complete sources as `cited` or `uncited` by wiki footnote definitions. Uncited sources are the ones still waiting to be synthesized. |

OpenAlex IDs (`W…`, `openalex:W…`, or work URLs) and DOIs are accepted for downloads. Supply an absolute `pdf_path` for a PDF you already have when no accessible download exists; metadata still comes from OpenAlex. Remote downloads use HTTPS and are limited to 50 MiB. Text extraction uses PDFMiner; it does not reconstruct tables or images, and scanned PDFs need OCR elsewhere. A failed import leaves no partial source, and source IDs are unique across `sources/` and never overwritten.

Set `OPENALEX_API_KEY` in the server environment if you have one; without it, requests share OpenAlex's per-IP daily budget. See its [authentication documentation](https://help.openalex.org/api/authentication/).

Search results are discovery material, not proof of peer review or findings. Read the full text before citing it.

## Development

```bash
uv pip install -e '.[dev]'
.venv/bin/pytest
```

Tests cover real PDF extraction, controlled HTTP responses, failure cleanup, and the stdio MCP session. They do not require a live API key. See [AGENTS.md](AGENTS.md) for contribution instructions.

Licensed under [MIT](LICENSE).
