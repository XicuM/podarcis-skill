# Podarcis

A portable skill for literature research in a local Markdown vault. OpenAlex supplies discovery and metadata; Podarcis imports papers, tracks wiki citations, and sets up the vault for any agent harness. Agents use their own file tools to read and write content.

```text
vault/
├── podarcis.yaml      # Git URLs, agent targets, MCP servers
├── sources/<optional/domain/folders>/<source_id>/
│   ├── source.md      # YAML metadata followed by extracted full text
│   └── original.pdf
├── wiki/              # Knowledge pages citing local sources
└── workspace/         # Deliverables citing wiki pages
```

The [skill](skills/podarcis/SKILL.md) tells agents to read metadata, abstracts, and relevant sections first, then expand their reading as needed. See the [vault contract](skills/podarcis/references/vault.md).

## Set up a vault

Requires git, [uv](https://docs.astral.sh/uv/), and Node.js 22+ (for `npx`). From a checkout of this repository:

```bash
uv run skills/podarcis/scripts/vault.py --root /path/to/vault setup
```

The first run creates `podarcis.yaml` with defaults, empty `sources/`, `wiki/`, and `workspace/` folders, and an empty private `.env`. It copies the skill to `.agents/skills/podarcis`, so later runs can use the vault's own copy. Edit `podarcis.yaml`, or use a panel, and run setup again:

```yaml
agents: [claude, codex, opencode, vscode]   # dotagents targets
repos:
  sources: git@github.com:you/podarcis-sources.git
  wiki: git@github.com:you/podarcis-wiki.git
  workspace: null                           # a local folder without Git
mcp:
  - semantic-scholar                        # from references/mcp-servers.yaml
  - name: my-server                         # any dotagents [[mcp]] entry
    url: https://example.org/mcp
skills: []                                  # extra dotagents [[skills]] entries
```

Setup clones folders that are missing or empty, repoints `origin` when a URL changes, and refuses to clone into a folder that already has files. Each folder is its own repository: commit and push inside it. It then writes `agents.toml` and runs [dotagents](https://dotagents.sentry.dev/) 3.4.0, which writes each harness's configuration:

| Target | Skills | MCP configuration |
|---|---|---|
| `claude` | `.claude/skills` → `.agents/skills` | `.mcp.json` |
| `codex` | `.agents/skills` | `.codex/config.toml` |
| `opencode` | `.agents/skills` | `.opencode/opencode.jsonc` |
| `vscode` (Copilot Chat) | `.agents/skills` | `.vscode/mcp.json` |

dotagents also supports `copilot`, `cursor`, and `pi`. Harnesses outside its list, such as Gemini CLI and Antigravity, need their MCP configuration added by hand. `agents.toml` is generated; edit `podarcis.yaml` instead. If the vault root is inside another Git repository, dotagents uses that repository's root.

### Recommended MCP servers

[`references/mcp-servers.yaml`](skills/podarcis/references/mcp-servers.yaml) lists the recommended servers. Each entry is a dotagents MCP entry plus a description, setup note, and any skill it needs. To recommend another server, add an entry there.

| Server | Credentials |
|---|---|
| `semantic-scholar` | Optional `SEMANTIC_SCHOLAR_API_KEY` in `.env` |
| `google-drive` | `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` in `.env`; OAuth on first use |
| `overleaf-suggest` | SSH access to `gitlab-internal.bsc.es`; one-time `login` command from its setup note |

Python servers run in their own pinned environment (`uv run --no-project --with <package>==<version>`), downloaded on first launch. Overleaf runs from its pinned Git commit through `npx`. Startup was checked for Semantic Scholar (MCP `initialize`) and Overleaf (`--help`); Drive requires your OAuth client.

## Panels

`panels/` contains an Obsidian plugin and a VS Code extension with the same form: repository URLs and state, agent targets, and recommended servers. **Apply** saves `podarcis.yaml` and runs setup; the output appears in the panel. Custom servers and skills stay in `podarcis.yaml`. Both panels run `uv run <skill>/scripts/vault.py`, so they need a local vault (Obsidian desktop, a local or remote-workspace VS Code). Their settings hold the uv command and the skill path, which defaults to `.agents/skills/podarcis`. Before the first setup, point the skill path at this checkout's `skills/podarcis`. Apps started from a desktop launcher may not see your shell's `PATH`; set an absolute uv path if needed.

```bash
cd panels
npm ci
npm run build          # obsidian/main.js, vscode/dist/
npm test
npm run package:vscode # podarcis-vscode.vsix
```

- **Obsidian:** copy `panels/obsidian/{main.js,manifest.json,styles.css}` to `<vault>/.obsidian/plugins/podarcis/` and enable Podarcis. Open it from the ribbon or **Podarcis: Open setup panel**.
- **VS Code:** `code --install-extension panels/podarcis-vscode.vsix`. The panel is in the activity bar.

## Use Podarcis

```bash
podarcis_skill=/path/to/vault/.agents/skills/podarcis
vault_root=/path/to/vault

uv run "$podarcis_skill/scripts/openalex_cli.py" filter works \
  --search 'gene regulation' --per-page 5 \
  --select id,display_name,publication_year,doi,type,best_oa_location

uv run "$podarcis_skill/scripts/vault.py" --root "$vault_root" status --status uncited
```

The [skill](skills/podarcis/SKILL.md) contains the full search, download, and import sequence. Import requires complete OpenAlex `get works` JSON and a matching PDF. Check the PDF's opening pages, DOI, title, and version: recorded locations can point to another journal edition or preprint. Extraction does not verify document identity or peer review.

Remote downloads require HTTPS, certificate validation, at most 50 MiB, and a new destination. Failed downloads leave no partial file. Ingestion uses PyMuPDF4LLM with OCR disabled and publishes `source.md` plus `original.pdf` only after extraction succeeds. Source IDs are unique across domains and never replaced. Layout and table recovery depend on the PDF; inspect figures in `original.pdf`. Scanned PDFs need OCR elsewhere.

`OPENALEX_API_KEY` is optional for metadata and required for OpenAlex-hosted full text; pass it through the command's environment (`uv run --env-file .env ...`). See [OpenAlex authentication](https://help.openalex.org/api/authentication/).

`status` computes cited/uncited state from wiki footnote definitions, not synthesis quality or peer review. Legacy `metadata.md`/`raw.md` sources remain listed without migration.

## Development

```bash
uv run --no-project --with pytest \
  --with-requirements skills/podarcis/scripts/vault.py \
  --with-requirements skills/podarcis/scripts/openalex_cli.py \
  pytest -p no:cacheprovider -o pythonpath=skills/podarcis/scripts tests
cd panels && npm ci && npm run build && npm test
```

Panel tests run TypeScript directly and need Node.js 22.18+.

Tests cover setup with real Git repositories and a recorded dotagents call, real PDF extraction, standalone commands, selected-vault isolation, duplicate protection, legacy sources, controlled HTTP failure paths, and the panels' shared form and script runner.

Tests take their dependencies from the scripts' PEP 723 headers, the same declarations an installed skill uses. The version is `metadata.version` in `SKILL.md`.

Version 1.0.0 replaces the 0.1.0 workspace setup (Makefile, root `agents.toml`, `pyproject.toml`, Node workspace, MCP dependency groups) and removes the installed `podarcis` and `podarcis-openalex` commands; run the scripts with `uv run`. Remove obsolete `podarcis-mcp` registrations when switching from the MCP server.

See [AGENTS.md](AGENTS.md). Podarcis is [AGPL-3.0](LICENSE); the adapted Google DeepMind OpenAlex CLI retains Apache-2.0 notices in [NOTICE.md](skills/podarcis/NOTICE.md).
