# Working on Podarcis

Podarcis is a portable research-vault skill with standalone OpenAlex and local ingestion commands. Agents read and write Markdown with their own file tools. Keep implementation and documentation limited to working behavior. Do not add a backend, provider, dependency, abstraction, or agent role for a hypothetical future need.

## Project layout

- `skills/podarcis/SKILL.md`: portable agent instructions, including selective reading.
- `skills/podarcis/references/vault.md`: source, wiki, and workspace contract.
- `skills/podarcis/scripts/openalex_cli.py`: adapted upstream OpenAlex CLI; provenance and modifications are recorded in `skills/podarcis/NOTICE.md`.
- `skills/podarcis/scripts/vault.py`: local ingestion and citation status; CLI selects one vault with `--root` or the current directory.
- `tests/`: behavioral tests, including command invocation, real PDF extraction, and controlled HTTP failures.
- `agents.toml`: repository-scoped skill, MCP, and plugin declarations managed by dotagents. `Makefile` restores the manually verified setup; Node and Python lockfiles pin its dependencies.

## Vault contract

Follow [the vault contract](skills/podarcis/references/vault.md) for research content. New sources contain `source.md` (YAML metadata plus extracted full text) and `original.pdf`. Wiki pages cite local sources; workspace deliverables cite wiki pages. Read metadata and relevant sections first; do not load entire papers or every source unless the task requires it. Scientific claims require examining relevant full-text evidence and checking publication status, including methods and limitations where they affect the claim. Never imply broader review than performed.

Keep personal identifiers and health information out of knowledge pages, source metadata, and external queries. IDs are unique across domains and existing sources are never overwritten. Citation status means cited or uncited in wiki footnote definitions, not verified, correctly synthesized, or peer reviewed. Legacy three-file sources are listed without modification; do not migrate research data or rewrite existing links without explicit authorization.

## Engineering

1. Inspect the code and existing changes before editing. Prefer deleting dead paths and duplicated declarations over adding machinery.
2. Keep operations direct. Do not introduce wrappers, classes, helpers, or module splits without a concrete need and user agreement.
3. Implement and verify the manual path before adding automation. Surface errors; do not silently substitute a different provider or downgrade a requested operation.
4. Run the fastest relevant tests first, then the complete suite for changes spanning the core workflow. Tests must cover observable behavior and failure paths.
5. Keep Python dependencies and the release version in `pyproject.toml`; keep Node workspace dependencies in `package.json`. Commit the corresponding lockfiles. Standalone scripts also declare runtime dependencies through PEP 723 so installed skills run without a separate package installation; keep those requirements aligned. Bump the version for platform commits; use a major bump for incompatible tool or vault changes.
6. Preserve unrelated local edits and research data. Do not migrate, delete, commit, or push user data without authorization.
7. Check `git status -sb` and fetch refs at the start when available. At completion, state validation results and whether changes are committed or pushed; never manufacture a clean tree by discarding work.
8. Keep the skill self-contained: an installed copy must work without this checkout or client-specific environment variables. Include its script lockfiles and test copied scripts outside the repository after packaging changes. Use the existing dotagents project installer for workspace declarations rather than introducing a Podarcis package manager. Preserve user-wide agent settings and local credentials. Keep incompatible MCP dependency groups isolated; do not combine them in the development environment.

Development and installation are in [README.md](README.md).
