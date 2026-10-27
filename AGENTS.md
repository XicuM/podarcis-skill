# Working on Podarcis

Podarcis is an MCP server with three literature tools for a local research vault. Agents read and write vault Markdown with their own file tools. Keep the implementation and documentation limited to working behavior. Do not add a backend, provider, dependency, abstraction, or agent role for a hypothetical future need.

## Project layout

- `src/podarcis/server.py`: CLI and MCP server. One server instance uses one vault, selected by `--root` or the current directory.
- `src/podarcis/tools/literature.py`: OpenAlex search, PDF ingestion, and citation status.
- `tests/`: Behavioral tests, including a real MCP stdio session and real PDF extraction.

## Vault contract

- Each source is a folder named by its ID anywhere under `sources/`, optionally inside nested domain folders. It contains `metadata.md`, `original.pdf`, and nonempty extracted `raw.md`. IDs are unique across domains. Existing sources are never overwritten.
- `wiki/` contains anonymous knowledge pages citing local source metadata. Do not place personal identifiers or health information in knowledge pages, source metadata, or external search queries.
- `workspace/` contains projects and deliverables, including slides, papers, reports, profiles, and protocols. Research claims cite wiki pages rather than bypassing them to cite raw evidence.
- Content pages use YAML fields `type`, `title`, `status`, and `sources`. Wiki pages use `type: concept`; workspace pages use `type: deliverable`. Each source entry has `id` and a relative `resource` path. Named footnote references and definitions match these IDs. `_index.md` is exempt from frontmatter requirements.
- Use relative Markdown links for internal files. Use snake_case for new content filenames. Write each paragraph on one line.
- For images and figures, use PNG format files with relative markdown links (e.g., `![label](./figure.png)`). Do not embed raw SVG XML or massive base64 data URIs directly inside Markdown documents, and avoid SVG files as they are unsafe in Obsidian and bloat context for agents.
- Never fabricate evidence. Read full text and verify publication status before making scientific claims; OpenAlex search results can include preprints.
- Source status means cited or uncited in the wiki; it does not mean verified, synthesized correctly, or peer reviewed.

## Engineering

1. Inspect the code and existing changes before editing. Prefer deleting dead paths and duplicated declarations over adding machinery.
2. Keep operations direct. Do not introduce wrappers, classes, helpers, or module splits without a concrete need and user agreement.
3. Implement and verify the manual path before adding automation. Surface errors; do not silently substitute a different provider or downgrade a requested operation.
4. Run the fastest relevant tests first, then the complete suite for changes spanning the core workflow. Tests must cover observable behavior and failure paths.
5. Keep dependencies and the release version in `pyproject.toml`. Bump the version for platform commits; use a major bump for incompatible tool or vault changes.
6. Preserve unrelated local edits and research data. Do not migrate, delete, commit, or push user data without authorization.
7. Check `git status -sb` and fetch refs at the start when available. At completion, state validation results and whether changes are committed or pushed; never manufacture a clean tree by discarding work.

Development setup is in [README.md](README.md).
