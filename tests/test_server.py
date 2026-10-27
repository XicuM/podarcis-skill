"""Exercise the actual MCP stdio transport from outside the selected vault."""

import asyncio
import subprocess
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


def test_stdio_serves_only_the_selected_vault(root, tmp_path):
    source = root / 'sources/testing/test_paper'
    source.mkdir(parents=True)
    for name in ('metadata.md', 'original.pdf', 'raw.md'):
        (source / name).write_text('content\n')
    other = tmp_path / 'other'
    (other / 'sources').mkdir(parents=True)

    async def run():
        for vault, count in ((root, 1), (other, 0)):
            params = StdioServerParameters(
                command=sys.executable,
                args=['-m', 'podarcis.server', '--root', str(vault)],
                cwd=str(other),
            )
            async with (
                stdio_client(params) as (read, write),
                ClientSession(read, write) as session,
            ):
                await session.initialize()
                tools = await session.list_tools()
                assert {tool.name for tool in tools.tools} == {
                    'literature_search',
                    'literature_download',
                    'literature_status',
                }
                result = await session.call_tool('literature_status', {})
                assert not result.isError
                assert len(result.structuredContent['result']) == count

    asyncio.run(asyncio.wait_for(run(), timeout=20))


def test_missing_sources_directory_is_rejected(tmp_path):
    result = subprocess.run(
        [sys.executable, '-m', 'podarcis.server', '--root', str(tmp_path)],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 2
    assert 'No sources/ directory' in result.stderr
