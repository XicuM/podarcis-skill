# Podarcis

A portable skill for literature research in a local Markdown vault. OpenAlex supplies discovery and metadata; Podarcis imports papers, tracks wiki citations, and sets up the vault for any agent harness.

```text
vault/
├── .agents/           # podarcis/ (config.yaml, tools.yaml, .env) and installed skills
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
uv run https://raw.githubusercontent.com/XicuM/podarcis-skill/v2.2.0/skills/podarcis/scripts/podarcis.py setup
```

This installs the skill in `.agents/skills/podarcis` and creates the vault folders and three files in `.agents/podarcis/`. Use a [sidebar](#sidebars), or edit the files and run setup again with `uv run .agents/skills/podarcis/scripts/podarcis.py setup`.

`config.yaml` says where each vault folder syncs to:

```yaml
repos:
  sources: git@github.com:you/podarcis-sources.git
  wiki: git@github.com:you/podarcis-wiki.git
  workspace: null                           # a local folder without Git
```

`tools.yaml` lists the tools every agent gets:

```yaml
mcp:
  - semantic-scholar                        # from references/mcp-servers.yaml
  - name: my-server                         # or command and args
    url: https://example.org/mcp
skills: []                                  # extra skills: name, source (owner/repo or git:<url>), ref, path
```

`.env` holds credentials and stays private.

Setup clones missing or empty folders (each is its own repository), installs the skills, and writes the MCP servers for every agent (Claude Code, Codex, OpenCode, and VS Code) to `.mcp.json`, `.codex/config.toml`, `.opencode/opencode.jsonc`, and `.vscode/mcp.json`, keeping their other settings. Agents pull each repository before changing vault content and commit and push their own changes afterwards, `sources/` first so wiki citations resolve. To upgrade, run the `setup` command above with a newer release tag, or press **Update** in a sidebar. From 2.2.0, setup splits an older `podarcis.yaml` (in `.agents/` or the root) into `config.yaml` and `tools.yaml`, moves `.env` into `.agents/podarcis/`, ignores the old `agents` list, and drops the retired `google-drive` server. Vaults set up with 1.x no longer use `agents.toml` and `agents.lock`; delete them.

[Recommended MCP servers](skills/podarcis/references/mcp-servers.yaml) and their credentials, kept in `.agents/podarcis/.env`:

| Server | Credentials |
|---|---|
| `semantic-scholar` | Optional `SEMANTIC_SCHOLAR_API_KEY` |
| `overleaf-suggest` | Node.js 22+, SSH access to `gitlab-internal.bsc.es`, and a one-time `login` command from its setup note |

## Sidebars

To configure a vault from a sidebar, including credentials, custom servers, and extra skills, install [podarcis-obsidian](https://github.com/XicuM/podarcis-obsidian) or [podarcis-vscode](https://github.com/XicuM/podarcis-vscode). They run this skill's `config` and `setup` commands, and in a vault without the skill run them from the latest release, which installs it.

## Development

```bash
uv run --no-project --with pytest \
  --with-requirements skills/podarcis/scripts/podarcis.py \
  --with-requirements skills/podarcis/scripts/openalex_cli.py \
  pytest -p no:cacheprovider -o pythonpath=skills/podarcis/scripts tests
```

To release, set the same version in `metadata.version` in `SKILL.md` and `VERSION` in `podarcis.py`, then push the tag `v<version>`; running the script from a URL installs the skill from that tag. Podarcis is [AGPL-3.0](LICENSE); the adapted Google DeepMind OpenAlex CLI retains Apache-2.0 notices in [NOTICE.md](skills/podarcis/NOTICE.md).
