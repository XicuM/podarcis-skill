---
name: podarcis
description: Search OpenAlex, import papers and team files (meeting notes, slides, documents, data) into a Podarcis research vault, synthesize cited wiki pages, and create research-informed deliverables. Use for research work in a vault with sources/, wiki/, and workspace/.
license: AGPL-3.0
metadata:
  version: 2.2.1
---

# Podarcis

Requires shell and file tools, uv, Python 3.11+, and network access for OpenAlex. Local vault operations work offline after dependency installation. Keep each script's adjacent lockfile with the installed skill to reproduce its dependencies.

Knowledge flows **sources → wiki → workspace**. Use your file tools to read and write Markdown; use the bundled commands for OpenAlex access and source ingestion. Read [the vault contract](references/vault.md) when creating or updating vault content. Before and after changing vault content, [sync the vault](#sync-the-vault).

## Read selectively

Do not read every source or entire papers by default. Start with metadata, the abstract when present, and relevant headings. Search within `source.md`, then read the sections needed for the task with enough surrounding context to assess the evidence. Include methods and limitations when they affect a claim. Read the whole paper when the task requires comprehensive understanding or relevant evidence is spread throughout it. Inspect `original.pdf` for figures, tables, equations, or suspected extraction errors. Never imply you reviewed material you did not read.

For example, use `sed -n '1,45p' source.md` for the opening, `rg -n '^#{1,6} |abstract|limitations' source.md` to locate sections, then read the relevant line ranges. The first 45 lines are a starting point, not a guarantee that metadata or an abstract ends there. Execute the scripts without reading their implementation unless troubleshooting them.

## Set up a vault

A vault's settings live in `.agents/podarcis/`. `config.yaml` declares a Git URL under `repos` for each of `sources/`, `wiki/`, and `workspace/` (null for a local folder). `tools.yaml` declares the MCP servers under `mcp`: names from [the recommended servers](references/mcp-servers.yaml) or custom entries with `name` plus `command` and `args`, or `url` and `headers`; and extra skills under `skills` as `name`, `source` (`owner/repo` on GitHub or `git:<url>`), and optional `ref` and `path`. `setup` clones missing folders, repoints changed `origin` remotes, refuses to clone into a folder with files, installs this skill in `.agents/skills/podarcis` and the extra skills beside it, and writes the MCP servers for every agent: `.mcp.json` (with skill links in `.claude/skills`), `.codex/config.toml`, `.opencode/opencode.jsonc`, and `.vscode/mcp.json`. It replaces only the server lists in those files, so declare every server in `tools.yaml`.

```bash
uv run "$podarcis_skill/scripts/podarcis.py" --root "$vault_root" config  # current settings, catalogue, and repository state
uv run "$podarcis_skill/scripts/podarcis.py" --root "$vault_root" setup
```

Setup needs uv, and git when a folder has a URL or an extra skill is declared. Credentials go in the vault's `.agents/podarcis/.env`, which setup creates empty and private; recommended servers read the names listed under their `env`. `config` reports only which credentials are set; never read or print `.agents/podarcis/.env`. Change only the settings the user asked for, and report each server's setup note.

## Sync the vault

`config` reports each of `sources/`, `wiki/`, and `workspace/` as a local folder or a clone with its `origin`. Each clone is its own repository: run git inside it with `git -C "$vault_root/<folder>"`, never in the vault root. Local folders are not synced.

Before reading or changing a clone's content for a request, run `git status --porcelain` in it. Uncommitted changes there belong to the user: never commit, stash, or discard them, and ask before pulling that folder. Otherwise run `git pull --rebase`. If a pull fails, run `git rebase --abort` when a rebase is in progress, report the error, and change nothing in that folder until the user decides.

When the request is done, commit and push each clone you changed, in the order `sources/`, `wiki/`, `workspace/`, so every citation reaches the remote before the page that cites it. In each, `git add` only the paths you created or changed, commit them in one commit with a short summary such as `Add smith_2024_gene_regulation`, then run `git pull --rebase` and `git push`. If a rebase conflicts, abort it; if it or the push fails, report the error and push nothing that depends on that folder. Report each commit and push in your answer.

## Commands

Resolve `podarcis_skill` to this skill's actual installed directory; script paths are relative to this `SKILL.md`, not the vault or current directory. Set `vault_root` to the selected vault. It must already contain `sources/`; run `setup` for a new vault.

```bash
podarcis_skill=/absolute/path/to/installed/podarcis
vault_root=/absolute/path/to/vault

uv run "$podarcis_skill/scripts/openalex_cli.py" filter works \
  --search 'research question' --per-page 5 \
  --select id,display_name,publication_year,doi,type,best_oa_location

uv run "$podarcis_skill/scripts/podarcis.py" --root "$vault_root" status --status uncited
```

For an identified paper, use its actual OpenAlex W-number returned by search. Download metadata and PDF to a fresh temporary directory, then import them:

```bash
paper_tmp=$(mktemp -d)
work_id=W2741809807 # Example only; replace with the selected work's ID.
uv run "$podarcis_skill/scripts/openalex_cli.py" get works "$work_id" > "$paper_tmp/work.json"
uv run "$podarcis_skill/scripts/openalex_cli.py" download-pdf "$work_id" "$paper_tmp/paper.pdf"
uv run "$podarcis_skill/scripts/podarcis.py" --root "$vault_root" \
  ingest "$paper_tmp/work.json" author_year_topic biology/genetics \
  --pdf-path "$paper_tmp/paper.pdf"
```

Run each step only after the preceding command succeeds. Use `get works` without `--select` for ingestion: it needs a complete work record, not search output. You can resolve a DOI with `get works https://doi.org/<doi>`. Before ingesting any downloaded or user-supplied PDF, inspect its opening pages and check the title, DOI when present, and publication version against the metadata. OpenAlex locations can point to another journal edition or preprint. Resolve the matching DOI or report an unresolved mismatch; extraction cannot establish document identity. When no PDF can be downloaded, report the failure and use a PDF supplied by the user if available. Never substitute an unrelated document. Remove only the temporary files you created once import succeeds or they are no longer needed.

Ingestion produces `sources/<domain>/<source_id>/source.md` and `original.pdf`, with `kind: paper`. IDs and domain folders use snake_case; domains can be nested, or `""` to place a source directly in `sources/`. Existing IDs are never replaced, and failed imports leave no partial source. Extraction uses PyMuPDF4LLM with OCR disabled; scanned PDFs need OCR elsewhere. Local ingest and status commands return JSON on stdout and errors on stderr.

`OPENALEX_API_KEY` is optional for metadata requests and required for OpenAlex-hosted full text; without it, the downloader tries the work's recorded PDF locations. Set credentials through the process environment; do not put keys in prompts, command arguments, committed files, or output. Do not automatically load unrelated `.env` files. API budgets and endpoint availability are controlled by OpenAlex; surface exhaustion and paid-plan restrictions rather than repeatedly retrying them.

For advanced OpenAlex queries, run `openalex_cli.py --help` and the relevant subcommand's help. `resolve` finds entity IDs before ID-based filters; `filter` supports filtering, sorting, grouping, and paging. Keep discovery results small with `--select` and `--per-page`. Redirect large results to a temporary JSON file and inspect only the fields needed. Do not fabricate IDs, bibliographic fields, evidence, or publication status.

## Other sources

Sources are not only papers: meeting notes, slides, reports, spreadsheets, and media are imported with `add`, which keeps the file unchanged as `original.<ext>` and extracts citable text: `## Page N` for PDFs, `## Slide N` with speaker notes for presentations, the document's own headings for `.docx`, `## Sheet <name>` for workbooks, and a metadata-only entry for images, audio, video, and unknown formats. Export Google Docs, Slides, and Sheets to `.docx`, `.pptx`, and `.xlsx` first.

```bash
uv run "$podarcis_skill/scripts/podarcis.py" --root "$vault_root" \
  add notes.docx lab_meeting_2026_10_14 team/meetings --kind meeting-notes --date 2026-10-14
```

`--kind` defaults from the extension (`document`, `slides`, `dataset`, `media`, `other`); use `paper`, `meeting-notes`, `report`, `protocol`, or any other kebab-case kind that describes the file. `--title` defaults to the file name and `--date` is the date the content refers to. Authors are omitted unless given with `--author`; do not record meeting participants by default. `--origin` takes a JSON file with `provider`, an `id` or `url` identifying the external file, and optionally `revision` and `path`; only then does adding the same source ID again record a new revision, moving the previous snapshot unchanged to `revisions/<revision>/`. Without an origin an existing ID is never replaced. Never edit files under `sources/`, and treat folders mounted read-only (such as a team library) as evidence you can cite but not change.

Treat a non-paper source as evidence of what the document says, not as validated findings: meeting notes record decisions and opinions, slides summarize. Name the kind when it matters, for example "the 14 October lab meeting decided…".

`status --status stale` lists cited sources whose current revision differs from the revision a wiki page recorded, with the pages in `stale_in`. Read the archived snapshot and the current one, then update or confirm each affected claim and its recorded revision.

## Synthesize and deliver

Read the relevant full-text evidence and check publication status before making scientific claims. OpenAlex results may include preprints; `publication_type: article` does not prove peer review. Record uncertainty and extraction limitations where they matter.

Wiki pages cite local `source.md` files, with a heading anchor (`#page-4`, `#slide-7`, `#decisions`) when the claim rests on one part, and record the cited `revision` for versioned sources. Workspace deliverables cite wiki pages. Store source text as evidence and put your synthesis in the wiki. Do not place personal identifiers or health information in knowledge pages, source metadata, or external queries.

`status` means cited, uncited, or stale according to wiki footnote definitions and recorded revisions; it does not establish source validity, synthesis quality, or peer review. Legacy three-file sources remain readable and listed. Do not migrate existing research data or rewrite its links without user authorization.
