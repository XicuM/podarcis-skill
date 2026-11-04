import assert from 'node:assert/strict';
import { test } from 'node:test';
import { nextConfig } from '../shared/form.ts';

test('form choices replace repos, agents, and recommended servers but keep custom entries', () => {
  const config = {
    agents: ['claude'],
    repos: { sources: 'git@host:s.git', wiki: null, workspace: null },
    mcp: ['semantic-scholar', { name: 'custom', url: 'https://example.org/mcp' }],
    skills: [{ name: 'extra', source: 'owner/repo' }],
  };
  assert.deepEqual(nextConfig(config, { sources: '', wiki: ' git@host:w.git ', workspace: '' }, ['claude', 'vscode'], ['google-drive']), {
    agents: ['claude', 'vscode'],
    repos: { sources: null, wiki: 'git@host:w.git', workspace: null },
    mcp: ['google-drive', { name: 'custom', url: 'https://example.org/mcp' }],
    skills: [{ name: 'extra', source: 'owner/repo' }],
  });
});
