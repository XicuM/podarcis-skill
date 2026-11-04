import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { runVault } from '../shared/run.ts';

const skill = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../skills/podarcis');

test('reads the vault configuration through the real script', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'podarcis-panel-'));
  const { state } = await runVault('uv', skill, root);
  assert.deepEqual(state.config.repos, { sources: null, wiki: null, workspace: null });
  assert.ok('semantic-scholar' in state.catalogue);
  assert.equal(state.repos.wiki.state, 'missing');
});

test('reports the script error instead of a configuration', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'podarcis-panel-'));
  await assert.rejects(runVault('uv', skill, root, { agents: [], repos: { notes: 'x' }, mcp: [], skills: [] }), /repos takes only/);
});

test('reports a missing uv', async () => {
  await assert.rejects(runVault('/nonexistent/uv', skill, tmpdir()), /Could not start \/nonexistent\/uv/);
});
