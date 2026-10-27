"""OpenAlex search and local paper ingestion served over MCP stdio."""

import argparse
from pathlib import Path

from mcp.server.fastmcp import FastMCP

from podarcis.tools import literature


def main() -> None:
    """Serve the literature tools for one vault over stdio."""
    parser = argparse.ArgumentParser(prog='podarcis-mcp')
    parser.add_argument('--root', default='.', help='Vault path; defaults to cwd')
    root = Path(parser.parse_args().root).expanduser().resolve()
    if not (root / 'sources').is_dir():
        parser.error(f'No sources/ directory in {root}.')
    literature.ROOT = root
    server = FastMCP('podarcis')
    for tool in (
        literature.literature_search,
        literature.literature_download,
        literature.literature_status,
    ):
        server.add_tool(tool)
    server.run(transport='stdio')


if __name__ == '__main__':
    main()
