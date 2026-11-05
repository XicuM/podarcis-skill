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

Requires [uv](https://docs.astral.sh/uv/), plus git to sync folders with Git repositories. In the vault folder:

```bash
uv run https://raw.githubusercontent.com/XicuM/podarcis-skill/v2.0.0/skills/podarcis/scripts/podarcis.py setup
```

This installs the skill in `.agents/skills/podarcis` and creates `podarcis.yaml`, the vault folders, and an empty `.env`. Edit `podarcis.yaml` (or use a sidebar) and run setup again with `uv run .agents/skills/podarcis/scripts/podarcis.py setup`:

```yaml
agents: [claude, codex, opencode, vscode]   # agents to configure
repos:
  sources: git@github.com:you/podarcis-sources.git
  wiki: git@github.com:you/podarcis-wiki.git
  workspace: null                           # a local folder without Git
mcp:
  - semantic-scholar                        # from references/mcp-servers.yaml
  - name: my-server                         # or command and args
    url: https://example.org/mcp
skills: []                                  # extra skills: name, source (owner/repo or git:<url>), ref, path
```

Setup clones missing or empty folders (each is its own repository), installs the skills, and writes each agent's MCP servers to `.mcp.json`, `.codex/config.toml`, `.opencode/opencode.jsonc`, and `.vscode/mcp.json`, keeping their other settings. To upgrade, run the `setup` command above with a newer release tag. Vaults set up with 1.x no longer use `agents.toml` and `agents.lock`; delete them.

[Recommended MCP servers](skills/podarcis/references/mcp-servers.yaml) and their credentials:

| Server | Credentials |
|---|---|
| `semantic-scholar` | Optional `SEMANTIC_SCHOLAR_API_KEY` in `.env` |
| `google-drive` | `GOOGLE_OAUTH_CLIENT_ID` and `GOOGLE_OAUTH_CLIENT_SECRET` in `.env`; OAuth on first use |
| `overleaf-suggest` | Node.js 22+, SSH access to `gitlab-internal.bsc.es`, and a one-time `login` command from its setup note |

## Sidebars

To configure a vault from a sidebar instead of `podarcis.yaml`, install [podarcis-obsidian](https://github.com/XicuM/podarcis-obsidian) or [podarcis-vscode](https://github.com/XicuM/podarcis-vscode). They run this skill's `config` and `setup` commands, and in a vault without the skill run them from the latest release, which installs it.

## Development

```bash
uv run --no-project --with pytest \
  --with-requirements skills/podarcis/scripts/podarcis.py \
  --with-requirements skills/podarcis/scripts/openalex_cli.py \
  pytest -p no:cacheprovider -o pythonpath=skills/podarcis/scripts tests
```

To release, set the same version in `metadata.version` in `SKILL.md` and `VERSION` in `podarcis.py`, then push the tag `v<version>`; running the script from a URL installs the skill from that tag. Podarcis is [AGPL-3.0](LICENSE); the adapted Google DeepMind OpenAlex CLI retains Apache-2.0 notices in [NOTICE.md](skills/podarcis/NOTICE.md).
