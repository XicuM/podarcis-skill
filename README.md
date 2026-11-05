# Podarcis

A portable skill for literature research in a local Markdown vault. OpenAlex supplies discovery and metadata; Podarcis imports papers, tracks wiki citations, and sets up the vault for any agent harness.

```text
vault/
├── podarcis.yaml      # Git URLs, agent targets, MCP servers
├── sources/<optional/domain/folders>/<source_id>/
│   ├── source.md      # YAML metadata followed by extracted full text
│   └── original.pdf
├── wiki/              # Knowledge pages citing local sources
└── workspace/         # Deliverables citing wiki pages
```

Agents follow the [skill](skills/podarcis/SKILL.md) and the [vault contract](skills/podarcis/references/vault.md).

## Set up a vault

Requires git, [uv](https://docs.astral.sh/uv/), and Node.js 22+. In the vault folder:

```bash
npx @sentry/dotagents@3.4.0 --project init --agents claude
npx @sentry/dotagents@3.4.0 --project add XicuM/podarcis-skill podarcis --ref v1.1.0
uv run .agents/skills/podarcis/scripts/vault.py setup
```

The first two commands install the skill; from a checkout, `uv run skills/podarcis/scripts/vault.py --root /path/to/vault setup` does the same. The first setup creates `podarcis.yaml`, the vault folders, and an empty `.env`, and pins the skill to its release tag in `agents.toml`. Edit `podarcis.yaml` (or use a sidebar) and run setup again:

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

Setup clones missing or empty folders (each is its own repository), then generates `agents.toml` and runs [dotagents](https://dotagents.sentry.dev/) to configure each harness. Edit `podarcis.yaml`, not `agents.toml`.

[Recommended MCP servers](skills/podarcis/references/mcp-servers.yaml) and their credentials:

| Server | Credentials |
|---|---|
| `semantic-scholar` | Optional `SEMANTIC_SCHOLAR_API_KEY` in `.env` |
| `google-drive` | `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` in `.env`; OAuth on first use |
| `overleaf-suggest` | SSH access to `gitlab-internal.bsc.es`; one-time `login` command from its setup note |

## Sidebars

To configure a vault from a sidebar instead of `podarcis.yaml`, install [podarcis-obsidian](https://github.com/XicuM/podarcis-obsidian) or [podarcis-vscode](https://github.com/XicuM/podarcis-vscode). They run this skill's `config` and `setup` commands and install the latest release in a vault without the skill.

## Development

```bash
uv run --no-project --with pytest \
  --with-requirements skills/podarcis/scripts/vault.py \
  --with-requirements skills/podarcis/scripts/openalex_cli.py \
  pytest -p no:cacheprovider -o pythonpath=skills/podarcis/scripts tests
```

To release, set `metadata.version` in `SKILL.md`, then push the tag `v<version>`; vault setup installs the skill from that tag. Podarcis is [AGPL-3.0](LICENSE); the adapted Google DeepMind OpenAlex CLI retains Apache-2.0 notices in [NOTICE.md](skills/podarcis/NOTICE.md).
