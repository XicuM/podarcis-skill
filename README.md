# Podarcis

A portable skill and repository-based setup for literature research in a local Markdown vault. OpenAlex supplies discovery and metadata; Podarcis imports papers and tracks wiki citations. Agents use their own file tools to read and write content.

```text
vault/
├── sources/<optional/domain/folders>/<source_id>/
│   ├── source.md      # YAML metadata followed by extracted full text
│   └── original.pdf
├── wiki/             # Knowledge pages citing local sources
└── workspace/        # Deliverables citing wiki pages
```

The [skill](skills/podarcis/SKILL.md) tells agents to read metadata, abstracts, and relevant sections first, then expand their reading as needed. See the [vault contract](skills/podarcis/references/vault.md).

## Reproduce the workspace

The complete setup is declared in this repository. On another machine, clone the same repository revision and run this command from its root:

```bash
make setup
```

Prerequisites: Git, Make, Node.js 22+, [uv](https://docs.astral.sh/uv/), and SSH access to `gitlab-internal.bsc.es:esantigo/overleaf-suggest.git`. Python 3.11+ is required by PyMuPDF 2; uv can supply Python. Playwright may also need operating-system browser libraries on a minimal Linux installation.

Setup installs locked Python and Node dependencies, installs the declared skills with [dotagents](https://github.com/getsentry/dotagents), generates local MCP configurations, creates an empty vault layout, preserves an existing `.env` or creates it from `.env.example`, and installs Overleaf's Chromium runtime. It does not sign into external accounts or change user-wide agent settings. Re-running setup restores the declared dependencies and refreshes managed configuration. An existing conflicting Antigravity config is preserved and causes setup to fail rather than being overwritten.

| Included capability | Implementation |
|---|---|
| Podarcis skill and commands | This repository's `skills/podarcis/` |
| Overleaf skill and MCP | BSC `esantigo/overleaf-suggest`, commit `21d3be3efd86f5686d289ca8df1672920a30b770` |
| Semantic Scholar MCP | [`s2-mcp-server`](https://github.com/smaniches/semantic-scholar-mcp), 1.7.12 |
| Google Drive MCP | [`workspace-mcp`](https://github.com/taylorwilsdon/google_workspace_mcp), 2.2.1, limited to Drive's core tools |

The Python MCP servers require incompatible MCP SDK versions. Their dependency groups are explicitly exclusive, and each server runs in its own isolated environment using `uv.lock`. They can run concurrently without replacing each other's dependencies or the development environment. Their locked packages are downloaded on first launch if not already cached; setup does not prewarm those separate environments.

Launch the chosen agent through Herdr **from this repository root**. Herdr supplies the terminal; the agent reads its normal project configuration. Other repositories do not acquire this setup automatically. To reproduce the set elsewhere, carry the repository's setup files and skill source at the same revision, rather than copying only generated agent folders. Credentials and research data are excluded from version control.

## Files and ownership

| File | Responsibility |
|---|---|
| `agents.toml` | Canonical list of skills, MCP commands, and any plugin declarations |
| `package.json`, `package-lock.json` | Pinned dotagents release, Overleaf Git revision, and Node dependency tree |
| `pyproject.toml`, `uv.lock` | Podarcis release version and Python dependencies, including isolated MCP groups |
| `skills/podarcis/scripts/*.py.lock` | Dependencies of the standalone skill scripts when copied to another repository |
| `Makefile` | The verified restore commands, exposed as `make setup` |
| `.env` | Local credentials and runtime paths; never committed |

`agents.lock` is dotagents' generated installation inventory, not a frozen dependency resolver; it is ignored. Reproducibility comes from the committed source revision, exact refs in `agents.toml`, and the Python and Node lockfiles. The Node override for Overleaf's unused Zod declaration selects patched Zod 4.6.7; its MCP SDK already uses this version.

Generated agent locations:

| Agent | Skills | MCP configuration |
|---|---|---|
| OpenCode | `.agents/skills/` | `.opencode/opencode.jsonc` |
| Claude Code | `.claude/skills/` | `.mcp.json` |
| Antigravity | `.agents/skills/` | `.agents/mcp_config.json` → `../.mcp.json` |
| Codex | `.agents/skills/` | `.codex/config.toml` |

Antigravity's [documented workspace format](https://antigravity.google/docs/mcp) accepts the same stdio entries, so a relative symlink shares the generated configuration without a second list. Its standalone `mcp list` command did not show project entries in our check; native session discovery remains unverified. Restart or refresh agents after setup and approve project trust where the client requires it. Repository scope controls discovery, not filesystem permissions or exclusion of the client's existing global configuration.

## Add skills, MCP servers, or plugins

Edit `agents.toml`, then run:

```bash
npm run agents:sync
npm run agents:check
```

Declare an additional skill with `[[skills]]`; use a local tracked directory or an exact remote commit. For example, replace the placeholders below with an actual source and verified commit:

```toml
[[skills]]
name = "skill-name"
source = "owner/skills-repository"
ref = "FULL_COMMIT_SHA"
```

MCP servers use `[[mcp]]` with `name`, `command` and `args`, or a remote `url`. Install their runtime through a locked dependency before registering its command. Python servers with incompatible dependencies need separate isolated groups like the existing servers. Native commands explicitly load this workspace's `.env`, avoiding differences between client-specific environment interpolation.

Plugins use `[[plugins]]`, with a pinned source and explicit targets:

```toml
[[plugins]]
name = "plugin-name"
source = "owner/plugin-repository"
ref = "FULL_COMMIT_SHA"
path = "plugins/plugin-name"
targets = ["opencode", "claude", "codex"]
```

No additional plugin is enabled by default. Plugin installation was exercised with a disposable local fixture. dotagents projects the components it supports; native OpenCode JavaScript extensions, Claude hooks, and other host-specific features do not automatically become equivalent across agents. Check the plugin's supported targets and the [dotagents reference](https://dotagents.sentry.dev/cli/) before adding it. Antigravity native-plugin installation is not managed by this setup.

After changing dependencies, deliberately update `package-lock.json` or `uv.lock`, rerun setup, review the generated configuration, and commit the declarations and locks together. Setup does not update floating remote skill/plugin refs for you safely: use exact commits for a repeatable set.

## Authentication

Setup preserves `.env` and creates no account connections. Put only this workspace's intended settings there; use [.env.example](.env.example) as the field reference. On a fresh setup it places Drive logs/tokens, the Overleaf session, and Chromium under ignored `.local/`. An existing `.env` is left unchanged, so upstream defaults apply to any paths not configured there. Dependency download caches may remain user-wide; that is separate from installing skills or agent configuration globally.

- **Semantic Scholar:** works without a key against shared access. An optional `SEMANTIC_SCHOLAR_API_KEY` gives the provider-assigned quota.
- **Google Drive:** supply your own `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET`, enable Drive API, and complete the provider's OAuth flow when using the server. See the [upstream setup](https://github.com/taylorwilsdon/google_workspace_mcp#quick-start). The setup validates MCP discovery, not access to your Drive.
- **Overleaf:** run `npm run overleaf:login` and sign in through its browser, or supply `OVERLEAF_SESSION_COOKIE` in `.env`. Setup does not access or edit a paper.
- **OpenAlex:** supply `OPENALEX_API_KEY` through the command's environment if needed. The standalone OpenAlex script does not automatically read `.env`; `uv run --env-file .env <script> ...` explicitly loads this workspace's file. Metadata works keyless within OpenAlex's shared budget; OpenAlex-hosted full text requires a key, while recorded publisher/repository locations may work without one. See [OpenAlex authentication](https://help.openalex.org/api/authentication/).

## Use Podarcis

The skill is self-contained. It can also be copied on its own, including scripts, adjacent lockfiles, references, and notices, into another repository's skill directory. This installs only Podarcis, not the complete workspace bundle.

```bash
podarcis_skill=/absolute/path/to/repository/.agents/skills/podarcis
vault_root=/absolute/path/to/vault

uv run "$podarcis_skill/scripts/openalex_cli.py" filter works \
  --search 'gene regulation' --per-page 5 \
  --select id,display_name,publication_year,doi,type,best_oa_location

uv run "$podarcis_skill/scripts/vault.py" --root "$vault_root" status --status uncited
```

The [skill](skills/podarcis/SKILL.md) contains the full search, download, and import sequence. Import requires complete OpenAlex `get works` JSON and a matching PDF. Check the PDF's opening pages, DOI, title, and version: recorded locations can point to another journal edition or preprint. Extraction does not verify document identity or peer review.

Remote downloads require HTTPS, certificate validation, at most 50 MiB, and a new destination. Failed downloads leave no partial file. Ingestion uses PyMuPDF4LLM with OCR disabled and publishes `source.md` plus `original.pdf` only after extraction succeeds. Source IDs are unique across domains and never replaced. Layout and table recovery depend on the PDF; inspect figures in `original.pdf`. Scanned PDFs need OCR elsewhere.

`status` computes cited/uncited state from wiki footnote definitions, not synthesis quality or peer review. Legacy `metadata.md`/`raw.md` sources remain listed without migration. The skill-based Podarcis starts at version 0.1.0 and replaces the earlier MCP interface; remove obsolete `podarcis-mcp` registrations when switching.

## Development

```bash
uv run --locked --extra dev pytest
```

The package provides `podarcis` (ingest/status) and `podarcis-openalex` (OpenAlex CLI). Tests cover real PDF extraction, installed and standalone commands, selected-vault isolation, duplicate protection, legacy sources, and controlled HTTP failure paths. Workspace validation also exercised actual MCP initialization/tool listing for all three servers, without reading cloud data or authenticating accounts. This does not establish discovery or model behavior inside every supported agent.

See [AGENTS.md](AGENTS.md). Podarcis is [AGPL-3.0](LICENSE); the adapted Google DeepMind OpenAlex CLI retains Apache-2.0 notices in [NOTICE.md](skills/podarcis/NOTICE.md).
