.PHONY: setup

setup:
	uv sync --locked
	npm ci
	npm run agents:sync
	test -f .env || cp .env.example .env
	mkdir -p .local/overleaf sources wiki workspace
	test "$$(readlink .agents/mcp_config.json)" = ../.mcp.json || ln -s ../.mcp.json .agents/mcp_config.json
	node --env-file=.env node_modules/playwright/cli.js install chromium
